//! Base64, for the one place binary belongs in a text protocol.
//!
//! The Model Context Protocol carries an image as base64 inside a JSON message, so a frame
//! the renderer produced has to be encoded before a client can look at it. This is the
//! standard alphabet with padding, written out rather than pulled in: it is thirty lines, it
//! has no configuration, and adding a dependency for it would be the kind of decision
//! ADR 0009 exists to prevent.
//!
//! There is deliberately no decoder. Nothing this server receives is base64 — clients send
//! identifiers, numbers and URIs — so a decoder would be code with no caller.

/// The standard alphabet, in the order RFC 4648 defines it.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes bytes as standard base64 with padding.
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = chunk.get(1).map_or(0, |byte| u32::from(*byte));
        let third = chunk.get(2).map_or(0, |byte| u32::from(*byte));
        let packed = (first << 16) | (second << 8) | third;
        out.push(char::from(ALPHABET[(packed >> 18) as usize & 0x3F]));
        out.push(char::from(ALPHABET[(packed >> 12) as usize & 0x3F]));
        // The last group is padded rather than truncated, so the line length always tells a
        // reader how many bytes were in it.
        out.push(if chunk.len() > 1 {
            char::from(ALPHABET[(packed >> 6) as usize & 0x3F])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(ALPHABET[packed as usize & 0x3F])
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_published_vectors_encode_as_published() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(input.as_bytes()), expected, "{input}");
        }
    }

    #[test]
    fn every_byte_round_trips_through_the_alphabet() {
        // A byte-by-byte sweep, because off-by-one errors in the sixth-bit masks only show
        // up on values that sit on a boundary.
        let all: Vec<u8> = (0..=255).collect();
        let encoded = encode(&all);
        assert_eq!(encoded.len(), 344);
        assert!(encoded.ends_with("w=="));
        assert!(
            encoded
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
        );
        // 255, 128 and 0 land on the three padding lengths in order.
        assert_eq!(encode(&[0xFF]), "/w==");
        assert_eq!(encode(&[0x00, 0x00]), "AAA=");
    }
}

//! Reading bytes without ever trusting them.
//!
//! Every parser in this backend reads through these two types, and neither of them can panic
//! on hostile input: there is no indexing, no slicing by a length a file stated, and no
//! arithmetic that is not checked. That is not fastidiousness — M2's gate is a corpus with
//! **zero crashes** in it, and a corpus contains a truncated file, a zero-length file and a
//! file whose extension lies. A parser that reaches for `bytes[offset + 8]` fails that gate
//! on the first one.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::MediaError;

/// The largest single read this backend will perform.
///
/// A container states the size of its own structures, so a damaged one can state a size that
/// would allocate the machine's memory. Sixteen mebibytes is far above any real header table
/// and far below a problem.
pub(crate) const MAX_READ: usize = 16 * 1024 * 1024;

/// The largest file this backend will read in one piece.
///
/// Stills are read whole because validating one means walking all of its chunks. A movie is
/// never read whole: its parsers seek. The cap is what keeps the first habit from becoming
/// the second by accident.
pub(crate) const MAX_WHOLE_FILE: u64 = 64 * 1024 * 1024;

/// A file, open, with its length, read only at offsets a parser has checked.
#[derive(Debug)]
pub(crate) struct Source {
    file: File,
    len: u64,
}

impl Source {
    /// Opens a file and asks the filesystem how long it is.
    ///
    /// # Errors
    ///
    /// [`MediaError::Io`] when the path is not a readable file, which is the `offline`
    /// condition and the only one that is about the file's surroundings rather than its
    /// contents. [`MediaError::Malformed`] for a file of zero length: it is present, so it is
    /// not offline, and an empty container is not what any container claims to be.
    pub(crate) fn open(path: &Path) -> Result<Self, MediaError> {
        let file = File::open(path)
            .map_err(|error| MediaError::Io(format!("{}: {error}", path.display())))?;
        let metadata = file
            .metadata()
            .map_err(|error| MediaError::Io(format!("{}: {error}", path.display())))?;
        if !metadata.is_file() {
            return Err(MediaError::Io(format!(
                "{} is not a regular file",
                path.display()
            )));
        }
        let len = metadata.len();
        if len == 0 {
            return Err(MediaError::Malformed(format!(
                "{} is empty: zero bytes are not a container",
                path.display()
            )));
        }
        Ok(Self { file, len })
    }

    /// The file's length in bytes.
    pub(crate) const fn len(&self) -> u64 {
        self.len
    }

    /// `len` bytes at `offset`.
    ///
    /// # Errors
    ///
    /// [`MediaError::Malformed`] when the range runs past the end of the file — which is what
    /// a truncated container looks like from in here — or when the length is implausible, and
    /// [`MediaError::Io`] when the read itself fails.
    pub(crate) fn bytes(&mut self, offset: u64, len: usize) -> Result<Vec<u8>, MediaError> {
        if len > MAX_READ {
            return Err(MediaError::Malformed(format!(
                "a structure declares {len} bytes, past this build's {MAX_READ}-byte limit"
            )));
        }
        let end = offset
            .checked_add(len as u64)
            .ok_or_else(|| MediaError::Malformed("an offset past the end of the".to_owned()))?;
        if end > self.len {
            return Err(MediaError::Malformed(format!(
                "a structure at {offset} runs {len} bytes, past the file's {}",
                self.len
            )));
        }
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|error| MediaError::Io(error.to_string()))?;
        let mut buffer = vec![0_u8; len];
        self.file.read_exact(&mut buffer).map_err(|error| {
            MediaError::Malformed(format!("a read at {offset} failed: {error}"))
        })?;
        Ok(buffer)
    }

    /// The whole file, for a format that cannot be validated any other way.
    ///
    /// # Errors
    ///
    /// [`MediaError::Unsupported`] when the file is larger than [`MAX_WHOLE_FILE`], because a
    /// still that size is not a still, and the three conditions [`Source::bytes`] reports.
    pub(crate) fn whole(&mut self) -> Result<Vec<u8>, MediaError> {
        if self.len > MAX_WHOLE_FILE {
            return Err(MediaError::Unsupported(format!(
                "{} bytes is too large to read as one still",
                self.len
            )));
        }
        let len = usize::try_from(self.len)
            .map_err(|_| MediaError::Unsupported("the file does not fit in memory".to_owned()))?;
        self.bytes(0, len)
    }
}

/// A cursor over bytes already read, with no operation that can panic.
///
/// Every getter advances by exactly what it read and answers `None` at the end of the buffer,
/// so a parser is a chain of `?` rather than a chain of assumptions. Positions are `usize`
/// and advance by checked addition.
#[derive(Debug, Clone)]
pub(crate) struct Cursor<'bytes> {
    bytes: &'bytes [u8],
    position: usize,
}

impl<'bytes> Cursor<'bytes> {
    /// A cursor at the start of a buffer.
    pub(crate) const fn new(bytes: &'bytes [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    /// Where the cursor is.
    pub(crate) const fn position(&self) -> usize {
        self.position
    }

    /// Moves forward, stopping at the end rather than past it.
    pub(crate) fn skip(&mut self, count: usize) -> Option<()> {
        let next = self.position.checked_add(count)?;
        if next > self.bytes.len() {
            return None;
        }
        self.position = next;
        Some(())
    }

    /// The next `count` bytes.
    pub(crate) fn take(&mut self, count: usize) -> Option<&'bytes [u8]> {
        let next = self.position.checked_add(count)?;
        let slice = self.bytes.get(self.position..next)?;
        self.position = next;
        Some(slice)
    }

    /// The next byte.
    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|slice| slice[0])
    }

    /// The next two bytes, big-endian.
    pub(crate) fn be16(&mut self) -> Option<u16> {
        self.take(2)
            .and_then(|slice| slice.try_into().ok())
            .map(u16::from_be_bytes)
    }

    /// The next four bytes, big-endian.
    pub(crate) fn be32(&mut self) -> Option<u32> {
        self.take(4)
            .and_then(|slice| slice.try_into().ok())
            .map(u32::from_be_bytes)
    }

    /// The next eight bytes, big-endian.
    pub(crate) fn be64(&mut self) -> Option<u64> {
        self.take(8)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_be_bytes)
    }

    /// The next four bytes, big-endian, as a signed value.
    pub(crate) fn be32i(&mut self) -> Option<i32> {
        self.be32().map(|value| value as i32)
    }

    /// The next two bytes, little-endian.
    pub(crate) fn le16(&mut self) -> Option<u16> {
        self.take(2)
            .and_then(|slice| slice.try_into().ok())
            .map(u16::from_le_bytes)
    }

    /// The next four bytes, little-endian.
    pub(crate) fn le32(&mut self) -> Option<u32> {
        self.take(4)
            .and_then(|slice| slice.try_into().ok())
            .map(u32::from_le_bytes)
    }

    /// The next four bytes as a four-character code.
    pub(crate) fn fourcc(&mut self) -> Option<[u8; 4]> {
        self.take(4).and_then(|slice| slice.try_into().ok())
    }
}

/// A four-character code as text, for a message a person reads.
pub(crate) fn fourcc_name(code: [u8; 4]) -> String {
    code.iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                '.'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cursor_stops_at_the_end_instead_of_panicking() {
        let bytes = [1_u8, 2, 3];
        let mut cursor = Cursor::new(&bytes);
        assert_eq!(cursor.u8(), Some(1));
        assert_eq!(cursor.be16(), Some(0x0203));
        assert_eq!(cursor.u8(), None);
        assert_eq!(cursor.be32(), None);
        assert_eq!(cursor.be64(), None);
        assert_eq!(cursor.fourcc(), None);
        assert_eq!(cursor.take(1), None);
        assert_eq!(cursor.skip(1), None);
        assert_eq!(cursor.position(), 3);
    }

    #[test]
    fn a_cursor_never_overflows_its_position() {
        let bytes = [0_u8; 4];
        let mut cursor = Cursor::new(&bytes);
        assert_eq!(cursor.skip(usize::MAX), None);
        assert_eq!(cursor.take(usize::MAX), None);
        assert_eq!(cursor.position(), 0, "a refused read moves nothing");
    }

    #[test]
    fn endianness_is_explicit_in_the_name() {
        let bytes = [0x01_u8, 0x02, 0x03, 0x04];
        assert_eq!(Cursor::new(&bytes).be32(), Some(0x0102_0304));
        assert_eq!(Cursor::new(&bytes).le32(), Some(0x0403_0201));
        assert_eq!(Cursor::new(&bytes).le16(), Some(0x0201));
        assert_eq!(Cursor::new(&[0xFF, 0xFF, 0xFF, 0xFF]).be32i(), Some(-1));
    }

    #[test]
    fn a_four_character_code_is_printable_even_when_it_is_not_text() {
        assert_eq!(fourcc_name(*b"moov"), "moov");
        assert_eq!(fourcc_name(*b"qt  "), "qt  ");
        assert_eq!(fourcc_name([0, 1, 2, 3]), "....");
    }

    #[test]
    fn an_absent_file_is_offline_and_an_empty_one_is_damaged() {
        let dir = std::env::temp_dir().join("maj-media-source");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let absent = dir.join("absent.bin");
        let _ = std::fs::remove_file(&absent);
        assert!(matches!(Source::open(&absent), Err(MediaError::Io(_))));

        let empty = dir.join("empty.bin");
        std::fs::write(&empty, b"").expect("write");
        assert!(matches!(
            Source::open(&empty),
            Err(MediaError::Malformed(_))
        ));
    }

    #[test]
    fn a_read_past_the_end_is_malformed_rather_than_short() {
        let dir = std::env::temp_dir().join("maj-media-source");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("eight.bin");
        std::fs::write(&path, b"12345678").expect("write");
        let mut source = Source::open(&path).expect("open");
        assert_eq!(source.len(), 8);
        assert_eq!(source.bytes(4, 4).expect("in range"), b"5678");
        assert!(matches!(source.bytes(6, 4), Err(MediaError::Malformed(_)),));
        assert!(matches!(
            source.bytes(u64::MAX, 4),
            Err(MediaError::Malformed(_))
        ));
        assert!(matches!(
            source.bytes(0, MAX_READ + 1),
            Err(MediaError::Malformed(_))
        ));
    }
}

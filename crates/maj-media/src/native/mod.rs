//! The backend that reads headers and no frames, in Rust, with no FFI at all.
//!
//! This is `maj-media`'s first [`MediaBackend`], and it exists because probe is the half of
//! M2 that needs no C library: finding out what a file *is* means reading its headers, and a
//! container's headers are a documented byte layout. So the probe path, the vocabulary it
//! answers in and the cache in front of it can all be built, tested and measured **before**
//! the decision about `unsafe` that `docs/MEDIA.md` §8 sets out — which is M2b's ADR and not
//! this phase's to make.
//!
//! What that buys, concretely: `PictureResolver::kind` can tell a damaged file from an absent
//! one today, which is a debt `.planning/STATE.md` has been carrying since P0.3.
//!
//! What it does not buy: a frame. [`NativeBackend::open_video`] refuses, every time, with
//! `undecodable` — the label that has always been the honest one for a build with no decoder
//! in it.
//!
//! # The format is decided by the bytes, never by the name
//!
//! Every parser here is selected by the file's first bytes. A `.mov` that is really a WAV
//! probes as a WAV, because that is what it is; the extension is a claim and
//! [`crate::ContainerKind::claimed_by_extension`] is where a caller can go to compare the
//! claim with the finding. That is one of the corpus's pathologies and it is the reason
//! sniffing is not an optimisation here.

mod bytes;
mod codec;
mod isobmff;
mod matroska;
mod riff;
mod still;

use std::path::Path;

use crate::container::ContainerKind;
use crate::decode::VideoReader;
use crate::error::MediaError;
use crate::probe::{DecoderIdentity, HardwareSupport, MediaBackend, MediaProbe};
use crate::stream::StreamInfo;

use bytes::Source;

/// The bytes read before a parser is chosen.
const SNIFF: usize = 16;

/// The signature every MXF file begins with: the SMPTE universal label of a partition pack.
const MXF_SIGNATURE: [u8; 11] = [
    0x06, 0x0E, 0x2B, 0x34, 0x02, 0x05, 0x01, 0x01, 0x0D, 0x01, 0x02,
];

/// What this backend is, so a cached answer carries its producer.
///
/// The configuration string names the parsers, which means **adding one changes the
/// identity**: every probe cached by an older build of this crate is stale by construction
/// rather than by someone remembering to clear a cache.
const CONFIGURATION: &str = "parsers=isobmff,matroska,riff,png,netpbm; decode=none; swscale=no";

/// Reads container headers in Rust: no FFI, no `unsafe`, no frames.
#[derive(Debug, Clone)]
pub struct NativeBackend {
    identity: DecoderIdentity,
}

impl NativeBackend {
    /// The backend.
    #[must_use]
    pub fn new() -> Self {
        Self {
            identity: DecoderIdentity::new(
                "native-headers",
                env!("CARGO_PKG_VERSION"),
                CONFIGURATION,
            ),
        }
    }
}

impl Default for NativeBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaBackend for NativeBackend {
    fn identity(&self) -> &DecoderIdentity {
        &self.identity
    }

    fn probe(&self, path: &Path) -> Result<MediaProbe, MediaError> {
        let mut source = Source::open(path)?;
        let head = source.bytes(0, SNIFF.min(source.len() as usize))?;
        let (container, streams) = read(&mut source, &head)?;
        Ok(MediaProbe {
            container,
            streams,
            decoder: self.identity.clone(),
        })
    }

    fn open_video(&self, path: &Path, stream: usize) -> Result<Box<dyn VideoReader>, MediaError> {
        // A probe first, so the three conditions stay apart: a file that is not there is
        // `offline` and one that is damaged is `damaged`, even when the ask was for a frame
        // this build could never have produced.
        self.probe(path)?;
        Err(MediaError::Unsupported(format!(
            "stream {stream} of {}: this build reads headers and decodes no frames (M2b)",
            path.display()
        )))
    }

    fn hardware(&self) -> HardwareSupport {
        // Not a prediction: there is no decoder to accelerate, so there is nothing to
        // capability-test. ADR 0005's measurement belongs to the build that has one.
        HardwareSupport::none()
    }
}

/// Picks a parser by signature and runs it.
fn read(source: &mut Source, head: &[u8]) -> Result<(ContainerKind, Vec<StreamInfo>), MediaError> {
    if isobmff::looks_like(head) {
        return isobmff::probe(source);
    }
    if matroska::looks_like(head) {
        return matroska::probe(source);
    }
    if riff::looks_like(head) {
        return riff::probe(source);
    }
    if still::looks_like_png(head) {
        let whole = source.whole()?;
        return still::probe_png(&whole).map(|streams| (ContainerKind::Png, streams));
    }
    if still::looks_like_netpbm(head) {
        let whole = source.whole()?;
        return still::probe_netpbm(&whole).map(|streams| (ContainerKind::Ppm, streams));
    }
    if head.starts_with(&MXF_SIGNATURE) {
        // Recognised and not read. `ContainerKind::Mxf` exists for the build that reads one;
        // answering with it here and no streams would be a file reported as readable.
        return Err(MediaError::Unsupported(
            "a Material Exchange Format file, which this build does not read".to_owned(),
        ));
    }
    Err(MediaError::Unsupported(format!(
        "no parser in this build recognises a file beginning {}",
        head.iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("maj-media-native");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write");
        path
    }

    #[test]
    fn the_identity_names_the_backend_and_its_parsers() {
        let backend = NativeBackend::new();
        assert_eq!(backend.identity().backend, "native-headers");
        assert!(backend.identity().key().starts_with("native-headers-"));
        let other = DecoderIdentity::new(
            "native-headers",
            env!("CARGO_PKG_VERSION"),
            "parsers=isobmff; decode=none",
        );
        assert_ne!(
            backend.identity().configuration,
            other.configuration,
            "adding a parser has to change the identity"
        );
    }

    #[test]
    fn no_frame_is_ever_produced_and_the_refusal_says_why() {
        let path = temp("still.ppm", b"P6\n2 2\n255\n            ");
        let backend = NativeBackend::new();
        let refused = backend.open_video(&path, 0).expect_err("no decoder");
        assert_eq!(refused.label(), "undecodable");
        assert!(refused.reason().contains("M2b"));
    }

    #[test]
    fn asking_for_a_frame_of_an_absent_file_is_still_offline() {
        let dir = std::env::temp_dir().join("maj-media-native");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let absent = dir.join("absent.mp4");
        let _ = std::fs::remove_file(&absent);
        let refused = NativeBackend::new()
            .open_video(&absent, 0)
            .expect_err("not there");
        assert_eq!(refused.label(), "offline");
    }

    #[test]
    fn nothing_is_capability_tested_because_there_is_nothing_to_accelerate() {
        assert_eq!(NativeBackend::new().hardware(), HardwareSupport::none());
    }

    #[test]
    fn the_extension_does_not_choose_the_parser() {
        // A Netpbm still called `.mov`. It probes as what it is.
        let path = temp("liar.mov", b"P6\n2 2\n255\n            ");
        let probe = NativeBackend::new().probe(&path).expect("readable");
        assert_eq!(probe.container, ContainerKind::Ppm);
        assert_eq!(
            ContainerKind::claimed_by_extension(&path),
            ContainerKind::QuickTime,
            "the claim and the finding are both available, and they disagree"
        );
    }

    #[test]
    fn an_unrecognised_file_is_undecodable_and_says_what_it_began_with() {
        let path = temp(
            "mystery.braw",
            &[0xDE, 0xAD, 0xBE, 0xEF, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        let refused = NativeBackend::new().probe(&path).expect_err("unknown");
        assert_eq!(refused.label(), "undecodable");
        assert!(refused.reason().contains("de ad be ef"));
    }

    #[test]
    fn an_mxf_is_recognised_and_refused_rather_than_reported_readable() {
        let mut bytes = MXF_SIGNATURE.to_vec();
        bytes.extend_from_slice(&[0x01, 0x01, 0x02, 0x04, 0x00]);
        let path = temp("broadcast.mxf", &bytes);
        let refused = NativeBackend::new().probe(&path).expect_err("not read");
        assert_eq!(refused.label(), "undecodable");
        assert!(refused.reason().contains("Material Exchange"));
    }

    #[test]
    fn an_empty_file_is_damaged_and_an_absent_one_is_offline() {
        let empty = temp("empty.mp4", b"");
        assert_eq!(
            NativeBackend::new()
                .probe(&empty)
                .expect_err("empty")
                .label(),
            "damaged"
        );
        let dir = std::env::temp_dir().join("maj-media-native");
        let absent = dir.join("never-written.mp4");
        let _ = std::fs::remove_file(&absent);
        assert_eq!(
            NativeBackend::new()
                .probe(&absent)
                .expect_err("absent")
                .label(),
            "offline"
        );
    }
}

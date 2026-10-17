//! What a file is, without decoding it — and who answered.

use std::fmt;
use std::path::Path;

use crate::container::ContainerKind;
use crate::decode::VideoReader;
use crate::error::MediaError;
use crate::stream::{AudioStream, StreamInfo, VideoStream};

/// Everything known about a file after reading its headers and no frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaProbe {
    /// What kind of container the bytes turned out to be.
    pub container: ContainerKind,
    /// Every stream the container declares, in container order.
    pub streams: Vec<StreamInfo>,
    /// The decoder build that produced this, so a cache entry cannot outlive its decoder.
    pub decoder: DecoderIdentity,
}

impl MediaProbe {
    /// The picture streams, in container order.
    pub fn video_streams(&self) -> impl Iterator<Item = &VideoStream> {
        self.streams.iter().filter_map(StreamInfo::as_video)
    }

    /// The sound streams, in container order.
    pub fn audio_streams(&self) -> impl Iterator<Item = &AudioStream> {
        self.streams.iter().filter_map(StreamInfo::as_audio)
    }

    /// The first picture stream, which is what a clip means by "the picture" until a project
    /// says otherwise.
    #[must_use]
    pub fn primary_video(&self) -> Option<&VideoStream> {
        self.video_streams().next()
    }

    /// Whether the container holds a picture at all.
    ///
    /// A `.wav` answers `false` and that is not a pathology; a `.mp4` with no video track
    /// answers `false` and is one. The difference is [`ContainerKind::carries_video`], which
    /// is why both facts are kept rather than collapsed into one.
    #[must_use]
    pub fn has_video(&self) -> bool {
        self.primary_video().is_some()
    }
}

/// Which decoder produced an answer. Part of every cache key and of the render manifest.
///
/// A probe result is only as good as the thing that produced it, so a cached one carries its
/// producer. When the backend changes — a different FFmpeg, a different configuration, a
/// different build of this crate — the identity changes with it and every entry keyed by the
/// old one is stale by construction rather than by someone remembering to clear a cache.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DecoderIdentity {
    /// Which backend answered: `"ffmpeg"`, or `"native-headers"` for the one in this crate.
    pub backend: &'static str,
    /// The backend's own version string.
    pub version: String,
    /// Hash of the backend's configuration, per `docs/FFMPEG.md` §4.
    pub configuration: [u8; 32],
}

impl DecoderIdentity {
    /// An identity whose configuration hash is BLAKE3 of a configuration string.
    #[must_use]
    pub fn new(backend: &'static str, version: impl Into<String>, configuration: &str) -> Self {
        Self {
            backend,
            version: version.into(),
            configuration: *blake3::hash(configuration.as_bytes()).as_bytes(),
        }
    }

    /// The short stable name, for a manifest: backend, version and the configuration's first
    /// four bytes.
    #[must_use]
    pub fn key(&self) -> String {
        let [a, b, c, d, ..] = self.configuration;
        format!(
            "{}-{} {a:02x}{b:02x}{c:02x}{d:02x}",
            self.backend, self.version
        )
    }
}

impl fmt::Display for DecoderIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.key())
    }
}

/// What the machine actually answered when asked about hardware decode.
///
/// Not what it is expected to do: ADR 0005 requires hardware decode to be capability-tested,
/// and a struct of booleans nobody filled in by asking is a prediction wearing a measurement's
/// clothes. Hardware decode is also Tier 1 only — a certified render decodes in software,
/// because a driver's output cannot be part of a claim that the same log produces the same
/// bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HardwareSupport {
    /// Direct3D 11 video acceleration.
    pub d3d11va: bool,
    /// DirectX Video Acceleration 2.
    pub dxva2: bool,
}

impl HardwareSupport {
    /// No hardware decode, which is what a backend with no FFI answers.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            d3d11va: false,
            dxva2: false,
        }
    }

    /// Whether any hardware path was found.
    #[must_use]
    pub const fn any(self) -> bool {
        self.d3d11va || self.dxva2
    }
}

/// A source of media answers: what a file is, and how to read its frames.
///
/// The trait exists so the decoder is a choice rather than a dependency. Two implementations
/// are foreseen and the surface is the same for both: the in-process one behind a leaf FFI
/// crate, and an out-of-process one that owns the C library in a helper the editor can
/// survive the death of (`docs/MEDIA.md` §8). Keeping that escape hatch as an implementation
/// of a trait rather than a rewrite is the reason this is a trait at all.
pub trait MediaBackend: fmt::Debug {
    /// Who this backend is. Goes into every cache key and into the render manifest.
    fn identity(&self) -> &DecoderIdentity;

    /// What a file is, from its headers and no frames.
    ///
    /// # Errors
    ///
    /// [`MediaError::Io`] when the file cannot be read, [`MediaError::Malformed`] when its
    /// bytes are not what its container claims, and [`MediaError::Unsupported`] when no
    /// parser in this build recognises it.
    fn probe(&self, path: &Path) -> Result<crate::MediaProbe, MediaError>;

    /// A reader for one picture stream of a file.
    ///
    /// # Errors
    ///
    /// The three conditions [`MediaBackend::probe`] reports, plus [`MediaError::Unsupported`]
    /// when the container is readable and its codec is not.
    fn open_video(&self, path: &Path, stream: usize) -> Result<Box<dyn VideoReader>, MediaError>;

    /// Hardware decode is capability-tested, never assumed (ADR 0005).
    fn hardware(&self) -> HardwareSupport;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picture::{BitDepth, PictureFormat, PlaneLayout};
    use crate::stream::{CodecKind, FieldOrder, FrameRateKind, Rotation, StreamRole};
    use maj_time::{Rational, Timebase};

    fn probe(streams: Vec<StreamInfo>) -> MediaProbe {
        MediaProbe {
            container: ContainerKind::Mp4,
            streams,
            decoder: DecoderIdentity::new("test", "0", "--none"),
        }
    }

    fn video(index: usize) -> StreamInfo {
        StreamInfo::Video(VideoStream {
            index,
            codec: CodecKind::H264,
            coded_size: (1_920, 1_080),
            sample_aspect: Rational::from_integer(1),
            timebase: Some(Timebase::FPS_25),
            stream_timebase: Rational::from_reduced(1, 25),
            frames: Some(5),
            rate: FrameRateKind::Constant(Timebase::FPS_25),
            rotation: Rotation::None,
            field_order: FieldOrder::Progressive,
            picture: PictureFormat::undeclared(PlaneLayout::Yuv420, BitDepth::Eight),
        })
    }

    #[test]
    fn a_probe_separates_the_streams_it_found() {
        let found = probe(vec![
            StreamInfo::Other {
                index: 0,
                role: StreamRole::Timecode,
            },
            video(1),
            StreamInfo::Audio(AudioStream {
                index: 2,
                codec: CodecKind::Aac,
                channels: 2,
                sample_rate: 48_000,
                sample_bits: None,
                stream_timebase: Rational::from_reduced(1, 48_000),
                frames: None,
            }),
        ]);
        assert!(found.has_video());
        assert_eq!(found.primary_video().map(|stream| stream.index), Some(1));
        assert_eq!(found.video_streams().count(), 1);
        assert_eq!(found.audio_streams().count(), 1);
        assert_eq!(found.streams.len(), 3, "the unread track is still counted");
    }

    #[test]
    fn a_container_with_no_picture_says_so_without_failing() {
        let found = probe(Vec::new());
        assert!(!found.has_video());
        assert!(found.primary_video().is_none());
    }

    #[test]
    fn an_identity_is_distinct_per_configuration() {
        let first = DecoderIdentity::new("ffmpeg", "9.0.2", "--disable-gpl");
        let second = DecoderIdentity::new("ffmpeg", "9.0.2", "--enable-gpl");
        assert_ne!(first, second, "configuration is part of who answered");
        assert_ne!(first.key(), second.key());
        assert!(first.key().starts_with("ffmpeg-9.0.2 "));
        assert_eq!(first.to_string(), first.key());
    }

    #[test]
    fn hardware_support_defaults_to_nothing_measured() {
        assert_eq!(HardwareSupport::default(), HardwareSupport::none());
        assert!(!HardwareSupport::none().any());
        assert!(
            HardwareSupport {
                d3d11va: true,
                dxva2: false
            }
            .any()
        );
    }
}

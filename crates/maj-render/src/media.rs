//! The adapter: `maj-media`'s probe behind the renderer's own seam.
//!
//! [`PictureResolver`] is defined here, at layer 4, and `maj-media` is layer 2, so the
//! decoder crate **cannot** implement it — dependencies point downward only. That is the
//! architecture working rather than an obstacle, and `docs/MEDIA.md` §2 records the split it
//! forces: the vocabulary and the probe live at layer 2, and the adapter that knows both
//! vocabularies lives here, in one place.
//!
//! # What this adapter does today, and what it does not
//!
//! It answers questions **about** media. It decodes nothing: there is no decoder in this
//! build, [`maj_media::MediaBackend::open_video`] refuses, and a movie still draws a
//! labelled placeholder. M2b puts software decode behind the same seam, and the shape of this
//! type does not change when it does — `resolve` gains a branch.
//!
//! What it changes immediately is the honesty of two answers:
//!
//! * **A present-but-damaged file is named `damaged`**, in the manifest as well as in the
//!   frame. That is the debt `.planning/STATE.md` recorded against
//!   [`PictureResolver::kind`]: a stat cannot tell a damaged file from a readable one, so a
//!   damaged still used to be named for its container in a document whose frame beside it
//!   said `damaged`. A cached probe tells them apart without a decode.
//! * **A missing movie is named `offline` rather than `undecodable`.**
//!   [`WorkspaceResolver`] has no parser for a `.mov`, so it reports `undecodable` whether
//!   the file is there or not — and "wait for a decoder" is the wrong instruction for a file
//!   that has simply moved. The probe knows which it is.
//!
//! # One invariant, tested rather than asserted
//!
//! [`PictureResolver::kind`] must name what [`PictureResolver::resolve`] would produce. The
//! two answers come from one probe here, so they cannot drift; `tests/media_resolver.rs`
//! checks it over every file of the synthetic corpus, which is the only way to know.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use maj_media::{ContainerKind, MediaBackend, MediaError, ProbeCache};

use crate::source::{
    DEFAULT_TRANSFER, MissingMedia, PictureResolver, PictureSource, Placeholder, Png, Ppm,
    SourceError, WorkspaceResolver, file_path,
};

/// Resolves a URI through a media backend's probe, and through the still readers this build
/// already has.
///
/// [`WorkspaceResolver`] is composed rather than replaced: `generated:`, `solid:`, `title:`,
/// `.png` and `.ppm` keep resolving exactly as they do now and keep their tests, and the probe
/// answers for everything that resolver calls [`SourceError::Unsupported`]. The
/// [`MissingMedia`] policy still sits at the outside, so `--strict-media` keeps working
/// unchanged over a strictly better-classified set of media.
///
/// Interior mutability is forced by the seam, not chosen: [`PictureResolver::resolve`] and
/// [`PictureResolver::kind`] both take `&self`, and a cache has to be written to. That is
/// sound today because `PictureResolver` carries no `Send`/`Sync` bound and the renderer,
/// including `render_tiled`, is single-threaded. **If `PictureResolver` ever gains
/// `Send + Sync`** — which parallel tiles or a distributed render would want — the cache
/// inside becomes a mutex or a reader per thread, and that is a change to this type and not
/// to the trait.
pub struct MediaResolver<B: MediaBackend> {
    stills: WorkspaceResolver,
    probes: ProbeCache<B>,
    /// Containers probed and refused, kept so a caller can report what a timeline could not
    /// read without probing the project a second time.
    reported: RefCell<BTreeSet<String>>,
}

impl<B: MediaBackend> MediaResolver<B> {
    /// A resolver over a backend, substituting a labelled placeholder for media it cannot read.
    #[must_use]
    pub fn new(backend: B) -> Self {
        Self::with_stills(WorkspaceResolver::new(), backend)
    }

    /// A resolver over a backend and an already-configured still resolver.
    ///
    /// The still resolver carries the transfer function assumed for files that do not declare
    /// one, and the [`MissingMedia`] policy, so a delivery's `--strict-media` arrives here by
    /// being set there.
    #[must_use]
    pub fn with_stills(stills: WorkspaceResolver, backend: B) -> Self {
        Self {
            stills,
            probes: ProbeCache::new(backend),
            reported: RefCell::new(BTreeSet::new()),
        }
    }

    /// What this resolver does about media it cannot read.
    #[must_use]
    pub const fn missing_media(&self) -> MissingMedia {
        self.stills.missing_media()
    }

    /// The probe cache, for a caller that wants the facts rather than the picture.
    ///
    /// A conform or quality-control pass wants exactly this: the container, the streams, the
    /// rate and the rotation of everything a timeline names, which is a probe each and not a
    /// render.
    #[must_use]
    pub const fn probes(&self) -> &ProbeCache<B> {
        &self.probes
    }

    /// Every container this resolver probed successfully and still could not draw, by its
    /// short name.
    ///
    /// The list a report wants: "this timeline is three `mov` files away from rendering" is a
    /// more useful sentence than three identical placeholders.
    #[must_use]
    pub fn undecodable_containers(&self) -> Vec<String> {
        self.reported.borrow().iter().cloned().collect()
    }

    /// Whether a URI is one the still resolver answers from the URI itself.
    fn is_generated(uri: &str) -> bool {
        let lowered = uri.to_ascii_lowercase();
        lowered.starts_with("generated:")
            || lowered.starts_with("solid:")
            || lowered.starts_with("title:")
    }

    /// Whether this build can draw a picture out of a container.
    ///
    /// Two stills, today. M2b adds the containers a decoder reads, and nothing else about
    /// this type changes — which is why the predicate is one function rather than a condition
    /// spread over `resolve` and `kind`.
    const fn decodes(container: ContainerKind) -> bool {
        matches!(container, ContainerKind::Png | ContainerKind::Ppm)
    }

    /// The condition a URI is in, as the renderer's own error type.
    ///
    /// `Ok(container)` for media this build can draw, and an error otherwise — including for
    /// media that probed perfectly well and has no decoder, which is
    /// [`SourceError::Unsupported`] and labels `undecodable`.
    fn classify(&self, uri: &str) -> Result<ContainerKind, SourceError> {
        let path = Path::new(file_path(uri));
        match self.probes.container(path) {
            Ok(container) if Self::decodes(container) => Ok(container),
            Ok(container) => {
                self.reported
                    .borrow_mut()
                    .insert(container.key().to_owned());
                Err(SourceError::Unsupported(format!(
                    "{uri} is a readable {} container and this build has no decoder for it",
                    container.key()
                )))
            }
            Err(MediaError::Io(reason)) => Err(SourceError::Io(reason)),
            Err(MediaError::Malformed(reason)) => Err(SourceError::Malformed(reason)),
            Err(MediaError::Unsupported(reason)) => Err(SourceError::Unsupported(reason)),
        }
    }

    /// Opens a still as the container the **bytes** turned out to be.
    ///
    /// Not as the one the name claims: handing the URI back to [`WorkspaceResolver`] would
    /// dispatch on the extension, so a PNG called `.mp4` would be named `png` by `kind` and
    /// drawn as `undecodable` — one file, two answers, which is the defect this adapter
    /// exists to close.
    fn open(
        &self,
        uri: &str,
        container: ContainerKind,
    ) -> Result<Box<dyn PictureSource>, SourceError> {
        let transfer = self.stills.transfer().unwrap_or(DEFAULT_TRANSFER);
        let path = file_path(uri);
        match container {
            ContainerKind::Png => Ok(Box::new(Png::from_path(path, transfer)?)),
            ContainerKind::Ppm => Ok(Box::new(Ppm::from_path(path, transfer)?)),
            // `decodes` admits exactly those two, so a third arm here would be a decoder this
            // build does not have.
            other => Err(SourceError::Unsupported(format!(
                "{uri} is a {} container and this build has no decoder for it",
                other.key()
            ))),
        }
    }

    /// The label for a condition, under this resolver's policy.
    fn label_of(&self, error: &SourceError) -> &'static str {
        match self.missing_media() {
            // A refusing resolver draws nothing, so there is no placeholder to name.
            MissingMedia::Refuse => "-",
            MissingMedia::Placeholder => match error {
                SourceError::Io(_) => Placeholder::OFFLINE,
                SourceError::Malformed(_) => Placeholder::DAMAGED,
                SourceError::Unsupported(_) => Placeholder::UNDECODABLE,
            },
        }
    }
}

impl<B: MediaBackend> fmt::Debug for MediaResolver<B> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MediaResolver")
            .field("stills", &self.stills)
            .field("decoder", &self.probes.backend().identity().key())
            .field("probed", &self.probes.len())
            .finish()
    }
}

impl<B: MediaBackend> PictureResolver for MediaResolver<B> {
    fn resolve(
        &self,
        uri: &str,
        generated_size: (u32, u32),
        source_frame: i64,
    ) -> Result<Box<dyn PictureSource>, SourceError> {
        if Self::is_generated(uri) {
            return self.stills.resolve(uri, generated_size, source_frame);
        }
        // One classification, then one policy applied to whatever it produced — so a failure
        // to open a file the probe called readable gets the same treatment as a failure to
        // probe it, and no path out of this function skips the policy.
        let opened = self
            .classify(uri)
            .and_then(|container| self.open(uri, container));
        match opened {
            Ok(source) => Ok(source),
            Err(reason) => match self.missing_media() {
                MissingMedia::Refuse => Err(reason),
                MissingMedia::Placeholder => Ok(Box::new(Placeholder::for_error(
                    &reason,
                    generated_size.0,
                    generated_size.1,
                ))),
            },
        }
    }

    fn kind(&self, uri: &str) -> &'static str {
        if Self::is_generated(uri) {
            return self.stills.kind(uri);
        }
        match self.classify(uri) {
            Ok(container) => container.key(),
            Err(reason) => self.label_of(&reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maj_media::NativeBackend;
    use maj_media::corpus;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("maj-render-media-{name}"));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        directory
    }

    fn resolver() -> MediaResolver<NativeBackend> {
        MediaResolver::new(NativeBackend::new())
    }

    #[test]
    fn a_damaged_still_is_named_damaged_rather_than_named_for_its_container() {
        let directory = scratch("damaged");
        let path = directory.join("still.png");
        let readable = corpus::files()
            .into_iter()
            .find(|file| file.name == "still-rgb-8bit.png")
            .expect("the corpus has a readable still");
        std::fs::write(&path, &readable.bytes).expect("write");

        let resolver = resolver();
        let uri = path.to_string_lossy().to_string();
        assert_eq!(resolver.kind(&uri), "png");

        // One byte different. The file is present, whole, and no longer what it claims.
        let mut damaged = readable.bytes.clone();
        let flip = damaged.len() / 2;
        damaged[flip] ^= 0xFF;
        std::fs::write(&path, &damaged).expect("rewrite");
        assert_eq!(
            resolver.kind(&uri),
            "damaged",
            "this is the debt: a stat cannot tell these apart, and a probe can"
        );
        let source = resolver.resolve(&uri, (16, 16), 0).expect("a placeholder");
        assert_eq!(
            source.kind(),
            "damaged",
            "the manifest and the frame have to agree"
        );
    }

    #[test]
    fn a_missing_movie_is_offline_rather_than_undecodable() {
        let directory = scratch("offline");
        let absent = directory.join("shot-01.mov");
        let uri = absent.to_string_lossy().to_string();

        // What the renderer said before this adapter existed: no parser, so no decoder.
        assert_eq!(WorkspaceResolver::new().kind(&uri), "undecodable");
        // What is actually true: the file is not there, and the instruction is to relink it.
        assert_eq!(resolver().kind(&uri), "offline");
    }

    #[test]
    fn a_readable_movie_is_undecodable_and_says_which_container_it_was() {
        let directory = scratch("readable-movie");
        let movie = corpus::files()
            .into_iter()
            .find(|file| file.name == "cfr-23976-h264-8bit-420-709-limited.mp4")
            .expect("the corpus has a movie");
        let path = directory.join(movie.name);
        std::fs::write(&path, &movie.bytes).expect("write");

        let resolver = resolver();
        let uri = path.to_string_lossy().to_string();
        assert_eq!(resolver.kind(&uri), "undecodable");
        assert_eq!(resolver.undecodable_containers(), vec!["mp4".to_owned()]);

        // And the probe is there for a report, without a second pass over the file.
        let probe = resolver
            .probes()
            .probe(&path)
            .expect("the container was read");
        assert_eq!(probe.container, ContainerKind::Mp4);
        assert_eq!(
            probe
                .primary_video()
                .and_then(|stream| stream.timebase)
                .map(|timebase| timebase.rate().num()),
            Some(24_000)
        );
    }

    #[test]
    fn the_generated_sources_are_untouched_by_the_adapter() {
        let resolver = resolver();
        for (uri, expected) in [
            ("generated:chart", "generated"),
            ("solid:0.5", "solid"),
            ("title:hello", "title"),
        ] {
            assert_eq!(resolver.kind(uri), expected);
            let source = resolver.resolve(uri, (32, 32), 0).expect("a picture");
            assert_eq!(source.kind(), expected);
        }
    }

    #[test]
    fn a_refusing_resolver_refuses_and_names_nothing() {
        let directory = scratch("refusing");
        let absent = directory.join("gone.mov");
        let uri = absent.to_string_lossy().to_string();
        let resolver = MediaResolver::with_stills(
            WorkspaceResolver::new().with_missing_media(MissingMedia::Refuse),
            NativeBackend::new(),
        );
        assert_eq!(resolver.missing_media(), MissingMedia::Refuse);
        assert_eq!(resolver.kind(&uri), "-");
        assert!(matches!(
            resolver.resolve(&uri, (16, 16), 0),
            Err(SourceError::Io(_))
        ));
    }

    #[test]
    fn a_still_is_probed_once_however_many_frames_name_it() {
        let directory = scratch("cached");
        let still = corpus::files()
            .into_iter()
            .find(|file| file.name == "still-rgb-8bit.ppm")
            .expect("the corpus has a Netpbm still");
        let path = directory.join(still.name);
        std::fs::write(&path, &still.bytes).expect("write");

        let resolver = resolver();
        let uri = path.to_string_lossy().to_string();
        for _ in 0..24 {
            assert_eq!(resolver.kind(&uri), "ppm");
        }
        assert_eq!(
            resolver.probes().misses(),
            1,
            "a frame of a timeline must not cost a probe"
        );
        assert_eq!(resolver.probes().hits(), 23);
    }

    #[test]
    fn the_debug_form_names_the_decoder_that_answered() {
        let shown = format!("{:?}", resolver());
        assert!(shown.contains("native-headers"), "{shown}");
    }
}

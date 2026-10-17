//! A probe answered once per file, so a question about media costs a stat and not a read.
//!
//! This module exists to repay a debt the project recorded rather than hid. `maj-render`'s
//! `PictureResolver::kind` has to name the source of a layer it is describing but not
//! rendering, it may cost a stat and must never cost a decode — and a stat cannot tell a
//! damaged file from a readable one. So a present-but-damaged still was named for its
//! container in the manifest while the frame it stood next to was a `damaged` placeholder.
//! Two statements about one file, disagreeing, in one document.
//!
//! A cached probe closes it. Reading a header is not decoding a frame, the answer is reused
//! for every later question about the same file, and the stat that was already being paid for
//! is what proves the answer is still about the file that is there now.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::container::ContainerKind;
use crate::error::MediaError;
use crate::probe::{MediaBackend, MediaProbe};

/// What a stat says about a file, which is what makes a cached probe safe to reuse.
///
/// Length and modification time, both as the filesystem reports them. Neither is a content
/// hash and this type does not pretend otherwise: a file rewritten in place within the
/// timestamp's resolution and to the same length would go unnoticed. Hashing every file on
/// every `kind` call would cost a full read, which is the exact cost the seam forbids, so the
/// trade is made deliberately and written down here. [`ProbeCache::invalidate`] is the
/// escape hatch for a caller that knows better.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl FileStamp {
    /// What the filesystem says about a path, or `None` when it will not say.
    fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        if !metadata.is_file() {
            return None;
        }
        Some(Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}

/// One answer, and the evidence that it is still about the right file and from the right
/// decoder.
#[derive(Debug)]
struct Entry {
    stamp: Option<FileStamp>,
    result: Result<MediaProbe, MediaError>,
}

/// Probe results, kept per path, with a backend behind them.
///
/// Interior mutability is not an implementation detail here: `PictureResolver::kind` and
/// `PictureResolver::resolve` both take `&self`, so an adapter that wants a cached answer has
/// no choice. That is sound today because `PictureResolver` carries no `Send`/`Sync` bound and
/// the renderer is single-threaded. The day the trait gains `Send + Sync` — which parallel
/// tiles or a distributed render would want — the [`RefCell`] here becomes a `Mutex` or a
/// cache per thread, and that is a change to this type and not to the seam.
#[derive(Debug)]
pub struct ProbeCache<B: MediaBackend> {
    backend: B,
    entries: RefCell<HashMap<PathBuf, Entry>>,
    hits: Cell<u64>,
    misses: Cell<u64>,
}

impl<B: MediaBackend> ProbeCache<B> {
    /// A cache over a backend.
    #[must_use]
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            entries: RefCell::new(HashMap::new()),
            hits: Cell::new(0),
            misses: Cell::new(0),
        }
    }

    /// The backend behind the cache, including its identity.
    #[must_use]
    pub const fn backend(&self) -> &B {
        &self.backend
    }

    /// Runs `read` over the probe of `path`, probing first if the answer is not already held.
    ///
    /// The closure form rather than a returned clone, because a variable-rate stream carries
    /// one timestamp per frame and a caller that only wants the container should not pay for
    /// copying a hundred thousand integers.
    ///
    /// # Panics
    ///
    /// If `read` calls back into the same cache. The borrow is held for the call, which is
    /// what makes it free; re-entering would be a second mutable borrow of one `RefCell`.
    pub fn with_probe<R>(
        &self,
        path: &Path,
        read: impl FnOnce(&Result<MediaProbe, MediaError>) -> R,
    ) -> R {
        let stamp = FileStamp::of(path);
        let fresh = {
            let entries = self.entries.borrow();
            entries
                .get(path)
                .is_some_and(|entry| entry.stamp == stamp && stamp.is_some())
        };
        if fresh {
            self.hits.set(self.hits.get() + 1);
        } else {
            self.misses.set(self.misses.get() + 1);
            let result = match stamp {
                Some(_) => self.backend.probe(path),
                // No stat, no file. Not cached as fresh: a file that appears later must be
                // noticed, and an absent one costs a stat to re-ask, which is the budget.
                None => Err(MediaError::Io(format!(
                    "{} is not a readable file",
                    path.display()
                ))),
            };
            self.entries
                .borrow_mut()
                .insert(path.to_path_buf(), Entry { stamp, result });
        }
        let entries = self.entries.borrow();
        let entry = entries
            .get(path)
            .expect("the entry was just inserted or found fresh");
        read(&entry.result)
    }

    /// What kind of container a file is, or why there is no answer.
    ///
    /// The cheap question, and the one the renderer's `kind` asks: the container is `Copy`, so
    /// this costs a stat and a hash lookup once the file has been probed.
    ///
    /// # Errors
    ///
    /// The three conditions in [`MediaError`], which are the three a placeholder can report.
    pub fn container(&self, path: &Path) -> Result<ContainerKind, MediaError> {
        self.with_probe(path, |result| match result {
            Ok(probe) => Ok(probe.container),
            Err(error) => Err(error.clone()),
        })
    }

    /// The whole probe, cloned.
    ///
    /// For a caller that needs to keep it. [`ProbeCache::with_probe`] is the one to reach for
    /// when the answer is only being looked at.
    ///
    /// # Errors
    ///
    /// The three conditions in [`MediaError`].
    pub fn probe(&self, path: &Path) -> Result<MediaProbe, MediaError> {
        self.with_probe(path, Clone::clone)
    }

    /// Forgets what is held about one path.
    ///
    /// For a caller that knows a file changed in a way a stat cannot see.
    pub fn invalidate(&self, path: &Path) {
        self.entries.borrow_mut().remove(path);
    }

    /// Forgets everything.
    pub fn clear(&self) {
        self.entries.borrow_mut().clear();
    }

    /// How many paths are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    /// Whether nothing is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }

    /// Questions answered from the cache.
    #[must_use]
    pub fn hits(&self) -> u64 {
        self.hits.get()
    }

    /// Questions that needed the backend.
    #[must_use]
    pub fn misses(&self) -> u64 {
        self.misses.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::NativeBackend;
    use crate::probe::DecoderIdentity;
    use std::cell::Cell as StdCell;

    /// A backend that counts how often it was asked, so caching is measured and not assumed.
    #[derive(Debug)]
    struct CountingBackend {
        identity: DecoderIdentity,
        calls: StdCell<u64>,
    }

    impl CountingBackend {
        fn new() -> Self {
            Self {
                identity: DecoderIdentity::new("counting", "0", "--none"),
                calls: StdCell::new(0),
            }
        }
    }

    impl MediaBackend for CountingBackend {
        fn identity(&self) -> &DecoderIdentity {
            &self.identity
        }

        fn probe(&self, path: &Path) -> Result<MediaProbe, MediaError> {
            self.calls.set(self.calls.get() + 1);
            NativeBackend::new().probe(path)
        }

        fn open_video(
            &self,
            _path: &Path,
            _stream: usize,
        ) -> Result<Box<dyn crate::decode::VideoReader>, MediaError> {
            Err(MediaError::Unsupported("counting backend".to_owned()))
        }

        fn hardware(&self) -> crate::probe::HardwareSupport {
            crate::probe::HardwareSupport::none()
        }
    }

    /// A readable binary Netpbm still of a given square size, header and body both.
    fn write_still(path: &Path, size: usize) {
        let mut bytes = format!("P6\n{size} {size}\n255\n").into_bytes();
        bytes.extend(std::iter::repeat_n(0_u8, size * size * 3));
        std::fs::write(path, &bytes).expect("write");
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maj-media-cache-{name}"));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_file_is_probed_once_however_often_it_is_asked_about() {
        let dir = temp_dir("probed-once");
        let path = dir.join("still.ppm");
        write_still(&path, 2);

        let cache = ProbeCache::new(CountingBackend::new());
        for _ in 0..5 {
            assert_eq!(cache.container(&path), Ok(ContainerKind::Ppm));
        }
        assert_eq!(
            cache.backend().calls.get(),
            1,
            "the backend was asked more than once about one unchanged file"
        );
        assert_eq!(cache.hits(), 4);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());
    }

    #[test]
    fn a_file_that_changes_is_probed_again() {
        let dir = temp_dir("changed");
        let path = dir.join("changing.ppm");
        write_still(&path, 2);

        let cache = ProbeCache::new(CountingBackend::new());
        assert_eq!(cache.container(&path), Ok(ContainerKind::Ppm));

        // A different length is a different file as far as a stat is concerned.
        write_still(&path, 4);
        let probe = cache.probe(&path).expect("still readable");
        assert_eq!(
            probe
                .primary_video()
                .map(|stream| stream.coded_size)
                .expect("a still is a picture"),
            (4, 4),
            "the cache answered about the file that used to be there"
        );
        assert_eq!(cache.backend().calls.get(), 2);
    }

    #[test]
    fn an_absent_file_is_offline_and_is_noticed_when_it_appears() {
        let dir = temp_dir("appears");
        let path = dir.join("late.ppm");
        let _ = std::fs::remove_file(&path);

        let cache = ProbeCache::new(CountingBackend::new());
        let missing = cache.container(&path).expect_err("not there");
        assert_eq!(missing.label(), "offline");
        assert_eq!(
            cache.backend().calls.get(),
            0,
            "an absent file costs a stat, never a read"
        );

        write_still(&path, 2);
        assert_eq!(
            cache.container(&path),
            Ok(ContainerKind::Ppm),
            "a file that appeared later was never looked at again"
        );
    }

    #[test]
    fn invalidating_forces_the_next_question_to_the_backend() {
        let dir = temp_dir("invalidate");
        let path = dir.join("still.ppm");
        write_still(&path, 2);

        let cache = ProbeCache::new(CountingBackend::new());
        assert!(cache.container(&path).is_ok());
        cache.invalidate(&path);
        assert!(cache.container(&path).is_ok());
        assert_eq!(cache.backend().calls.get(), 2);

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn a_damaged_file_is_told_apart_from_an_absent_one_without_a_decode() {
        let dir = temp_dir("damaged");
        let damaged = dir.join("damaged.ppm");
        let absent = dir.join("absent.ppm");
        // A Netpbm header that states a size and then stops.
        std::fs::write(&damaged, b"P6\n2 2\n").expect("write");
        let _ = std::fs::remove_file(&absent);

        let cache = ProbeCache::new(NativeBackend::new());
        assert_eq!(
            cache.container(&damaged).expect_err("damaged").label(),
            "damaged"
        );
        assert_eq!(
            cache.container(&absent).expect_err("absent").label(),
            "offline"
        );
    }
}

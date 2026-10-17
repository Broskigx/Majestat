//! Measuring a file, and finding it again by what it measured.
//!
//! This is the first real code in `maj-media`, and it is deliberately the part that needs
//! **no decoder**: hashing bytes is `std::fs` and `blake3`, and it is enough for the two
//! facts that make a project survivable — knowing which bytes an asset is, and finding those
//! bytes again after a drive letter changes.
//!
//! What it cannot do is read a container. `docs/PROJECT-FORMAT.md` and P1's schema three
//! record that as an explicit `None` rather than a zero: [`MediaFingerprint`] carries the
//! hash and the size, and the duration stays unknown until M2a's probe can fill it in. The
//! distinction is not pedantry — a duration of zero would be a measurement that never
//! happened, and every trim in the project is clamped against exactly that number.
//!
//! Three things about the error type are decisions rather than accidents:
//!
//! * It has the same three variants `maj_render::SourceError` has, because P0.3 settled that
//!   media that cannot be read is one condition with one policy and a decoder is the obvious
//!   place to reopen it by accident. This module cannot *produce* `Undecodable` — hashing
//!   requires no decoder — and says so rather than leaving a reader to wonder.
//! * A file that is not there is `Offline` and a file that cannot be read is `Damaged`.
//!   Neither is an I/O error escaping as a string: the caller has a policy to apply.
//! * The URI vocabulary is closed at the *asset*, not here. A procedural URI
//!   (`generated:`, `solid:`, `title:`) names no file at all, which is
//!   [`Measurement::NotAFile`] — not a failure, and not something to guess at.

use std::fmt;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use maj_core::{MediaAssetId, MediaFingerprint};

/// Media that could not be read, in the vocabulary P0.3 fixed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaError {
    /// The file is not where the project says it is.
    Offline(PathBuf),
    /// The file is there and could not be read, with the reason.
    Damaged(PathBuf, String),
    /// The bytes are readable and this build cannot make a picture out of them. A probe or
    /// a decoder produces this, never the measurement in this module.
    Undecodable(PathBuf, String),
}

impl fmt::Display for MediaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Offline(path) => write!(formatter, "`{}` is not there", path.display()),
            Self::Damaged(path, reason) => {
                write!(
                    formatter,
                    "`{}` could not be read: {reason}",
                    path.display()
                )
            }
            Self::Undecodable(path, reason) => {
                write!(
                    formatter,
                    "`{}` cannot be decoded: {reason}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for MediaError {}

/// What asking a URI to measure itself produced.
///
/// Three answers and no fourth, because the three questions are different:
/// *is there a file at all*, *what is in it*, and *why not*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Measurement {
    /// The bytes were read and hashed.
    Measured(MediaFingerprint),
    /// The URI names no file: it is generated, solid, a title, or a scheme this build does
    /// not read. Nothing is wrong with the project; there is nothing on disk to measure.
    NotAFile,
    /// There is a file and it could not be measured.
    Unavailable(MediaError),
}

/// The size a hash is computed in.
///
/// A megabyte at a time: large enough that a 100 GB card dump is not a syscall storm, small
/// enough that the buffer is not a memory claim on a machine that is also rendering.
const CHUNK: usize = 1 << 20;

/// Measures a file: its BLAKE3 hash and its length in bytes.
///
/// The duration is deliberately `None` — see the module documentation.
///
/// # Errors
///
/// Returns [`MediaError::Offline`] when the path is not a readable file and
/// [`MediaError::Damaged`] when it is there and the read failed part way through. A
/// directory is `Damaged` rather than `Offline`: it exists, and it is not a file.
pub fn measure(path: &Path) -> Result<MediaFingerprint, MediaError> {
    if !path.exists() {
        return Err(MediaError::Offline(path.to_path_buf()));
    }
    let file = std::fs::File::open(path)
        .map_err(|error| MediaError::Damaged(path.to_path_buf(), error.to_string()))?;
    let mut hasher = blake3::Hasher::new();
    let mut reader = BufReader::with_capacity(CHUNK, file);
    let mut buffer = vec![0_u8; CHUNK];
    let mut size = 0_u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| MediaError::Damaged(path.to_path_buf(), error.to_string()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    MediaFingerprint::bytes(hasher.finalize().to_hex().to_string(), size)
        .map_err(|error| MediaError::Damaged(path.to_path_buf(), error.to_string()))
}

/// Measures whatever a URI points at.
///
/// Never fails: a URI that names no file is [`Measurement::NotAFile`], and a file that could
/// not be measured is [`Measurement::Unavailable`], because both are answers a caller
/// reports rather than errors it propagates.
#[must_use]
pub fn measure_uri(uri: &str) -> Measurement {
    match url_to_path(uri) {
        None => Measurement::NotAFile,
        Some(path) => match measure(&path) {
            Ok(fingerprint) => Measurement::Measured(fingerprint),
            Err(error) => Measurement::Unavailable(error),
        },
    }
}

/// The path a `file:` URI names, or `None` when it names no file of this machine.
///
/// Percent-escapes are decoded, a leading `/` before a drive letter is dropped, and every
/// other scheme returns `None`. This is not an RFC 3986 parser and does not claim to be: it
/// is exactly as much as the model's closed URI vocabulary needs, and an absolute path with
/// a `#` in it is the thing it would get wrong.
#[must_use]
pub fn url_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(rest)?;
    // `file:///C:/media/a.mov` arrives as `/C:/media/a.mov`: the third slash is the path
    // separator, and the drive letter is not a directory.
    let trimmed = decoded
        .strip_prefix('/')
        .filter(|tail| {
            let mut characters = tail.chars();
            characters
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic())
                && characters.next() == Some(':')
        })
        .unwrap_or(&decoded);
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

/// The `file:` URI of a path, the inverse of [`url_to_path`].
///
/// The same limits as its inverse: the unreserved characters are left alone, and the ones
/// that would change the parse of the URI (`%`, `#`, `?`, the space) are escaped.
#[must_use]
pub fn uri_of(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut encoded = String::with_capacity(text.len() + 8);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                encoded.push(char::from(byte));
            }
            _ => {
                encoded.push('%');
                encoded.push_str(&format!("{byte:02X}"));
            }
        }
    }
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

/// Decodes `%xx` escapes, or `None` when one is malformed or the bytes are not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes.get(index + 1..index + 3)?;
            let value = u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
            out.push(value);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// An asset's identity and the size that lets a file be rejected without hashing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    /// Which asset is looking for its file.
    pub asset: MediaAssetId,
    /// The hash to match, lowercase hex as the document stores it.
    pub content_hash: String,
    /// The length the file must have. A file of another length cannot be this one, so it is
    /// never hashed — which is the whole reason the size is in the fingerprint.
    pub size_bytes: u64,
}

/// A file that matches one of the wanted identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Which asset it is the file of.
    pub asset: MediaAssetId,
    /// Where it is now.
    pub path: PathBuf,
}

/// What a search over a directory tree did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    /// The matches, ordered by asset so two runs over the same tree emit the same commands.
    pub found: Vec<Found>,
    /// Files whose size matched and whose hash was therefore computed.
    pub hashed: usize,
    /// Files that were skipped because their size matched nothing: the cheap rejection.
    pub skipped_by_size: usize,
    /// Directories that could not be listed, with the reason.
    pub unreadable: Vec<(PathBuf, String)>,
}

/// Finds, inside `root`, a file for each of `wanted`, by hash.
///
/// A file is hashed only when its length is one of the wanted lengths, so a card dump is
/// walked cheaply. The tree is visited depth first with each directory's entries sorted by
/// name, so the result does not depend on the order the filesystem happens to return — the
/// commands this produces go into a log, and a log whose order moves is not reproducible.
///
/// # Errors
///
/// Returns [`MediaError::Offline`] when `root` is not a directory. A directory *inside* the
/// tree that cannot be listed is recorded in [`Search::unreadable`] and the search carries
/// on: one locked folder should not cost the other nine hundred files their relink.
pub fn find_by_hash(root: &Path, wanted: &[Wanted]) -> Result<Search, MediaError> {
    if !root.is_dir() {
        return Err(MediaError::Offline(root.to_path_buf()));
    }
    let mut search = Search {
        found: Vec::new(),
        hashed: 0,
        skipped_by_size: 0,
        unreadable: Vec::new(),
    };
    if wanted.is_empty() {
        return Ok(search);
    }
    let sizes: Vec<u64> = wanted.iter().map(|entry| entry.size_bytes).collect();
    let mut pending: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                search.unreadable.push((directory, error.to_string()));
                continue;
            }
        };
        let mut paths: Vec<PathBuf> = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => paths.push(entry.path()),
                Err(error) => search
                    .unreadable
                    .push((directory.clone(), error.to_string())),
            }
        }
        paths.sort();
        for path in paths {
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let Ok(metadata) = path.metadata() else {
                continue;
            };
            if !sizes.contains(&metadata.len()) {
                search.skipped_by_size += 1;
                continue;
            }
            search.hashed += 1;
            let Ok(fingerprint) = measure(&path) else {
                continue;
            };
            for entry in wanted {
                if entry.content_hash == fingerprint.content_hash()
                    && entry.size_bytes == fingerprint.size_bytes()
                {
                    search.found.push(Found {
                        asset: entry.asset,
                        path: path.clone(),
                    });
                }
            }
        }
    }
    search.found.sort_by_key(|found| found.asset);
    search.found.dedup_by_key(|found| found.asset);
    Ok(search)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory nothing else in the suite is using.
    fn scratch(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "maj-media-{}-{}-{label}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&path).expect("a scratch directory");
        path
    }

    #[test]
    fn a_measured_file_knows_its_hash_and_its_length_and_not_its_duration() {
        let directory = scratch("measure");
        let path = directory.join("a.bin");
        std::fs::write(&path, b"majestat").unwrap();

        let measured = measure(&path).expect("a file that is there");
        assert_eq!(
            measured.content_hash(),
            blake3::hash(b"majestat").to_hex().to_string(),
            "the hash is of the bytes and nothing else"
        );
        assert_eq!(measured.size_bytes(), 8);
        assert_eq!(
            measured.duration_frames(),
            None,
            "hashing needs no decoder, so nothing here can claim a duration"
        );

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_file_that_is_not_there_is_offline_and_a_uri_that_names_no_file_is_neither() {
        let directory = scratch("missing");
        let absent = directory.join("gone.bin");
        assert_eq!(
            measure(&absent),
            Err(MediaError::Offline(absent.clone())),
            "the project can be told to wait for a drive that is not mounted yet"
        );
        assert!(
            matches!(measure(&directory), Err(MediaError::Damaged(path, _)) if path == directory),
            "a directory is damaged rather than offline, because it is there"
        );

        assert_eq!(measure_uri("generated:shot-01"), Measurement::NotAFile);
        assert_eq!(
            measure_uri("title:serif:23:ffffff:centre:MAJESTAT"),
            Measurement::NotAFile
        );
        assert_eq!(measure_uri("solid:0,0,0"), Measurement::NotAFile);
        assert!(matches!(
            measure_uri(&uri_of(&absent)),
            Measurement::Unavailable(MediaError::Offline(_))
        ));

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_uri_round_trips_through_a_path() {
        let path = PathBuf::from("C:/media/two words/take 1.mov");
        let uri = uri_of(&path);
        assert_eq!(uri, "file:///C:/media/two%20words/take%201.mov");
        assert_eq!(url_to_path(&uri), Some(path));
        assert_eq!(
            url_to_path("file:///home/editor/a.mov"),
            Some(PathBuf::from("/home/editor/a.mov"))
        );
        assert_eq!(url_to_path("http://example.com/a.mov"), None);
        assert_eq!(url_to_path("generated:shot"), None);
    }

    #[test]
    fn a_file_of_the_right_size_and_the_wrong_bytes_is_not_a_match() {
        // The size is in the fingerprint to reject a file without hashing it. This is that
        // claim measured: the decoy has the same length as the asset and different bytes,
        // so it is hashed and then refused, which is the only way the size can be trusted
        // as a filter rather than as an identity.
        let directory = scratch("search");
        let real = directory.join("real.bin");
        let decoy = directory.join("nested").join("decoy.bin");
        std::fs::create_dir_all(decoy.parent().unwrap()).unwrap();
        std::fs::write(&real, b"AAAAAAAA").unwrap();
        std::fs::write(&decoy, b"BBBBBBBB").unwrap();
        std::fs::write(directory.join("other.bin"), b"tiny").unwrap();

        let wanted = vec![Wanted {
            asset: MediaAssetId::new(7),
            content_hash: blake3::hash(b"AAAAAAAA").to_hex().to_string(),
            size_bytes: 8,
        }];
        let search = find_by_hash(&directory, &wanted).expect("a directory that is there");
        assert_eq!(
            search.found,
            vec![Found {
                asset: MediaAssetId::new(7),
                path: real.clone()
            }]
        );
        assert_eq!(
            search.hashed, 2,
            "the decoy shares the length, so it is hashed"
        );
        assert_eq!(
            search.skipped_by_size, 1,
            "`other.bin` is the wrong length, so it is never read"
        );
        assert!(search.unreadable.is_empty());

        // And nothing to look for is not an error, and finds nothing.
        let empty = find_by_hash(&directory, &[]).expect("a directory that is there");
        assert!(empty.found.is_empty());
        assert_eq!(
            find_by_hash(&directory.join("nope"), &wanted),
            Err(MediaError::Offline(directory.join("nope")))
        );

        std::fs::remove_dir_all(&directory).ok();
    }
}

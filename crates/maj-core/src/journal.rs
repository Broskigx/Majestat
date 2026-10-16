//! The autosave journal: the work that is not in the document yet.
//!
//! # Why this is a sidecar and not part of the format
//!
//! The document already *is* a log, so it is tempting to make saving continuous and be done:
//! apply a command, write the document, and there is nothing to recover because there is
//! nothing unsaved. Two things say no. Writing the whole document on every edit is a full
//! serialisation per keystroke, and — the one that matters — the document's log carries the
//! *grouping* of edits, which P0.2 deliberately kept as session state. A journal that
//! recorded groups into the document would make "which commands were one undo step" part of
//! the record, and a reopened project would then undo differently from the session that
//! wrote it. So the journal is a sidecar: it holds groups, it is written where the editing
//! happens, and **the document never depends on it**.
//!
//! # What a recovered project is
//!
//! The same project the session would have saved. That is checkable rather than plausible,
//! and it is the gate: recovering a journal and replaying the same commands cleanly must
//! give the same `document_hash`, because the hash covers the log and the log records who
//! made each edit. That is why a journal entry stores its author — a copy that recovered
//! every command but attributed them to nobody would be a different document.
//!
//! # Reading is a prefix, and it says where it stopped
//!
//! Each entry is one JSON line, appended in order, and each records **the document hash it
//! was applied against**. Two kinds of damage are therefore distinguishable and are treated
//! differently:
//!
//! * A line that cannot be read ends the journal: the entries before it are applied, and
//!   [`Recovery::unreadable_from`] names the line. A half-written line is the real shape of a
//!   crash — the process died mid-`write` — and it is recovered *up to* the damage rather
//!   than thrown away whole. It is also what happens when a crash leaves a partial line and
//!   the next session appends after it, so the damage is not assumed to be at the end:
//!   [`Recovery::torn_tail`] says whether it looked like a cut-off write or something later.
//! * An entry whose `base` is not the hash of the project as it now stands is **skipped**,
//!   never applied. That is the case where the document was saved after some of these edits:
//!   applying them again would double the work. A journal written against an older revision
//!   is skipped forward to the first entry that does chain, and one whose chain breaks after
//!   some entries were applied stops there, with [`Recovery::stale_from`] naming the entry.
//!
//! Neither case is silent. A recovery that applied nothing says so, and says why.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::command::Command;
use crate::error::CoreError;
use crate::project::Project;

/// Extension appended to a document's path for its journal.
pub const JOURNAL_EXTENSION: &str = "journal";

/// The journal that belongs beside a document: `cut.maj` → `cut.maj.journal`.
#[must_use]
pub fn journal_path_for(document: impl AsRef<Path>) -> PathBuf {
    let mut path = document.as_ref().as_os_str().to_owned();
    path.push(format!(".{JOURNAL_EXTENSION}"));
    PathBuf::from(path)
}

/// One group of commands, as it was applied, with what it was applied to.
// Not `Eq`: a command carries parameter values, and a parameter value can be a float.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalEntry {
    /// The document hash this group was applied against.
    ///
    /// Not a revision number, because two different projects can both be at revision seven
    /// and a revision does not say *which* history it is at.
    base: String,
    /// Who made the edit, in the log's own vocabulary. Stored so that a recovered document
    /// attributes the work to the same hand.
    author: String,
    /// The commands of one group, in the order they were applied.
    commands: Vec<Command>,
}

impl JournalEntry {
    /// Records a group against the hash of the document it is about to be applied to.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidArgument`] for an empty command list or an empty base or
    /// author: a journal line that records nothing is noise a recovery would then have to
    /// reason about, and an entry with no author would recover into a different document.
    pub fn new(
        base: impl Into<String>,
        author: impl Into<String>,
        commands: Vec<Command>,
    ) -> Result<Self, CoreError> {
        let base = base.into();
        let author = author.into();
        if base.trim().is_empty() {
            return Err(CoreError::invalid(
                "a journal entry needs the document hash it was written against",
            ));
        }
        if author.trim().is_empty() {
            return Err(CoreError::invalid("a journal entry needs an author"));
        }
        if commands.is_empty() {
            return Err(CoreError::invalid(
                "a journal entry records a group, and a group is never empty",
            ));
        }
        Ok(Self {
            base,
            author,
            commands,
        })
    }

    /// The document hash this group was applied against.
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Who made the edit.
    #[must_use]
    pub fn author(&self) -> &str {
        &self.author
    }

    /// The commands of the group, in order.
    #[must_use]
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }
}

/// Every way the journal can fail.
#[derive(Debug)]
pub enum JournalError {
    /// The journal could not be read or written.
    Io {
        /// The file.
        path: PathBuf,
        /// What the filesystem said.
        reason: String,
    },
    /// The model refused a command while applying an entry that did chain.
    ///
    /// A line that cannot be *read* is not this: it ends the readable prefix and is
    /// reported in the [`Recovery`] instead, because a crash is not a refusal.
    Refused {
        /// One-based line number of the entry.
        line: usize,
        /// The model's own refusal.
        error: CoreError,
    },
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, reason } => write!(formatter, "`{}`: {reason}", path.display()),
            Self::Refused { line, error } => {
                write!(
                    formatter,
                    "journal line {line} was refused by the model: {error}"
                )
            }
        }
    }
}

impl std::error::Error for JournalError {}

/// Where a line that cannot be read sits, why, and whether it looks like a torn write.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Damage {
    /// One-based line number.
    line: usize,
    /// Why it could not be read.
    reason: String,
    /// Whether it is the last line and the file does not end with a newline: a write that
    /// was cut off, which is the crash this journal exists for.
    torn: bool,
}

/// What a recovery did, in full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    /// The journal's path.
    pub path: PathBuf,
    /// Revision of the document before anything was applied.
    pub revision_before: u64,
    /// Revision after.
    pub revision_after: u64,
    /// The document hash after, which is the recovery's whole claim.
    pub document_hash: String,
    /// Entries applied.
    pub applied: usize,
    /// Commands applied, across those entries.
    pub commands: usize,
    /// Entries skipped because they were written against another revision.
    pub skipped: usize,
    /// One-based line number of the first line that could not be read, if any.
    pub unreadable_from: Option<usize>,
    /// Why that line could not be read.
    pub unreadable_reason: Option<String>,
    /// Whether that line is a torn write: last in the file, with no newline after it.
    pub torn_tail: bool,
    /// One-based line number of the first entry whose chain broke after some had been
    /// applied, if any.
    pub stale_from: Option<usize>,
}

impl Recovery {
    /// Whether the journal had nothing to contribute.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.applied == 0
    }
}

/// Appends one group to a journal, creating it if it is not there.
///
/// One line per group, written and flushed before returning, because the point of the file
/// is to survive the process dying immediately afterwards.
///
/// # Errors
///
/// Returns [`JournalError::Io`] when the file cannot be written.
pub fn append(path: &Path, entry: &JournalEntry) -> Result<(), JournalError> {
    use std::io::Write as _;
    let line = serde_json::to_string(entry).map_err(|error| JournalError::Io {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| JournalError::Io {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
    file.write_all(line.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.flush())
        .map_err(|error| JournalError::Io {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })
}

/// Reads the readable prefix of a journal, and where it stopped.
///
/// A prefix rather than the whole file, and a report rather than a silent truncation: the
/// caller can tell "the journal is empty" from "the journal stopped at line three".
fn read_prefix(text: &str) -> (Vec<JournalEntry>, Option<Damage>) {
    let ends_with_newline = text.ends_with('\n');
    let lines: Vec<&str> = text.lines().collect();
    let mut entries = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let number = index + 1;
        let torn = number == lines.len() && !ends_with_newline;
        if line.trim().is_empty() {
            // A blank line cannot be a group: it is the tail of a torn write, or the
            // separator a crash left between two of its own appends.
            return (
                entries,
                Some(Damage {
                    line: number,
                    reason: "the line is empty".to_owned(),
                    torn,
                }),
            );
        }
        match serde_json::from_str::<JournalEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(error) => {
                return (
                    entries,
                    Some(Damage {
                        line: number,
                        reason: error.to_string(),
                        torn,
                    }),
                );
            }
        }
    }
    (entries, None)
}

/// Replays a journal into a project, applying the entries that still belong to it.
///
/// The project is left untouched by an entry that cannot be read or that was written
/// against another revision; see the module documentation for the two rules and why they
/// differ. Every applied group goes through [`crate::History::apply_group`], so one entry is
/// one undo step and an entry the model refuses rolls back as a unit without reaching the
/// project.
///
/// # Errors
///
/// Returns [`JournalError::Io`] when the journal cannot be read and
/// [`JournalError::Refused`] when the model refuses a command in an entry that *did* chain.
pub fn recover(project: &mut Project, path: &Path) -> Result<Recovery, JournalError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        // A document with no journal beside it is the ordinary case, not a failure.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(JournalError::Io {
                path: path.to_path_buf(),
                reason: error.to_string(),
            });
        }
    };
    let (entries, damage) = read_prefix(&text);
    let revision_before = project.revision();
    let mut history = crate::History::new();
    let mut applied = 0_usize;
    let mut commands = 0_usize;
    let mut skipped = 0_usize;
    let mut stale_from = None;
    for (index, entry) in entries.iter().enumerate() {
        let hash = project
            .document_hash()
            .map_err(|error| JournalError::Refused {
                line: index + 1,
                error,
            })?;
        if entry.base() != hash {
            if applied == 0 {
                // Written against an older revision: the document has been saved since, so
                // these groups are already in it. Skipping forward is the only reading that
                // cannot double an edit.
                skipped += 1;
                continue;
            }
            stale_from = Some(index + 1);
            break;
        }
        history
            .apply_group(project, entry.commands().to_vec(), entry.author())
            .map_err(|error| JournalError::Refused {
                line: index + 1,
                error,
            })?;
        applied += 1;
        commands += entry.commands().len();
    }
    let document_hash = project
        .document_hash()
        .map_err(|error| JournalError::Refused {
            line: entries.len(),
            error,
        })?;
    Ok(Recovery {
        path: path.to_path_buf(),
        revision_before,
        revision_after: project.revision(),
        document_hash,
        applied,
        commands,
        skipped,
        unreadable_from: damage.as_ref().map(|damage| damage.line),
        unreadable_reason: damage.as_ref().map(|damage| damage.reason.clone()),
        torn_tail: damage.as_ref().is_some_and(|damage| damage.torn),
        stale_from,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MediaAsset, MediaAssetId, Sequence, SequenceId};
    use maj_time::Timebase;

    /// The author the journal entries in these tests are written by.
    const AUTHOR: &str = "ui:local";

    /// A scratch document path nothing else in the suite is using.
    fn scratch(label: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "maj-journal-{}-{}-{label}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        directory.join("cut.maj")
    }

    /// A project with one asset and one sequence: revision two.
    fn fixture() -> Project {
        let asset = MediaAsset::new(
            MediaAssetId::new(1),
            "shot.mov",
            "file:///media/shot.mov",
            Timebase::FPS_24,
            240,
        )
        .unwrap();
        let sequence = Sequence::new(
            SequenceId::new(2),
            "Main",
            Timebase::FPS_24,
            1920,
            1080,
            48_000,
        )
        .unwrap();
        let mut project = Project::new("cut");
        let mut history = crate::History::new();
        history
            .apply_group(
                &mut project,
                vec![
                    Command::CreateMediaAsset { index: 0, asset },
                    Command::CreateSequence { index: 0, sequence },
                ],
                AUTHOR,
            )
            .unwrap();
        project
    }

    /// A group of commands that a journal can record.
    fn group(frames: i64) -> Vec<Command> {
        vec![Command::SetSequenceStartTimecode {
            sequence: SequenceId::new(2),
            frames,
        }]
    }

    /// Applies a group to a project, as the session would.
    fn apply(project: &mut Project, commands: Vec<Command>) {
        let mut history = crate::History::new();
        history.apply_group(project, commands, AUTHOR).unwrap();
    }

    /// Records a group against a project's current hash, applies it, and returns the project.
    fn record(project: &mut Project, journal: &Path, commands: Vec<Command>) {
        let entry =
            JournalEntry::new(project.document_hash().unwrap(), AUTHOR, commands.clone()).unwrap();
        append(journal, &entry).expect("a writable journal");
        apply(project, commands);
    }

    #[test]
    fn a_recovered_journal_reproduces_the_same_document_hash_as_a_clean_run() {
        // The gate P1 was written for: kill the session, replay the journal, and get the
        // document the session would have saved — not a similar one. The hash covers the log
        // and the log records who made each edit, so this also proves the author was carried
        // into the recovery rather than defaulted.
        let document = scratch("recover");
        let journal = journal_path_for(&document);
        let mut session = fixture();
        crate::format::save_to_path(&session, &document).unwrap();

        record(&mut session, &journal, group(86_400));
        record(
            &mut session,
            &journal,
            vec![Command::RelinkAsset {
                asset: MediaAssetId::new(1),
                uri: "file:///online/shot.mov".to_owned(),
            }],
        );

        // The same two groups, applied cleanly to a project that was never interrupted.
        let mut clean = fixture();
        apply(&mut clean, group(86_400));
        apply(
            &mut clean,
            vec![Command::RelinkAsset {
                asset: MediaAssetId::new(1),
                uri: "file:///online/shot.mov".to_owned(),
            }],
        );

        // Nothing was saved: the document on disk is still the one the session started from.
        let mut reopened = crate::format::load_from_path(&document).unwrap();
        assert_eq!(reopened.revision(), 2);
        let recovery = recover(&mut reopened, &journal).expect("a journal that reads");
        assert_eq!(recovery.applied, 2);
        assert_eq!(recovery.commands, 2);
        assert_eq!(recovery.skipped, 0);
        assert!(recovery.unreadable_from.is_none());
        assert!(recovery.unreadable_reason.is_none());
        assert!(!recovery.torn_tail);
        assert_eq!(recovery.revision_before, 2);
        assert_eq!(recovery.revision_after, 4);
        assert_eq!(recovery.document_hash, session.document_hash().unwrap());
        assert_eq!(
            recovery.document_hash,
            clean.document_hash().unwrap(),
            "the recovered document is the document, not something like it"
        );

        std::fs::remove_dir_all(document.parent().unwrap()).ok();
    }

    #[test]
    fn a_torn_last_line_costs_that_line_and_not_the_journal() {
        // What a `kill -9` in the middle of a write actually leaves behind.
        let document = scratch("torn");
        let journal = journal_path_for(&document);
        let mut session = fixture();
        record(&mut session, &journal, group(48_000));
        record(&mut session, &journal, group(72_000));

        let text = std::fs::read_to_string(&journal).unwrap();
        // Cut the last line in half, which is what a write that was interrupted leaves.
        let cut = text.len() - 25;
        std::fs::write(&journal, &text.as_bytes()[..cut]).unwrap();

        let mut only_one = fixture();
        apply(&mut only_one, group(48_000));

        // The project the session started from, which is what the first entry chains onto.
        let mut reopened = fixture();
        let recovery = recover(&mut reopened, &journal).expect("the readable prefix is applied");
        assert_eq!(recovery.applied, 1, "the first group survived the crash");
        assert_eq!(
            recovery.unreadable_from,
            Some(2),
            "and the report names the line that did not"
        );
        assert!(recovery.unreadable_reason.is_some());
        assert!(
            recovery.torn_tail,
            "the file does not end with a newline, which is what a cut-off write looks like"
        );
        assert_eq!(
            recovery.document_hash,
            only_one.document_hash().unwrap(),
            "what was recovered is exactly the readable prefix"
        );

        std::fs::remove_dir_all(document.parent().unwrap()).ok();
    }

    #[test]
    fn a_journal_written_against_another_revision_is_skipped_and_not_applied() {
        // The case that would double an edit: the work was saved after these groups were
        // written, so the document already contains them.
        let document = scratch("stale");
        let journal = journal_path_for(&document);
        let mut session = fixture();
        record(&mut session, &journal, group(12_000));
        record(&mut session, &journal, group(24_000));

        // Saved after the first group: that entry is already in the document, the second is
        // not, and only the second has a base that still matches.
        let mut saved = fixture();
        apply(&mut saved, group(12_000));
        let mut reopened = saved.clone();
        let recovery = recover(&mut reopened, &journal).expect("a journal that reads");
        assert_eq!(
            recovery.skipped, 1,
            "the group already in the document is skipped"
        );
        assert_eq!(recovery.applied, 1, "and the one that is not is applied");
        assert_eq!(
            recovery.revision_after, 4,
            "three from before the journal, plus the one entry that was applied"
        );
        assert!(recovery.stale_from.is_none(), "the chain never broke");
        assert_eq!(
            recovery.document_hash,
            session.document_hash().unwrap(),
            "and the result is still the session's document"
        );

        std::fs::remove_dir_all(document.parent().unwrap()).ok();
    }

    #[test]
    fn a_journal_whose_chain_breaks_after_some_entries_stops_and_says_where() {
        // A stale middle: nothing after the break is applied, because an entry that does not
        // follow from the one before it cannot be trusted to be a continuation.
        let document = scratch("break");
        let journal = journal_path_for(&document);
        let mut session = fixture();
        record(&mut session, &journal, group(1_000));
        record(&mut session, &journal, group(2_000));

        let text = std::fs::read_to_string(&journal).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        // Rewrite the second entry against a hash that is not the one it followed.
        let mut entry: JournalEntry = serde_json::from_str(lines[1]).unwrap();
        entry.base = "0000".to_owned();
        std::fs::write(
            &journal,
            format!("{}\n{}\n", lines[0], serde_json::to_string(&entry).unwrap()),
        )
        .unwrap();

        let mut reopened = fixture();
        let recovery = recover(&mut reopened, &journal).expect("a journal that reads");
        assert_eq!(recovery.applied, 1);
        assert_eq!(recovery.stale_from, Some(2));
        assert_eq!(recovery.revision_after, 3);

        std::fs::remove_dir_all(document.parent().unwrap()).ok();
    }

    #[test]
    fn a_journal_that_is_not_json_is_read_as_nothing_rather_than_applied() {
        let document = scratch("corrupt");
        let journal = journal_path_for(&document);
        std::fs::write(&journal, "not a journal at all\n").unwrap();
        let mut project = fixture();
        let recovery = recover(&mut project, &journal).expect("reading does not fail the caller");
        assert_eq!(recovery.applied, 0);
        assert!(recovery.is_empty());
        assert_eq!(recovery.unreadable_from, Some(1));
        assert!(
            !recovery.torn_tail,
            "it ends with a newline, so nothing was cut off"
        );
        assert_eq!(
            recovery.revision_after, recovery.revision_before,
            "nothing was applied, so the revision did not move"
        );

        std::fs::remove_dir_all(document.parent().unwrap()).ok();
    }

    #[test]
    fn a_missing_journal_is_not_a_failure() {
        let document = scratch("absent");
        let mut project = fixture();
        let recovery = recover(&mut project, &journal_path_for(&document))
            .expect("nothing to recover is fine");
        assert!(recovery.is_empty());
        assert!(recovery.unreadable_from.is_none());
        assert_eq!(recovery.document_hash, project.document_hash().unwrap());

        std::fs::remove_dir_all(document.parent().unwrap()).ok();
    }

    #[test]
    fn an_entry_needs_a_hash_an_author_and_at_least_one_command() {
        assert!(JournalEntry::new("", AUTHOR, group(0)).is_err());
        assert!(JournalEntry::new("   ", AUTHOR, group(0)).is_err());
        assert!(JournalEntry::new("abc", "", group(0)).is_err());
        assert!(JournalEntry::new("abc", "  ", group(0)).is_err());
        assert!(JournalEntry::new("abc", AUTHOR, Vec::new()).is_err());
        let entry = JournalEntry::new("abc", AUTHOR, group(7)).expect("a well-formed entry");
        assert_eq!(entry.base(), "abc");
        assert_eq!(entry.author(), AUTHOR);
        assert_eq!(entry.commands().len(), 1);
    }

    #[test]
    fn the_journal_beside_a_document_is_named_after_it() {
        assert_eq!(
            journal_path_for("C:/media/cut.maj"),
            PathBuf::from("C:/media/cut.maj.journal")
        );
    }
}

//! # maj-core — layer 1
//!
//! The authoritative project model, the command log, undo and redo, and the `.maj`
//! document format.
//!
//! ## The one idea this crate implements
//!
//! **Everything is a command.** [`Command::apply`] mutates a [`Project`] *and returns
//! the command that undoes it*. The log of applied commands is the document. From that
//! single arrangement, without any extra machinery, the project gets:
//!
//! * exact undo and redo, including the position of a restored clip inside its track;
//! * a state that is a pure function of the log, so it can be replayed and verified;
//! * a diffable, reviewable, attributable record of an edit;
//! * the same command surface that the AI agent (M11), remote collaborators (M12) and
//!   the headless CLI (M8) will drive. They do not need a separate API.
//!
//! ## Layering
//!
//! `maj-core` depends only on [`maj_time`]. It performs no I/O of its own beyond
//! reading and writing a project file, holds no GPU or codec types, and never blocks.
//! Rendering, decoding, transport and interface all sit above it.
//!
//! ## Constructing a project
//!
//! ```
//! use maj_core::{Command, History, MediaAsset, MediaAssetId, Project};
//! use maj_time::Timebase;
//!
//! let mut project = Project::new("demo");
//! let mut history = History::new();
//!
//! let asset = MediaAsset::new(
//!     MediaAssetId::new(1),
//!     "shot-01.mov",
//!     "file:///media/shot-01.mov",
//!     Timebase::FPS_23_976,
//!     2_400,
//! )
//! .unwrap();
//!
//! history
//!     .apply_local(
//!         &mut project,
//!         Command::CreateMediaAsset { index: 0, asset },
//!     )
//!     .unwrap();
//! assert_eq!(project.revision(), 1);
//!
//! history.undo(&mut project).unwrap();
//! assert_eq!(project.revision(), 0);
//! assert_eq!(project.assets().len(), 0);
//! ```
//!
//! ## Modules
//!
//! - [`Project`] — the durable model and its invariants.
//! - [`Command`] / [`LogEntry`] — the log format.
//! - [`History`] — the session undo and redo stacks.
//! - [`IdAllocator`] — runtime identifier allocation, deliberately not persisted.
//! - [`determinism`] — canonical serialisation and content hashing.
//! - [`mod@format`] — reading and writing `.maj`.
//! - [`mod@journal`] — the autosave sidecar: the work that is not in the document yet.
//! - [`CoreError`] — every way this layer can fail.

mod command;
pub mod composite;
mod entity;
mod error;
mod ids;
mod project;
mod undo;

pub mod determinism;
pub mod format;
pub mod journal;

pub use command::{Command, DEFAULT_AUTHOR, LogEntry};
pub use composite::{
    AnimChannel, BlendMode, ChannelAnimation, EffectInstance, EffectKind, Interpolation, Keyframe,
    Mask, MaskShape, Param, ParamSpec, ParamValue, Rgba, Transform, Transition, TransitionKind,
};
pub use entity::{
    Clip, Marker, MarkerColour, MediaAsset, MediaFingerprint, Sequence, Track, TrackKind,
};
pub use error::CoreError;
pub use ids::{
    ClipId, EffectId, IdAllocator, MarkerId, MaskId, MediaAssetId, SequenceId, TrackId,
    TransitionId,
};
pub use journal::{JOURNAL_EXTENSION, JournalEntry, JournalError, Recovery, journal_path_for};
pub use project::{CURRENT_SCHEMA_VERSION, MAJ_MAGIC, Project};
// The time types the model already hands back and accepts: re-exported so a caller can name
// a `FrameRange` it was given, or build the `Timebase` a sequence needs, without reaching
// into a lower layer for it.
pub use maj_time::{FrameRange, Rational, RetimeMap, Timebase};
pub use undo::History;

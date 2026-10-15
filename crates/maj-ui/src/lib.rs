//! `maj-ui` — the shell over the command log: state, intentions, and the keys that express
//! them.
//!
//! **Layer 6.** See `docs/ARCHITECTURE.md` for the layering rule and `docs/UI.md` for the
//! interface itself.
//!
//! # What this crate is, and what it is not
//!
//! This crate holds the part of the interface that does not draw: the session the shell
//! edits, the closed set of intentions a person can express, the table that maps a keystroke
//! to one of them, the preferences that live beside the document rather than inside it, and
//! the two languages it speaks.
//!
//! It does **not** draw, and it does not talk to a window system. That split is the point.
//! Everything with a guarantee lives here and is tested without a screen: that an intention
//! becomes exactly one command, that the command it becomes is the command the engine
//! already defines, that two interfaces driving the same edits produce the same document,
//! and that no string is missing from either language. The widgets are then a thin layer that
//! reads this state and pushes [`Action`]s into it.
//!
//! Three rules are inherited from the engine and are not negotiable here:
//!
//! * **The interface never edits the model directly.** A person's action becomes a
//!   [`Command`](maj_core::Command) and is applied through the same session the Model
//!   Context Protocol server uses, with a different author tag. There is one door.
//! * **Nothing is invisible.** A preview drawn at a quarter resolution says so, a panel whose
//!   subsystem does not exist yet says which phase owns it, and a refusal is shown rather
//!   than swallowed.
//! * **No panic is a control path.** Every failure is a [`UiError`] that the shell displays.
//!
//! # The session
//!
//! The shell holds a [`maj_mcp::Session`], not a bare project: the same type the agent's
//! server holds. An edit from a person and an edit from a model differ only in the author
//! they record, which is what makes "review what the agent did" a real capability rather
//! than a promise.

#![forbid(unsafe_code)]

pub mod action;
pub mod app;
pub mod i18n;
pub mod keys;
pub mod prefs;
pub mod view;

pub use action::{Action, Context, Draft, KINDS, Panel, Panels, TransformField, kind_of};
pub use app::{
    AUTHOR, NO_FURTHER_CLIP, NO_FURTHER_EDIT, NO_FURTHER_KEYFRAME, NOTHING_ANIMATED, Note,
    NoteKind, Palette, Preview, PreviewKey, Report, SESSION, Shell,
};
pub use i18n::{Key, LANGS, Lang, text};
pub use keys::{Binding, Direction, Press, Stroke};
pub use prefs::Prefs;
// The animated channels are part of the interface's vocabulary: a diamond in an inspector stands
// for one, and a presenter drawing that diamond should be naming the same six the model does
// rather than keeping a list of its own.
pub use maj_core::AnimChannel;
// The rational clock the shell already speaks: re-exported so a presenter can print a
// timecode without taking a dependency of its own on the time layer.
pub use maj_time::{Timecode, TimecodeMode};
pub use view::{PAGES, Page, SPLIT_RANGE, Split, View};

/// What went wrong on the interface's side of the command model.
///
/// Deliberately two cases. Either an intention does not apply to the state it arrived in —
/// trimming nothing, splitting where there is no cut — or the engine refused the command,
/// in which case its own message is carried through rather than replaced by something
/// friendlier and less true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiError {
    /// The intention does not apply: the message names what was missing.
    NotApplicable(&'static str),
    /// The engine refused the command, with its own explanation attached.
    Refused(String),
}

impl std::fmt::Display for UiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotApplicable(what) => write!(formatter, "{what}"),
            Self::Refused(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for UiError {}

impl UiError {
    /// The refusal that means "there is no active sequence to act on".
    #[must_use]
    pub const fn no_sequence() -> Self {
        Self::NotApplicable("there is no sequence to act on")
    }

    /// The refusal that means "nothing is selected".
    #[must_use]
    pub const fn no_selection() -> Self {
        Self::NotApplicable("nothing is selected")
    }
}

/// Human-readable statement of this crate's responsibility.
pub const ROLE: &str = "shell, timeline, viewer, panels, command palette";

/// Layer in the Majestat architecture, from `docs/ARCHITECTURE.md`.
pub const LAYER: u8 = 6;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_its_role_and_layer() {
        assert!(!ROLE.is_empty(), "every crate must declare its role");
        assert_eq!(LAYER, 6);
    }

    #[test]
    fn errors_display_their_own_message() {
        assert_eq!(
            UiError::no_sequence().to_string(),
            "there is no sequence to act on"
        );
        assert_eq!(
            UiError::Refused("speed must be greater than zero".to_owned()).to_string(),
            "speed must be greater than zero"
        );
    }
}

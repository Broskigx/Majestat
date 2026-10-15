use core::fmt;

use serde::{Deserialize, Serialize};

use crate::project::Project;

macro_rules! define_entity_id {
    ($name:ident, $label:literal, $slug:literal) => {
        #[doc = concat!("Typed identifier for a ", $label, ".")]
        ///
        /// Identifiers appear in the command log, so a command replayed years later
        /// still resolves to the same entity. They are allocated by [`IdAllocator`]
        /// and, in a shared log, never reused.
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            #[doc = concat!("Wraps a raw identifier for a ", $label, ".")]
            #[must_use]
            pub const fn new(raw: u64) -> Self {
                Self(raw)
            }

            /// Returns the raw identifier, for logs and diagnostics.
            #[must_use]
            pub const fn raw(self) -> u64 {
                self.0
            }

            #[doc = concat!("Human-readable name of this entity kind, used in errors.")]
            pub const KIND: &'static str = $slug;
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}#{}", $slug, self.0)
            }
        }
    };
}

define_entity_id!(MediaAssetId, "media asset", "asset");
define_entity_id!(SequenceId, "sequence", "sequence");
define_entity_id!(TrackId, "track", "track");
define_entity_id!(ClipId, "clip", "clip");
define_entity_id!(EffectId, "effect", "effect");
define_entity_id!(MaskId, "mask", "mask");
define_entity_id!(TransitionId, "transition", "transition");

/// Hands out fresh identifiers for entities about to be created.
///
/// # Why this is not stored in the project document
///
/// An allocator in the document is a determinism trap. Create five entities, undo
/// two, and a stored counter no longer matches anything derivable from the log, so
/// replaying the same log produces a different counter — and a different project
/// hash. Majestat therefore derives the next free identifier from the identifiers
/// actually in use ([`Project::next_unused_id`]) and keeps this type as a runtime
/// helper only.
///
/// A consequence worth stating: locally undoing a creation frees that identifier for
/// reuse. That is safe because the entity is gone. Collaborative undo (M12) appends
/// an inverse command instead of truncating the log, so shared histories never
/// reclaim an identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdAllocator {
    next: u64,
}

impl IdAllocator {
    /// Starts allocating from `next`.
    #[must_use]
    pub const fn starting_from(next: u64) -> Self {
        Self { next }
    }

    /// Starts allocating from the first identifier the project has not used.
    #[must_use]
    pub fn for_project(project: &Project) -> Self {
        Self::starting_from(project.next_unused_id())
    }

    /// Returns the next identifier that would be handed out.
    #[must_use]
    pub const fn peek(&self) -> u64 {
        self.next
    }

    /// Allocates a media asset identifier.
    pub fn media_asset(&mut self) -> MediaAssetId {
        MediaAssetId::new(self.take())
    }

    /// Allocates a sequence identifier.
    pub fn sequence(&mut self) -> SequenceId {
        SequenceId::new(self.take())
    }

    /// Allocates a track identifier.
    pub fn track(&mut self) -> TrackId {
        TrackId::new(self.take())
    }

    /// Allocates a clip identifier.
    pub fn clip(&mut self) -> ClipId {
        ClipId::new(self.take())
    }

    /// Allocates an effect identifier.
    pub fn effect(&mut self) -> EffectId {
        EffectId::new(self.take())
    }

    /// Allocates a mask identifier.
    pub fn mask(&mut self) -> MaskId {
        MaskId::new(self.take())
    }

    /// Allocates a transition identifier.
    pub fn transition(&mut self) -> TransitionId {
        TransitionId::new(self.take())
    }

    fn take(&mut self) -> u64 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        id
    }
}

impl Default for IdAllocator {
    fn default() -> Self {
        Self::starting_from(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_of_different_kinds_are_distinct_types() {
        // The compiler rejects mixing these; this test documents the shared counter.
        let mut ids = IdAllocator::default();
        let asset = ids.media_asset();
        let clip = ids.clip();
        assert_eq!(asset.raw(), 1);
        assert_eq!(clip.raw(), 2);
        assert_eq!(ids.peek(), 3);
    }

    #[test]
    fn display_names_the_kind_and_the_number() {
        assert_eq!(ClipId::new(12).to_string(), "clip#12");
        assert_eq!(MediaAssetId::new(3).to_string(), "asset#3");
        assert_eq!(SequenceId::KIND, "sequence");
        assert_eq!(TrackId::KIND, "track");
    }

    #[test]
    fn a_project_without_entities_starts_at_one() {
        let project = Project::new("empty");
        assert_eq!(project.next_unused_id(), 1);
        assert_eq!(IdAllocator::for_project(&project).peek(), 1);
    }
}

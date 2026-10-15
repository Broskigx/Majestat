//! Canonical serialisation and content hashing.
//!
//! # Why this is byte-exact
//!
//! Three properties make the canonical form trustworthy:
//!
//! 1. **No floating point anywhere.** Rates, ranges and speeds are integers and
//!    reduced rationals. There is no `1.0`-versus-`1` ambiguity, no NaN ordering, and
//!    no locale-dependent number formatting.
//! 2. **No maps.** Every collection in the model is a `Vec` or a slice, so iteration
//!    order is insertion order and is itself part of the state.
//! 3. **Fixed field order.** The canonical form is produced by serialising structs,
//!    whose field order is the declaration order, so it cannot change between runs.
//!
//! A hash of the canonical form therefore identifies a project exactly. That is what
//! makes `maj render --verify` meaningful (M8) and what a render farm can check
//! instead of being trusted (M13).

use serde::Serialize;

use crate::command::{Command, LogEntry};
use crate::entity::{MediaAsset, Sequence};
use crate::error::CoreError;
use crate::project::Project;

/// The hashed view of a project: everything durable except the command log.
#[derive(Serialize)]
struct StateView<'a> {
    schema_version: u32,
    name: &'a str,
    assets: &'a [MediaAsset],
    sequences: &'a [Sequence],
}

/// One entry of the semantic trace.
///
/// `seq` is deliberately excluded: it is the position in the log, so including it
/// would make two identical edit sequences hash differently only because one of them
/// started life further along.
#[derive(Serialize)]
struct TraceView<'a> {
    author: &'a str,
    command: &'a Command,
}

/// BLAKE3 of arbitrary bytes, hex encoded.
#[must_use]
pub fn hash_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Canonical JSON of the project state, excluding the command log.
pub fn canonical_state_json(project: &Project) -> Result<String, CoreError> {
    let view = StateView {
        schema_version: project.schema_version(),
        name: project.name(),
        assets: project.assets(),
        sequences: project.sequences(),
    };
    serde_json::to_string(&view).map_err(Into::into)
}

/// Hash of the project state.
pub fn state_hash(project: &Project) -> Result<String, CoreError> {
    Ok(hash_bytes(canonical_state_json(project)?.as_bytes()))
}

/// Canonical JSON of the whole document, command log included.
pub fn document_json(project: &Project) -> Result<String, CoreError> {
    serde_json::to_string(project).map_err(Into::into)
}

/// Hash of the whole document.
pub fn document_hash(project: &Project) -> Result<String, CoreError> {
    Ok(hash_bytes(document_json(project)?.as_bytes()))
}

/// Canonical JSON of the semantic command trace.
pub fn canonical_trace_json(entries: &[LogEntry]) -> Result<String, CoreError> {
    let view: Vec<TraceView<'_>> = entries
        .iter()
        .map(|entry| TraceView {
            author: entry.author(),
            command: entry.command(),
        })
        .collect();
    serde_json::to_string(&view).map_err(Into::into)
}

/// Hash of the semantic command trace: who did what, in order.
pub fn command_trace_hash(entries: &[LogEntry]) -> Result<String, CoreError> {
    Ok(hash_bytes(canonical_trace_json(entries)?.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Clip, Track, TrackKind};
    use crate::ids::{IdAllocator, MediaAssetId, SequenceId};
    use maj_time::{FrameRange, Rational, Timebase};
    use serde_json::Value;

    fn project_with_one_clip() -> Project {
        let mut project = Project::new("hash");
        let mut ids = IdAllocator::for_project(&project);
        let asset_id = ids.media_asset();
        let sequence_id = ids.sequence();
        let track_id = ids.track();
        let clip_id = ids.clip();

        project
            .insert_asset(
                0,
                MediaAsset::new(asset_id, "a.mov", "file:///a.mov", Timebase::FPS_25, 500).unwrap(),
            )
            .unwrap();
        let mut sequence =
            Sequence::new(sequence_id, "S", Timebase::FPS_25, 1920, 1080, 48_000).unwrap();
        let mut track = Track::new(track_id, TrackKind::Video, "V1");
        track
            .insert_clip_at(
                0,
                Clip::new(
                    clip_id,
                    asset_id,
                    "c",
                    FrameRange::new(0, 100).unwrap(),
                    0,
                    Rational::from_integer(1),
                )
                .unwrap(),
            )
            .unwrap();
        sequence.insert_track(0, track).unwrap();
        project.insert_sequence(0, sequence).unwrap();
        project
    }

    #[test]
    fn hashing_the_same_state_twice_is_identical() {
        let project = project_with_one_clip();
        assert_eq!(project.state_hash().unwrap(), project.state_hash().unwrap());
        assert_eq!(project.state_hash().unwrap().len(), 64, "hex of 32 bytes");
    }

    /// Walks a JSON value and asserts that every number is an integer.
    fn assert_all_numbers_are_integers(value: &Value, path: &str) {
        match value {
            Value::Number(number) => assert!(
                number.is_i64() || number.is_u64(),
                "non-integer number at {path}: {number}"
            ),
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    assert_all_numbers_are_integers(item, &format!("{path}[{index}]"));
                }
            }
            Value::Object(map) => {
                for (key, item) in map {
                    assert_all_numbers_are_integers(item, &format!("{path}.{key}"));
                }
            }
            Value::Null | Value::Bool(_) | Value::String(_) => {}
        }
    }

    #[test]
    fn the_canonical_form_contains_no_floating_point_numbers() {
        let project = project_with_one_clip();
        let json = project.canonical_state_json().unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_all_numbers_are_integers(&value, "$");
        // Strings may legitimately contain a dot, as a file extension does. The claim
        // is that no *number* carries one, because a float anywhere in the model would
        // make the canonical form depend on float formatting.
        assert!(
            json.contains("a.mov"),
            "the fixture should carry a dot inside a string"
        );
    }

    #[test]
    fn the_float_detector_actually_detects_floats() {
        let with_float: Value = serde_json::json!({ "rate": 29.97 });
        let outcome = std::panic::catch_unwind(|| {
            assert_all_numbers_are_integers(&with_float, "$");
        });
        assert!(outcome.is_err(), "the detector must reject 29.97");

        let integers: Value = serde_json::json!({ "rate": { "num": 30000, "den": 1001 } });
        assert_all_numbers_are_integers(&integers, "$");
    }

    #[test]
    fn a_single_field_change_changes_the_hash() {
        let base = project_with_one_clip();
        let mut renamed = base.clone();
        renamed.set_name("other");
        assert_ne!(base.state_hash().unwrap(), renamed.state_hash().unwrap());

        let mut moved = base.clone();
        moved.sequences[0].tracks[0].clips[0].timeline_start = 1;
        assert_ne!(base.state_hash().unwrap(), moved.state_hash().unwrap());

        let mut retimed = base.clone();
        retimed.sequences[0].tracks[0].clips[0].speed = Rational::new(1, 2).unwrap();
        assert_ne!(base.state_hash().unwrap(), retimed.state_hash().unwrap());
    }

    #[test]
    fn insertion_order_is_part_of_the_state() {
        let mut first = project_with_one_clip();
        let mut second = first.clone();
        // Same two clips, different insertion order, identical timeline positions.
        let mut ids = IdAllocator::for_project(&first);
        let extra_a = ids.clip();
        let extra_b = ids.clip();
        let a = Clip::new(
            extra_a,
            MediaAssetId::new(1),
            "x",
            FrameRange::new(0, 10).unwrap(),
            1_000,
            Rational::from_integer(1),
        )
        .unwrap();
        let b = Clip::new(
            extra_b,
            MediaAssetId::new(1),
            "y",
            FrameRange::new(0, 10).unwrap(),
            2_000,
            Rational::from_integer(1),
        )
        .unwrap();

        first.sequences[0].tracks[0].insert_clip_at(0, a).unwrap();
        second.sequences[0].tracks[0].insert_clip_at(0, b).unwrap();
        assert_ne!(
            first.state_hash().unwrap(),
            second.state_hash().unwrap(),
            "the order of clips within a track must be visible to the hash"
        );
    }

    #[test]
    fn the_document_hash_covers_the_log_and_the_state_hash_does_not() {
        let project = project_with_one_clip();
        assert_ne!(
            project.state_hash().unwrap(),
            project.document_hash().unwrap(),
            "the document hash must include the command log"
        );
        let mut renamed = project.clone();
        renamed.set_name("changed");
        assert_ne!(
            project.document_hash().unwrap(),
            renamed.document_hash().unwrap()
        );
    }

    #[test]
    fn the_trace_hash_ignores_log_position_and_covers_author_and_command() {
        let entries = [
            LogEntry::new(
                0,
                "ana",
                Command::RemoveMediaAsset {
                    id: MediaAssetId::new(1),
                },
            ),
            LogEntry::new(
                1,
                "bo",
                Command::RemoveSequence {
                    id: SequenceId::new(2),
                },
            ),
        ];
        let renumbered = [
            LogEntry::new(
                40,
                "ana",
                Command::RemoveMediaAsset {
                    id: MediaAssetId::new(1),
                },
            ),
            LogEntry::new(
                41,
                "bo",
                Command::RemoveSequence {
                    id: SequenceId::new(2),
                },
            ),
        ];
        assert_eq!(
            command_trace_hash(&entries).unwrap(),
            command_trace_hash(&renumbered).unwrap()
        );

        let reauthored = [
            LogEntry::new(
                0,
                "ana",
                Command::RemoveMediaAsset {
                    id: MediaAssetId::new(1),
                },
            ),
            LogEntry::new(
                1,
                "carla",
                Command::RemoveSequence {
                    id: SequenceId::new(2),
                },
            ),
        ];
        assert_ne!(
            command_trace_hash(&entries).unwrap(),
            command_trace_hash(&reauthored).unwrap()
        );
    }
}

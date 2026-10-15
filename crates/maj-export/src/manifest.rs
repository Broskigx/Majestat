//! The render manifest on disk: what was rendered, and how.
//!
//! A still on its own says nothing about how it was made. Which policy produced it, which
//! revision of the project it came from, which effects were live, whether the pixels were
//! built by a whole-frame pass or a hundred tiles — none of that survives in a PNG. The
//! manifest is the missing half of the delivery, and it is written as JSON because a
//! manifest that cannot be read with `cat` and diffed with `git diff` is not auditable.
//!
//! The text is a pure function of the manifest, which is itself a pure function of the
//! render. Two runs therefore produce byte-identical manifests, and the field order is the
//! order the struct declares, so a diff between two revisions shows the field that changed
//! and not a reshuffled document.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use maj_render::RenderManifest;

use crate::ExportError;

/// The manifest text for a render, pretty-printed and newline-terminated.
///
/// # Errors
///
/// Returns [`ExportError::Manifest`] if the manifest cannot be serialised, which would be a
/// programming error rather than a data one: every field is an integer, a string, a boolean
/// or a list of those.
pub fn to_json(manifest: &RenderManifest) -> Result<String, ExportError> {
    let mut text = serde_json::to_string_pretty(manifest)
        .map_err(|error| ExportError::Manifest(error.to_string()))?;
    text.push('\n');
    Ok(text)
}

/// Where a frame's manifest belongs, given the frame's own path.
///
/// `shot.0001.png` beside `shot.0001.maj.json`: the manifest sorts next to its frame, and
/// the extension says it is Majestat's record rather than a generic JSON file.
#[must_use]
pub fn path_for(frame: &Path) -> PathBuf {
    let stem = frame.file_stem().map_or_else(
        || "frame".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    frame.with_file_name(format!("{stem}.maj.json"))
}

/// The one-line summary a person or a script reads first.
#[must_use]
pub fn summary(manifest: &RenderManifest) -> String {
    let mut text = String::new();
    let _ = write!(
        text,
        "{}x{} frame {} of sequence {} on the {} path",
        manifest.width, manifest.height, manifest.frame, manifest.sequence, manifest.policy
    );
    let _ = write!(
        text,
        ": {} layer(s), halo {}, grid {}x{}, digest {}",
        manifest.layers.len(),
        manifest.halo,
        manifest.grid.0,
        manifest.grid.1,
        &manifest.digest[..manifest.digest.len().min(12)]
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use maj_core::{History, MediaAsset, MediaAssetId, Sequence, SequenceId, Timebase};
    use maj_render::{RenderRequest, RenderTask, WorkspaceResolver};

    /// A tiny project with one generated clip, rendered, so the manifest under test is a
    /// real one rather than a hand-built struct.
    fn manifest() -> RenderManifest {
        let mut project = maj_core::Project::new("manifest-test");
        let mut history = History::new();
        let sequence = SequenceId::new(1);
        history
            .apply_local(
                &mut project,
                maj_core::Command::CreateSequence {
                    index: 0,
                    sequence: Sequence::new(sequence, "cut", Timebase::FPS_25, 16, 8, 48_000)
                        .unwrap(),
                },
            )
            .unwrap();
        history
            .apply_local(
                &mut project,
                maj_core::Command::CreateMediaAsset {
                    index: 0,
                    asset: MediaAsset::new(
                        MediaAssetId::new(1),
                        "chart",
                        "generated:bars",
                        Timebase::FPS_25,
                        24,
                    )
                    .unwrap(),
                },
            )
            .unwrap();
        let track = maj_core::TrackId::new(1);
        history
            .apply_local(
                &mut project,
                maj_core::Command::CreateTrack {
                    sequence,
                    index: 0,
                    track: maj_core::Track::new(track, maj_core::TrackKind::Video, "V1"),
                },
            )
            .unwrap();
        history
            .apply_local(
                &mut project,
                maj_core::Command::AppendClip {
                    sequence,
                    track,
                    clip: maj_core::Clip::new(
                        maj_core::ClipId::new(1),
                        MediaAssetId::new(1),
                        "chart",
                        maj_time::FrameRange::new(0, 24).unwrap(),
                        0,
                        maj_time::Rational::from_integer(1),
                    )
                    .unwrap(),
                },
            )
            .unwrap();
        RenderTask::single(RenderRequest::new(sequence, 1, 16, 8))
            .render(&project, &WorkspaceResolver::new())
            .expect("the fixture renders")
            .manifest
    }

    #[test]
    fn the_manifest_holds_no_floating_point() {
        // The same discipline the project format keeps, for the same reason: a decimal
        // rendering of a float is a place where two identical runs can differ.
        let text = to_json(&manifest()).expect("the manifest serialises");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        fn walk(value: &serde_json::Value, path: &str) {
            match value {
                serde_json::Value::Number(number) => {
                    assert!(
                        !number.is_f64(),
                        "{path} is a floating-point number: {number}"
                    );
                }
                serde_json::Value::Array(items) => {
                    for (index, item) in items.iter().enumerate() {
                        walk(item, &format!("{path}[{index}]"));
                    }
                }
                serde_json::Value::Object(fields) => {
                    for (key, field) in fields {
                        walk(field, &format!("{path}.{key}"));
                    }
                }
                serde_json::Value::Null
                | serde_json::Value::Bool(_)
                | serde_json::Value::String(_) => {}
            }
        }
        walk(&value, "$");
    }

    #[test]
    fn the_same_render_produces_the_same_text_and_the_fields_are_all_there() {
        let first = manifest();
        let second = manifest();
        assert_eq!(first, second, "the manifest is a function of the render");
        assert_eq!(
            to_json(&first).expect("serialises"),
            to_json(&second).expect("serialises")
        );

        let text = to_json(&first).expect("serialises");
        assert!(
            text.ends_with("}\n"),
            "the text ends with the document and a newline"
        );
        for field in [
            "\"schema\"",
            "\"policy\"",
            "\"certified\"",
            "\"sequence\"",
            "\"frame\"",
            "\"width\"",
            "\"height\"",
            "\"project_revision\"",
            "\"state_hash\"",
            "\"halo\"",
            "\"grid\"",
            "\"layers\"",
            "\"digest\"",
            "\"peak_thousandths\"",
        ] {
            assert!(text.contains(field), "the manifest must record {field}");
        }
        assert!(text.contains("\"certified\": true"));
        assert!(text.contains("\"policy\": \"certified\""));
    }

    #[test]
    fn a_manifest_sits_beside_its_frame() {
        assert_eq!(
            path_for(Path::new("/renders/shot.0001.png")),
            PathBuf::from("/renders/shot.0001.maj.json")
        );
        assert_eq!(
            path_for(Path::new("frame.ppm")),
            PathBuf::from("frame.maj.json")
        );
        assert_eq!(path_for(Path::new("")), PathBuf::from("frame.maj.json"));
    }

    #[test]
    fn the_summary_says_what_a_person_needs_first() {
        let text = summary(&manifest());
        assert!(
            text.starts_with("16x8 frame 1 of sequence 1 on the certified path"),
            "{text}"
        );
        assert!(text.contains("1 layer(s)"), "{text}");
        assert!(text.contains("grid 1x1"), "{text}");
    }
}

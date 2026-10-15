//! End-to-end: the binary, a real project file on disk, real outputs.
//!
//! These tests run `maj` as a process rather than calling into the library, because the
//! claim being checked is about the *interface*: that a person with a `.maj` file and no
//! editor can render a frame, verify it, and get a non-zero exit code when the check fails.
//! A test that calls the same functions the command calls would prove none of that.

use std::path::{Path, PathBuf};
use std::process::{Command as Process, Output};

use maj_core::{
    BlendMode, Clip, ClipId, Command, History, Interpolation, Keyframe, Mask, MaskId, MaskShape,
    MediaAsset, MediaAssetId, ParamValue, Project, Sequence, SequenceId, Timebase, Track, TrackId,
    Transition, TransitionId, TransitionKind,
};
use maj_time::{FrameRange, Rational};

/// Runs the binary and returns its output, without asserting anything about it.
fn maj(args: &[&str]) -> Output {
    Process::new(env!("CARGO_BIN_EXE_maj"))
        .args(args)
        .output()
        .expect("the maj binary runs")
}

/// Runs the binary and asserts it succeeded, returning its standard output.
fn maj_ok(args: &[&str]) -> String {
    let output = maj(args);
    assert!(
        output.status.success(),
        "maj {args:?} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A directory of its own for one test, so tests never share a file.
fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("majestat-cli-{name}"));
    std::fs::remove_dir_all(&directory).ok();
    std::fs::create_dir_all(&directory).expect("a temporary directory");
    directory
}

/// A project that exercises the render path: two video tracks, a transition, masks,
/// animation, an effect chain and an audio track the picture must ignore.
fn demo_project() -> Project {
    let mut project = Project::new("demo");
    let mut history = History::new();
    let sequence = SequenceId::new(1);
    let video = TrackId::new(1);
    let upper = TrackId::new(2);
    let audio = TrackId::new(3);

    history
        .apply_local(
            &mut project,
            Command::CreateSequence {
                index: 0,
                sequence: Sequence::new(sequence, "cut", Timebase::FPS_25, 32, 18, 48_000).unwrap(),
            },
        )
        .unwrap();
    for (id, name, uri, video_stream, audio_stream) in [
        (1, "bars", "generated:bars", true, false),
        (2, "accent", "solid:255,64,0", true, false),
        (3, "music", "generated:tone", false, true),
    ] {
        let index = project.assets().len();
        history
            .apply_local(
                &mut project,
                Command::CreateMediaAsset {
                    index,
                    asset: MediaAsset::new(MediaAssetId::new(id), name, uri, Timebase::FPS_25, 100)
                        .unwrap()
                        .with_streams(video_stream, audio_stream),
                },
            )
            .unwrap();
    }
    for (id, kind, name) in [
        (video, maj_core::TrackKind::Video, "V1"),
        (upper, maj_core::TrackKind::Video, "V2"),
        (audio, maj_core::TrackKind::Audio, "A1"),
    ] {
        let index = project.sequences()[0].tracks().len();
        history
            .apply_local(
                &mut project,
                Command::CreateTrack {
                    sequence,
                    index,
                    track: Track::new(id, kind, name),
                },
            )
            .unwrap();
    }

    // V1: two clips with a cross dissolve across the cut between them.
    for (id, name, start, asset) in [(1, "bars-A", 0, 1), (2, "bars-B", 24, 1)] {
        history
            .apply_local(
                &mut project,
                Command::AppendClip {
                    sequence,
                    track: video,
                    clip: Clip::new(
                        ClipId::new(id),
                        MediaAssetId::new(asset),
                        name,
                        FrameRange::new(0, 24).unwrap(),
                        start,
                        Rational::from_integer(1),
                    )
                    .unwrap(),
                },
            )
            .unwrap();
    }
    history
        .apply_local(
            &mut project,
            Command::AddTransition {
                sequence,
                track: video,
                index: 0,
                transition: Transition::new(
                    TransitionId::new(1),
                    TransitionKind::CrossDissolve,
                    22,
                    4,
                    Some(ClipId::new(1)),
                    Some(ClipId::new(2)),
                )
                .unwrap(),
            },
        )
        .unwrap();

    // An effect chain on the first clip, so the renderer has work with a halo to do.
    for (id, kind) in [
        (1, maj_core::EffectKind::Blur),
        (2, maj_core::EffectKind::Contrast),
    ] {
        let index = project.sequences()[0].tracks()[0].clips()[0]
            .effects()
            .len();
        history
            .apply_local(
                &mut project,
                Command::AddEffect {
                    sequence,
                    clip: ClipId::new(1),
                    index,
                    effect: {
                        let mut instance =
                            maj_core::EffectInstance::new(maj_core::EffectId::new(id), kind);
                        if kind == maj_core::EffectKind::Blur {
                            instance
                                .set_param(
                                    "radius",
                                    ParamValue::Rational(Rational::from_integer(3)),
                                )
                                .unwrap();
                        }
                        instance
                    },
                },
            )
            .unwrap();
    }

    // V2: a masked, half-transparent accent that fades in, blended over V1.
    let accent = ClipId::new(3);
    history
        .apply_local(
            &mut project,
            Command::AppendClip {
                sequence,
                track: upper,
                clip: Clip::new(
                    accent,
                    MediaAssetId::new(2),
                    "accent",
                    FrameRange::new(0, 60).unwrap(),
                    0,
                    Rational::from_integer(1),
                )
                .unwrap(),
            },
        )
        .unwrap();
    history
        .apply_local(
            &mut project,
            Command::AddMask {
                sequence,
                clip: accent,
                index: 0,
                mask: Mask::with_geometry(
                    MaskId::new(1),
                    MaskShape::Ellipse,
                    Rational::from_reduced(1, 2),
                    Rational::from_reduced(1, 2),
                    Rational::from_reduced(3, 4),
                    Rational::from_integer(1),
                    Rational::from_integer(0),
                    Rational::from_reduced(1, 8),
                    Rational::from_reduced(3, 4),
                    false,
                )
                .unwrap(),
            },
        )
        .unwrap();
    history
        .apply_local(
            &mut project,
            Command::SetClipBlend {
                sequence,
                clip: accent,
                blend: BlendMode::Screen,
            },
        )
        .unwrap();
    history
        .apply_local(
            &mut project,
            Command::SetChannelKeys {
                sequence,
                clip: accent,
                channel: maj_core::AnimChannel::Opacity,
                keys: vec![
                    Keyframe::new(
                        0,
                        ParamValue::Rational(Rational::from_integer(0)),
                        Interpolation::Linear,
                    ),
                    Keyframe::new(
                        12,
                        ParamValue::Rational(Rational::from_integer(1)),
                        Interpolation::Linear,
                    ),
                    Keyframe::new(
                        48,
                        ParamValue::Rational(Rational::from_integer(1)),
                        Interpolation::Linear,
                    ),
                ],
            },
        )
        .unwrap();

    // A1: sound, which the picture renderer must ignore entirely.
    history
        .apply_local(
            &mut project,
            Command::AppendClip {
                sequence,
                track: audio,
                clip: Clip::new(
                    ClipId::new(4),
                    MediaAssetId::new(3),
                    "music",
                    FrameRange::new(0, 60).unwrap(),
                    0,
                    Rational::from_integer(1),
                )
                .unwrap(),
            },
        )
        .unwrap();

    project
}

/// Writes the demo project into a scratch directory and returns its path.
fn demo_file(name: &str) -> (PathBuf, PathBuf) {
    let directory = scratch(name);
    let path = directory.join("demo.maj");
    maj_core::format::save_to_path(&demo_project(), &path).expect("the demo project is written");
    (directory, path)
}

/// The digest a manifest records, read out of its JSON text.
fn digest_of(text: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(text).expect("the manifest is JSON");
    value
        .get("digest")
        .and_then(|digest| digest.as_str())
        .expect("the manifest records a digest")
        .to_owned()
}

#[test]
fn a_frame_renders_to_a_still_and_a_manifest_beside_it() {
    let (directory, project) = demo_file("render");
    let out = directory.join("frame.0003.png");
    let text = maj_ok(&[
        "render",
        "--project",
        project.to_str().unwrap(),
        "--frame",
        "3",
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(text.contains("certified path"), "{text}");
    assert!(text.contains("32x18 frame 3 of sequence 1"), "{text}");

    let image = std::fs::read(&out).expect("the still is on disk");
    assert_eq!(
        &image[..8],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
    );
    let manifest_path = directory.join("frame.0003.maj.json");
    let manifest = std::fs::read_to_string(&manifest_path).expect("the manifest is on disk");
    assert_eq!(digest_of(&manifest).len(), 64);
    assert!(manifest.contains("\"frame\": 3"));
    assert!(manifest.contains("\"policy\": \"certified\""));

    // A PPM beside it, at which point the still is readable by any tool at all.
    let ppm = directory.join("frame.0003.ppm");
    maj_ok(&[
        "render",
        "--project",
        project.to_str().unwrap(),
        "--frame",
        "3",
        "--out",
        ppm.to_str().unwrap(),
    ]);
    let bytes = std::fs::read(&ppm).expect("the PPM is on disk");
    assert_eq!(&bytes[..2], b"P6");

    // An unknown extension is refused by name, and the exit code says who is at fault.
    let output = maj(&[
        "render",
        "--project",
        project.to_str().unwrap(),
        "--out",
        "frame.tiff",
    ]);
    assert_eq!(output.status.code(), Some(2), "a usage error is code two");
    assert!(String::from_utf8_lossy(&output.stderr).contains("not a png or a ppm"));
}

#[test]
fn three_renders_of_one_revision_produce_identical_bytes() {
    // The export determinism gate, at the level of stills: same input, same bytes, three
    // times, with the renderer, the quantiser and the encoder all in the path.
    let (directory, project) = demo_file("determinism");
    let mut digests = Vec::new();
    for run in 1..=3 {
        let out = directory.join(format!("run{run}.png"));
        maj_ok(&[
            "render",
            "--project",
            project.to_str().unwrap(),
            "--frame",
            "23",
            "--tiles",
            "2x2",
            "--depth",
            "16",
            "--out",
            out.to_str().unwrap(),
        ]);
        let manifest = std::fs::read_to_string(directory.join(format!("run{run}.maj.json")))
            .expect("the manifest is on disk");
        digests.push(digest_of(&manifest));
    }
    assert_eq!(digests[0], digests[1]);
    assert_eq!(digests[1], digests[2]);

    let first = std::fs::read(directory.join("run1.png")).expect("the still");
    let second = std::fs::read(directory.join("run2.png")).expect("the still");
    let third = std::fs::read(directory.join("run3.png")).expect("the still");
    assert_eq!(first, second, "the encoder has no clock and no state");
    assert_eq!(second, third);
    assert!(!first.is_empty());

    // Tiling is not part of the output, so a tiled render is the same file as an untiled one.
    let whole = directory.join("whole.png");
    maj_ok(&[
        "render",
        "--project",
        project.to_str().unwrap(),
        "--frame",
        "23",
        "--out",
        whole.to_str().unwrap(),
    ]);
    assert_eq!(std::fs::read(&whole).expect("the still"), first);
}

#[test]
fn verify_checks_two_ways_of_rendering_and_fails_loudly() {
    let (_directory, project) = demo_file("verify");
    let text = maj_ok(&[
        "verify",
        "--project",
        project.to_str().unwrap(),
        "--frame",
        "23",
        "--tiles",
        "3x2",
    ]);
    assert!(text.contains("OK: the same bytes three ways"), "{text}");
    assert!(
        text.contains("halo: 6 pixel(s)"),
        "a blur of radius three needs six: {text}"
    );

    // A grid of one is not a check, so it is refused rather than passed.
    let output = maj(&[
        "verify",
        "--project",
        project.to_str().unwrap(),
        "--tiles",
        "1x1",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("1x1"));
}

#[test]
fn probe_reports_what_the_project_holds() {
    let (_directory, project) = demo_file("probe");
    let text = maj_ok(&["probe", "--project", project.to_str().unwrap()]);
    assert!(text.contains("project        demo"), "{text}");
    assert!(text.contains("sequence#1 `cut` 32x18 @ 25 fps"), "{text}");
    assert!(text.contains("track#3 `A1` (audio, 1)"), "{text}");
    assert!(text.contains("clip#1 `bars-A` 0..24"), "{text}");
    assert!(text.contains("effects: blur, contrast"), "{text}");
    assert!(
        text.contains("transition#1 cross_dissolve at 22 for 4 frames"),
        "{text}"
    );
    assert!(text.contains("generated:bars"), "{text}");
}

#[test]
fn bench_measures_the_host_and_checks_that_it_repeats() {
    let (_directory, project) = demo_file("bench");
    let text = maj_ok(&[
        "bench",
        "--project",
        project.to_str().unwrap(),
        "--frame",
        "10",
        "--repeat",
        "2",
    ]);
    assert!(text.contains("run 1:"), "{text}");
    assert!(text.contains("run 2:"), "{text}");
    assert!(text.contains("mean"), "{text}");
    assert!(
        text.contains("the timings are this machine today; the digest below is not a timing"),
        "{text}"
    );
}

#[test]
fn edit_applies_a_command_and_the_render_follows() {
    let (directory, project) = demo_file("edit");
    let before = directory.join("before.png");
    maj_ok(&[
        "render",
        "--project",
        project.to_str().unwrap(),
        "--frame",
        "10",
        "--out",
        before.to_str().unwrap(),
    ]);
    let first = digest_of(
        &std::fs::read_to_string(directory.join("before.maj.json")).expect("the manifest"),
    );

    // Turning the top track's clip off changes what the frame is, and the edit is a command
    // like any other, so it goes in the log and moves the revision.
    let edited = maj_ok(&[
        "edit",
        "--project",
        project.to_str().unwrap(),
        "--command",
        r#"{"kind":"set_clip_enabled","sequence":1,"clip":3,"enabled":false}"#,
    ]);
    // One command moves the revision by exactly one, whatever it was before.
    let line = edited
        .lines()
        .find(|line| line.starts_with("revision "))
        .expect("the command reports the revision");
    let movement: Vec<u64> = line
        .trim_start_matches("revision ")
        .split(',')
        .next()
        .expect("a revision pair")
        .split("->")
        .filter_map(|token| token.trim().parse::<u64>().ok())
        .collect();
    assert_eq!(movement.len(), 2, "{edited}");
    assert_eq!(movement[1], movement[0] + 1, "{edited}");

    let after = directory.join("after.png");
    maj_ok(&[
        "render",
        "--project",
        project.to_str().unwrap(),
        "--frame",
        "10",
        "--out",
        after.to_str().unwrap(),
    ]);
    let second = digest_of(
        &std::fs::read_to_string(directory.join("after.maj.json")).expect("the manifest"),
    );
    assert_ne!(first, second, "an edit must change the picture");
    assert_ne!(
        std::fs::read(&before).expect("the still"),
        std::fs::read(&after).expect("the still")
    );

    // A command the model refuses is an operational failure, not a usage one.
    let output = maj(&[
        "edit",
        "--project",
        project.to_str().unwrap(),
        "--command",
        r#"{"kind":"remove_clip","sequence":1,"clip":9999}"#,
    ]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("maj:"));

    // And a command that is not a command at all is a usage error.
    let output = maj(&[
        "edit",
        "--project",
        project.to_str().unwrap(),
        "--command",
        r#"{"kind":"make_it_pop"}"#,
    ]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn a_missing_or_unusable_project_fails_with_a_reason() {
    let (_directory, project) = demo_file("missing");
    let gone = Path::new(&project).with_file_name("nothing-here.maj");
    let output = maj(&[
        "render",
        "--project",
        gone.to_str().unwrap(),
        "--out",
        "out.png",
    ]);
    assert_eq!(output.status.code(), Some(3));
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.starts_with("maj: "), "{message}");
    assert!(message.contains("nothing-here.maj"), "{message}");

    // A sequence that is not in the document is named rather than defaulted.
    let output = maj(&[
        "verify",
        "--project",
        project.to_str().unwrap(),
        "--sequence",
        "ghost",
        "--tiles",
        "2x2",
    ]);
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("no sequence `ghost`"));
}

#[test]
fn every_command_names_the_options_it_needs() {
    for command in ["render", "verify", "probe", "bench", "edit"] {
        let output = maj(&[command]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{command} must ask for --project"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("--project"),
            "{command} must name the missing option"
        );
    }
    let help = maj_ok(&["help"]);
    assert!(help.contains("maj <command> [options]"), "{help}");
    assert!(help.contains("--tiles <CxR>"), "{help}");
}

#[test]
fn strict_media_turns_a_placeholder_into_a_refusal() {
    // The two legitimate answers about media that cannot be read, both reachable from the
    // command line, because the command line is where a delivery is scripted. A script that
    // wanted the strict answer and silently got the forgiving one would ship the placeholder.
    let directory = scratch("strict-media");
    let project_path = directory.join("offline.maj");
    let mut project = demo_project();
    let mut history = History::new();
    // A clip whose picture this build cannot produce: the file is not there.
    let index = project.assets().len();
    history
        .apply_local(
            &mut project,
            Command::CreateMediaAsset {
                index,
                asset: MediaAsset::new(
                    MediaAssetId::new(99),
                    "gone",
                    "file:///nowhere/at/all/shot.png",
                    Timebase::FPS_25,
                    100,
                )
                .unwrap(),
            },
        )
        .unwrap();
    history
        .apply_local(
            &mut project,
            Command::AppendClip {
                sequence: SequenceId::new(1),
                track: TrackId::new(2),
                clip: Clip::new(
                    ClipId::new(99),
                    MediaAssetId::new(99),
                    "gone",
                    FrameRange::new(0, 50).unwrap(),
                    0,
                    Rational::from_integer(1),
                )
                .unwrap(),
            },
        )
        .unwrap();
    maj_core::format::save_to_path(&project, &project_path).expect("the project saves");
    let path = project_path.to_str().unwrap();
    let out = directory.join("frame.png");

    // By default the frame renders, and the manifest says what stood in for what rather than
    // leaving a substitution for somebody to notice in a master.
    maj_ok(&[
        "render",
        "--project",
        path,
        "--frame",
        "10",
        "--out",
        out.to_str().unwrap(),
    ]);
    let manifest = std::fs::read_to_string(directory.join("frame.maj.json")).expect("a manifest");
    assert!(
        manifest.contains("\"source\": \"offline\""),
        "the manifest must name the substitution: {manifest}"
    );

    // With the switch, the same frame is refused, and the message names the file.
    let refused = maj(&[
        "render",
        "--project",
        path,
        "--frame",
        "10",
        "--out",
        out.to_str().unwrap(),
        "--strict-media",
    ]);
    assert!(!refused.status.success(), "{refused:?}");
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(said.contains("shot.png"), "{said}");

    // And it is a switch the help names, because an option nobody can find is not a policy.
    assert!(maj_ok(&["help"]).contains("--strict-media"));
}

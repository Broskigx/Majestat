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
///
/// The process identifier is part of the name because two sessions *can* run this suite at the
/// same time: `cargo` takes the build directory lock for the build and releases it before
/// running the binaries, so a second run of the same suite in one checkout collides with the
/// first on any name that does not say which process it belongs to. That collision has already
/// produced three false red gates in this repository; a scratch name is not worth one more.
fn scratch(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("majestat-cli-{name}-{}", std::process::id()));
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

/// A project with one asset pointing at a real file, one procedural asset and one whose file
/// is not there: the three answers `maj fingerprint` has to give.
fn project_with_files(directory: &Path, payload: &[u8]) -> (Project, PathBuf) {
    let media = directory.join("card").join("A001C001.mov");
    std::fs::create_dir_all(media.parent().unwrap()).expect("a card directory");
    std::fs::write(&media, payload).expect("a file on the card");
    let mut project = Project::new("relink");
    let mut history = History::new();
    history
        .apply_local(
            &mut project,
            Command::CreateSequence {
                index: 0,
                sequence: Sequence::new(
                    SequenceId::new(1),
                    "cut",
                    Timebase::FPS_25,
                    32,
                    18,
                    48_000,
                )
                .unwrap(),
            },
        )
        .unwrap();
    for (id, name, uri) in [
        (1, "A001C001", maj_media::uri_of(&media)),
        (2, "bars", "generated:bars".to_owned()),
        (
            3,
            "gone",
            maj_media::uri_of(&directory.join("card").join("missing.mov")),
        ),
    ] {
        let index = project.assets().len();
        history
            .apply_local(
                &mut project,
                Command::CreateMediaAsset {
                    index,
                    asset: MediaAsset::new(MediaAssetId::new(id), name, uri, Timebase::FPS_25, 240)
                        .unwrap()
                        .with_streams(true, false),
                },
            )
            .unwrap();
    }
    (project, media)
}

#[test]
fn fingerprinting_records_a_measurement_and_invents_nothing() {
    // The two measurements are made by different amounts of the engine, and the document has
    // to say which of them happened: the hash is `std::fs`, the duration needs a probe that
    // does not exist yet, and a duration of zero would be a trim bound nobody measured.
    let directory = scratch("fingerprint");
    let project_path = directory.join("relink.maj");
    let payload = b"first eight bytes";
    let (project, media) = project_with_files(&directory, payload);
    maj_core::format::save_to_path(&project, &project_path).expect("the project saves");
    let path = project_path.to_str().unwrap();

    let printed = maj_ok(&["fingerprint", "--project", path]);
    assert!(printed.contains("procedural"), "{printed}");
    assert!(printed.contains("not there"), "{printed}");

    let after = maj_core::format::load_from_path(&project_path).unwrap();
    let measured = after.asset(MediaAssetId::new(1)).unwrap();
    assert!(measured.is_identified(), "the bytes were measured");
    assert!(
        !measured.is_verified(),
        "and the duration was not: nothing in this build has read the container"
    );
    let fingerprint = measured.fingerprint().expect("a recorded measurement");
    assert_eq!(fingerprint.size_bytes(), payload.len() as u64);
    assert_eq!(
        fingerprint.content_hash(),
        blake3::hash(payload).to_hex().to_string(),
        "the document holds the hash of the bytes and nothing else"
    );
    assert!(matches!(
        maj_media::measure(&media),
        Ok(their) if their.content_hash() == fingerprint.content_hash()
    ));

    // A procedural URI names no file, and a missing one is not a measurement either.
    assert!(
        after
            .asset(MediaAssetId::new(2))
            .unwrap()
            .fingerprint()
            .is_none()
    );
    assert!(
        after
            .asset(MediaAssetId::new(3))
            .unwrap()
            .fingerprint()
            .is_none()
    );

    // And the dry run is a real dry run: nothing in the document moves.
    let dry = directory.join("dry.maj");
    maj_core::format::save_to_path(&project, &dry).expect("a second copy");
    let before = maj_core::format::load_from_path(&dry)
        .unwrap()
        .document_hash()
        .unwrap();
    let said = maj_ok(&[
        "fingerprint",
        "--project",
        dry.to_str().unwrap(),
        "--dry-run",
    ]);
    assert!(said.contains("nothing written"), "{said}");
    let unchanged = maj_core::format::load_from_path(&dry)
        .unwrap()
        .document_hash()
        .unwrap();
    assert_eq!(before, unchanged, "a dry run writes nothing at all");
}

#[test]
fn relinking_follows_the_bytes_and_not_the_name() {
    // A relink by name can resolve two different shots to one asset and nothing reports it.
    // The decoy here has the same length as the real file, so it is hashed and then refused —
    // which is the size doing the job it is in the fingerprint for.
    let directory = scratch("relink");
    let (project, media) = project_with_files(&directory, b"the bytes of shot one");
    let project_path = directory.join("relink.maj");
    maj_core::format::save_to_path(&project, &project_path).expect("the project saves");
    let path = project_path.to_str().unwrap();
    maj_ok(&["fingerprint", "--project", path]);

    // Without a measurement there is no identity to search for, and it says so.
    let unmeasured = directory.join("unmeasured.maj");
    maj_core::format::save_to_path(&project, &unmeasured).expect("a copy");
    let before = maj_core::format::load_from_path(&unmeasured)
        .unwrap()
        .document_hash()
        .unwrap();
    let said = maj_ok(&[
        "relink",
        "--project",
        unmeasured.to_str().unwrap(),
        "--search",
        directory.to_str().unwrap(),
    ]);
    assert!(said.contains("has been measured"), "{said}");
    assert_eq!(
        before,
        maj_core::format::load_from_path(&unmeasured)
            .unwrap()
            .document_hash()
            .unwrap()
    );

    // The card is mounted elsewhere now, under a different name, and a file of the same
    // length with different bytes sits where the original was.
    let moved_to = directory.join("online").join("A001C777.mov");
    std::fs::create_dir_all(moved_to.parent().unwrap()).unwrap();
    std::fs::rename(&media, &moved_to).unwrap();
    std::fs::write(&media, b"the bytes of shot TWO").unwrap();

    let printed = maj_ok(&[
        "relink",
        "--project",
        path,
        "--search",
        directory.to_str().unwrap(),
    ]);
    assert!(printed.contains("rejected on length"), "{printed}");

    let after = maj_core::format::load_from_path(&project_path).unwrap();
    let asset = after.asset(MediaAssetId::new(1)).unwrap();
    assert_eq!(
        asset.uri(),
        maj_media::uri_of(&moved_to),
        "the identity found the file, not the name: {printed}"
    );
    assert!(
        asset.is_identified(),
        "relinking moves where the bytes come from and nothing else"
    );

    // A second run has nothing left to do, because the project now points where the bytes are.
    let again = maj_ok(&[
        "relink",
        "--project",
        path,
        "--search",
        directory.to_str().unwrap(),
    ]);
    assert!(again.contains("nothing to relink"), "{again}");
}

#[test]
fn a_session_recovered_from_its_journal_is_the_session_that_would_have_saved() {
    // P1's own gate, through the interface a person has: the process dies, the journal is
    // replayed, and the document that comes out is the one the session would have written —
    // byte for byte, because the log records who made each edit and the hash covers the log.
    let directory = scratch("recover");
    let (project, _) = project_with_files(&directory, b"bytes");
    let session = directory.join("session.maj");
    maj_core::format::save_to_path(&project, &session).expect("the project saves");
    let journal = maj_core::journal_path_for(&session);
    let path = session.to_str().unwrap();
    let journal_text = journal.to_str().unwrap();

    let commands = [
        r#"{"kind":"set_sequence_start_timecode","sequence":1,"frames":86400}"#,
        r#"{"kind":"set_sequence_start_timecode","sequence":1,"frames":172800}"#,
        r#"{"kind":"relink_asset","asset":1,"uri":"file:///online/A001C001.mov"}"#,
    ];
    for command in commands {
        let printed = maj_ok(&[
            "edit",
            "--project",
            path,
            "--journal",
            journal_text,
            "--no-save",
            "--command",
            command,
        ]);
        assert!(printed.contains("not saved"), "{printed}");
    }
    // Nothing was saved: the document on disk is the one the session started from.
    let on_disk = maj_core::format::load_from_path(&session).unwrap();
    assert_eq!(on_disk.revision(), project.revision());
    assert_eq!(
        on_disk.asset(MediaAssetId::new(1)).unwrap().uri(),
        project.asset(MediaAssetId::new(1)).unwrap().uri()
    );

    let recovered = directory.join("recovered.maj");
    let printed = maj_ok(&[
        "recover",
        "--project",
        path,
        "--out",
        recovered.to_str().unwrap(),
    ]);
    assert!(printed.contains("3 applied"), "{printed}");
    assert!(printed.contains("document hash"), "{printed}");

    // The same three commands, applied to a session that saved after each one.
    let clean = directory.join("clean.maj");
    maj_core::format::save_to_path(&project, &clean).expect("a clean session");
    for command in commands {
        maj_ok(&[
            "edit",
            "--project",
            clean.to_str().unwrap(),
            "--command",
            command,
        ]);
    }
    assert_eq!(
        std::fs::read(&recovered).unwrap(),
        std::fs::read(&clean).unwrap(),
        "the recovered document is the document the session would have saved"
    );
}

#[test]
fn a_journal_that_was_cut_off_recovers_the_prefix_and_says_so() {
    let directory = scratch("torn-journal");
    let (project, _) = project_with_files(&directory, b"bytes");
    let session = directory.join("session.maj");
    maj_core::format::save_to_path(&project, &session).expect("the project saves");
    let journal = maj_core::journal_path_for(&session);
    let path = session.to_str().unwrap();
    let journal_text = journal.to_str().unwrap();

    for frames in [3600, 7200] {
        let command =
            format!(r#"{{"kind":"set_sequence_start_timecode","sequence":1,"frames":{frames}}}"#);
        maj_ok(&[
            "edit",
            "--project",
            path,
            "--journal",
            journal_text,
            "--no-save",
            "--command",
            &command,
        ]);
    }
    // The last write was cut off, which is what a kill during an append leaves behind.
    let text = std::fs::read_to_string(&journal).unwrap();
    let cut = text.len() - 20;
    std::fs::write(&journal, &text.as_bytes()[..cut]).unwrap();

    let before = maj_core::format::load_from_path(&session)
        .unwrap()
        .document_hash()
        .unwrap();
    let printed = maj_ok(&["recover", "--project", path, "--dry-run"]);
    assert!(printed.contains("1 applied"), "{printed}");
    assert!(printed.contains("cut off"), "{printed}");
    assert!(printed.contains("nothing written"), "{printed}");
    assert_eq!(
        before,
        maj_core::format::load_from_path(&session)
            .unwrap()
            .document_hash()
            .unwrap(),
        "a dry run recovers nothing into the document"
    );

    // Recovering for real keeps the prefix and leaves the journal alone, because a partial
    // recovery has work behind it that somebody may still want to look at.
    let out = directory.join("recovered.maj");
    let printed = maj_ok(&["recover", "--project", path, "--out", out.to_str().unwrap()]);
    assert!(printed.contains("left in place"), "{printed}");
    assert!(journal.exists(), "the recovery did not delete the evidence");
    let recovered = maj_core::format::load_from_path(&out).unwrap();
    assert_eq!(recovered.revision(), project.revision() + 1);
}

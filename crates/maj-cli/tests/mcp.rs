//! Two doors into one engine, and the same bytes behind both.
//!
//! The claim this project makes is that the interface is not where the truth lives: a render
//! is a function of the project, whoever asked for it. These tests hold the two interfaces
//! this repository actually ships — `maj render` and `maj mcp` — against each other on the
//! same document, and they check the interesting direction as well: a project an *agent*
//! built over the wire is verified by the command line, and vice versa.
//!
//! If these ever disagree, one of the two is lying about what the engine does, which is the
//! failure this architecture exists to make impossible.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

/// A directory under the target's temporary space, cleared on the way in.
fn scratch(name: &str) -> PathBuf {
    let directory = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a scratch directory");
    directory
}

/// Runs one `maj` command and returns its standard output.
fn maj(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_maj"))
        .args(args)
        .output()
        .expect("the command runs");
    assert!(
        output.status.success(),
        "maj {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("the output is utf-8")
}

/// The digest a manifest records.
fn digest_of(path: &Path) -> String {
    let text = std::fs::read_to_string(path).expect("the manifest is on disk");
    let value: Value = serde_json::from_str(&text).expect("the manifest is JSON");
    value["digest"].as_str().expect("a digest").to_owned()
}

/// An MCP server, spoken to over its own pipes.
struct Wire {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next: u64,
}

impl Wire {
    /// Starts `maj mcp` on a document.
    fn start(project: &Path, extra: &[&str]) -> Self {
        let mut args = vec![
            "mcp".to_owned(),
            "--project".to_owned(),
            project.display().to_string(),
        ];
        args.extend(extra.iter().map(|argument| (*argument).to_owned()));
        let mut child = Command::new(env!("CARGO_BIN_EXE_maj"))
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the server starts");
        let input = child.stdin.take().expect("stdin is piped");
        let output = child.stdout.take().expect("stdout is piped");
        Self {
            child,
            input,
            output: BufReader::new(output),
            next: 1,
        }
    }

    /// Starts `maj mcp` with no document, on an empty project.
    fn standalone(name: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_maj"))
            .args(["mcp", "--name", name])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the server starts");
        let input = child.stdin.take().expect("stdin is piped");
        let output = child.stdout.take().expect("stdout is piped");
        Self {
            child,
            input,
            output: BufReader::new(output),
            next: 1,
        }
    }

    /// Completes the handshake.
    fn ready(project: &Path) -> Self {
        let mut wire = Self::start(project, &[]);
        let response = wire.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "clientInfo": {"name": "cli-test"}}),
        );
        assert_eq!(response["result"]["serverInfo"]["name"], json!("majestat"));
        wire
    }

    /// Sends a request and reads the answer.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.input
            .write_all(message.to_string().as_bytes())
            .expect("written");
        self.input.write_all(b"\n").expect("the line ends");
        self.input.flush().expect("sent");
        let mut line = String::new();
        self.output.read_line(&mut line).expect("readable");
        serde_json::from_str(&line).unwrap_or_else(|error| panic!("{error} in {line}"))
    }

    /// Calls a tool that is expected to succeed, returning its structured answer.
    fn ok(&mut self, name: &str, arguments: Value) -> Value {
        let response = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        assert!(response.get("error").is_none(), "{name}: {response}");
        assert_eq!(
            response["result"]["isError"],
            json!(false),
            "{name}: {}",
            response["result"]["content"][0]["text"]
        );
        response["result"]["structuredContent"].clone()
    }
}

impl Drop for Wire {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A project with one generated shot, one track and one clip with an effect on it.
fn demo(path: &Path) {
    use maj_core::{
        Clip, ClipId, Command, EffectId, EffectInstance, EffectKind, History, MediaAsset,
        MediaAssetId, Project, Sequence, SequenceId, Track, TrackId, TrackKind,
    };
    use maj_time::{FrameRange, Rational, Timebase};

    let mut project = Project::new("mcp cross-check");
    let mut history = History::new();
    history
        .apply_local(
            &mut project,
            Command::CreateMediaAsset {
                index: 0,
                asset: MediaAsset::new(
                    MediaAssetId::new(1),
                    "shot-01",
                    "generated:shot-01",
                    Timebase::FPS_24,
                    240,
                )
                .expect("a valid asset"),
            },
        )
        .expect("applied");
    history
        .apply_local(
            &mut project,
            Command::CreateSequence {
                index: 0,
                sequence: Sequence::new(
                    SequenceId::new(2),
                    "Reel A",
                    Timebase::FPS_24,
                    32,
                    18,
                    48_000,
                )
                .expect("a valid sequence"),
            },
        )
        .expect("applied");
    history
        .apply_local(
            &mut project,
            Command::CreateTrack {
                sequence: SequenceId::new(2),
                index: 0,
                track: Track::new(TrackId::new(3), TrackKind::Video, "V1"),
            },
        )
        .expect("applied");
    history
        .apply_local(
            &mut project,
            Command::AppendClip {
                sequence: SequenceId::new(2),
                track: TrackId::new(3),
                clip: Clip::new(
                    ClipId::new(4),
                    MediaAssetId::new(1),
                    "first",
                    FrameRange::new(0, 60).expect("a range"),
                    0,
                    Rational::from_integer(1),
                )
                .expect("a valid clip"),
            },
        )
        .expect("applied");
    history
        .apply_local(
            &mut project,
            Command::AddEffect {
                sequence: SequenceId::new(2),
                clip: ClipId::new(4),
                index: 0,
                effect: EffectInstance::new(EffectId::new(5), EffectKind::Blur),
            },
        )
        .expect("applied");
    maj_core::format::save_to_path(&project, path).expect("the document is written");
}

#[test]
fn a_frame_rendered_by_the_command_line_and_by_the_server_is_the_same_frame() {
    let directory = scratch("cross-digest");
    let project = directory.join("demo.maj");
    demo(&project);

    // Through the command line, with its manifest as the record.
    let still = directory.join("cli.png");
    maj(&[
        "render",
        "--project",
        project.to_str().expect("a path"),
        "--frame",
        "23",
        "--out",
        still.to_str().expect("a path"),
    ]);
    let cli_digest = digest_of(&directory.join("cli.maj.json"));

    // Through the server, which renders on the same certified path.
    let mut wire = Wire::ready(&project);
    let rendered = wire.ok("maj_render_frame", json!({"sequence": 2, "frame": 23}));
    assert_eq!(rendered["digest"], json!(cli_digest));
    assert_eq!(rendered["manifest"]["policy"], json!("certified"));
    assert_eq!(rendered["width"], json!(32));
    assert_eq!(rendered["height"], json!(18));

    // The server also proves the render repeats, which the command line's verify does.
    let verified = wire.ok("maj_verify_frame", json!({"frame": 23, "tiles": "2x2"}));
    assert_eq!(verified["verified"], json!(true));

    // The state hash in the manifest is the same state both interfaces saw.
    let state = wire.ok("maj_project_state", json!({}));
    assert_eq!(
        rendered["manifest"]["state_hash"], state["digests"]["state"],
        "the render's recorded state and the session's state disagree"
    );
}

#[test]
fn a_project_an_agent_builds_over_the_wire_verifies_from_the_command_line() {
    let directory = scratch("cross-agent");
    let project = directory.join("agent.maj");

    // The agent starts from nothing and builds the whole thing. A server opened on a document
    // that does not exist refuses to start, which is why this one is not given one.
    let mut wire = Wire::standalone("agent");
    wire.request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name": "cli-test"}}),
    );
    wire.ok(
        "maj_create_media_asset",
        json!({"asset": {"name": "shot-01", "uri": "generated:shot-01", "timebase": "24",
                          "duration_frames": 240}}),
    );
    wire.ok(
        "maj_setup_sequence",
        json!({"name": "Reel A", "timebase": "24", "width": 32, "height": 18,
                "tracks": ["video"]}),
    );
    // The identifiers come from the answers above: the asset is one, the sequence two, its
    // track three, so the clip the engine allocates is four.
    wire.ok(
        "maj_place_clip",
        json!({"sequence": 2, "kind": "video", "asset": 1, "source_range": [0, 60],
                "timeline_start": 0, "name": "first"}),
    );
    wire.ok(
        "maj_add_effect",
        json!({"sequence": 2, "clip": 4, "effect": {"kind": "blur", "params": {"radius": 3}}}),
    );
    wire.ok(
        "maj_save_project",
        json!({"path": project.display().to_string()}),
    );
    drop(wire);

    // The command line reads what the agent wrote, and reproduces it exactly.
    let report = maj(&["probe", "--project", project.to_str().expect("a path")]);
    assert!(report.contains("shot-01"), "{report}");
    assert!(report.contains("blur"), "{report}");

    let still = directory.join("agent.png");
    let manifest = directory.join("agent.maj.json");
    maj(&[
        "render",
        "--project",
        project.to_str().expect("a path"),
        "--frame",
        "12",
        "--out",
        still.to_str().expect("a path"),
    ]);
    let from_cli = digest_of(&manifest);

    // And the two ways of rendering that frame agree, which is the verification gate.
    let verified = maj(&[
        "verify",
        "--project",
        project.to_str().expect("a path"),
        "--frame",
        "12",
        "--tiles",
        "2x2",
    ]);
    assert!(
        verified.contains("OK: the same bytes three ways"),
        "{verified}"
    );

    // The frame the agent saw is the frame the command line writes.
    let mut wire = Wire::ready(&project);
    let rendered = wire.ok("maj_render_frame", json!({"frame": 12}));
    assert_eq!(rendered["digest"], json!(from_cli));
    drop(wire);

    // A read-only server answers questions and refuses every edit, whatever it is asked.
    let mut wire = Wire::start(&project, &["--read-only"]);
    wire.request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name": "cli-test"}}),
    );
    let refused = wire.request(
        "tools/call",
        json!({"name": "maj_undo", "arguments": {"count": 1}}),
    );
    assert_eq!(refused["result"]["isError"], json!(true));
    let state = wire.ok("maj_project_state", json!({}));
    assert_eq!(
        state["revision"],
        json!(5),
        "nothing a read-only server was asked could change the project"
    );
}

//! The protocol end to end, through a real process on a real pipe.
//!
//! Every other test in this crate calls the server as a library, which is fast and proves the
//! logic. This file proves the *interface*: that the program an MCP client launches speaks
//! the protocol over its own standard input and output, that a session survives a malformed
//! line, that a picture arrives as base64, and that every tool in the published catalogue can
//! be called with the example the catalogue itself prints.
//!
//! The last one is the point. A catalogue is documentation, and documentation that is never
//! executed is a claim; here each tool's worked call is sent over the wire and the answer is
//! checked against the protocol's own shape.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Map, Value, json};

/// A server process, with a way to send it a message and read the answer.
struct Wire {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next: u64,
}

impl Wire {
    /// Starts a server with the given arguments.
    ///
    /// The working directory is a scratch one, so a tool whose example writes a file writes
    /// it there rather than into the repository.
    fn start(arguments: &[&str]) -> Self {
        let scratch = scratch();
        let mut child = Command::new(env!("CARGO_BIN_EXE_maj-mcp"))
            .args(arguments)
            .current_dir(&scratch)
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

    /// A server that has completed the handshake.
    fn ready() -> Self {
        let mut wire = Self::start(&[]);
        let response = wire.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "wire-test", "version": "1"},
            }),
        );
        assert_eq!(response["result"]["protocolVersion"], json!("2025-06-18"));
        wire.notify("notifications/initialized", json!({}));
        wire
    }

    /// Sends one raw line and reads one line back.
    fn raw(&mut self, line: &str) -> Value {
        self.input
            .write_all(line.as_bytes())
            .expect("the message is written");
        self.input.write_all(b"\n").expect("the line ends");
        self.input.flush().expect("the message is sent");
        self.read()
    }

    /// Reads one message.
    fn read(&mut self) -> Value {
        let mut line = String::new();
        let read = self
            .output
            .read_line(&mut line)
            .expect("the response is readable");
        assert!(read > 0, "the server closed the connection");
        serde_json::from_str(&line).unwrap_or_else(|error| panic!("not JSON: {error} in {line}"))
    }

    /// Sends a request and returns its answer.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let response = self.raw(&message.to_string());
        assert_eq!(response["jsonrpc"], json!("2.0"));
        assert_eq!(response["id"], json!(id));
        response
    }

    /// Sends a notification. The server answers notifications with nothing.
    fn notify(&mut self, method: &str, params: Value) {
        let message = json!({"jsonrpc": "2.0", "method": method, "params": params});
        self.input
            .write_all(message.to_string().as_bytes())
            .expect("written");
        self.input.write_all(b"\n").expect("the line ends");
        self.input.flush().expect("sent");
    }

    /// Calls a tool and returns the whole response.
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))
    }

    /// Calls a tool that is expected to succeed, returning its structured answer.
    fn ok(&mut self, name: &str, arguments: Value) -> Value {
        let response = self.call(name, arguments);
        assert!(
            response.get("error").is_none(),
            "{name} answered with a protocol error: {response}"
        );
        assert_eq!(
            response["result"]["isError"],
            json!(false),
            "{name} failed: {}",
            response["result"]["content"][0]["text"]
        );
        response["result"]["structuredContent"].clone()
    }
}

impl Drop for Wire {
    fn drop(&mut self) {
        // Closing the pipe ends the server's own loop; the kill is the backstop for a test
        // that panicked halfway through a message.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The scratch directory these tests run in.
fn scratch() -> std::path::PathBuf {
    let directory = std::env::temp_dir().join("maj-mcp-wire-tests");
    std::fs::create_dir_all(&directory).expect("a scratch directory");
    directory
}

/// The arguments out of a tool's published description.
fn example(tool: &Value) -> Map<String, Value> {
    let description = tool["description"].as_str().expect("a description");
    let example = description
        .rsplit_once("worked call:\n")
        .map(|(_, example)| example.trim())
        .unwrap_or_else(|| panic!("{} publishes no worked call", tool["name"]));
    match serde_json::from_str(example).expect("the example is JSON") {
        Value::Object(map) => map,
        other => panic!("the example is not an object: {other}"),
    }
}

#[test]
fn a_client_edits_a_project_over_the_wire_and_saves_it() {
    let mut wire = Wire::ready();
    let path = scratch().join("session.maj");

    // The catalogue is what a client reads first.
    let listing = wire.request("tools/list", json!({}));
    let tools = listing["result"]["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), maj_mcp::TOOLS.len());
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(names.contains(&"maj_place_clip"), "{names:?}");
    assert!(names.contains(&"maj_render_frame"), "{names:?}");

    // Two commands build the project, and the answer says which revision they reached.
    let asset = wire.ok(
        "maj_create_media_asset",
        json!({"asset": {"name": "shot-01", "uri": "generated:shot-01",
                          "timebase": "24000/1001", "duration_frames": 240}}),
    );
    assert_eq!(asset["revision"], json!(1));
    assert_eq!(
        asset["command"]["asset"]["id"],
        json!(1),
        "the identifier was allocated for the caller"
    );
    let sequence = wire.ok(
        "maj_setup_sequence",
        json!({"name": "Reel A", "timebase": "24000/1001", "width": 64, "height": 36,
                "tracks": ["video"]}),
    );
    // The asset took identifier one, the sequence two, so the track the engine creates is
    // three: every identifier in this session comes from an answer, never from a guess.
    assert_eq!(sequence["tracks"], json!(["track#3 V1"]));

    // An edit through a tool that appends exactly one command.
    let placed = wire.ok(
        "maj_place_clip",
        json!({"sequence": 2, "kind": "video", "asset": 1,
                "source_range": [0, 48], "timeline_start": 0, "name": "first"}),
    );
    assert_eq!(placed["revision"], json!(4));

    // A frame comes back as a picture, and the picture is a PNG.
    let rendered = wire.ok("maj_render_frame", json!({"sequence": 2, "frame": 12}));
    let digest = rendered["digest"].as_str().expect("a digest").to_owned();
    assert_eq!(digest.len(), 64);
    assert_eq!(rendered["manifest"]["policy"], json!("certified"));
    let response = wire.call("maj_render_frame", json!({"frame": 12}));
    let image = response["result"]["content"][1]["data"]
        .as_str()
        .expect("the picture is base64")
        .to_owned();
    assert!(
        image.starts_with("iVBORw0KGgo"),
        "the picture is not a PNG: {}",
        &image[..12.min(image.len())]
    );
    assert_eq!(
        response["result"]["content"][1]["mimeType"],
        json!("image/png")
    );

    // A resource delivers the same frame to a client that only reads.
    let frame_resource = wire.request("resources/read", json!({"uri": "maj://frame/2/12"}));
    assert_eq!(
        frame_resource["result"]["contents"][0]["mimeType"],
        json!("image/png")
    );
    assert!(
        frame_resource["result"]["contents"][0]["blob"]
            .as_str()
            .expect("a blob")
            .starts_with("iVBORw0KGgo")
    );

    // The log says who made each change, and the state is a function of that log.
    let history = wire.ok("maj_history", json!({}));
    let entries = history["entries"].as_array().expect("entries");
    assert!(entries.len() >= 4);
    assert!(
        entries
            .iter()
            .all(|entry| entry["author"] == json!("mcp:wire-test")),
        "{history}"
    );
    let check = wire.ok("maj_replay_check", json!({}));
    assert_eq!(check["consistent"], json!(true));

    // Undo through the wire, then save what is left.
    let undone = wire.ok("maj_undo", json!({"count": 1}));
    let after = wire.ok("maj_project_state", json!({}));
    assert_eq!(
        undone["revision"], after["revision"],
        "the answer and the state agree"
    );
    assert_eq!(after["revision"], json!(3), "the clip was the last command");
    let saved = wire.call(
        "maj_save_project",
        json!({"path": path.display().to_string()}),
    );
    let text = saved["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(text.contains("session.maj"), "{text}");
    assert!(text.contains("document hash"), "{text}");

    // The document on disk is a real `.maj` that the model accepts.
    let project = maj_core::format::load_from_path(&path).expect("the document loads");
    project.validate().expect("the document is valid");
    // The undo was saved with it, so the document holds three commands, not four.
    assert_eq!(project.revision(), 3);
    assert_eq!(
        project.log().len(),
        3,
        "the log is what the document stores"
    );
    assert_eq!(project.assets().len(), 1);
    assert!(project.sequences()[0].tracks()[0].clips().is_empty());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_same_edits_twice_produce_the_same_state_hash() {
    // Determinism, at the interface a client actually uses: two servers, one script.
    let script = |wire: &mut Wire| -> String {
        wire.ok(
            "maj_create_media_asset",
            json!({"asset": {"name": "shot", "uri": "generated:shot", "timebase": "24",
                              "duration_frames": 120}}),
        );
        wire.ok(
            "maj_setup_sequence",
            json!({"name": "Reel", "timebase": "24", "width": 32, "height": 18,
                    "tracks": ["video"]}),
        );
        wire.ok(
            "maj_place_clip",
            json!({"sequence": 2, "kind": "video", "asset": 1,
                    "source_range": [0, 24], "timeline_start": 0}),
        );
        let state = wire.ok("maj_project_state", json!({}));
        state["digests"]["state"]
            .as_str()
            .expect("a state hash")
            .to_owned()
    };
    let mut first = Wire::ready();
    let once = script(&mut first);
    drop(first);
    let mut second = Wire::ready();
    let twice = script(&mut second);
    assert_eq!(once, twice, "two sessions of the same script diverged");
}

#[test]
fn every_tool_in_the_catalogue_answers_its_own_published_example() {
    let mut wire = Wire::ready();
    let listing = wire.request("tools/list", json!({}));
    let tools = listing["result"]["tools"]
        .as_array()
        .expect("tools")
        .clone();
    assert!(tools.len() > 30, "the catalogue shrank unexpectedly");
    for tool in &tools {
        let name = tool["name"].as_str().expect("a name");
        let arguments = example(tool);
        let response = wire.call(name, json!(arguments));
        assert!(
            response.get("error").is_none(),
            "{name} refused its own example with a protocol error: {response}"
        );
        assert!(
            response["result"]["isError"].is_boolean(),
            "{name} answered without a status: {response}"
        );
        // The session survives every call, including the ones that could not work yet.
        let pong = wire.request("ping", json!({}));
        assert_eq!(pong["result"], json!({}));
    }
}

#[test]
fn a_malformed_line_and_an_unknown_tool_do_not_end_the_session() {
    let mut wire = Wire::ready();
    let broken = wire.raw("{ this is not json");
    assert_eq!(broken["error"]["code"], json!(-32_700));
    let unknown = wire.call("maj_do_the_impossible", json!({}));
    assert_eq!(unknown["error"]["code"], json!(-32_602));
    // A batch is answered as a batch, and a blank line is not a message at all.
    wire.input.write_all(b"\n").expect("written");
    wire.input.flush().expect("sent");
    let batch = wire.raw(r#"[{"jsonrpc":"2.0","id":77,"method":"ping"}]"#);
    assert_eq!(batch["id"], json!(77));
    assert_eq!(batch["result"], json!({}));
    // And the session is still a session.
    let timeline = wire.ok("maj_timeline", json!({}));
    assert_eq!(timeline["sequences"], json!([]));
}

#[test]
fn a_server_started_on_a_document_answers_about_it_and_refuses_its_missing_sibling() {
    let directory = scratch();
    let path = directory.join("opened.maj");
    maj_core::format::save_to_path(&maj_core::Project::new("opened from disk"), &path)
        .expect("written");

    let mut wire = Wire::start(&["--project", path.to_str().expect("a path")]);
    let response = wire.request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "clientInfo": {"name": "wire-test"}}),
    );
    assert_eq!(response["result"]["serverInfo"]["name"], json!("majestat"));
    let state = wire.ok("maj_project_state", json!({}));
    assert_eq!(state["project"], json!("opened from disk"));
    drop(wire);

    // A document that does not exist is refused before the handshake, with a non-zero exit.
    let missing = directory.join("absent.maj");
    let output = Command::new(env!("CARGO_BIN_EXE_maj-mcp"))
        .args(["--project", missing.to_str().expect("a path")])
        .stdin(Stdio::null())
        .output()
        .expect("the program runs");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("absent.maj"), "{stderr}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_catalogue_can_be_printed_without_starting_a_server() {
    let output = Command::new(env!("CARGO_BIN_EXE_maj-mcp"))
        .arg("--list-tools")
        .output()
        .expect("the program runs");
    assert!(output.status.success());
    let listing: Value = serde_json::from_slice(&output.stdout).expect("the catalogue is JSON");
    let tools = listing["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), maj_mcp::TOOLS.len());
    assert_eq!(tools[0]["inputSchema"]["type"], json!("object"));
    // Standard error is where a person's messages go, and it is empty for this flag.
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
}

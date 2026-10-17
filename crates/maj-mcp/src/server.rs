//! The server: the handshake, the method table, and the loop that reads lines.
//!
//! Everything above this module is about the editor — the catalogue, the decoder, the
//! sessions, the renderer. This module is about the protocol: one line in, zero or one line
//! out, and a table that turns a method name into an answer. It is the only place that knows
//! the two are connected.
//!
//! # The handshake, and what it decides
//!
//! `initialize` establishes three things. The **revision**, which is negotiated rather than
//! assumed. The **capabilities**, which are declared honestly rather than maximally: this
//! server offers tools, resources and prompts, and does not claim to push notifications,
//! subscribe to resources or carry logs. And the **author** of every edit that follows, taken
//! from the client's own name, so the command log records which program made a change. That
//! last one is not protocol bookkeeping: the log is the product, and an unattributed edit in
//! it is an edit nobody can review.
//!
//! Before `initialize` completes, every request but `initialize` and `ping` is refused with
//! the protocol's own error for it. That is deliberate: a client that has not said which
//! revision it speaks cannot be answered in a way it is obliged to understand.
//!
//! # Standard output is the protocol
//!
//! Nothing in this module writes to standard output except a response. A client that launches
//! a server and finds a friendly banner in the middle of its message stream has found a bug,
//! so banners go to standard error and nowhere else.

use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::catalog::{TOOLS, advertised};
use crate::prompts;
use crate::protocol::{self, Fault, Incoming, Notification, Request};
use crate::resources;
use crate::session::{DEFAULT_SESSION, Session, SessionError, Sessions};
use crate::tools::{self, Outcome};

/// The name this server reports to a client.
pub const SERVER_NAME: &str = "majestat";

/// What a client is told about the engine before it calls anything.
///
/// A model that does not know the rules will break them on its first call and spend its
/// turns discovering why, so the rules are stated once, here, in the order they matter.
pub const INSTRUCTIONS: &str = "\
Majestat is a deterministic video editing engine. Every edit is one immutable command \
appended to a log: the project is a pure function of that log, so anything you do can be \
undone exactly, replayed, diffed and attributed to you.\n\
\n\
How to work here:\n\
- Read before you write. `maj_project_state` and `maj_timeline` are the only source of truth \
about what exists, and every identifier you use must come from them.\n\
- Every editing tool appends exactly one command. Identifiers are allocated for you when you \
omit them, so never invent one.\n\
- Numbers are exact. Write `0.5`, `\"1/2\"` or `{\"num\":1,\"den\":2}`; the engine converts a \
decimal to a ratio without rounding.\n\
- Look at your work. `maj_render_frame` returns the picture and its digest, and \
`maj_verify_frame` proves the render repeats. A judgement about an image that was never \
rendered is a guess.\n\
- A refusal is information, not a failure: the answer will say which rule was broken. Report \
it rather than working around it.\n\
- Nothing you do is committed until `maj_save_project`, and `maj_undo` reverts exactly.\n";

/// How a server is started.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// A document to open into the starting session.
    pub project: Option<PathBuf>,
    /// The name of the project to start when no document was named.
    pub name: Option<String>,
    /// The name of the starting session.
    pub session: Option<String>,
    /// Whether edits are refused.
    pub read_only: bool,
}

impl Options {
    /// Options that open a document.
    #[must_use]
    pub fn with_project(path: impl Into<PathBuf>) -> Self {
        Self {
            project: Some(path.into()),
            ..Self::default()
        }
    }

    /// The same options, with a name for a project that is not opened from a file.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The same options, refusing every edit.
    ///
    /// For a client that is reviewing rather than cutting: a read-only server answers every
    /// question and refuses every change, which is a policy the client cannot override by
    /// calling a different tool.
    #[must_use]
    pub fn locked(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// The same options, under another session name.
    #[must_use]
    pub fn under(mut self, session: impl Into<String>) -> Self {
        self.session = Some(session.into());
        self
    }
}

/// Why a server could not be started or run.
#[derive(Debug)]
pub enum ServeError {
    /// The document could not be opened.
    Session(SessionError),
    /// The transport failed.
    Io(std::io::Error),
}

impl fmt::Display for ServeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ServeError {}

impl From<SessionError> for ServeError {
    fn from(error: SessionError) -> Self {
        Self::Session(error)
    }
}

impl From<std::io::Error> for ServeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// A server, with the sessions it holds and the client it is talking to.
#[derive(Debug)]
pub struct Server {
    options: Options,
    sessions: Sessions,
    client: Option<String>,
    revision: &'static str,
}

impl Server {
    /// Builds a server, opening the project it was started with.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::Core`] when a document was named and cannot be read or is
    /// rejected by the model's own load gates. Failing here rather than on the first call is
    /// the point: a server that cannot open its project should say so before a client starts
    /// asking it questions.
    pub fn new(options: Options) -> Result<Self, SessionError> {
        let name = options
            .session
            .clone()
            .unwrap_or_else(|| DEFAULT_SESSION.to_owned());
        let session = match &options.project {
            Some(path) => Session::open(name.clone(), path, "mcp")?,
            None => Session::new(
                name.clone(),
                options
                    .name
                    .clone()
                    .unwrap_or_else(|| "untitled".to_owned()),
                "mcp",
            ),
        };
        Ok(Self {
            options,
            sessions: Sessions::new(name, session),
            client: None,
            revision: protocol::LATEST_REVISION,
        })
    }

    /// The sessions this server holds.
    #[must_use]
    pub const fn sessions(&self) -> &Sessions {
        &self.sessions
    }

    /// The sessions, mutably, for a host that wants to open one itself.
    pub const fn sessions_mut(&mut self) -> &mut Sessions {
        &mut self.sessions
    }

    /// The revision being spoken.
    #[must_use]
    pub const fn revision(&self) -> &'static str {
        self.revision
    }

    /// The client's name, once it has said what it is.
    #[must_use]
    pub fn client(&self) -> Option<&str> {
        self.client.as_deref()
    }

    /// Reads one line, writes whatever it calls for, and reports how many messages went out.
    ///
    /// A blank line writes nothing; a batch writes one message per element, including for the
    /// elements that were malformed, because a batch is answered as a batch.
    ///
    /// # Errors
    ///
    /// Returns the transport's error when the response cannot be written.
    pub fn handle_line(&mut self, line: &str, out: &mut impl Write) -> std::io::Result<usize> {
        let mut written = 0;
        for message in protocol::decode(line) {
            let value = match message {
                Ok(Incoming::Notification(notification)) => {
                    self.take_notification(&notification);
                    None
                }
                Ok(Incoming::Request(request)) => Some(self.answer(&request)),
                Err(fault) => Some(protocol::failure(&fault)),
            };
            if let Some(value) = value {
                out.write_all(protocol::encode(&value).as_bytes())?;
                written += 1;
            }
        }
        Ok(written)
    }

    /// Runs until the input ends, flushing after every message.
    ///
    /// Flushing per message is not an optimisation missed: a request that waits for a
    /// response that is sitting in a buffer is a deadlock, and the protocol's own transport is
    /// a request-response one.
    ///
    /// # Errors
    ///
    /// Returns the transport's error on either side.
    pub fn serve(&mut self, input: impl BufRead, out: &mut impl Write) -> std::io::Result<()> {
        for line in input.lines() {
            self.handle_line(&line?, out)?;
            out.flush()?;
        }
        Ok(())
    }

    /// Answers one request.
    fn answer(&mut self, request: &Request) -> Value {
        match self.dispatch(request) {
            Ok(result) => protocol::reply(request.id(), result),
            Err(fault) => protocol::failure(&fault.addressed_to(request.id().clone())),
        }
    }

    /// Turns a method name into an answer.
    fn dispatch(&mut self, request: &Request) -> Result<Value, Fault> {
        let method = request.method();
        if self.client.is_none() && !matches!(method, "initialize" | "ping") {
            return Err(Fault::not_initialized());
        }
        match method {
            "initialize" => Ok(self.initialize(request)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": TOOLS.iter().map(advertised).collect::<Vec<_>>(),
            })),
            "tools/call" => self.call_tool(request),
            "resources/list" => Ok(resources::list(self.sessions.default_name())),
            "resources/templates/list" => Ok(resources::templates()),
            "resources/read" => self.read_resource(request),
            "prompts/list" => Ok(prompts::list()),
            "prompts/get" => self.get_prompt(request),
            other => Err(Fault::method_not_found(other)),
        }
    }

    /// Completes the handshake.
    fn initialize(&mut self, request: &Request) -> Value {
        self.revision = protocol::negotiate(request.text("protocolVersion"));
        let client = request
            .get("clientInfo")
            .and_then(|info| info.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("client")
            .to_owned();
        // The author is what the log will say. `mcp:` keeps an agent's edits distinguishable
        // from a person's without a second table to join against.
        self.sessions.set_author(&format!("mcp:{client}"));
        self.client = Some(client);
        json!({
            "protocolVersion": self.revision,
            "capabilities": {
                "tools": {"listChanged": false},
                "resources": {"subscribe": false, "listChanged": false},
                "prompts": {"listChanged": false},
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "title": "Majestat",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "instructions": INSTRUCTIONS,
        })
    }

    /// Calls a tool.
    fn call_tool(&mut self, request: &Request) -> Result<Value, Fault> {
        let Some(name) = request.text("name") else {
            return Err(Fault::invalid_params("`name` is required"));
        };
        let arguments = match request.get("arguments") {
            None => Map::new(),
            Some(Value::Object(arguments)) => arguments.clone(),
            Some(_) => return Err(Fault::invalid_params("`arguments` must be an object")),
        };
        if self.options.read_only && crate::catalog::tool(name).is_some_and(|tool| !tool.read_only)
        {
            return Ok(result(&Outcome::error(format!(
                "this server is read-only, and `{name}` would change the project"
            ))));
        }
        match tools::call(&mut self.sessions, name, &arguments) {
            Ok(outcome) => Ok(result(&outcome)),
            // The one failure that is a protocol error: a client asked for a tool that does
            // not exist, which is a mistake about the catalogue rather than about the project.
            Err(error) => Err(Fault::invalid_params(error.to_string())),
        }
    }

    /// Reads a resource.
    fn read_resource(&self, request: &Request) -> Result<Value, Fault> {
        let Some(uri) = request.text("uri") else {
            return Err(Fault::invalid_params("`uri` is required"));
        };
        match resources::read(&self.sessions, uri) {
            Ok(contents) => Ok(json!({"contents": [contents.to_value()]})),
            Err(error) => Err(Fault::invalid_params(error.to_string())),
        }
    }

    /// Renders a prompt.
    fn get_prompt(&self, request: &Request) -> Result<Value, Fault> {
        let Some(name) = request.text("name") else {
            return Err(Fault::invalid_params("`name` is required"));
        };
        let arguments = request.get("arguments").and_then(Value::as_object);
        prompts::get(name, arguments).ok_or_else(|| {
            Fault::invalid_params(format!("`{name}` is not a prompt this server offers"))
        })
    }

    /// Handles a notification.
    ///
    /// Both of the protocol's own notifications are acknowledgements of what this server has
    /// already done — `initialized` closes the handshake, `cancelled` says a request the
    /// client abandoned will never be answered — and neither changes anything here. A
    /// notification for an unknown method is ignored rather than refused, which is what
    /// JSON-RPC requires: there is no reply to send a complaint in.
    fn take_notification(&mut self, notification: &Notification) {
        match notification.method() {
            "notifications/initialized" | "notifications/cancelled" => {}
            _ => {}
        }
    }
}

/// A tool call's result, as `tools/call` sends it.
fn result(outcome: &Outcome) -> Value {
    let mut value = json!({
        "content": outcome.content(),
        "isError": outcome.is_error,
    });
    if let Some(structured) = &outcome.structured {
        value["structuredContent"] = structured.clone();
    }
    value
}

/// Runs a server on the process's own standard input and output.
///
/// # Errors
///
/// Returns [`ServeError`] when the project cannot be opened or the transport fails.
pub fn serve(options: Options) -> Result<(), ServeError> {
    let mut server = Server::new(options)?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    let outcome = server.serve(BufReader::new(stdin.lock()), &mut out);
    out.flush()?;
    outcome?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A server, with a way to send it one line and read what came back.
    struct Harness {
        server: Server,
        out: Vec<u8>,
    }

    impl Harness {
        fn new(options: Options) -> Self {
            Self {
                server: Server::new(options).expect("a server"),
                out: Vec::new(),
            }
        }

        fn ready() -> Self {
            let mut harness = Self::new(Options::default());
            let handshake = harness.send(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"harness","version":"1"}}}"#,
            );
            assert_eq!(handshake["result"]["protocolVersion"], json!("2025-06-18"));
            harness
        }

        fn send(&mut self, line: &str) -> Value {
            self.out.clear();
            let written = self
                .server
                .handle_line(line, &mut self.out)
                .expect("the response is written");
            assert_eq!(written, 1, "expected exactly one message for: {line}");
            serde_json::from_slice(&self.out).expect("the response is JSON")
        }

        fn send_silently(&mut self, line: &str) {
            self.out.clear();
            let written = self
                .server
                .handle_line(line, &mut self.out)
                .expect("nothing to write");
            assert_eq!(written, 0, "expected no message for: {line}");
            assert!(self.out.is_empty());
        }

        fn call(&mut self, name: &str, arguments: Value) -> Value {
            self.send(
                &json!({
                    "jsonrpc": "2.0",
                    "id": 9,
                    "method": "tools/call",
                    "params": {"name": name, "arguments": arguments},
                })
                .to_string(),
            )
        }
    }

    #[test]
    fn the_handshake_declares_the_revision_the_capabilities_and_the_author() {
        let mut harness = Harness::new(Options::default());
        let response = harness.send(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{"name":"claude-desktop","version":"0.7"}}}"#,
        );
        let result = &response["result"];
        assert_eq!(result["protocolVersion"], json!("2024-11-05"));
        assert_eq!(result["serverInfo"]["name"], json!(SERVER_NAME));
        assert_eq!(
            result["serverInfo"]["version"],
            json!(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(result["capabilities"]["tools"]["listChanged"], json!(false));
        assert_eq!(
            result["capabilities"]["resources"]["subscribe"],
            json!(false)
        );
        assert!(
            result["instructions"]
                .as_str()
                .expect("instructions")
                .contains("deterministic")
        );
        assert_eq!(harness.server.client(), Some("claude-desktop"));
        // The author is what every edit after this will be recorded as.
        assert_eq!(
            harness.server.sessions().get(None).expect("open").author(),
            "mcp:claude-desktop"
        );
    }

    #[test]
    fn an_unknown_revision_is_answered_in_the_newest_one_this_server_speaks() {
        let mut harness = Harness::new(Options::default());
        let response = harness.send(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2030-01-01"}}"#,
        );
        assert_eq!(
            response["result"]["protocolVersion"],
            json!(protocol::LATEST_REVISION)
        );
    }

    #[test]
    fn a_request_before_the_handshake_is_refused_with_the_protocols_own_error() {
        let mut harness = Harness::new(Options::default());
        let response =
            harness.send(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#);
        assert_eq!(response["error"]["code"], json!(-32_002));
        assert!(
            response["error"]["message"]
                .as_str()
                .expect("message")
                .contains("initialize")
        );
        // Ping is allowed before it, because it is how a client checks the pipe.
        let pong = harness.send(r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#);
        assert_eq!(pong["result"], json!({}));
    }

    #[test]
    fn the_catalogue_lists_every_tool_with_a_schema_and_an_example() {
        let mut harness = Harness::ready();
        let response =
            harness.send(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#);
        let tools = response["result"]["tools"].as_array().expect("a list");
        assert_eq!(tools.len(), TOOLS.len());
        for tool in tools {
            let name = tool["name"].as_str().expect("a name");
            assert!(
                tool["description"]
                    .as_str()
                    .expect("a description")
                    .contains("worked call")
            );
            assert_eq!(tool["inputSchema"]["type"], json!("object"));
            assert_eq!(tool["inputSchema"]["additionalProperties"], json!(false));
            // Every tool a schema advertises must be callable and must refuse invented
            // arguments rather than ignoring them.
            let refused = harness.call(name, json!({"no_such_argument": 1}));
            assert_eq!(refused["result"]["isError"], json!(true), "{name}");
        }
    }

    #[test]
    fn an_edit_over_the_wire_changes_the_project_and_is_attributed() {
        let mut harness = Harness::new(Options::default());
        harness.send(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"harness"}}}"#,
        );
        let created = harness.call(
            "maj_setup_sequence",
            json!({"name":"Reel","timebase":"24","width":64,"height":36,"tracks":["video"]}),
        );
        assert_eq!(created["result"]["isError"], json!(false));
        assert_eq!(created["result"]["structuredContent"]["sequence"], json!(1));
        let timeline = harness.call("maj_timeline", json!({}));
        assert!(
            timeline["result"]["content"][0]["text"]
                .as_str()
                .expect("text")
                .contains("sequence#1 Reel")
        );
        let history = harness.send(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"maj_history","arguments":{}}}"#,
        );
        let entries = history["result"]["structuredContent"]["entries"]
            .as_array()
            .expect("entries");
        assert_eq!(entries.len(), 2);
        assert!(
            entries
                .iter()
                .all(|entry| entry["author"] == json!("mcp:harness"))
        );
    }

    #[test]
    fn a_refusal_is_a_result_with_is_error_and_the_project_is_untouched() {
        let mut harness = Harness::ready();
        let refused = harness.call(
            "maj_create_track",
            json!({"sequence": 99, "track": {"kind": "video", "name": "V1"}}),
        );
        assert_eq!(refused["result"]["isError"], json!(true));
        assert!(
            refused["result"]["content"][0]["text"]
                .as_str()
                .expect("text")
                .contains("99"),
            "{refused}"
        );
        let state = harness.call("maj_project_state", json!({}));
        assert_eq!(state["result"]["structuredContent"]["revision"], json!(0));
    }

    #[test]
    fn a_tool_that_does_not_exist_is_a_protocol_error() {
        let mut harness = Harness::ready();
        let response = harness.call("maj_do_the_impossible", json!({}));
        assert_eq!(response["error"]["code"], json!(-32_602));
        assert!(
            response["error"]["message"]
                .as_str()
                .expect("message")
                .contains("tools/list")
        );
    }

    #[test]
    fn an_unknown_method_is_refused_by_name() {
        let mut harness = Harness::ready();
        let response =
            harness.send(r#"{"jsonrpc":"2.0","id":4,"method":"sampling/createMessage"}"#);
        assert_eq!(response["error"]["code"], json!(-32_601));
        assert!(
            response["error"]["message"]
                .as_str()
                .expect("message")
                .contains("sampling/createMessage")
        );
    }

    #[test]
    fn a_server_in_read_only_mode_refuses_every_edit_and_allows_every_question() {
        let mut harness = Harness::new(Options::default().locked());
        harness.send(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
        let refused = harness.call(
            "maj_setup_sequence",
            json!({"name":"Reel","timebase":"24","width":64,"height":36,"tracks":["video"]}),
        );
        assert_eq!(refused["result"]["isError"], json!(true));
        assert!(
            refused["result"]["content"][0]["text"]
                .as_str()
                .expect("text")
                .contains("read-only")
        );
        let allowed = harness.call("maj_timeline", json!({}));
        assert_eq!(allowed["result"]["isError"], json!(false));
        assert_eq!(
            allowed["result"]["structuredContent"]["sequences"],
            json!([])
        );
    }

    #[test]
    fn a_malformed_line_is_a_parse_error_addressed_to_nobody() {
        let mut harness = Harness::ready();
        let response = harness.send("{not json at all");
        assert_eq!(response["error"]["code"], json!(-32_700));
        assert_eq!(response["id"], json!(null));
    }

    #[test]
    fn a_batch_is_answered_as_a_batch_including_for_the_element_that_was_wrong() {
        let mut harness = Harness::ready();
        harness.out.clear();
        let written = harness
            .server
            .handle_line(
                r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","id":2,"method":"nope"}]"#,
                &mut harness.out,
            )
            .expect("written");
        assert_eq!(written, 2);
        let text = std::str::from_utf8(&harness.out).expect("utf-8");
        let responses: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("each response is JSON"))
            .collect();
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["result"], json!({}));
        assert_eq!(responses[1]["error"]["code"], json!(-32_601));
    }

    #[test]
    fn notifications_are_acknowledged_with_silence() {
        let mut harness = Harness::ready();
        harness.send_silently(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        harness.send_silently(
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
        );
        harness.send_silently(r#"{"jsonrpc":"2.0","method":"notifications/whatever"}"#);
    }

    #[test]
    fn resources_and_prompts_are_readable_over_the_wire() {
        let mut harness = Harness::ready();
        let listing =
            harness.send(r#"{"jsonrpc":"2.0","id":1,"method":"resources/list","params":{}}"#);
        assert_eq!(
            listing["result"]["resources"].as_array().map(Vec::len),
            Some(5)
        );
        let templates = harness
            .send(r#"{"jsonrpc":"2.0","id":2,"method":"resources/templates/list","params":{}}"#);
        assert_eq!(
            templates["result"]["resourceTemplates"]
                .as_array()
                .map(Vec::len),
            Some(3)
        );
        let document = harness.send(
            r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"maj://project"}}"#,
        );
        let text = document["result"]["contents"][0]["text"]
            .as_str()
            .expect("text");
        assert!(text.contains("\"schema_version\""), "{text}");
        let missing = harness.send(
            r#"{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"maj://nowhere"}}"#,
        );
        assert_eq!(missing["error"]["code"], json!(-32_602));

        let prompts =
            harness.send(r#"{"jsonrpc":"2.0","id":5,"method":"prompts/list","params":{}}"#);
        assert_eq!(
            prompts["result"]["prompts"].as_array().map(Vec::len),
            Some(3)
        );
        let rendered = harness.send(
            r#"{"jsonrpc":"2.0","id":6,"method":"prompts/get","params":{"name":"maj_conform_check"}}"#,
        );
        assert!(
            rendered["result"]["messages"][0]["content"]["text"]
                .as_str()
                .expect("text")
                .contains("maj_replay_check")
        );
        let absent = harness.send(
            r#"{"jsonrpc":"2.0","id":7,"method":"prompts/get","params":{"name":"maj_absent"}}"#,
        );
        assert_eq!(absent["error"]["code"], json!(-32_602));
    }

    #[test]
    fn a_stream_of_lines_is_served_until_it_ends() {
        let mut harness = Harness::new(Options::default());
        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n",
        );
        let mut out = Vec::new();
        harness
            .server
            .serve(std::io::Cursor::new(input), &mut out)
            .expect("served");
        let lines: Vec<&str> = std::str::from_utf8(&out).expect("utf-8").lines().collect();
        assert_eq!(
            lines.len(),
            2,
            "one response per request, none for a notification"
        );
        let first: Value = serde_json::from_str(lines[0]).expect("JSON");
        assert_eq!(first["id"], json!(1));
        let second: Value = serde_json::from_str(lines[1]).expect("JSON");
        assert_eq!(second["id"], json!(2));
        assert!(harness.server.client().is_some());
    }

    #[test]
    fn a_server_opened_on_a_document_answers_about_it_immediately() {
        let directory =
            std::env::temp_dir().join(format!("maj-mcp-server-tests-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let path = directory.join("opened.maj");
        maj_core::format::save_to_path(&maj_core::Project::new("on disk"), &path)
            .expect("the document is written");
        let mut harness = Harness::new(Options::with_project(&path));
        let response =
            harness.send(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
        assert_eq!(response["result"]["serverInfo"]["name"], json!(SERVER_NAME));
        let state = harness.call("maj_project_state", json!({}));
        assert_eq!(
            state["result"]["structuredContent"]["project"],
            json!("on disk")
        );
        assert_eq!(
            state["result"]["structuredContent"]["path"],
            json!(path.display().to_string())
        );
        let _ = std::fs::remove_file(&path);
        // A document that does not exist is refused before any handshake, which is the point
        // of opening it at construction.
        assert!(Server::new(Options::with_project(directory.join("absent.maj"))).is_err());
    }
}

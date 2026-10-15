//! `maj-mcp` — the editor, exposed over the Model Context Protocol.
//!
//! **Layer 5.** See `docs/ARCHITECTURE.md` for the layering rule and
//! `docs/adr/0011-exposing-commands-over-mcp.md` for why this exists.
//!
//! # The claim this crate makes
//!
//! Majestat is an engine whose product is a *command log*. MCP is a protocol for handing a
//! language model a set of tools. Putting the two together is not an integration project,
//! because the hard part was already solved one layer down: **the tools an AI needs are the
//! commands the log already stores**, and [`Command::apply`](maj_core::Command::apply)
//! already returns the command that reverses each one.
//!
//! So this crate does not invent an API for the agent. It *exposes* the typed one:
//!
//! * Every edit tool appends exactly one [`Command`](maj_core::Command) to the project's log,
//!   attributed to the client that called it, so an AI's work is undoable, reviewable,
//!   diffable and replayable by the same code that handles a human's.
//! * Identifier arguments are optional. When a model omits one, the engine allocates it from
//!   [`Project::next_unused_id`](maj_core::Project::next_unused_id) instead of asking the model
//!   to guess — the counter lives in the model, not in the server, so replay stays honest.
//! * Numbers arrive as `1`, `0.5`, `"1/2"` or `{"num":1,"den":2}` and are resolved to an exact
//!   [`Rational`](maj_time::Rational). Nothing on this path is a float.
//! * There is no clock, no thread and no randomness here, so two sessions that make the same
//!   calls produce the same bytes and the same hashes.
//!
//! # What a client gets
//!
//! | Surface | Count | What it is |
//! |---|---|---|
//! | Tools | one per command, plus the session and render set | The editing surface: [`catalog`] |
//! | Resources | the document, the log, the timeline, a rendered frame | Readable with `resources/read` |
//! | Prompts | editorial procedures | Workflows that name the tools to use |
//!
//! The catalogue is data, not code — see [`catalog::TOOLS`] — and the JSON Schema a client
//! sees, the arguments the server accepts and the worked example in each tool's description
//! are all derived from that one declaration, so they cannot drift apart. A test asserts that
//! every variant of [`Command`](maj_core::Command) has a tool, so a new command cannot be
//! added to the model and forgotten here.
//!
//! # Talking to it
//!
//! One JSON-RPC 2.0 message per line, in and out, on standard input and output:
//!
//! ```
//! use maj_mcp::{Options, Server};
//!
//! let mut server = Server::new(Options::default()).expect("an empty project always opens");
//! let mut out = Vec::new();
//! server.handle_line(
//!     r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
//!     &mut out,
//! )?;
//! let text = String::from_utf8(out).expect("the server writes utf-8");
//! assert!(text.contains("\"protocolVersion\":\"2025-06-18\""));
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! # Deliberately absent
//!
//! There is no HTTP transport. A remote MCP endpoint needs authentication, TLS and a policy
//! for who may edit which project, and that belongs to the cloud phase rather than here; a
//! half-built one would look finished and not be. `maj mcp` speaks to a local client over a
//! pipe, which is how every current client launches a server anyway.
//!
//! There is also no LLM. This crate exposes the editor to a model; it does not call one, and
//! it holds no model credentials. The Pilot (M11) is the layer that plans.

#![forbid(unsafe_code)]

pub mod args;
pub mod base64;
pub mod catalog;
pub mod prompts;
pub mod protocol;
pub mod resources;
pub mod server;
pub mod session;
pub mod tools;

pub use args::{ArgError, decode};
pub use catalog::{Field, FieldKind, TOOLS, Tool, tool};
pub use server::{Options, Server, serve};
pub use session::{Session, SessionError, Sessions};
pub use tools::{Outcome, ToolError, call};

/// Human-readable statement of this crate's responsibility.
pub const ROLE: &str =
    "the Model Context Protocol server: the typed command surface as agent tools";

/// Layer in the Majestat architecture, from `docs/ARCHITECTURE.md`.
pub const LAYER: u8 = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_its_role_and_layer() {
        assert!(!ROLE.is_empty(), "every crate must declare its role");
        assert_eq!(LAYER, 5);
    }
}

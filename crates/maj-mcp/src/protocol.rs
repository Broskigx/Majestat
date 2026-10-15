//! The wire: JSON-RPC 2.0 messages, one per line.
//!
//! The Model Context Protocol reuses JSON-RPC 2.0 unchanged for its message format, and its
//! standard-input transport frames those messages by writing each one on a single line with
//! no embedded newlines. That is the whole of the framing: no length prefix, no chunking,
//! and nothing to negotiate. This module is therefore a parser, an encoder and a table of
//! error codes — and it is deliberately the only place in the crate that knows the protocol
//! is JSON at all.
//!
//! # Revisions
//!
//! MCP is dated by revision, and a client states the one it wants during `initialize`. A
//! server answers with a revision *it* supports, so the negotiation here is a small,
//! total function: the client's revision if it is one of [`KNOWN_REVISIONS`], and
//! [`LATEST_REVISION`] otherwise. A client that cannot live with the answer disconnects,
//! which is the protocol's own remedy and needs no special case.

use std::fmt;

use serde_json::{Map, Value};

/// The newest protocol revision this server implements.
pub const LATEST_REVISION: &str = "2025-06-18";

/// Every revision this server can answer in.
///
/// Newest first. The list is short on purpose: a revision is only claimed when its
/// requirements are actually implemented, and the three below share the same message shapes.
pub const KNOWN_REVISIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// JSON-RPC error codes, plus the one MCP adds to them.
pub mod code {
    /// The line was not JSON, or not JSON-RPC.
    pub const PARSE: i64 = -32_700;
    /// The message was JSON but not a valid request object.
    pub const INVALID_REQUEST: i64 = -32_600;
    /// The method is not one this server implements.
    pub const METHOD_NOT_FOUND: i64 = -32_601;
    /// The method exists and its parameters do not fit it.
    pub const INVALID_PARAMS: i64 = -32_602;
    /// The server failed in a way that is not the client's fault.
    pub const INTERNAL: i64 = -32_603;
    /// A request arrived before `initialize` completed.
    pub const NOT_INITIALIZED: i64 = -32_002;
}

/// One message read from the transport.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// A request, which expects exactly one response.
    Request(Request),
    /// A notification, which expects none.
    Notification(Notification),
}

/// A request: an identifier, a method and an object of parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    id: Value,
    method: String,
    params: Map<String, Value>,
}

impl Request {
    /// The identifier to answer with, echoed exactly as it arrived.
    #[must_use]
    pub const fn id(&self) -> &Value {
        &self.id
    }

    /// The method being called.
    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Every parameter, as the client sent them.
    #[must_use]
    pub const fn params(&self) -> &Map<String, Value> {
        &self.params
    }

    /// One parameter, if it is present.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.params.get(key)
    }

    /// One string parameter, if it is present and a string.
    #[must_use]
    pub fn text(&self, key: &str) -> Option<&str> {
        self.params.get(key).and_then(Value::as_str)
    }
}

/// A notification: a method and parameters, with no reply expected.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    method: String,
    params: Map<String, Value>,
}

impl Notification {
    /// The method being notified.
    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Every parameter, as the client sent them.
    #[must_use]
    pub const fn params(&self) -> &Map<String, Value> {
        &self.params
    }
}

/// A JSON-RPC error, ready to be sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Fault {
    /// JSON-RPC error code.
    pub code: i64,
    /// One sentence a person can act on.
    pub message: String,
    /// The request this answers, when it is known.
    pub id: Option<Value>,
}

impl Fault {
    /// A fault with an explicit code.
    #[must_use]
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            id: None,
        }
    }

    /// The line was not a JSON-RPC message.
    #[must_use]
    pub fn parse(message: impl Into<String>) -> Self {
        Self::new(code::PARSE, message)
    }

    /// The message was JSON but not a request.
    #[must_use]
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(code::INVALID_REQUEST, message)
    }

    /// The method is not implemented.
    #[must_use]
    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            code::METHOD_NOT_FOUND,
            format!("`{method}` is not a method this server implements"),
        )
    }

    /// The method exists and its parameters do not fit it.
    #[must_use]
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(code::INVALID_PARAMS, message)
    }

    /// A request arrived before `initialize`.
    #[must_use]
    pub fn not_initialized() -> Self {
        Self::new(
            code::NOT_INITIALIZED,
            "call `initialize` before any other method",
        )
    }

    /// The server itself failed.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(code::INTERNAL, message)
    }

    /// The same fault, now addressed to a request.
    #[must_use]
    pub fn addressed_to(mut self, id: Value) -> Self {
        self.id = Some(id);
        self
    }

    /// The error object as it appears on the wire.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut error = Map::new();
        error.insert("code".to_owned(), Value::from(self.code));
        error.insert("message".to_owned(), Value::from(self.message.clone()));
        Value::Object(error)
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for Fault {}

/// The revision to answer a client with.
///
/// A client that asks for a revision this build knows is answered in that revision; any
/// other request — including no request at all — is answered in [`LATEST_REVISION`].
#[must_use]
pub fn negotiate(requested: Option<&str>) -> &'static str {
    match requested {
        Some(revision) => KNOWN_REVISIONS
            .iter()
            .find(|known| **known == revision)
            .copied()
            .unwrap_or(LATEST_REVISION),
        None => LATEST_REVISION,
    }
}

/// Decodes a batch, a single message, or a blank line.
///
/// A blank line produces no messages at all, which is how the transports tolerate the
/// trailing newline of a well-formed stream. A JSON array produces one entry per element,
/// each decoded on its own, so one malformed element does not discard its siblings.
#[must_use]
pub fn decode(text: &str) -> Vec<Result<Incoming, Fault>> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            return vec![Err(Fault::parse(format!("the line is not JSON: {error}")))];
        }
    };
    match value {
        Value::Array(elements) => {
            if elements.is_empty() {
                return vec![Err(Fault::invalid_request(
                    "an empty batch is not a request",
                ))];
            }
            elements.into_iter().map(one).collect()
        }
        other => vec![one(other)],
    }
}

/// Decodes one message value.
fn one(value: Value) -> Result<Incoming, Fault> {
    let Value::Object(mut map) = value else {
        return Err(Fault::invalid_request(
            "a JSON-RPC message must be an object",
        ));
    };
    if let Some(version) = map.get("jsonrpc")
        && version.as_str() != Some("2.0")
    {
        return Err(Fault::invalid_request(
            "`jsonrpc` must be the string \"2.0\"",
        ));
    }
    let id = map.remove("id");
    let Some(method) = map.remove("method") else {
        return Err(Fault::invalid_request("a request must carry a `method`"));
    };
    let Some(method) = method.as_str() else {
        return Err(Fault::invalid_request("`method` must be a string"));
    };
    if method.is_empty() {
        return Err(Fault::invalid_request("`method` must not be empty"));
    }
    let params = match map.remove("params") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(params)) => params,
        Some(_) => {
            return Err(Fault::invalid_request("`params` must be an object"));
        }
    };
    let method = method.to_owned();
    match id {
        // A request identifier must be a string or a number, so a response can echo it.
        Some(id @ (Value::String(_) | Value::Number(_) | Value::Null)) => {
            Ok(Incoming::Request(Request { id, method, params }))
        }
        Some(_) => Err(Fault::invalid_request(
            "`id` must be a string, a number or null",
        )),
        None => Ok(Incoming::Notification(Notification { method, params })),
    }
}

/// A successful response.
#[must_use]
pub fn reply(id: &Value, result: Value) -> Value {
    let mut message = Map::new();
    message.insert("jsonrpc".to_owned(), Value::from("2.0"));
    message.insert("id".to_owned(), id.clone());
    message.insert("result".to_owned(), result);
    Value::Object(message)
}

/// An error response.
#[must_use]
pub fn failure(fault: &Fault) -> Value {
    let mut message = Map::new();
    message.insert("jsonrpc".to_owned(), Value::from("2.0"));
    message.insert("id".to_owned(), fault.id.clone().unwrap_or(Value::Null));
    message.insert("error".to_owned(), fault.to_value());
    Value::Object(message)
}

/// A notification, as a server sends one.
#[must_use]
pub fn notification(method: &str, params: Value) -> Value {
    let mut message = Map::new();
    message.insert("jsonrpc".to_owned(), Value::from("2.0"));
    message.insert("method".to_owned(), Value::from(method));
    message.insert("params".to_owned(), params);
    Value::Object(message)
}

/// One message as a line of the transport, trailing newline included.
///
/// # Panics
///
/// Panics only if the value cannot be serialised, which no `Value` built from JSON can do.
#[must_use]
pub fn encode(value: &Value) -> String {
    let mut line = value.to_string();
    line.push('\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(text: &str) -> Request {
        match decode(text).remove(0) {
            Ok(Incoming::Request(request)) => request,
            other => panic!("expected a request, got {other:?}"),
        }
    }

    #[test]
    fn a_request_carries_its_id_method_and_params() {
        let request = request(r#"{"jsonrpc":"2.0","id":7,"method":"tools/list","params":{}}"#);
        assert_eq!(request.id(), &json!(7));
        assert_eq!(request.method(), "tools/list");
        assert!(request.params().is_empty());
    }

    #[test]
    fn params_may_be_absent_and_are_then_empty() {
        let request = request(r#"{"jsonrpc":"2.0","id":"a","method":"ping"}"#);
        assert!(request.params().is_empty());
        assert_eq!(request.id(), &json!("a"));
    }

    #[test]
    fn a_message_without_an_id_is_a_notification() {
        let messages = decode(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], Ok(Incoming::Notification(_))));
    }

    #[test]
    fn a_blank_line_is_silence() {
        assert!(decode("\n").is_empty());
        assert!(decode("   ").is_empty());
    }

    #[test]
    fn a_batch_decodes_element_by_element() {
        let messages = decode(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"method":7}]"#);
        assert_eq!(messages.len(), 2);
        assert!(messages[0].is_ok());
        assert_eq!(
            messages[1].as_ref().unwrap_err().code,
            code::INVALID_REQUEST
        );
    }

    #[test]
    fn malformed_input_is_a_parse_error_not_a_panic() {
        let messages = decode("{");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].as_ref().unwrap_err().code, code::PARSE);
        assert_eq!(
            decode("42")[0].as_ref().unwrap_err().code,
            code::INVALID_REQUEST
        );
        assert_eq!(
            decode("[]")[0].as_ref().unwrap_err().code,
            code::INVALID_REQUEST
        );
    }

    #[test]
    fn a_wrong_jsonrpc_version_is_refused() {
        let fault = decode(r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#).remove(0);
        assert_eq!(fault.unwrap_err().code, code::INVALID_REQUEST);
    }

    #[test]
    fn an_array_of_params_is_refused_because_mcp_uses_objects() {
        let fault = decode(r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":[]}"#).remove(0);
        assert!(
            fault
                .unwrap_err()
                .message
                .contains("`params` must be an object")
        );
    }

    #[test]
    fn the_negotiated_revision_is_known_or_latest() {
        assert_eq!(negotiate(Some("2024-11-05")), "2024-11-05");
        assert_eq!(negotiate(Some("2025-06-18")), "2025-06-18");
        assert_eq!(negotiate(Some("1999-01-01")), LATEST_REVISION);
        assert_eq!(negotiate(None), LATEST_REVISION);
    }

    #[test]
    fn a_fault_keeps_the_identifier_it_answers() {
        let fault = Fault::method_not_found("nope").addressed_to(json!(3));
        let value = failure(&fault);
        assert_eq!(value["id"], json!(3));
        assert_eq!(value["error"]["code"], json!(code::METHOD_NOT_FOUND));
        assert!(value["error"]["message"].as_str().unwrap().contains("nope"));
        // An unaddressed fault answers with a null identifier, as JSON-RPC requires.
        assert_eq!(failure(&Fault::parse("x"))["id"], Value::Null);
    }

    #[test]
    fn encoding_is_one_line_per_message() {
        let line = encode(&reply(&json!(1), json!({"ok": true})));
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
    }
}

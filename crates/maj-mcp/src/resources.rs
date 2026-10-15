//! The project as things a client can read, and a frame as a picture.
//!
//! Tools are how a model *acts*. Resources are how it *looks*: the document, the log, the
//! timeline and the hashes are all addressable, so a client can attach them to a request, poll
//! them between turns, or hand them to a person. Three of them are templates — a sequence, a
//! clip and a frame — because those are the ones whose address depends on the project.
//!
//! # One deliberate duplication
//!
//! `maj://frame/{sequence}/{frame}` renders the same thing `maj_render_frame` returns. That is
//! not redundancy for its own sake: a resource can be linked, cached and shown by a client
//! that never calls a tool, and the renderer behind both is the same certified path, so the
//! pictures cannot disagree. The resource answers with base64 in a `blob`, which is how the
//! protocol carries binary.

use std::fmt;

use serde_json::{Value, json};

use crate::session::{Session, Sessions};

/// The document, as canonical JSON.
pub const PROJECT_URI: &str = "maj://project";

/// The project's state and its identifiers, in prose.
pub const SUMMARY_URI: &str = "maj://project/summary";

/// The command log.
pub const LOG_URI: &str = "maj://project/log";

/// The timeline, in prose.
pub const TIMELINE_URI: &str = "maj://project/timeline";

/// The three content hashes.
pub const HASHES_URI: &str = "maj://project/hashes";

/// The template for one sequence.
pub const SEQUENCE_TEMPLATE: &str = "maj://sequence/{id}";

/// The template for one clip.
pub const CLIP_TEMPLATE: &str = "maj://clip/{id}";

/// The template for one rendered frame.
pub const FRAME_TEMPLATE: &str = "maj://frame/{sequence}/{frame}";

/// Why a resource could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceError {
    /// The URI names nothing this server publishes.
    Unknown {
        /// The URI as it was asked for.
        uri: String,
    },
    /// The URI names something that does not exist in the project.
    Missing {
        /// The URI as it was asked for.
        uri: String,
        /// What was not found.
        what: String,
    },
    /// The document could not be serialised, or the render failed.
    Failed {
        /// The URI as it was asked for.
        uri: String,
        /// What went wrong.
        message: String,
    },
}

impl fmt::Display for ResourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown { uri } => {
                write!(formatter, "`{uri}` is not a resource this server publishes")
            }
            Self::Missing { uri, what } => write!(formatter, "`{uri}`: {what} does not exist"),
            Self::Failed { uri, message } => write!(formatter, "`{uri}`: {message}"),
        }
    }
}

impl std::error::Error for ResourceError {}

/// The contents of one resource, already encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contents {
    /// The URI it answers.
    pub uri: String,
    /// The media type.
    pub mime: String,
    /// Text, for everything that is not binary.
    pub text: Option<String>,
    /// Base64, for a frame.
    pub blob: Option<String>,
}

impl Contents {
    fn text(uri: &str, mime: &str, body: String) -> Self {
        Self {
            uri: uri.to_owned(),
            mime: mime.to_owned(),
            text: Some(body),
            blob: None,
        }
    }

    fn blob(uri: &str, mime: &str, bytes: &[u8]) -> Self {
        Self {
            uri: uri.to_owned(),
            mime: mime.to_owned(),
            text: None,
            blob: Some(crate::base64::encode(bytes)),
        }
    }

    /// The resource as `resources/read` sends it.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut value = json!({"uri": self.uri, "mimeType": self.mime});
        if let Some(text) = &self.text {
            value["text"] = Value::from(text.clone());
        }
        if let Some(blob) = &self.blob {
            value["blob"] = Value::from(blob.clone());
        }
        value
    }
}

/// Every resource this server publishes, as `resources/list` sends them.
#[must_use]
pub fn list(default_session: &str) -> Value {
    let entries = [
        (
            PROJECT_URI,
            "The whole project document",
            "application/json",
        ),
        (
            SUMMARY_URI,
            "The session, its revision and its identifiers",
            "text/plain",
        ),
        (
            TIMELINE_URI,
            "The sequences, tracks, clips and media, in prose",
            "text/plain",
        ),
        (LOG_URI, "The command log, newest first", "application/json"),
        (
            HASHES_URI,
            "The state, document and trace hashes",
            "application/json",
        ),
    ];
    json!({
        "resources": entries
            .iter()
            .map(|(uri, description, mime)| json!({
                "uri": uri,
                "name": uri.strip_prefix("maj://").unwrap_or(uri),
                "title": description,
                "description": format!("{description}. Session `{default_session}` unless the URI names another."),
                "mimeType": mime,
            }))
            .collect::<Vec<_>>(),
    })
}

/// The resources whose address depends on the project.
#[must_use]
pub fn templates() -> Value {
    json!({
        "resourceTemplates": [
            {
                "uriTemplate": SEQUENCE_TEMPLATE,
                "name": "sequence",
                "title": "One sequence",
                "description": "A sequence, as an object: its frame size, its rate and its tracks.",
                "mimeType": "application/json",
            },
            {
                "uriTemplate": CLIP_TEMPLATE,
                "name": "clip",
                "title": "One clip",
                "description": "A clip in full: transform, effects with their parameter ranges, masks and keyframes.",
                "mimeType": "application/json",
            },
            {
                "uriTemplate": FRAME_TEMPLATE,
                "name": "frame",
                "title": "One rendered frame",
                "description": "A timeline frame rendered on the certified path, as an eight-bit PNG.",
                "mimeType": "image/png",
            },
        ],
    })
}

/// Reads one resource.
///
/// The session is chosen by a `?session=` query, which is the only query this server
/// understands; without it the call addresses the default session.
///
/// # Errors
///
/// Returns [`ResourceError`] for a URI this server does not publish, for one whose identifiers
/// do not exist, and for a frame that cannot be rendered.
pub fn read(sessions: &Sessions, uri: &str) -> Result<Contents, ResourceError> {
    let (path, session_name) = match uri.split_once('?') {
        Some((path, query)) => (path, query.strip_prefix("session=").map(str::to_owned)),
        None => (uri, None),
    };
    let session = sessions
        .get(session_name.as_deref())
        .map_err(|error| ResourceError::Failed {
            uri: uri.to_owned(),
            message: error.to_string(),
        })?;
    match path {
        PROJECT_URI => document(uri, session),
        SUMMARY_URI => Ok(Contents::text(
            uri,
            "text/plain",
            format!("{}\n", session.summary()),
        )),
        TIMELINE_URI => Ok(Contents::text(uri, "text/plain", session.timeline_report())),
        LOG_URI => Ok(Contents::text(
            uri,
            "application/json",
            rendered(
                &json!({
                    "session": session.name(),
                    "revision": session.project().revision(),
                    "entries": session.project().log().iter().map(|entry| json!({
                        "seq": entry.seq(),
                        "author": entry.author(),
                        "command": entry.command(),
                    })).collect::<Vec<_>>(),
                }),
                uri,
            )?,
        )),
        HASHES_URI => Ok(Contents::text(
            uri,
            "application/json",
            rendered(
                &session.digests().map_err(|error| ResourceError::Failed {
                    uri: uri.to_owned(),
                    message: error.to_string(),
                })?,
                uri,
            )?,
        )),
        _ => route(uri, path, session),
    }
}

/// Reads one of the template resources.
fn route(uri: &str, path: &str, session: &Session) -> Result<Contents, ResourceError> {
    if let Some(rest) = path.strip_prefix("maj://sequence/") {
        let id = parse_id(uri, rest)?;
        let sequence = session
            .project()
            .sequences()
            .iter()
            .find(|sequence| sequence.id().raw() == id)
            .ok_or_else(|| ResourceError::Missing {
                uri: uri.to_owned(),
                what: format!("sequence {id}"),
            })?;
        let timeline = session.timeline();
        let found = timeline["sequences"]
            .as_array()
            .and_then(|sequences| {
                sequences
                    .iter()
                    .find(|entry| entry["id"] == json!(id))
                    .cloned()
            })
            .unwrap_or(Value::Null);
        let _ = sequence;
        return Ok(Contents::text(
            uri,
            "application/json",
            rendered(&found, uri)?,
        ));
    }
    if let Some(rest) = path.strip_prefix("maj://clip/") {
        let id = parse_id(uri, rest)?;
        let detail = session
            .clip_detail(maj_core::ClipId::new(id))
            .ok_or_else(|| ResourceError::Missing {
                uri: uri.to_owned(),
                what: format!("clip {id}"),
            })?;
        return Ok(Contents::text(
            uri,
            "application/json",
            rendered(&detail, uri)?,
        ));
    }
    if let Some(rest) = path.strip_prefix("maj://frame/") {
        let (sequence, frame) = rest.split_once('/').ok_or_else(|| ResourceError::Unknown {
            uri: uri.to_owned(),
        })?;
        let sequence = parse_id(uri, sequence)?;
        let frame: i64 = frame.trim().parse().map_err(|_| ResourceError::Missing {
            uri: uri.to_owned(),
            what: format!("frame `{frame}`"),
        })?;
        let (png, _digest) =
            crate::tools::frame_png(session, maj_core::SequenceId::new(sequence), frame).map_err(
                |message| ResourceError::Failed {
                    uri: uri.to_owned(),
                    message,
                },
            )?;
        return Ok(Contents::blob(uri, "image/png", &png));
    }
    Err(ResourceError::Unknown {
        uri: uri.to_owned(),
    })
}

/// The document itself.
fn document(uri: &str, session: &Session) -> Result<Contents, ResourceError> {
    let text = maj_core::format::to_pretty_json(session.project()).map_err(|error| {
        ResourceError::Failed {
            uri: uri.to_owned(),
            message: error.to_string(),
        }
    })?;
    Ok(Contents::text(uri, "application/json", text))
}

/// An identifier out of a URI path.
fn parse_id(uri: &str, text: &str) -> Result<u64, ResourceError> {
    text.trim().parse().map_err(|_| ResourceError::Missing {
        uri: uri.to_owned(),
        what: format!("`{text}` as an identifier"),
    })
}

/// A value as pretty JSON, or a resource error naming the URI.
fn rendered(value: &Value, uri: &str) -> Result<String, ResourceError> {
    serde_json::to_string_pretty(value).map_err(|error| ResourceError::Failed {
        uri: uri.to_owned(),
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{DEFAULT_SESSION, Sessions};

    fn sessions() -> Sessions {
        Sessions::new(
            DEFAULT_SESSION,
            Session::new(DEFAULT_SESSION, "scratch", "mcp:test"),
        )
    }

    #[test]
    fn the_document_round_trips_through_its_own_resource() {
        let sessions = sessions();
        let contents = read(&sessions, PROJECT_URI).expect("read");
        assert_eq!(contents.mime, "application/json");
        let text = contents.text.expect("text");
        let project = maj_core::format::load_from_str(&text).expect("the document reloads");
        assert_eq!(project.name(), "scratch");
        assert!(project.is_valid());
    }

    #[test]
    fn the_summary_and_the_timeline_say_what_the_session_holds() {
        let sessions = sessions();
        let summary = read(&sessions, SUMMARY_URI).expect("read");
        assert_eq!(summary.mime, "text/plain");
        assert!(summary.text.expect("text").contains("revision 0"));
        let timeline = read(&sessions, TIMELINE_URI).expect("read");
        assert_eq!(timeline.text.expect("text"), "no sequences yet\n");
    }

    #[test]
    fn the_log_and_the_hashes_are_json_a_client_can_parse() {
        let sessions = sessions();
        let log: Value =
            serde_json::from_str(&read(&sessions, LOG_URI).expect("read").text.expect("text"))
                .expect("the log is JSON");
        assert_eq!(log["entries"], json!([]));
        let hashes: Value = serde_json::from_str(
            &read(&sessions, HASHES_URI)
                .expect("read")
                .text
                .expect("text"),
        )
        .expect("the hashes are JSON");
        assert_eq!(hashes["state"].as_str().map(str::len), Some(64));
    }

    #[test]
    fn a_resource_that_does_not_exist_is_refused_by_name() {
        let sessions = sessions();
        let error = read(&sessions, "maj://nonsense").expect_err("refused");
        assert!(error.to_string().contains("maj://nonsense"));
        let error = read(&sessions, "maj://clip/7").expect_err("refused");
        assert!(matches!(error, ResourceError::Missing { .. }));
        let error = read(&sessions, "maj://frame/1/0").expect_err("refused");
        assert!(
            error.to_string().contains("no sequence with identifier 1"),
            "{error}"
        );
    }

    #[test]
    fn a_resource_can_name_a_session_that_is_not_the_default_one() {
        let mut sessions = sessions();
        sessions.insert("b-roll", Session::new("b-roll", "stringout", "mcp:test"));
        let contents = read(&sessions, "maj://project/summary?session=b-roll").expect("read");
        assert!(contents.text.expect("text").contains("stringout"));
        let error = read(&sessions, "maj://project/summary?session=missing").expect_err("refused");
        assert!(error.to_string().contains("missing"), "{error}");
    }

    #[test]
    fn the_listing_and_the_templates_name_every_uri_that_reads() {
        let listing = list(DEFAULT_SESSION);
        let resources = listing["resources"].as_array().expect("a list");
        assert_eq!(resources.len(), 5);
        for entry in resources {
            let uri = entry["uri"].as_str().expect("a uri");
            read(&sessions(), uri).unwrap_or_else(|error| panic!("{uri} did not read: {error}"));
        }
        let templates = templates();
        let entries = templates["resourceTemplates"].as_array().expect("a list");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2]["mimeType"], json!("image/png"));
    }

    #[test]
    fn contents_encode_as_the_protocol_reads_them() {
        let text = Contents::text("maj://x", "text/plain", "hello".to_owned()).to_value();
        assert_eq!(text["text"], json!("hello"));
        assert!(text.get("blob").is_none());
        let blob = Contents::blob("maj://y", "image/png", &[1, 2, 3]).to_value();
        assert_eq!(blob["blob"], json!("AQID"));
        assert!(blob.get("text").is_none());
    }
}

//! Dispatch: one tool call, one effect.
//!
//! Two kinds of tool live here, and the difference between them is the whole design:
//!
//! * A **command tool** decodes its arguments into a [`Command`] and
//!   applies it. It changes the project in exactly the way the log says it did, so it is
//!   undoable, reviewable, attributable and replayable by the same code that handles a
//!   human's edit. There is no second path into the model.
//! * A **read, session or render tool** changes no state, or changes it through another tool.
//!   It reads the project, saves it, or asks the certified renderer for a frame — and a
//!   frame is the only answer to "does it look right" that this server accepts.
//!
//! # Two failures, deliberately separated
//!
//! A tool that does not exist is a *protocol* failure and is answered with a JSON-RPC error;
//! a tool that exists and cannot do what it was asked is a *tool* failure, and is answered as
//! a normal result marked `isError`. The distinction is the Model Context Protocol's own, and
//! it exists for a good reason: a model should see "the engine refused a speed of zero" as
//! something it can act on, not as a transport event.
//!
//! Nothing here holds a lock, a clock or a thread-local. Every call is a function of the
//! session it was given, which is why two clients making the same calls get the same bytes.

use std::fmt;

use maj_color::TransferFunction;
use maj_core::{Command, MediaAssetId, Project, Sequence, SequenceId, TrackId, TrackKind};
use maj_export::manifest;
use maj_render::{
    BitDepth, RenderRequest, RenderTask, WorkspaceResolver, render_frame, render_tiled,
};
use serde_json::{Map, Value, json};

use crate::args::decode;
use crate::base64;
use crate::catalog::Tool;
use crate::session::{Session, Sessions};

/// A call that names a tool this server does not have.
///
/// The only failure that becomes a JSON-RPC error rather than a result: everything else a
/// model can get wrong is something a model can fix, and it needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    /// The name that was called.
    pub name: String,
}

impl fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "`{}` is not a tool this server offers; call `tools/list` for the catalogue",
            self.name
        )
    }
}

impl std::error::Error for ToolError {}

/// A picture to show a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Media type, always a PNG here.
    pub mime: String,
    /// The encoded bytes.
    pub bytes: Vec<u8>,
}

/// What a call produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The answer in prose, which is what a model reads.
    pub text: String,
    /// The same answer as an object, for a client that would rather parse it.
    pub structured: Option<Value>,
    /// Pictures, if the call made any.
    pub images: Vec<Image>,
    /// Whether the call failed without failing the protocol.
    pub is_error: bool,
}

impl Outcome {
    /// A successful answer in prose.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            structured: None,
            images: Vec::new(),
            is_error: false,
        }
    }

    /// A failure the model should read and act on.
    #[must_use]
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            structured: None,
            images: Vec::new(),
            is_error: true,
        }
    }

    /// The same answer, with an object attached.
    #[must_use]
    pub fn structured(mut self, value: Value) -> Self {
        self.structured = Some(value);
        self
    }

    /// The same answer, with a picture attached.
    #[must_use]
    pub fn image(mut self, image: Image) -> Self {
        self.images.push(image);
        self
    }

    /// The answer as the `content` array of a `tools/call` result.
    #[must_use]
    pub fn content(&self) -> Value {
        let mut content = vec![json!({"type": "text", "text": self.text})];
        for image in &self.images {
            content.push(json!({
                "type": "image",
                "data": base64::encode(&image.bytes),
                "mimeType": image.mime,
            }));
        }
        Value::Array(content)
    }
}

/// Calls one tool.
///
/// # Errors
///
/// Returns [`ToolError`] for a name this server does not offer. Every other failure —
/// arguments that do not fit, a session that is not open, a command the model refuses, a
/// render that fails — is an [`Outcome`] with `is_error` set, because it is information the
/// caller can use.
pub fn call(
    sessions: &mut Sessions,
    name: &str,
    args: &Map<String, Value>,
) -> Result<Outcome, ToolError> {
    let Some(tool) = crate::catalog::tool(name) else {
        return Err(ToolError {
            name: name.to_owned(),
        });
    };
    if let Some(key) = args
        .keys()
        .find(|key| !tool.fields.iter().any(|field| field.name == *key))
    {
        return Ok(Outcome::error(format!(
            "`{name}` has no argument `{key}`; its arguments are {}",
            tool.fields
                .iter()
                .map(|field| field.name)
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let wanted = args.get("session").and_then(Value::as_str);
    let session = match sessions.get_mut(wanted) {
        Ok(session) => session,
        Err(error) => return Ok(Outcome::error(error.to_string())),
    };
    Ok(dispatch(session, tool, args))
}

/// Runs one tool against one session.
fn dispatch(session: &mut Session, tool: &Tool, args: &Map<String, Value>) -> Outcome {
    if tool.command.is_some() {
        return edit(session, tool, args);
    }
    match tool.name {
        "maj_project_state" => state(session),
        "maj_timeline" => timeline(session),
        "maj_clip_detail" => clip_detail(session, args),
        "maj_history" => history(session, args),
        "maj_undo" => undo(session, args),
        "maj_redo" => redo(session, args),
        "maj_open_project" => open(session, args),
        "maj_new_project" => reset(session, args),
        "maj_save_project" => save(session, args),
        "maj_replay_check" => replay_check(session),
        "maj_place_clip" => place_clip(session, args),
        "maj_setup_sequence" => setup_sequence(session, args),
        "maj_render_frame" => render(session, args),
        "maj_verify_frame" => verify(session, args),
        "maj_export_frame" => export(session, args),
        other => Outcome::error(format!("`{other}` is declared but not implemented")),
    }
}

/// Applies one command tool.
fn edit(session: &mut Session, tool: &Tool, args: &Map<String, Value>) -> Outcome {
    let canonical = match decode(tool, args, session.project()) {
        Ok(canonical) => canonical,
        Err(error) => return Outcome::error(error.to_string()),
    };
    let command: Command = match serde_json::from_value(canonical.clone()) {
        Ok(command) => command,
        Err(error) => {
            return Outcome::error(format!(
                "the engine refused this call: {error}\nArguments as decoded: {canonical}"
            ));
        }
    };
    match session.apply(command) {
        Ok(revision) => {
            let mut text = format!(
                "{} \u{b7} now at revision {revision}\n{}",
                tool.title,
                describe(&canonical),
            );
            let mut structured = json!({
                "revision": revision,
                "command": canonical,
            });
            if let Ok(digests) = session.digests() {
                text.push_str(&format!(
                    "\nstate hash {}",
                    digests["state"].as_str().unwrap_or_default()
                ));
                structured["digests"] = digests;
            }
            Outcome::text(text).structured(structured)
        }
        Err(error) => Outcome::error(format!("the engine refused this call: {error}")),
    }
}

/// One line naming what a command did, for a log a person reads.
fn describe(command: &Value) -> String {
    let mut parts = Vec::new();
    let kind = command["kind"].as_str().unwrap_or("command");
    if let Some(object) = command.as_object() {
        for (key, value) in object {
            if key == "kind" {
                continue;
            }
            // Nested entities are summarised by their identifier or name, so the line stays
            // readable when a whole sequence was created in one call.
            let shown = match value {
                Value::Object(inner) => {
                    if let Some(id) = inner.get("id") {
                        format!("{id}")
                    } else if let Some(name) = inner.get("name").and_then(Value::as_str) {
                        name.to_owned()
                    } else {
                        "\u{2026}".to_owned()
                    }
                }
                other => other.to_string(),
            };
            parts.push(format!("{key} {shown}"));
        }
    }
    format!("{kind}: {}", parts.join(", "))
}

/// The state of the session.
fn state(session: &Session) -> Outcome {
    match session.state() {
        Ok(value) => {
            let (undo, redo) = session.depths();
            Outcome::text(format!(
                "{}\nrevision {}, undo depth {undo}, redo depth {redo}",
                session.summary(),
                session.project().revision(),
            ))
            .structured(value)
        }
        Err(error) => Outcome::error(error.to_string()),
    }
}

/// The timeline, as text an editor reads and as an object a client can walk.
fn timeline(session: &Session) -> Outcome {
    Outcome::text(session.timeline_report()).structured(session.timeline())
}

/// One clip in full.
fn clip_detail(session: &Session, args: &Map<String, Value>) -> Outcome {
    let id = match identifier_arg(args, "clip") {
        Ok(id) => id,
        Err(message) => return Outcome::error(message),
    };
    match session.clip_detail(maj_core::ClipId::new(id)) {
        Some(detail) => {
            let name = detail["name"].as_str().unwrap_or("clip");
            Outcome::text(format!(
                "clip#{id} `{name}`\n{}",
                serde_json::to_string_pretty(&detail).unwrap_or_default()
            ))
            .structured(detail)
        }
        None => Outcome::error(format!("no clip with identifier {id} is in this project")),
    }
}

/// The command log.
fn history(session: &Session, args: &Map<String, Value>) -> Outcome {
    let limit = match integer_arg(args, "limit", 50) {
        Ok(limit) => usize::try_from(limit.max(0)).unwrap_or(usize::MAX),
        Err(message) => return Outcome::error(message),
    };
    let author = args.get("author").and_then(Value::as_str);
    let log = session.log(limit, author);
    let mut text = format!(
        "{} entries (of {}), newest first\n",
        log["entries_shown"], log["entries_total"]
    );
    if let Some(entries) = log["entries"].as_array() {
        for entry in entries {
            text.push_str(&format!(
                "#{} {} {}\n",
                entry["seq"],
                entry["author"].as_str().unwrap_or("?"),
                describe(&entry["command"]),
            ));
        }
    }
    Outcome::text(text).structured(log)
}

/// Reverts commands.
fn undo(session: &mut Session, args: &Map<String, Value>) -> Outcome {
    let count = match integer_arg(args, "count", 1) {
        Ok(count) => usize::try_from(count.max(0)).unwrap_or(usize::MAX),
        Err(message) => return Outcome::error(message),
    };
    match session.undo(count) {
        Ok(done) => Outcome::text(format!(
            "reverted {done} command(s) · revision {}",
            session.project().revision()
        ))
        .structured(json!({
            "reverted": done,
            "revision": session.project().revision(),
            "undo_depth": session.depths().0,
            "redo_depth": session.depths().1,
        })),
        Err(error) => Outcome::error(format!("undo failed: {error}")),
    }
}

/// Re-applies commands.
fn redo(session: &mut Session, args: &Map<String, Value>) -> Outcome {
    let count = match integer_arg(args, "count", 1) {
        Ok(count) => usize::try_from(count.max(0)).unwrap_or(usize::MAX),
        Err(message) => return Outcome::error(message),
    };
    match session.redo(count) {
        Ok(done) => Outcome::text(format!(
            "re-applied {done} command(s) · revision {}",
            session.project().revision()
        ))
        .structured(json!({
            "reapplied": done,
            "revision": session.project().revision(),
            "undo_depth": session.depths().0,
            "redo_depth": session.depths().1,
        })),
        Err(error) => Outcome::error(format!("redo failed: {error}")),
    }
}

/// Opens a document into the addressed session.
fn open(session: &mut Session, args: &Map<String, Value>) -> Outcome {
    let Some(path) = args.get("path").and_then(Value::as_str) else {
        return Outcome::error("`path` is required");
    };
    match Session::open(session.name().to_owned(), path, session.author().to_owned()) {
        Ok(opened) => {
            let summary = format!(
                "opened {} · {} · revision {}",
                path,
                opened.project().name(),
                opened.project().revision()
            );
            *session = opened;
            Outcome::text(summary)
        }
        Err(error) => Outcome::error(format!("{path} could not be opened: {error}")),
    }
}

/// Replaces the session with an empty project.
fn reset(session: &mut Session, args: &Map<String, Value>) -> Outcome {
    let Some(name) = args.get("name").and_then(Value::as_str) else {
        return Outcome::error("`name` is required");
    };
    let previous = session.project().name().to_owned();
    session.replace(Project::new(name));
    Outcome::text(format!(
        "session {} now holds a new project `{name}`, replacing `{previous}`",
        session.name()
    ))
}

/// Writes the document.
fn save(session: &mut Session, args: &Map<String, Value>) -> Outcome {
    let path = args.get("path").and_then(Value::as_str);
    match session.save(path.map(std::path::Path::new)) {
        Ok((written, digest)) => {
            let bytes = std::fs::metadata(&written).map_or(0, |metadata| metadata.len());
            Outcome::text(format!(
                "wrote {} · {bytes} bytes on disk · document hash {digest}",
                written.display(),
            ))
            .structured(json!({
                "path": written.display().to_string(),
                "bytes": bytes,
                "document_hash": digest,
            }))
        }
        Err(error) => Outcome::error(error.to_string()),
    }
}

/// Checks the state against the log that claims to explain it.
fn replay_check(session: &Session) -> Outcome {
    match session.replay_check() {
        Ok(check) => {
            let text = if check["consistent"] == json!(true) {
                format!(
                    "consistent: {} commands replay to the live state, hash {}",
                    check["entries"], check["live_state"]
                )
            } else {
                format!(
                    "DIVERGED: replaying {} commands gives {}, the live project is {}",
                    check["entries"], check["replayed_state"], check["live_state"]
                )
            };
            Outcome::text(text).structured(check)
        }
        Err(error) => Outcome::error(format!("the log could not be replayed: {error}")),
    }
}

/// Registers media if needed and appends a clip for it.
///
/// The editorial shorthand, and the one place a call may append two commands. It does not
/// hide that: the answer names both, and each is in the log under its own position.
fn place_clip(session: &mut Session, args: &Map<String, Value>) -> Outcome {
    let asset = match identifier_arg(args, "asset") {
        Ok(asset) => MediaAssetId::new(asset),
        Err(message) => return Outcome::error(message),
    };
    let sequence = match sequence_arg(session.project(), args) {
        Ok(sequence) => sequence,
        Err(message) => return Outcome::error(message),
    };
    let Some(asset) = session.project().asset(asset) else {
        return Outcome::error(format!(
            "no asset with identifier {} is in the media pool; register it first",
            asset.raw()
        ));
    };
    let kind = match args.get("kind").and_then(Value::as_str) {
        Some("video") => TrackKind::Video,
        Some("audio") => TrackKind::Audio,
        other => {
            return Outcome::error(format!("`kind` must be `video` or `audio`, not {other:?}"));
        }
    };
    let track = match args.get("track") {
        Some(_) => match identifier_arg(args, "track") {
            Ok(track) => TrackId::new(track),
            Err(message) => return Outcome::error(message),
        },
        None => {
            let Some(sequence) = session.project().sequence(sequence) else {
                return Outcome::error(format!("no sequence with identifier {}", sequence.raw()));
            };
            let Some(track) = sequence.tracks().iter().find(|track| track.kind() == kind) else {
                return Outcome::error(format!(
                    "sequence {} has no {kind:?} track; create one with `maj_create_track`",
                    sequence.id().raw()
                ));
            };
            track.id()
        }
    };
    let Some(source_range) = args.get("source_range") else {
        return Outcome::error("`source_range` is required");
    };
    let source_range = match crate::args::frame_range("source_range", source_range) {
        Ok(range) => range,
        Err(error) => return Outcome::error(error.to_string()),
    };
    let timeline_start = match integer_arg(args, "timeline_start", 0) {
        Ok(value) => value,
        Err(message) => return Outcome::error(message),
    };
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_else(|| asset.name())
        .to_owned();
    let speed = match args.get("speed") {
        Some(value) => match crate::args::ratio("speed", value) {
            Ok(speed) => speed,
            Err(error) => return Outcome::error(error.to_string()),
        },
        None => maj_time::Rational::from_integer(1),
    };
    let clip = maj_core::Clip::new(
        maj_core::ClipId::new(session.project().next_unused_id()),
        asset.id(),
        name,
        source_range,
        timeline_start,
        speed,
    );
    let clip = match clip {
        Ok(clip) => clip,
        Err(error) => return Outcome::error(format!("the clip is not valid: {error}")),
    };
    let id = clip.id().raw();
    let length = clip.timeline_length();
    match session.apply(Command::AppendClip {
        sequence,
        track,
        clip,
    }) {
        Ok(revision) => Outcome::text(format!(
            "clip#{id} `{}` appended to track#{} of sequence#{} · {} frames · revision {revision}",
            session
                .project()
                .clip(maj_core::ClipId::new(id))
                .map_or("", |clip| clip.name()),
            track.raw(),
            sequence.raw(),
            length,
        ))
        .structured(json!({
            "revision": revision,
            "sequence": sequence.raw(),
            "track": track.raw(),
            "clip": id,
            "timeline_start": timeline_start,
            "timeline_length": length,
        })),
        Err(error) => Outcome::error(format!("the engine refused this call: {error}")),
    }
}

/// Creates a sequence and the tracks it was asked for, in order.
fn setup_sequence(session: &mut Session, args: &Map<String, Value>) -> Outcome {
    let Some(name) = args.get("name").and_then(Value::as_str) else {
        return Outcome::error("`name` is required");
    };
    let timebase = match args.get("timebase") {
        Some(value) => match crate::args::ratio("timebase", value) {
            Ok(rate) => rate,
            Err(error) => return Outcome::error(error.to_string()),
        },
        None => return Outcome::error("`timebase` is required"),
    };
    let timebase = match maj_time::Timebase::new(timebase) {
        Ok(timebase) => timebase,
        Err(error) => return Outcome::error(format!("`timebase` is not a frame rate: {error}")),
    };
    let width = match integer_arg(args, "width", 0) {
        Ok(width) => width,
        Err(message) => return Outcome::error(message),
    };
    let height = match integer_arg(args, "height", 0) {
        Ok(height) => height,
        Err(message) => return Outcome::error(message),
    };
    let sample_rate = match integer_arg(args, "sample_rate", 48_000) {
        Ok(rate) => rate,
        Err(message) => return Outcome::error(message),
    };
    let Some(kinds) = args.get("tracks").and_then(Value::as_array) else {
        return Outcome::error("`tracks` is required, as a list of `video` or `audio`");
    };
    let mut tracks = Vec::with_capacity(kinds.len());
    for (index, kind) in kinds.iter().enumerate() {
        let kind = match kind.as_str() {
            Some("video") => TrackKind::Video,
            Some("audio") => TrackKind::Audio,
            other => {
                return Outcome::error(format!(
                    "`tracks[{index}]` must be `video` or `audio`, not {other:?}"
                ));
            }
        };
        tracks.push(kind);
    }
    let (width, height, sample_rate) = match (
        u32::try_from(width).ok(),
        u32::try_from(height).ok(),
        u32::try_from(sample_rate).ok(),
    ) {
        (Some(width), Some(height), Some(sample_rate))
            if width > 0 && height > 0 && sample_rate > 0 =>
        {
            (width, height, sample_rate)
        }
        _ => {
            return Outcome::error(
                "`width`, `height` and `sample_rate` must be positive whole numbers",
            );
        }
    };
    let index = session.project().sequences().len();
    let sequence_id = session.project().next_unused_id();
    let sequence = match maj_core::Sequence::new(
        SequenceId::new(sequence_id),
        name,
        timebase,
        width,
        height,
        sample_rate,
    ) {
        Ok(sequence) => sequence,
        Err(error) => return Outcome::error(format!("the sequence is not valid: {error}")),
    };
    if let Err(error) = session.apply(Command::CreateSequence { index, sequence }) {
        return Outcome::error(format!("the engine refused this call: {error}"));
    }
    let mut created = Vec::new();
    // Tracks are named the way an editor names them: counted per kind, so a sequence with two
    // video tracks and one audio track has `V1`, `V2` and `A1`.
    let mut counts = (0_usize, 0_usize);
    for (index, kind) in tracks.into_iter().enumerate() {
        let track_id = session.project().next_unused_id();
        let label = match kind {
            TrackKind::Video => {
                counts.0 += 1;
                format!("V{}", counts.0)
            }
            TrackKind::Audio => {
                counts.1 += 1;
                format!("A{}", counts.1)
            }
        };
        let track = maj_core::Track::new(TrackId::new(track_id), kind, label.clone());
        match session.apply(Command::CreateTrack {
            sequence: SequenceId::new(sequence_id),
            index,
            track,
        }) {
            Ok(_) => created.push(format!("track#{track_id} {label}")),
            Err(error) => {
                return Outcome::error(format!(
                    "sequence#{sequence_id} was created but track {index} was refused: {error}"
                ));
            }
        }
    }
    Outcome::text(format!(
        "sequence#{sequence_id} `{name}` created at {width}x{height} {sample_rate} Hz\n{}",
        if created.is_empty() {
            "no tracks".to_owned()
        } else {
            created.join("\n")
        }
    ))
    .structured(json!({
        "sequence": sequence_id,
        "name": name,
        "revision": session.project().revision(),
        "tracks": created,
    }))
}

/// Renders a frame and shows it.
fn render(session: &Session, args: &Map<String, Value>) -> Outcome {
    let plan = match RenderPlan::read(session.project(), args, (1, 1)) {
        Ok(plan) => plan,
        Err(message) => return Outcome::error(message),
    };
    let wants_image = match boolean_arg(args, "image", true) {
        Ok(wants) => wants,
        Err(message) => return Outcome::error(message),
    };
    let task = RenderTask::tiled(plan.request, plan.grid);
    let report = match task.render(session.project(), &WorkspaceResolver::new()) {
        Ok(report) => report,
        Err(error) => return Outcome::error(format!("the render failed: {error}")),
    };
    let digest = report.frame.digest();
    let summary = manifest::summary(&report.manifest);
    let structured = json!({
        "sequence": plan.request.sequence.raw(),
        "frame": plan.request.frame,
        "width": plan.request.width,
        "height": plan.request.height,
        "grid": [plan.grid.0, plan.grid.1],
        "transfer": plan.transfer_name,
        "digest": digest,
        "manifest": serde_json::from_str::<Value>(&manifest::to_json(&report.manifest).unwrap_or_default())
            .unwrap_or(Value::Null),
    });
    let mut outcome = Outcome::text(format!(
        "{summary}\nframebuffer digest {digest}\n{}",
        layer_lines(&structured),
    ))
    .structured(structured);
    if wants_image {
        let png = maj_export::png::encode(&report.frame, BitDepth::Eight, plan.transfer);
        outcome = outcome.image(Image {
            mime: "image/png".to_owned(),
            bytes: png,
        });
    }
    outcome
}

/// Renders a frame whole and tiled and compares the two.
fn verify(session: &Session, args: &Map<String, Value>) -> Outcome {
    // A verification that rendered the whole frame twice would prove nothing, so its own
    // default grid is a real one.
    let plan = match RenderPlan::read(session.project(), args, (2, 2)) {
        Ok(plan) => plan,
        Err(message) => return Outcome::error(message),
    };
    let resolver = WorkspaceResolver::new();
    let whole = match render_frame(session.project(), plan.request, &resolver) {
        Ok(frame) => frame,
        Err(error) => return Outcome::error(format!("the whole-frame render failed: {error}")),
    };
    let tiled = match render_tiled(session.project(), plan.request, &resolver, plan.grid) {
        Ok(frame) => frame,
        Err(error) => return Outcome::error(format!("the tiled render failed: {error}")),
    };
    let whole_digest = whole.digest();
    let tiled_digest = tiled.digest();
    let verified = whole_digest == tiled_digest;
    let text = if verified {
        format!(
            "verified: whole and {}x{} tiled renders agree at {whole_digest}",
            plan.grid.0, plan.grid.1
        )
    } else {
        format!(
            "DIVERGED: whole {whole_digest}, {}x{} tiled {tiled_digest}",
            plan.grid.0, plan.grid.1
        )
    };
    Outcome::text(text).structured(json!({
        "verified": verified,
        "whole": whole_digest,
        "tiled": tiled_digest,
        "grid": [plan.grid.0, plan.grid.1],
        "sequence": plan.request.sequence.raw(),
        "frame": plan.request.frame,
    }))
}

/// Renders a frame and writes it, with its manifest, to disk.
fn export(session: &Session, args: &Map<String, Value>) -> Outcome {
    let plan = match RenderPlan::read(session.project(), args, (1, 1)) {
        Ok(plan) => plan,
        Err(message) => return Outcome::error(message),
    };
    let Some(out) = args.get("out").and_then(Value::as_str) else {
        return Outcome::error("`out` is required");
    };
    let task = RenderTask::single(plan.request);
    let report = match task.render(session.project(), &WorkspaceResolver::new()) {
        Ok(report) => report,
        Err(error) => return Outcome::error(format!("the render failed: {error}")),
    };
    match maj_export::write_frame_with_manifest(
        out,
        &report.frame,
        &report.manifest,
        BitDepth::Sixteen,
        plan.transfer,
    ) {
        Ok((written, manifest_path)) => Outcome::text(format!(
            "wrote {out} ({written} bytes) and its manifest to {}\ndigest {}",
            manifest_path.display(),
            report.frame.digest(),
        )),
        Err(error) => Outcome::error(format!("{out} could not be written: {error}")),
    }
}

/// Renders one timeline frame as an eight-bit sRGB PNG, with its digest.
///
/// The same certified path the `render` tool uses, so a picture delivered as a resource and
/// the same picture delivered as a tool result cannot disagree about what the frame looks
/// like.
///
/// # Errors
///
/// Returns a message for a sequence that does not exist and for a render that fails.
pub fn frame_png(
    session: &Session,
    sequence: SequenceId,
    frame: i64,
) -> Result<(Vec<u8>, String), String> {
    let Some(sequence) = session.project().sequence(sequence) else {
        return Err(format!("no sequence with identifier {}", sequence.raw()));
    };
    let request = RenderRequest::of(sequence, frame);
    let report = RenderTask::single(request)
        .render(session.project(), &WorkspaceResolver::new())
        .map_err(|error| format!("the render failed: {error}"))?;
    let digest = report.frame.digest();
    Ok((
        maj_export::png::encode(&report.frame, BitDepth::Eight, TransferFunction::Srgb),
        digest,
    ))
}

/// The manifest's layer list, one line per layer, for the text answer.
fn layer_lines(structured: &Value) -> String {
    let Some(layers) = structured["manifest"]["layers"].as_array() else {
        return String::new();
    };
    if layers.is_empty() {
        return "the frame is empty: no layer reached it".to_owned();
    }
    layers
        .iter()
        .map(|layer| {
            format!(
                "  clip#{} {} {} {} {}",
                layer["clip"],
                layer["name"].as_str().unwrap_or(""),
                layer["source"].as_str().unwrap_or("-"),
                layer["blend"].as_str().unwrap_or("-"),
                if layer["visible"] == json!(true) {
                    "visible"
                } else {
                    "not visible"
                },
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Everything a render needs, decoded from one call's arguments.
struct RenderPlan {
    request: RenderRequest,
    grid: (u32, u32),
    transfer: TransferFunction,
    transfer_name: String,
}

impl RenderPlan {
    fn read(
        project: &Project,
        args: &Map<String, Value>,
        default_grid: (u32, u32),
    ) -> Result<Self, String> {
        let sequence = sequence_arg(project, args)?;
        let Some(sequence_ref) = project.sequence(sequence) else {
            return Err(format!("no sequence with identifier {}", sequence.raw()));
        };
        let frame = integer_arg(args, "frame", 0)?;
        let width = match integer_arg(args, "width", i64::from(sequence_ref.width()))? {
            width if width > 0 => {
                u32::try_from(width).map_err(|_| format!("`width` {width} is not a usable size"))?
            }
            width => return Err(format!("`width` must be positive, not {width}")),
        };
        let height = match integer_arg(args, "height", i64::from(sequence_ref.height()))? {
            height if height > 0 => u32::try_from(height)
                .map_err(|_| format!("`height` {height} is not a usable size"))?,
            height => return Err(format!("`height` must be positive, not {height}")),
        };
        let grid = match args.get("tiles") {
            Some(value) => {
                let text = value
                    .as_str()
                    .ok_or_else(|| "`tiles` is a grid such as \"2x2\"".to_owned())?;
                parse_grid(text)?
            }
            None => default_grid,
        };
        let (transfer, transfer_name) = match args.get("transfer") {
            Some(value) => {
                let text = value
                    .as_str()
                    .ok_or_else(|| "`transfer` is a curve name".to_owned())?;
                (parse_transfer(text)?, text.to_owned())
            }
            None => (TransferFunction::Srgb, "srgb".to_owned()),
        };
        Ok(Self {
            request: RenderRequest::new(sequence, frame, width, height),
            grid,
            transfer,
            transfer_name,
        })
    }
}

/// A frame rate name, in the shape `maj render --transfer` takes.
fn parse_transfer(name: &str) -> Result<TransferFunction, String> {
    match name {
        "linear" => Ok(TransferFunction::Linear),
        "srgb" => Ok(TransferFunction::Srgb),
        "rec709" => Ok(TransferFunction::Rec709),
        "pq" => Ok(TransferFunction::Pq),
        "hlg" => Ok(TransferFunction::Hlg),
        other => {
            let Some(rest) = other.strip_prefix("gamma_") else {
                return Err(format!(
                    "`{other}` is not a curve; use linear, srgb, rec709, pq, hlg or gamma_<num>_<den>"
                ));
            };
            let (numerator, denominator) = rest
                .split_once('_')
                .ok_or_else(|| format!("`{other}` needs a gamma_<num>_<den> exponent"))?;
            let numerator: i32 = numerator
                .parse()
                .map_err(|_| format!("`{numerator}` is not a numerator"))?;
            let denominator: i32 = denominator
                .parse()
                .map_err(|_| format!("`{denominator}` is not a denominator"))?;
            let gamma = TransferFunction::Gamma {
                num: numerator,
                den: denominator,
            };
            if gamma.decode(0.5).is_nan() {
                return Err(format!("`{other}` is not a usable exponent"));
            }
            Ok(gamma)
        }
    }
}

/// A tile grid, as `CxR`.
fn parse_grid(text: &str) -> Result<(u32, u32), String> {
    let (columns, rows) = text
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("`{text}` is not a grid; write it as `2x2`"))?;
    let columns: u32 = columns
        .trim()
        .parse()
        .map_err(|_| format!("`{columns}` is not a column count"))?;
    let rows: u32 = rows
        .trim()
        .parse()
        .map_err(|_| format!("`{rows}` is not a row count"))?;
    if columns == 0 || rows == 0 {
        return Err(format!("`{text}` has an empty axis"));
    }
    Ok((columns, rows))
}

/// The sequence a call addresses: by identifier, by name, or the first one.
fn sequence_arg(project: &Project, args: &Map<String, Value>) -> Result<SequenceId, String> {
    let Some(value) = args.get("sequence") else {
        return project
            .sequences()
            .first()
            .map(Sequence::id)
            .ok_or_else(|| "this project has no sequence yet".to_owned());
    };
    if let Some(id) = value.as_u64() {
        return Ok(SequenceId::new(id));
    }
    let Some(name) = value.as_str() else {
        return Err("`sequence` is an identifier or a name".to_owned());
    };
    match project
        .sequences()
        .iter()
        .find(|sequence| sequence.name() == name)
    {
        Some(sequence) => Ok(sequence.id()),
        None => Err(format!("no sequence is named `{name}`")),
    }
}

/// An argument that must be a whole number.
fn integer_arg(args: &Map<String, Value>, key: &str, default: i64) -> Result<i64, String> {
    match args.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_i64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            .ok_or_else(|| format!("`{key}` must be a whole number")),
    }
}

/// An argument that must be a whole number and is required.
fn identifier_arg(args: &Map<String, Value>, key: &str) -> Result<u64, String> {
    let Some(value) = args.get(key) else {
        return Err(format!("`{key}` is required"));
    };
    if let Some(number) = value.as_u64() {
        return Ok(number);
    }
    value
        .as_str()
        .map(|text| text.rsplit_once('#').map_or(text, |(_, digits)| digits))
        .and_then(|digits| digits.parse().ok())
        .ok_or_else(|| format!("`{key}` must be an identifier"))
}

/// An argument that must be a switch.
fn boolean_arg(args: &Map<String, Value>, key: &str, default: bool) -> Result<bool, String> {
    match args.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| format!("`{key}` must be true or false")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::DEFAULT_SESSION;

    fn sessions() -> Sessions {
        Sessions::new(
            DEFAULT_SESSION,
            Session::new(DEFAULT_SESSION, "scratch", "mcp:test"),
        )
    }

    fn call_with(sessions: &mut Sessions, name: &str, text: &str) -> Outcome {
        let args: Map<String, Value> =
            serde_json::from_str(text).expect("the test's arguments are JSON");
        call(sessions, name, &args).expect("the tool exists")
    }

    /// A project with one media asset and a sequence with one video track.
    fn prepared() -> Sessions {
        let mut sessions = sessions();
        let outcome = call_with(
            &mut sessions,
            "maj_create_media_asset",
            r#"{"asset":{"name":"shot","uri":"generated:shot","timebase":"24","duration_frames":240}}"#,
        );
        assert!(!outcome.is_error, "{}", outcome.text);
        let outcome = call_with(
            &mut sessions,
            "maj_setup_sequence",
            r#"{"name":"Reel A","timebase":"24","width":64,"height":36,"tracks":["video"]}"#,
        );
        assert!(!outcome.is_error, "{}", outcome.text);
        sessions
    }

    #[test]
    fn an_unknown_tool_is_the_one_failure_that_is_not_a_result() {
        let mut sessions = sessions();
        let error = call(&mut sessions, "maj_do_the_impossible", &Map::new()).expect_err("unknown");
        assert_eq!(error.name, "maj_do_the_impossible");
        assert!(error.to_string().contains("tools/list"));
    }

    #[test]
    fn an_unknown_argument_is_reported_with_the_ones_that_exist() {
        let mut sessions = sessions();
        let outcome = call_with(&mut sessions, "maj_timeline", r#"{"fram":24}"#);
        assert!(outcome.is_error);
        assert!(outcome.text.contains("fram"), "{}", outcome.text);
        assert!(outcome.text.contains("session"), "{}", outcome.text);
    }

    #[test]
    fn an_edit_answers_with_the_new_revision_and_the_hash_it_produced() {
        let mut sessions = prepared();
        let outcome = call_with(
            &mut sessions,
            "maj_place_clip",
            r#"{"sequence":2,"kind":"video","asset":1,"source_range":[0,48],"timeline_start":0,"name":"first"}"#,
        );
        assert!(!outcome.is_error, "{}", outcome.text);
        assert!(outcome.text.contains("clip#4"), "{}", outcome.text);
        let state = call_with(&mut sessions, "maj_project_state", r#"{}"#);
        // Two commands built the project, the third placed the clip.
        assert_eq!(state.structured.expect("structured")["revision"], json!(4));
    }

    #[test]
    fn a_command_refusal_is_a_result_a_model_can_read_and_act_on() {
        let mut sessions = prepared();
        // Speed zero is refused by the model itself, not by the server's schema.
        let outcome = call_with(
            &mut sessions,
            "maj_place_clip",
            r#"{"sequence":2,"kind":"video","asset":1,"source_range":[0,48],"timeline_start":0,"speed":0}"#,
        );
        assert!(outcome.is_error);
        assert!(outcome.text.contains("speed"), "{}", outcome.text);
        // The project is untouched: a refused command changes nothing.
        let state = call_with(&mut sessions, "maj_project_state", r#"{}"#);
        assert_eq!(state.structured.expect("structured")["revision"], json!(3));
    }

    #[test]
    fn a_sequence_and_its_tracks_come_from_one_call_and_stay_undoable() {
        let mut sessions = sessions();
        let outcome = call_with(
            &mut sessions,
            "maj_setup_sequence",
            r#"{"name":"Reel","timebase":"24000/1001","width":1920,"height":1080,"tracks":["video","video","audio"]}"#,
        );
        assert!(!outcome.is_error, "{}", outcome.text);
        let timeline = call_with(&mut sessions, "maj_timeline", r#"{}"#);
        let structured = timeline.structured.expect("structured");
        assert_eq!(
            structured["sequences"][0]["tracks"]
                .as_array()
                .map(Vec::len),
            Some(3)
        );
        assert_eq!(structured["sequences"][0]["tracks"][2]["name"], json!("A1"));
        // Four commands: the sequence and three tracks, each undoable on its own.
        let undone = call_with(&mut sessions, "maj_undo", r#"{"count":1}"#);
        assert!(undone.text.contains("reverted 1"), "{}", undone.text);
        let timeline = call_with(&mut sessions, "maj_timeline", r#"{}"#);
        assert_eq!(
            timeline.structured.expect("structured")["sequences"][0]["tracks"]
                .as_array()
                .map(Vec::len),
            Some(2)
        );
    }

    #[test]
    fn a_render_returns_a_picture_a_digest_and_the_manifest_behind_it() {
        let mut sessions = prepared();
        call_with(
            &mut sessions,
            "maj_place_clip",
            r#"{"sequence":2,"kind":"video","asset":1,"source_range":[0,48],"timeline_start":0}"#,
        );
        let outcome = call_with(
            &mut sessions,
            "maj_render_frame",
            r#"{"sequence":2,"frame":0}"#,
        );
        assert!(!outcome.is_error, "{}", outcome.text);
        assert_eq!(outcome.images.len(), 1);
        assert_eq!(outcome.images[0].mime, "image/png");
        assert_eq!(&outcome.images[0].bytes[..8], &maj_export::png::SIGNATURE);
        let structured = outcome.structured.expect("structured");
        assert_eq!(structured["width"], json!(64));
        assert_eq!(structured["height"], json!(36));
        assert_eq!(structured["digest"].as_str().map(str::len), Some(64));
        assert_eq!(structured["manifest"]["policy"], json!("certified"));
        assert_eq!(structured["manifest"]["layers"][0]["clip"], json!(4));
        assert!(outcome.text.contains("certified path"), "{}", outcome.text);
    }

    #[test]
    fn the_same_call_twice_renders_the_same_bytes_which_is_the_claim_verify_checks() {
        let mut sessions = prepared();
        call_with(
            &mut sessions,
            "maj_place_clip",
            r#"{"sequence":2,"kind":"video","asset":1,"source_range":[0,48],"timeline_start":0}"#,
        );
        let first = call_with(&mut sessions, "maj_render_frame", r#"{"frame":12}"#);
        let second = call_with(&mut sessions, "maj_render_frame", r#"{"frame":12}"#);
        assert_eq!(first.images, second.images);
        assert_eq!(
            first.structured.expect("structured")["digest"],
            second.structured.expect("structured")["digest"]
        );
        let verified = call_with(
            &mut sessions,
            "maj_verify_frame",
            r#"{"frame":12,"tiles":"3x2"}"#,
        );
        assert_eq!(
            verified.structured.expect("structured")["verified"],
            json!(true),
            "{}",
            verified.text
        );
    }

    #[test]
    fn a_frame_with_no_layers_says_so_rather_than_looking_broken() {
        let mut sessions = prepared();
        let outcome = call_with(&mut sessions, "maj_render_frame", r#"{"frame":0}"#);
        assert!(
            outcome.text.contains("no layer reached it"),
            "{}",
            outcome.text
        );
        assert_eq!(outcome.images.len(), 1, "a black frame is still a frame");
    }

    #[test]
    fn a_render_can_be_asked_for_without_its_picture() {
        let mut sessions = prepared();
        let outcome = call_with(
            &mut sessions,
            "maj_render_frame",
            r#"{"frame":0,"image":false}"#,
        );
        assert!(outcome.images.is_empty());
    }

    #[test]
    fn a_clip_detail_carries_the_parameter_ranges_the_model_would_otherwise_guess() {
        let mut sessions = prepared();
        call_with(
            &mut sessions,
            "maj_place_clip",
            r#"{"sequence":2,"kind":"video","asset":1,"source_range":[0,48],"timeline_start":0}"#,
        );
        let added = call_with(
            &mut sessions,
            "maj_add_effect",
            r#"{"sequence":2,"clip":4,"effect":{"kind":"vignette","params":{"amount":"1/8"}}}"#,
        );
        assert!(!added.is_error, "{}", added.text);
        let detail = call_with(&mut sessions, "maj_clip_detail", r#"{"clip":4}"#);
        let structured = detail.structured.expect("structured");
        assert_eq!(structured["effects"][0]["params"][0]["minimum"], json!("0"));
        assert_eq!(structured["effects"][0]["params"][0]["maximum"], json!("2"));
        assert!(detail.text.contains("vignette"), "{}", detail.text);
    }

    #[test]
    fn an_export_writes_the_still_and_its_manifest_and_names_both() {
        let mut sessions = prepared();
        call_with(
            &mut sessions,
            "maj_place_clip",
            r#"{"sequence":2,"kind":"video","asset":1,"source_range":[0,48],"timeline_start":0}"#,
        );
        let directory =
            std::env::temp_dir().join(format!("maj-mcp-tool-tests-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let out = directory.join("frame.0000.png");
        let outcome = call_with(
            &mut sessions,
            "maj_export_frame",
            &format!(r#"{{"frame":0,"out":{:?}}}"#, out.display().to_string()),
        );
        assert!(!outcome.is_error, "{}", outcome.text);
        let image = std::fs::read(&out).expect("the still is on disk");
        assert_eq!(&image[..8], &maj_export::png::SIGNATURE);
        let manifest_path = directory.join("frame.0000.maj.json");
        assert!(manifest_path.exists(), "the manifest is beside it");
        assert!(
            outcome.text.contains("frame.0000.maj.json"),
            "{}",
            outcome.text
        );
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&manifest_path);
    }

    #[test]
    fn replay_check_agrees_with_the_log_the_edits_wrote() {
        let mut sessions = prepared();
        call_with(
            &mut sessions,
            "maj_place_clip",
            r#"{"sequence":2,"kind":"video","asset":1,"source_range":[0,48],"timeline_start":0}"#,
        );
        let outcome = call_with(&mut sessions, "maj_replay_check", r#"{}"#);
        assert!(!outcome.is_error, "{}", outcome.text);
        assert_eq!(
            outcome.structured.expect("structured")["consistent"],
            json!(true)
        );
        assert!(outcome.text.contains("consistent:"), "{}", outcome.text);
    }

    #[test]
    fn the_history_reads_newest_first_and_shows_who_made_each_change() {
        let mut sessions = prepared();
        let outcome = call_with(&mut sessions, "maj_history", r#"{"limit":2}"#);
        assert!(!outcome.is_error, "{}", outcome.text);
        let structured = outcome.structured.expect("structured");
        // Three commands so far: the asset, the sequence and its track. Two are shown, and
        // they are the newest two.
        assert_eq!(structured["entries_total"], json!(3));
        assert_eq!(structured["entries_shown"], json!(2));
        assert_eq!(structured["entries"][0]["seq"], json!(2));
        assert_eq!(structured["entries"][1]["seq"], json!(1));
        assert!(outcome.text.contains("mcp:test"), "{}", outcome.text);
    }

    #[test]
    fn a_new_project_replaces_the_session_and_an_edit_needs_a_sequence() {
        let mut sessions = sessions();
        let outcome = call_with(&mut sessions, "maj_new_project", r#"{"name":"fresh"}"#);
        assert!(outcome.text.contains("fresh"), "{}", outcome.text);
        let outcome = call_with(&mut sessions, "maj_render_frame", r#"{"frame":0}"#);
        assert!(outcome.is_error);
        assert!(outcome.text.contains("no sequence"), "{}", outcome.text);
    }

    #[test]
    fn a_session_that_is_not_open_is_named_rather_than_silently_redirected() {
        let mut sessions = sessions();
        let outcome = call_with(&mut sessions, "maj_timeline", r#"{"session":"b-roll"}"#);
        assert!(outcome.is_error);
        assert!(outcome.text.contains("b-roll"), "{}", outcome.text);
    }

    #[test]
    fn a_grid_and_a_curve_are_read_the_way_a_person_writes_them() {
        assert_eq!(parse_grid("2x3").expect("a grid"), (2, 3));
        assert!(parse_grid("2by3").is_err());
        assert!(parse_grid("0x1").is_err());
        assert_eq!(
            parse_transfer("gamma_11_5").expect("a curve"),
            TransferFunction::Gamma { num: 11, den: 5 }
        );
        assert!(parse_transfer("sine").is_err());
        assert!(parse_transfer("gamma_1_0").is_err());
    }

    #[test]
    fn a_sequence_may_be_named_or_numbered() {
        let sessions = prepared();
        let project = sessions.get(None).expect("open").project();
        assert_eq!(
            sequence_arg(project, &Map::new()).expect("first"),
            SequenceId::new(2)
        );
        let mut args = Map::new();
        args.insert("sequence".to_owned(), Value::from("Reel A"));
        assert_eq!(
            sequence_arg(project, &args).expect("by name"),
            SequenceId::new(2)
        );
        args.insert("sequence".to_owned(), Value::from("reel a"));
        assert!(sequence_arg(project, &args).is_err());
    }

    #[test]
    fn a_frame_delivered_as_a_resource_is_the_frame_delivered_as_a_tool_result() {
        let mut sessions = prepared();
        call_with(
            &mut sessions,
            "maj_place_clip",
            r#"{"sequence":2,"kind":"video","asset":1,"source_range":[0,48],"timeline_start":0}"#,
        );
        let (png, digest) =
            frame_png(sessions.get(None).expect("open"), SequenceId::new(2), 6).expect("renders");
        let outcome = call_with(
            &mut sessions,
            "maj_render_frame",
            r#"{"sequence":2,"frame":6}"#,
        );
        assert_eq!(outcome.images[0].bytes, png);
        assert_eq!(
            outcome.structured.expect("structured")["digest"],
            json!(digest)
        );
        assert!(frame_png(sessions.get(None).expect("open"), SequenceId::new(99), 0).is_err());
    }

    #[test]
    fn the_content_array_is_text_first_and_then_the_pictures() {
        let outcome = Outcome::text("hello").image(Image {
            mime: "image/png".to_owned(),
            bytes: vec![1, 2, 3],
        });
        let content = outcome.content();
        assert_eq!(content[0]["type"], json!("text"));
        assert_eq!(content[1]["type"], json!("image"));
        assert_eq!(content[1]["mimeType"], json!("image/png"));
        assert_eq!(content[1]["data"], json!("AQID"));
    }
}

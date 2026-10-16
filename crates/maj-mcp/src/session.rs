//! A session: one project, one undo stack, one author.
//!
//! # Why the server holds sessions at all
//!
//! The command line is stateless by design — `maj edit` reads a document, applies one
//! command, writes it back — and that is the right shape for a script. An agent is not a
//! script: it reads the timeline, makes four related edits, looks at a frame, and undoes one
//! of them. Each of those steps needs the *same* project, and the undo stack needs to be the
//! same stack. So a session is a project plus the local inverse stack, and the server keeps a
//! small map of them so one client can hold a reel and a string-out at once.
//!
//! # What a session will not do
//!
//! It will not fabricate an undo stack after a load. The inverse of a command is computed
//! from the state the command observed, and after a round trip through the file that state is
//! gone. Opening a project therefore starts with an empty history, which is the honest
//! answer; the log itself is intact and can be replayed, diffed and reviewed either way.
//!
//! # Attribution
//!
//! Every command this module applies is recorded with the name of the client that asked for
//! it, taken from the MCP handshake. An agent's edits are therefore distinguishable from a
//! human's in the one place that matters — the log — without a separate side table.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use maj_core::{Command, CoreError, History, MaskShape, Project, TrackKind, format};
use maj_time::Timebase;
use serde_json::{Value, json};

/// Why a session could not be opened or addressed.
#[derive(Debug)]
pub enum SessionError {
    /// The model refused the document, or could not write it.
    Core(CoreError),
    /// A document could not be read, named as the caller named it.
    ///
    /// The model's own error says what was wrong with the document; it does not know which
    /// file the caller was talking about, and a report of a failure to open one of five
    /// candidate paths is worth much less than one that names the path.
    Open {
        /// The path, as the caller gave it.
        path: String,
        /// What the model said.
        message: String,
    },
    /// No session is open under that name.
    Unknown(String),
    /// The session has never been written, so there is nowhere to save it by default.
    NoPath(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(error) => write!(formatter, "{error}"),
            Self::Open { path, message } => write!(formatter, "`{path}`: {message}"),
            Self::Unknown(name) => write!(formatter, "no session named `{name}` is open"),
            Self::NoPath(name) => write!(
                formatter,
                "session `{name}` has never been saved, so `path` is required"
            ),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<CoreError> for SessionError {
    fn from(error: CoreError) -> Self {
        Self::Core(error)
    }
}

/// One open project, with its undo stack and the author its edits are recorded under.
#[derive(Debug, Clone)]
pub struct Session {
    name: String,
    project: Project,
    history: History,
    author: String,
    path: Option<PathBuf>,
}

impl Session {
    /// A session holding an empty project.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        project_name: impl Into<String>,
        author: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            project: Project::new(project_name),
            history: History::new(),
            author: author.into(),
            path: None,
        }
    }

    /// A session holding a document read from disk.
    ///
    /// The undo stack starts empty: see the module note. The path is remembered so a later
    /// save needs no argument.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::Core`] when the document cannot be read or is rejected by the
    /// model's own load gates.
    pub fn open(
        name: impl Into<String>,
        path: impl AsRef<Path>,
        author: impl Into<String>,
    ) -> Result<Self, SessionError> {
        let path = path.as_ref();
        let project = format::load_from_path(path).map_err(|error| SessionError::Open {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
        Ok(Self {
            name: name.into(),
            project,
            history: History::new(),
            author: author.into(),
            path: Some(path.to_path_buf()),
        })
    }

    /// The name the session answers to.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The author its commands are recorded under.
    #[must_use]
    pub fn author(&self) -> &str {
        &self.author
    }

    /// Records future commands under another author.
    ///
    /// Called once, when the client identifies itself during the handshake, so the log says
    /// which program made an edit rather than merely that one was made.
    pub fn set_author(&mut self, author: impl Into<String>) {
        self.author = author.into();
    }

    /// The project.
    #[must_use]
    pub const fn project(&self) -> &Project {
        &self.project
    }

    /// Where it was opened, when it was.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Applies one command, attributed to this session's author.
    ///
    /// # Errors
    ///
    /// Returns the model's own error for a command it refuses. A refused command changes
    /// nothing: it is applied to the project before it is logged, and it validates before it
    /// mutates.
    pub fn apply(&mut self, command: Command) -> Result<u64, CoreError> {
        self.history.apply(&mut self.project, command, &self.author)
    }

    /// Applies several commands as **one** edit: one undo step, and all or nothing.
    ///
    /// This is for the case where one thing a person or an agent asked for is more than one
    /// command in the model — adding a title is an asset and then a clip. Applied
    /// separately those are two edits to undo and, if the second is refused, the first
    /// stays. Grouped, undo reverts the whole thing and a refusal leaves the project
    /// untouched.
    ///
    /// The log is unchanged either way: each command is still recorded separately, under
    /// this session's author, so the document, its hashes and an agent's view of the log do
    /// not know the group happened. See [`History::apply_group`].
    ///
    /// # Errors
    ///
    /// Returns the model's own error for the command it refused, after rolling the group
    /// back.
    pub fn apply_group(&mut self, commands: Vec<Command>) -> Result<u64, CoreError> {
        self.history
            .apply_group(&mut self.project, commands, &self.author)
    }

    /// Reverts up to `count` commands, returning how many were reverted.
    ///
    /// Asking for more than the stack holds is not an error: it reverts what there is, which
    /// is what an agent catching up after a failure wants.
    ///
    /// # Errors
    ///
    /// Returns the model's error if an inverse cannot be applied, which would mean the
    /// stacks and the project had diverged.
    pub fn undo(&mut self, count: usize) -> Result<usize, CoreError> {
        let mut done = 0;
        for _ in 0..count {
            if self.history.undo(&mut self.project)?.is_none() {
                break;
            }
            done += 1;
        }
        Ok(done)
    }

    /// Re-applies up to `count` commands, returning how many were re-applied.
    ///
    /// # Errors
    ///
    /// Returns the model's error if a command cannot be re-applied.
    pub fn redo(&mut self, count: usize) -> Result<usize, CoreError> {
        let mut done = 0;
        for _ in 0..count {
            if self.history.redo(&mut self.project)?.is_none() {
                break;
            }
            done += 1;
        }
        Ok(done)
    }

    /// Replaces the project, as opening a new one or starting over does.
    pub fn replace(&mut self, project: Project) {
        self.project = project;
        self.history = History::new();
    }

    /// Writes the document, returning where it went and the hash that was written.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::NoPath`] when the session has no path and none was given, and
    /// [`SessionError::Core`] when the write or the hashing fails.
    pub fn save(&mut self, path: Option<&Path>) -> Result<(PathBuf, String), SessionError> {
        let destination = match path {
            Some(path) => path.to_path_buf(),
            None => self
                .path
                .clone()
                .ok_or_else(|| SessionError::NoPath(self.name.clone()))?,
        };
        format::save_to_path(&self.project, &destination)?;
        let digest = self.project.document_hash()?;
        self.path = Some(destination.clone());
        Ok((destination, digest))
    }

    /// How many steps can still be reverted and re-applied.
    #[must_use]
    pub fn depths(&self) -> (usize, usize) {
        (self.history.undo_depth(), self.history.redo_depth())
    }

    /// The project's three content hashes, which are what a verification compares.
    ///
    /// # Errors
    ///
    /// Returns the model's error if the project cannot be canonicalised.
    pub fn digests(&self) -> Result<Value, CoreError> {
        Ok(json!({
            "state": self.project.state_hash()?,
            "document": self.project.document_hash()?,
            "trace": self.project.command_trace_hash()?,
        }))
    }

    /// The state of the session, as a small object a client can read at a glance.
    ///
    /// # Errors
    ///
    /// Returns the model's error if the hashes cannot be computed.
    pub fn state(&self) -> Result<Value, CoreError> {
        let (undo, redo) = self.depths();
        let mut tracks = 0;
        let mut clips = 0;
        for sequence in self.project.sequences() {
            tracks += sequence.tracks().len();
            for track in sequence.tracks() {
                clips += track.clips().len();
            }
        }
        Ok(json!({
            "session": self.name,
            "path": self.path.as_ref().map(|path| path.display().to_string()),
            "project": self.project.name(),
            "schema_version": self.project.schema_version(),
            "revision": self.project.revision(),
            "assets": self.project.assets().len(),
            "sequences": self.project.sequences().len(),
            "tracks": tracks,
            "clips": clips,
            "undo_depth": undo,
            "redo_depth": redo,
            "digests": self.digests()?,
        }))
    }

    /// A one-line summary a person can read in a log.
    #[must_use]
    pub fn summary(&self) -> String {
        let (undo, redo) = self.depths();
        format!(
            "{} · {} · revision {} · {} sequences · {} assets · undo {undo}, redo {redo}",
            self.name,
            self.project.name(),
            self.project.revision(),
            self.project.sequences().len(),
            self.project.assets().len(),
        )
    }

    /// The timeline, as an object a client can walk.
    #[must_use]
    pub fn timeline(&self) -> Value {
        let sequences: Vec<Value> = self
            .project
            .sequences()
            .iter()
            .map(|sequence| {
                let tracks: Vec<Value> = sequence
                    .tracks()
                    .iter()
                    .map(|track| {
                        let clips: Vec<Value> = track
                            .clips()
                            .iter()
                            .map(|clip| {
                                let effects: Vec<Value> = clip
                                    .effects()
                                    .iter()
                                    .map(|effect| {
                                        json!({
                                            "id": effect.id().raw(),
                                            "kind": effect.kind().key(),
                                            "enabled": effect.is_enabled(),
                                        })
                                    })
                                    .collect();
                                json!({
                                    "id": clip.id().raw(),
                                    "name": clip.name(),
                                    "asset": clip.asset_id().raw(),
                                    "asset_name": self
                                        .project
                                        .asset(clip.asset_id())
                                        .map_or_else(String::new, |asset| asset.name().to_owned()),
                                    "source_range": {
                                        "start": clip.source_range().start(),
                                        "end": clip.source_range().end(),
                                    },
                                    "timeline_start": clip.timeline_start(),
                                    "timeline_length": clip.timeline_length(),
                                    "speed": rate(clip.speed()),
                                    "enabled": clip.is_enabled(),
                                    "opacity": rate(clip.opacity()),
                                    "blend": clip.blend().key(),
                                    "effects": effects,
                                    "masks": clip.masks().len(),
                                    "animated_channels": clip
                                        .animation()
                                        .iter()
                                        .map(|channel| channel.channel().key())
                                        .collect::<Vec<_>>(),
                                })
                            })
                            .collect();
                        let transitions: Vec<Value> = track
                            .transitions()
                            .iter()
                            .map(|transition| {
                                json!({
                                    "id": transition.id().raw(),
                                    "kind": transition.kind().key(),
                                    "at": transition.at(),
                                    "duration": transition.duration(),
                                    "from": transition.from().map(maj_core::ClipId::raw),
                                    "to": transition.to().map(maj_core::ClipId::raw),
                                })
                            })
                            .collect();
                        json!({
                            "id": track.id().raw(),
                            "kind": kind_name(track.kind()),
                            "name": track.name(),
                            "enabled": track.is_enabled(),
                            "locked": track.is_locked(),
                            "muted": track.is_muted(),
                            "clips": clips,
                            "transitions": transitions,
                        })
                    })
                    .collect();
                json!({
                    "id": sequence.id().raw(),
                    "name": sequence.name(),
                    "timebase": rate(sequence.timebase().into()),
                    "width": sequence.width(),
                    "height": sequence.height(),
                    "sample_rate": sequence.sample_rate(),
                    "duration_frames": sequence.duration_frames(),
                    "tracks": tracks,
                })
            })
            .collect();
        json!({
            "session": self.name,
            "revision": self.project.revision(),
            "assets": self
                .project
                .assets()
                .iter()
                .map(|asset| json!({
                    "id": asset.id().raw(),
                    "name": asset.name(),
                    "uri": asset.uri(),
                    "timebase": rate(asset.timebase().into()),
                    "duration_frames": asset.duration_frames(),
                    "has_video": asset.has_video(),
                    "has_audio": asset.has_audio(),
                }))
                .collect::<Vec<_>>(),
            "sequences": sequences,
        })
    }

    /// The timeline as text, in the shape an editor reads.
    ///
    /// Long timelines are a fact of the work, so this is the form an agent should read to
    /// orient itself: one line per sequence, one per track, one per clip, with the
    /// identifiers every other call needs.
    #[must_use]
    pub fn timeline_report(&self) -> String {
        let mut out = String::new();
        if self.project.sequences().is_empty() {
            out.push_str("no sequences yet\n");
        }
        for sequence in self.project.sequences() {
            out.push_str(&format!(
                "sequence#{} {}  {}x{}  {}  {} frames  {} tracks\n",
                sequence.id().raw(),
                sequence.name(),
                sequence.width(),
                sequence.height(),
                rate(sequence.timebase().into()),
                sequence.duration_frames().unwrap_or(-1),
                sequence.tracks().len(),
            ));
            for track in sequence.tracks() {
                out.push_str(&format!(
                    "  track#{} {} {}  {} clips",
                    track.id().raw(),
                    kind_name(track.kind()),
                    track.name(),
                    track.clips().len(),
                ));
                if !track.is_enabled() {
                    out.push_str("  disabled");
                }
                if track.is_locked() {
                    out.push_str("  locked");
                }
                if track.is_muted() {
                    out.push_str("  muted");
                }
                out.push('\n');
                for clip in track.clips() {
                    out.push_str(&format!(
                        "    clip#{} {}  source [{}, {})  at {}  {} frames  speed {}{}\n",
                        clip.id().raw(),
                        clip.name(),
                        clip.source_range().start(),
                        clip.source_range().end(),
                        clip.timeline_start(),
                        clip.timeline_length(),
                        rate(clip.speed()),
                        if clip.is_enabled() { "" } else { "  disabled" },
                    ));
                }
                for transition in track.transitions() {
                    out.push_str(&format!(
                        "    transition#{} {}  at {}  {} frames\n",
                        transition.id().raw(),
                        transition.kind().key(),
                        transition.at(),
                        transition.duration(),
                    ));
                }
            }
        }
        if !self.project.assets().is_empty() {
            out.push_str("media\n");
            for asset in self.project.assets() {
                out.push_str(&format!(
                    "  asset#{} {}  {}  {}  {} frames\n",
                    asset.id().raw(),
                    asset.name(),
                    asset.uri(),
                    rate(asset.timebase().into()),
                    asset.duration_frames(),
                ));
            }
        }
        out
    }

    /// One clip in full, with each effect's parameters and their declared ranges.
    ///
    /// The ranges are here rather than in the tool schema because the schema describes the
    /// *shape* of a call and this describes the *meaning* of a value: a key with no bounds
    /// shown might be one the model will refuse, and a model that has to discover the legal
    /// range by trial is a model that will spend its turns doing it.
    #[must_use]
    pub fn clip_detail(&self, clip: maj_core::ClipId) -> Option<Value> {
        let clip = self.project.clip(clip)?;
        let (sequence, track_index, _) = self.project.locate_clip(clip.id())?;
        let track = self.project.sequence(sequence)?.track_at(track_index)?.id();
        let effects: Vec<Value> = clip
            .effects()
            .iter()
            .map(|effect| {
                let params: Vec<Value> = effect
                    .params()
                    .iter()
                    .map(|param| {
                        let spec = effect.kind().spec(param.key());
                        json!({
                            "key": param.key(),
                            "value": param.value(),
                            "minimum": spec.and_then(|spec| spec.min).map(rate),
                            "maximum": spec.and_then(|spec| spec.max).map(rate),
                        })
                    })
                    .collect();
                json!({
                    "id": effect.id().raw(),
                    "kind": effect.kind().key(),
                    "enabled": effect.is_enabled(),
                    "params": params,
                })
            })
            .collect();
        let transform = clip.transform();
        Some(json!({
            "id": clip.id().raw(),
            "name": clip.name(),
            "sequence": sequence.raw(),
            "track": track.raw(),
            "asset": clip.asset_id().raw(),
            "source_range": {"start": clip.source_range().start(), "end": clip.source_range().end()},
            "timeline_start": clip.timeline_start(),
            "timeline_length": clip.timeline_length(),
            "speed": rate(clip.speed()),
            "enabled": clip.is_enabled(),
            "opacity": rate(clip.opacity()),
            "blend": clip.blend().key(),
            "transform": {
                "x": rate(transform.x()),
                "y": rate(transform.y()),
                "scale_x": rate(transform.scale_x()),
                "scale_y": rate(transform.scale_y()),
                "rotation": rate(transform.rotation()),
                "anchor_x": rate(transform.anchor_x()),
                "anchor_y": rate(transform.anchor_y()),
            },
            "effects": effects,
            "masks": clip.masks().iter().map(|mask| json!({
                "id": mask.id().raw(),
                "shape": shape_name(mask.shape()),
                "centre": [rate(mask.center().0), rate(mask.center().1)],
                "size": [rate(mask.size().0), rate(mask.size().1)],
                "feather": rate(mask.feather()),
                "opacity": rate(mask.opacity()),
                "inverted": mask.is_inverted(),
            })).collect::<Vec<_>>(),
            "animation": clip.animation().iter().map(|channel| json!({
                "channel": channel.channel().key(),
                "keys": channel.keys().iter().map(|key| json!({
                    "offset": key.offset(),
                    "value": key.value(),
                    "interpolation": key.interpolation(),
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }))
    }

    /// The command log, newest last, filtered to one author and capped at `limit`.
    #[must_use]
    pub fn log(&self, limit: usize, author: Option<&str>) -> Value {
        let entries: Vec<&maj_core::LogEntry> = self
            .project
            .log()
            .iter()
            .filter(|entry| author.is_none_or(|author| entry.author() == author))
            .collect();
        let shown = entries.len().min(limit);
        let (undo, redo) = self.depths();
        json!({
            "session": self.name,
            "revision": self.project.revision(),
            "entries_total": entries.len(),
            "entries_shown": shown,
            "undo_depth": undo,
            "redo_depth": redo,
            // The log reads newest first here, because a reader who asked for ten of two
            // thousand wants the ten that just happened.
            "entries": entries
                .iter()
                .rev()
                .take(shown)
                .map(|entry| json!({
                    "seq": entry.seq(),
                    "author": entry.author(),
                    "command": entry.command(),
                }))
                .collect::<Vec<_>>(),
        })
    }

    /// Rebuilds the project from its own log and compares the two.
    ///
    /// This is the check the whole architecture exists to make possible: the state is a
    /// function of the log, so anything that changed the state without logging it shows up
    /// here as a difference rather than as a mystery.
    ///
    /// # Errors
    ///
    /// Returns the model's error when the log cannot be replayed at all, which is a stronger
    /// statement than a mismatch and is reported as one.
    pub fn replay_check(&self) -> Result<Value, CoreError> {
        let rebuilt = format::rebuild_from_log(self.project.name(), self.project.log())?;
        let live = self.project.state_hash()?;
        let replayed = rebuilt.state_hash()?;
        Ok(json!({
            "consistent": live == replayed,
            "entries": self.project.log().len(),
            "live_state": live,
            "replayed_state": replayed,
            "live_document": self.project.document_hash()?,
            "replayed_document": rebuilt.document_hash()?,
        }))
    }
}

/// The name the model spells a track kind with.
///
/// Written out rather than round-tripped through serde, because the layer below stores the
/// kind as an enum and this is the name a person reads in a report.
fn kind_name(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Video => "video",
        TrackKind::Audio => "audio",
    }
}

/// The name the model spells a mask shape with.
fn shape_name(shape: MaskShape) -> &'static str {
    match shape {
        MaskShape::Rectangle => "rectangle",
        MaskShape::Ellipse => "ellipse",
    }
}

/// A rational as the `num/den` text a person and a model both read without ambiguity.
fn rate(value: maj_time::Rational) -> String {
    if value.den() == 1 {
        value.num().to_string()
    } else {
        format!("{}/{}", value.num(), value.den())
    }
}

/// The sessions a server holds, keyed by name.
///
/// A map with an ordered key keeps the listing stable, which matters because the same client
/// asking twice should get the same answer — the same reason everything else here is
/// deterministic.
#[derive(Debug, Clone)]
pub struct Sessions {
    sessions: BTreeMap<String, Session>,
    default: String,
}

impl Sessions {
    /// A registry holding one session, which is the one an unqualified call addresses.
    #[must_use]
    pub fn new(name: impl Into<String>, session: Session) -> Self {
        let name = name.into();
        let mut sessions = BTreeMap::new();
        sessions.insert(name.clone(), session);
        Self {
            sessions,
            default: name,
        }
    }

    /// The name an unqualified call addresses.
    #[must_use]
    pub fn default_name(&self) -> &str {
        &self.default
    }

    /// Every open session's name, in order.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.sessions.keys().map(String::as_str).collect()
    }

    /// Whether a session is open under a name.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.sessions.contains_key(name)
    }

    /// The session a call addresses, defaulting to the starting one.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::Unknown`] for a name that is not open, naming it, because a
    /// silently redirecting call to the default session would edit the wrong project.
    pub fn get(&self, name: Option<&str>) -> Result<&Session, SessionError> {
        self.sessions
            .get(self.resolve(name))
            .ok_or_else(|| SessionError::Unknown(self.resolve(name).to_owned()))
    }

    /// The same, mutably.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::Unknown`] as [`Self::get`] does.
    pub fn get_mut(&mut self, name: Option<&str>) -> Result<&mut Session, SessionError> {
        let key = self.resolve(name).to_owned();
        self.sessions
            .get_mut(&key)
            .ok_or(SessionError::Unknown(key))
    }

    /// Adds or replaces a session, which is what opening a second project does.
    pub fn insert(&mut self, name: impl Into<String>, session: Session) {
        self.sessions.insert(name.into(), session);
    }

    /// Records future commands of every open session under one author.
    pub fn set_author(&mut self, author: &str) {
        for session in self.sessions.values_mut() {
            session.set_author(author.to_owned());
        }
    }

    /// Closes a session, returning it.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::Unknown`] for a name that is not open.
    pub fn remove(&mut self, name: &str) -> Result<Session, SessionError> {
        self.sessions
            .remove(name)
            .ok_or_else(|| SessionError::Unknown(name.to_owned()))
    }

    /// Resolves an optional name to a key.
    fn resolve<'a>(&'a self, name: Option<&'a str>) -> &'a str {
        name.unwrap_or(&self.default)
    }
}

/// The name a session takes when a server is started without one.
pub const DEFAULT_SESSION: &str = "default";

/// A timebase as the text a person writes: `24000/1001`, or `24`.
///
/// # Panics
///
/// Never: a timebase is always a positive rational, which always has a name.
#[must_use]
pub fn timebase_name(timebase: Timebase) -> String {
    rate(timebase.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use maj_core::{MediaAsset, MediaAssetId, Sequence, SequenceId, Track, TrackId, TrackKind};
    use maj_time::{FrameRange, Rational};

    fn session() -> Session {
        Session::new("default", "scratch", "mcp:test")
    }

    fn populated() -> Session {
        let mut session = session();
        session
            .apply(Command::CreateMediaAsset {
                index: 0,
                asset: MediaAsset::new(
                    MediaAssetId::new(1),
                    "shot-01",
                    "generated:shot-01",
                    Timebase::FPS_24,
                    240,
                )
                .expect("a valid asset"),
            })
            .expect("applied");
        session
            .apply(Command::CreateSequence {
                index: 0,
                sequence: Sequence::new(
                    SequenceId::new(2),
                    "Reel A",
                    Timebase::FPS_24,
                    1920,
                    1080,
                    48_000,
                )
                .expect("a valid sequence"),
            })
            .expect("applied");
        session
            .apply(Command::CreateTrack {
                sequence: SequenceId::new(2),
                index: 0,
                track: Track::new(TrackId::new(3), TrackKind::Video, "V1"),
            })
            .expect("applied");
        session
            .apply(Command::AppendClip {
                sequence: SequenceId::new(2),
                track: TrackId::new(3),
                clip: maj_core::Clip::new(
                    maj_core::ClipId::new(4),
                    MediaAssetId::new(1),
                    "first",
                    FrameRange::new(0, 48).expect("a range"),
                    0,
                    Rational::from_integer(1),
                )
                .expect("a valid clip"),
            })
            .expect("applied");
        session
    }

    #[test]
    fn an_applied_command_is_recorded_against_the_client_that_asked() {
        let session = populated();
        assert_eq!(session.project().revision(), 4);
        let log = session.log(10, None);
        assert_eq!(log["entries_total"], json!(4));
        assert_eq!(log["entries"][0]["author"], json!("mcp:test"));
        assert_eq!(log["entries"][0]["command"]["kind"], json!("append_clip"));
        assert_eq!(log["undo_depth"], json!(4));
    }

    #[test]
    fn undo_and_redo_move_the_revision_back_and_forth() {
        let mut session = populated();
        assert_eq!(session.undo(2).expect("undoable"), 2);
        assert_eq!(session.project().revision(), 2);
        assert_eq!(session.redo(1).expect("redoable"), 1);
        assert_eq!(session.project().revision(), 3);
        // Asking for more than there is reverts what there is rather than failing.
        assert_eq!(session.undo(99).expect("undoable"), 3);
        assert_eq!(session.project().revision(), 0);
        assert_eq!(session.undo(1).expect("undoable"), 0);
    }

    #[test]
    fn the_state_is_the_state_of_the_log_it_wrote() {
        let session = populated();
        let check = session.replay_check().expect("replayable");
        assert_eq!(check["consistent"], json!(true), "{check}");
        assert_eq!(check["entries"], json!(4));
        assert_eq!(check["live_state"], check["replayed_state"]);
    }

    #[test]
    fn a_saved_project_reopens_with_the_same_document_hash() {
        let directory =
            std::env::temp_dir().join(format!("maj-mcp-session-tests-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let path = directory.join("session.maj");
        let mut session = populated();
        let (written, digest) = session.save(Some(&path)).expect("saved");
        assert_eq!(written, path);
        let reopened = Session::open("default", &path, "mcp:test").expect("reopened");
        assert_eq!(reopened.project().document_hash().expect("hashed"), digest);
        assert_eq!(reopened.project().revision(), 4);
        // The undo stack is session state and does not survive the round trip: the honest
        // answer, and the one the model's own documentation gives.
        assert_eq!(reopened.depths(), (0, 0));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn saving_a_session_that_was_never_written_needs_a_path() {
        let mut session = session();
        let error = session.save(None).expect_err("no destination");
        assert!(matches!(error, SessionError::NoPath(_)));
        assert!(error.to_string().contains("never been saved"), "{error}");
    }

    #[test]
    fn the_timeline_reports_the_identifiers_every_other_call_needs() {
        let session = populated();
        let timeline = session.timeline();
        assert_eq!(timeline["sequences"][0]["id"], json!(2));
        assert_eq!(timeline["sequences"][0]["timebase"], json!("24"));
        assert_eq!(timeline["sequences"][0]["tracks"][0]["id"], json!(3));
        let clip = &timeline["sequences"][0]["tracks"][0]["clips"][0];
        assert_eq!(clip["id"], json!(4));
        assert_eq!(clip["asset_name"], json!("shot-01"));
        assert_eq!(clip["source_range"], json!({"start": 0, "end": 48}));
        assert_eq!(clip["timeline_length"], json!(48));
        assert_eq!(clip["blend"], json!("normal"));
        assert_eq!(clip["animated_channels"], json!([]));
        assert_eq!(timeline["assets"][0]["uri"], json!("generated:shot-01"));
    }

    #[test]
    fn the_text_report_names_every_entity_a_caller_can_refer_to() {
        let report = populated().timeline_report();
        for fragment in [
            "sequence#2 Reel A",
            "1920x1080",
            "track#3 video V1",
            "clip#4 first",
            "source [0, 48)",
            "asset#1 shot-01",
        ] {
            assert!(
                report.contains(fragment),
                "{fragment} missing from:\n{report}"
            );
        }
        assert_eq!(
            Session::new("default", "empty", "mcp:test").timeline_report(),
            "no sequences yet\n"
        );
    }

    #[test]
    fn a_clip_detail_lists_each_parameter_with_its_declared_range() {
        let mut session = populated();
        session
            .apply(Command::AddEffect {
                sequence: SequenceId::new(2),
                clip: maj_core::ClipId::new(4),
                index: 0,
                effect: maj_core::EffectInstance::new(
                    maj_core::EffectId::new(5),
                    maj_core::EffectKind::Blur,
                ),
            })
            .expect("applied");
        let detail = session
            .clip_detail(maj_core::ClipId::new(4))
            .expect("the clip exists");
        assert_eq!(detail["sequence"], json!(2));
        assert_eq!(detail["track"], json!(3));
        assert_eq!(detail["effects"][0]["kind"], json!("blur"));
        assert_eq!(detail["effects"][0]["params"][0]["key"], json!("radius"));
        assert_eq!(
            detail["effects"][0]["params"][0]["value"]["type"],
            json!("rational")
        );
        assert_eq!(detail["effects"][0]["params"][0]["minimum"], json!("0"));
        assert_eq!(detail["effects"][0]["params"][0]["maximum"], json!("256"));
        assert_eq!(detail["transform"]["anchor_x"], json!("1/2"));
        assert!(session.clip_detail(maj_core::ClipId::new(99)).is_none());
    }

    #[test]
    fn the_log_can_be_filtered_to_one_author() {
        let mut session = populated();
        session = Session {
            author: "mcp:other".to_owned(),
            ..session
        };
        session
            .apply(Command::RemoveClip {
                sequence: SequenceId::new(2),
                clip: maj_core::ClipId::new(4),
            })
            .expect("applied");
        let mine = session.log(10, Some("mcp:test"));
        assert_eq!(mine["entries_total"], json!(4));
        let theirs = session.log(10, Some("mcp:other"));
        assert_eq!(theirs["entries_total"], json!(1));
        assert_eq!(
            theirs["entries"][0]["command"]["kind"],
            json!("remove_clip")
        );
    }

    #[test]
    fn a_call_can_address_a_named_session_and_an_unknown_one_is_named_back() {
        let sessions = Sessions::new(DEFAULT_SESSION, session());
        assert_eq!(sessions.names(), vec![DEFAULT_SESSION]);
        assert!(sessions.get(None).is_ok());
        assert!(sessions.get(Some(DEFAULT_SESSION)).is_ok());
        let error = sessions.get(Some("b-roll")).expect_err("not open");
        assert_eq!(error.to_string(), "no session named `b-roll` is open");
    }

    #[test]
    fn a_second_session_can_be_opened_beside_the_first() {
        let mut sessions = Sessions::new(DEFAULT_SESSION, session());
        sessions.insert("b-roll", Session::new("b-roll", "stringout", "mcp:test"));
        assert_eq!(sessions.names(), vec!["b-roll", DEFAULT_SESSION]);
        sessions
            .get_mut(Some("b-roll"))
            .expect("open")
            .apply(Command::CreateMediaAsset {
                index: 0,
                asset: MediaAsset::new(
                    MediaAssetId::new(1),
                    "b-roll-01",
                    "generated:b-roll-01",
                    Timebase::FPS_24,
                    96,
                )
                .expect("a valid asset"),
            })
            .expect("applied");
        assert_eq!(
            sessions
                .get(Some("b-roll"))
                .expect("open")
                .project()
                .assets()
                .len(),
            1
        );
        assert_eq!(
            sessions.get(None).expect("open").project().assets().len(),
            0,
            "the default session is untouched"
        );
        let closed = sessions.remove("b-roll").expect("open");
        assert_eq!(closed.name(), "b-roll");
        assert!(sessions.remove("b-roll").is_err());
    }
}

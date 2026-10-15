//! The shell: what a person is editing, and the one door every edit goes through.
//!
//! The state here is the session, the selection, the playhead, the zoom, the view choices and
//! the notes the interface has to say. Every mutation of the document goes through
//! [`Shell::dispatch`], which turns an [`Action`] into engine commands and applies them
//! through the same [`Session`] the Model Context Protocol server holds — the same type, the
//! same history, the same log, with a different author tag. There is no method here that
//! writes to the project directly, which is what makes "the interface cannot corrupt the
//! document" a structural fact rather than a habit.
//!
//! Nothing here draws, and nothing here opens a window, so every guarantee in this module is
//! tested without a screen.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};

use maj_color::TransferFunction;
use maj_core::{
    AnimChannel, Clip, ClipId, MediaAsset, MediaAssetId, Project, Sequence, SequenceId, TrackKind,
};
use maj_mcp::{Session, Sessions};
use maj_render::{
    Framebuffer, RenderManifest, RenderPolicy, RenderRequest, RenderTask, TitleAlign, TitleFace,
    WorkspaceResolver, render_frame,
};
use maj_time::{Rational, Timebase, Timecode, TimecodeMode};
use serde_json::{Map, Value};

use crate::UiError;
use crate::action::{Action, Context, Draft, Panels, TITLE_DEFAULT_DIVISOR, kind_of, title_uri};
use crate::i18n::{Key, Lang, text};
use crate::keys::Binding;
use crate::prefs::{Prefs, ZOOM_RANGE};
use crate::view::{PAGES, Page, SPLIT_RANGE, Switch, View};

/// The author recorded on commands the interface applies.
///
/// Deliberately distinct from the model's `DEFAULT_AUTHOR` (`local`) and from the server's
/// `mcp:<client>` names: a reviewer reading a log should be able to tell a person at the
/// keyboard from a model on a pipe without guessing from the timestamps.
pub const AUTHOR: &str = "ui:local";

/// The name the interface's session answers to.
pub const SESSION: &str = "ui";

/// The refusal the shell gives for a binding that only a window can resolve.
///
/// Four of the table's bindings are not the shell's to answer: fitting the sequence into the
/// lane depends on how wide the lane is, opening and saving-as need a path a person picks in a
/// dialog, writing a still needs the same, and the clock is a window's timer. The window
/// intercepts every one of them before they reach [`Shell::perform`], which is why they are
/// bound at all; this sentence is what the shell says if one ever stops being intercepted,
/// and a test asserts it does.
pub const NEEDS_WINDOW: &str = "this key is resolved by the window, not by the shell";

/// What stepping past the last cut says.
pub const NO_FURTHER_EDIT: &str = "there is no edit point that way";

/// What selecting past the last clip says.
pub const NO_FURTHER_CLIP: &str = "there is no clip that way";

/// What stepping past the last keyframe says.
pub const NO_FURTHER_KEYFRAME: &str = "there is no keyframe that way";

/// What a keyframe gesture says about a clip nobody has animated yet.
///
/// The interface will not invent a curve to put a key on: a parameter they never animated is a
/// parameter with a value, and recording it as a keyframe would be the interface deciding to
/// animate on their behalf rather than recording what they did.
pub const NOTHING_ANIMATED: &str = "nothing on this clip is animated yet";

/// How many notes the interface keeps before it starts dropping the oldest.
pub const NOTE_LIMIT: usize = 16;

/// The tile grid a tiled preview uses, which is not `1x1` and therefore means something.
pub const TILED_GRID: (u32, u32) = (2, 2);

/// How loud a note is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    /// Something happened that a person asked for.
    Info,
    /// Something failed. Never raised for a refusal the caller was already told about.
    Error,
}

/// Something the interface has to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// What it says.
    pub text: String,
    /// How loud it is.
    pub kind: NoteKind,
}

/// The command palette's state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Palette {
    /// Whether it is showing.
    pub open: bool,
    /// What has been typed into the search field.
    pub query: String,
    /// Which row is selected.
    pub row: usize,
    /// The argument fields of the selected row, as text.
    pub args: BTreeMap<String, String>,
}

/// What a dispatch did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    /// The commands that were appended, as their log names.
    pub commands: Vec<&'static str>,
    /// Something worth saying, when there is.
    pub note: Option<String>,
}

impl Report {
    /// A dispatch that changed nothing.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            commands: Vec::new(),
            note: None,
        }
    }

    /// Whether the document changed.
    #[must_use]
    pub fn is_edit(&self) -> bool {
        !self.commands.is_empty()
    }

    /// The first command's name, for a caller that only wants one.
    #[must_use]
    pub fn first(&self) -> Option<&'static str> {
        self.commands.first().copied()
    }
}

/// Everything that changes the pixels the viewer asks for.
///
/// The viewer keeps the frame it is showing next to this, so a worker can be asked for a
/// render that is already in flight only once, and a stale frame can be recognised as stale
/// without hashing anything.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewKey {
    /// The sequence being rendered.
    pub sequence: Option<SequenceId>,
    /// The frame.
    pub frame: i64,
    /// The width the renderer is asked for.
    pub width: u32,
    /// The height the renderer is asked for.
    pub height: u32,
    /// Whether it goes through tiles.
    pub tiled: bool,
    /// The curve its code values are drawn through.
    pub transfer: TransferFunction,
    /// The project revision, so an edit re-renders the same frame.
    pub revision: u64,
}

/// A rendered frame, with the manifest that says where it came from.
#[derive(Debug, Clone)]
pub struct Preview {
    /// The pixels, in scene-linear light.
    pub frame: Framebuffer,
    /// What the renderer recorded about them.
    pub manifest: RenderManifest,
    /// What was asked for.
    pub request: RenderRequest,
}

impl Preview {
    /// The digest of the pixels.
    #[must_use]
    pub fn digest(&self) -> String {
        self.frame.digest()
    }

    /// Whether any layer landed on the frame.
    #[must_use]
    pub fn drew_something(&self) -> bool {
        self.manifest.drew_something()
    }
}

/// The shell: the state a person edits.
#[derive(Debug)]
pub struct Shell {
    sessions: Sessions,
    sequence: Option<SequenceId>,
    selection: Option<ClipId>,
    media: Option<MediaAssetId>,
    playhead: i64,
    zoom: f64,
    draft: Draft,
    tiled: bool,
    transfer: TransferFunction,
    lang: Lang,
    panels: Panels,
    /// The window's arrangement: the page, the dividers and the three switches. None of it is
    /// document state, which is why it is here beside the preferences rather than in the log.
    view: View,
    palette: Palette,
    /// The title studio: the words, face, cap height and alignment a person is about to set.
    /// None of it is in the document until a title is added, which is why it sits beside the
    /// preferences rather than in the log.
    studio: TitleStudio,
    notes: VecDeque<Note>,
    saved_revision: u64,
    last_render_ms: Option<f64>,
}

/// The title studio: what a title would say before it exists.
///
/// It is deliberately plain data with no document in it, so a test can assert that typing
/// words moves no revision — the same rule every other piece of view state follows.
#[derive(Debug, Clone, PartialEq)]
pub struct TitleStudio {
    /// The words.
    pub text: String,
    /// The built-in face.
    pub face: TitleFace,
    /// The cap height in pixels, or zero for the sequence's own tenth.
    pub size: f32,
    /// Where the line sits between the margins.
    pub align: TitleAlign,
}

impl Default for TitleStudio {
    fn default() -> Self {
        Self {
            text: String::new(),
            face: TitleFace::Sans,
            size: 0.0,
            align: TitleAlign::Centre,
        }
    }
}

impl Shell {
    /// A shell over an empty project.
    #[must_use]
    pub fn new(project_name: &str) -> Self {
        let session = Session::new(SESSION, project_name, AUTHOR);
        Self::with_session(session, Prefs::default())
    }

    /// A shell over a session that already holds a document.
    #[must_use]
    pub fn with_session(session: Session, prefs: Prefs) -> Self {
        let prefs = prefs.sanitised();
        let saved_revision = session.project().revision();
        let sequence = session.project().sequences().first().map(Sequence::id);
        let mut shell = Self {
            sessions: Sessions::new(session.name().to_owned(), session),
            sequence,
            selection: None,
            media: None,
            playhead: 0,
            zoom: prefs.zoom,
            draft: prefs.draft,
            tiled: prefs.tiled,
            transfer: prefs.transfer,
            lang: prefs.lang,
            panels: prefs.panels,
            view: View {
                page: prefs.page,
                panels: prefs.panels,
                snap: prefs.snap,
                link: prefs.link,
                loops: prefs.loops,
                ..View::default()
            },
            palette: Palette::default(),
            studio: TitleStudio::default(),
            notes: VecDeque::new(),
            saved_revision,
            last_render_ms: None,
        };
        shell.reconcile();
        shell
    }

    /// A shell over a document read from disk.
    ///
    /// # Errors
    ///
    /// Returns a refusal carrying the model's own message when the document cannot be read
    /// or is rejected by its load gates.
    pub fn open(path: impl AsRef<Path>, prefs: Prefs) -> Result<Self, UiError> {
        let session = Session::open(SESSION, path.as_ref(), AUTHOR).map_err(|error| {
            UiError::Refused(format!("{}: {error}", text(prefs.lang, Key::ErrorOpen)))
        })?;
        Ok(Self::with_session(session, prefs))
    }

    /// Replaces the document with one read from disk, in place.
    ///
    /// # Errors
    ///
    /// Returns a refusal when the document cannot be read. The shell is left as it was, so an
    /// open that fails cannot lose the work in front of it.
    pub fn open_into(&mut self, path: impl AsRef<Path>) -> Result<Report, UiError> {
        let replacement = Self::open(path.as_ref(), self.prefs())?;
        self.sessions = replacement.sessions;
        self.sequence = replacement.sequence;
        self.selection = None;
        self.media = None;
        self.playhead = replacement.playhead;
        self.saved_revision = replacement.saved_revision;
        self.reconcile();
        Ok(Report {
            commands: Vec::new(),
            note: Some(format!("opened {}", path.as_ref().display())),
        })
    }

    /// Builds the example project, out of ordinary commands.
    ///
    /// This is not a fixture bolted beside the model: every step is the same [`Action`] the
    /// interface would take if a person performed it, applied through the same door. If the
    /// example ever stops being buildable, the interface itself is broken, and the tests say
    /// so.
    ///
    /// # Errors
    ///
    /// Returns whatever the engine refuses, which a correct build never does.
    pub fn example(project_name: &str) -> Result<Self, UiError> {
        let mut shell = Self::new(project_name);
        // Three shots that read as a graded sequence — a cold exterior, a warm interior, a
        // cool night — rather than as a test chart.
        //
        // They used to be `generated:`, which is the seeded chart: saturated bars over a ramp
        // over an orange checker. That chart has a job, and it is a good one — flat colour,
        // a ramp and hard edges are exactly what a renderer has to get right, and the tests
        // use it for that. It is the wrong thing to meet a person with. The first frame
        // somebody sees of an editor is the editor telling them what it is, and bars say
        // "test pattern". These are muted and distinguishable, so the viewer, the pool
        // thumbnails and the timeline all have real pixels to show and none of them shouts.
        //
        // The numbers are **scene-linear light**, not screen codes: `Solid` divides them by
        // 255 and hands the result to the compositor as light, and the viewer encodes once at
        // the output boundary. So they look roughly twice as bright as they read here — these
        // are chosen for what comes out of that transfer, not for what they say.
        for (index, (name, uri)) in [
            ("shot-01", "solid:11,24,30"),
            ("shot-02", "solid:47,26,11"),
            ("shot-03", "solid:20,27,40"),
        ]
        .into_iter()
        .enumerate()
        {
            shell.dispatch(Action::CreateMediaAsset {
                name: name.to_owned(),
                uri: uri.to_owned(),
                timebase: Timebase::FPS_24,
                duration_frames: 240,
                has_video: true,
                has_audio: index == 0,
            })?;
        }
        shell.dispatch(Action::CreateSequence {
            name: "Reel A".to_owned(),
            timebase: Timebase::FPS_24,
            width: 320,
            height: 180,
            sample_rate: 48_000,
        })?;
        shell.dispatch(Action::AddTrack(TrackKind::Video))?;
        shell.dispatch(Action::AddTrack(TrackKind::Audio))?;
        for name in ["shot-01", "shot-02", "shot-03"] {
            let id = shell
                .project()
                .assets()
                .iter()
                .find(|asset| asset.name() == name)
                .map(MediaAsset::id)
                .ok_or_else(|| UiError::Refused(format!("the example lost {name}")))?;
            shell.dispatch(Action::AppendMedia(id))?;
        }
        shell.dispatch(Action::GoToStart)?;
        Ok(shell)
    }

    /// The document.
    #[must_use]
    pub fn project(&self) -> &Project {
        self.session().project()
    }

    /// The session, which the interface shares with the agent's server.
    ///
    /// The shell holds exactly one session, created in its own constructor and never
    /// replaced except by another shell's, so this cannot fail in practice; if it ever did,
    /// the structure is broken rather than the input, and saying so loudly is better than
    /// handing back a wrong document.
    #[must_use]
    pub fn session(&self) -> &Session {
        self.sessions
            .get(None)
            .expect("the shell always holds its own session")
    }

    /// The session, mutably.
    fn session_mut(&mut self) -> &mut Session {
        self.sessions
            .get_mut(None)
            .expect("the shell always holds its own session")
    }

    /// The active sequence, if the document has one.
    #[must_use]
    pub fn sequence_entity(&self) -> Option<&Sequence> {
        self.sequence.and_then(|id| self.project().sequence(id))
    }

    /// The active sequence's identifier.
    #[must_use]
    pub const fn active_sequence(&self) -> Option<SequenceId> {
        self.sequence
    }

    /// The selected clip, if it still exists.
    #[must_use]
    pub fn selected_clip(&self) -> Option<&Clip> {
        self.selection.and_then(|id| self.project().clip(id))
    }

    /// The selected clip's identifier.
    #[must_use]
    pub const fn selection(&self) -> Option<ClipId> {
        self.selection
    }

    /// What a channel's diamond should look like: whether the channel has a curve at all, and
    /// whether a key sits on the frame under the playhead.
    ///
    /// Both answers come from the document rather than from the window, so a diamond cannot show
    /// a state the log does not have.
    #[must_use]
    pub fn keyframe_state(&self, channel: AnimChannel) -> (bool, bool) {
        let Some(clip) = self.selected_clip() else {
            return (false, false);
        };
        let Some(animation) = clip
            .animation()
            .iter()
            .find(|animation| animation.channel() == channel)
        else {
            return (false, false);
        };
        let offset = self.playhead.saturating_sub(clip.timeline_start());
        (
            true,
            animation.keys().iter().any(|key| key.offset() == offset),
        )
    }

    /// The value a channel has on the frame under the playhead: its curve's sample, or the
    /// clip's own field. `None` when nothing is selected or the channel is not a number.
    ///
    /// This is the number an inspector shows beside a channel, and it is deliberately the
    /// *sampled* value rather than the static one: a row that showed the field while a curve was
    /// running would be describing something the picture is not doing.
    #[must_use]
    pub fn channel_value_at_playhead(&self, channel: AnimChannel) -> Option<Rational> {
        let clip = self.selected_clip()?;
        let offset = self.playhead.saturating_sub(clip.timeline_start());
        if let Some(animation) = clip
            .animation()
            .iter()
            .find(|animation| animation.channel() == channel)
        {
            return animation.sample(offset).ok()?.as_rational();
        }
        let transform = clip.transform();
        Some(match channel {
            AnimChannel::Opacity => clip.opacity(),
            AnimChannel::X => transform.x(),
            AnimChannel::Y => transform.y(),
            AnimChannel::ScaleX => transform.scale_x(),
            AnimChannel::ScaleY => transform.scale_y(),
            AnimChannel::Rotation => transform.rotation(),
        })
    }

    /// Every keyframe of the selected clip, as the channel and the frame it sits on.
    ///
    /// The frames are timeline frames rather than clip offsets, because that is where the marks
    /// are drawn: a person reads a key's position off the ruler, not off the clip's own clock.
    #[must_use]
    pub fn selected_keyframes(&self) -> Vec<(AnimChannel, i64)> {
        let Some(clip) = self.selected_clip() else {
            return Vec::new();
        };
        let start = clip.timeline_start();
        let mut keys: Vec<(AnimChannel, i64)> = clip
            .animation()
            .iter()
            .flat_map(|animation| {
                animation
                    .keys()
                    .iter()
                    .map(move |key| (animation.channel(), start.saturating_add(key.offset())))
            })
            .collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    /// The media pool's selection.
    #[must_use]
    pub const fn media_selection(&self) -> Option<MediaAssetId> {
        self.media
    }

    /// The title studio: the words, face, cap height and alignment a title would be set with.
    #[must_use]
    pub const fn studio(&self) -> &TitleStudio {
        &self.studio
    }

    /// The cap height the studio would set a title at.
    ///
    /// A person who has not chosen one gets the sequence's own tenth, because a title is set
    /// for the frame it is going to be seen in and that frame is the sequence's.
    #[must_use]
    pub fn title_size(&self) -> f32 {
        if self.studio.size > 0.0 {
            return self.studio.size;
        }
        self.sequence_entity().map_or(72.0, |sequence| {
            f64::from(sequence.height()) as f32 / TITLE_DEFAULT_DIVISOR
        })
    }

    /// The URI a title built from the studio right now would carry.
    ///
    /// The window shows it under the field, because it is the whole seam between the interface
    /// and the alphabet: what a person typed, and what the renderer will be told.
    #[must_use]
    pub fn title_uri(&self) -> String {
        title_uri(
            &self.studio.text,
            self.studio.face,
            self.title_size(),
            self.studio.align,
        )
    }

    /// The intention that adds the studio's title at the playhead.
    ///
    /// The studio's own size is passed through rather than the resolved one, so a person who
    /// has not chosen a cap height keeps getting the sequence's tenth even if the sequence's
    /// size changes before the title is added.
    #[must_use]
    pub fn add_title(&self) -> Action {
        Action::AddTitle {
            text: self.studio.text.clone(),
            face: self.studio.face,
            size: self.studio.size,
            align: self.studio.align,
        }
    }

    /// The frame the playhead sits on.
    #[must_use]
    pub const fn playhead(&self) -> i64 {
        self.playhead
    }

    /// Pixels per frame on the timeline.
    #[must_use]
    pub const fn zoom(&self) -> f64 {
        self.zoom
    }

    /// The preview resolution.
    #[must_use]
    pub const fn draft(&self) -> Draft {
        self.draft
    }

    /// Whether the viewer renders through tiles.
    #[must_use]
    pub const fn tiled(&self) -> bool {
        self.tiled
    }

    /// The curve the viewer draws through.
    #[must_use]
    pub const fn transfer(&self) -> TransferFunction {
        self.transfer
    }

    /// The language the interface speaks.
    #[must_use]
    pub const fn lang(&self) -> Lang {
        self.lang
    }

    /// Which panels are open.
    #[must_use]
    pub const fn panels(&self) -> Panels {
        self.panels
    }

    /// The page that is showing.
    #[must_use]
    pub const fn page(&self) -> Page {
        self.view.page
    }

    /// The window's arrangement: the page, the dividers and the switches.
    #[must_use]
    pub const fn view(&self) -> &View {
        &self.view
    }

    /// Goes to a page. A page whose engine does not exist is refused rather than shown, so a
    /// person is never standing on a page that cannot say anything.
    ///
    /// # Errors
    ///
    /// Returns a refusal naming the phase that owns the page.
    pub fn set_page(&mut self, page: Page) -> Result<Report, UiError> {
        if !page.built() {
            return Err(UiError::Refused(format!(
                "{} is {}'s",
                page.key(),
                page.owner()
            )));
        }
        self.view = self.view.with_page(page);
        self.persist();
        Ok(Report::empty())
    }

    /// The palette's state.
    #[must_use]
    pub const fn palette(&self) -> &Palette {
        &self.palette
    }

    /// What the interface has said recently, oldest first.
    #[must_use]
    pub fn notes(&self) -> Vec<Note> {
        self.notes.iter().cloned().collect()
    }

    /// How long the last preview took, when one has been rendered.
    #[must_use]
    pub const fn last_render_ms(&self) -> Option<f64> {
        self.last_render_ms
    }

    /// Whether the document has changed since it was last written.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.project().revision() != self.saved_revision
    }

    /// How many commands can be undone, and how many redone.
    #[must_use]
    pub fn depths(&self) -> (usize, usize) {
        self.session().depths()
    }

    /// The document's hashes.
    ///
    /// # Errors
    ///
    /// Returns a refusal when the model cannot canonicalise the document.
    pub fn digests(&self) -> Result<Value, UiError> {
        self.session().digests().map_err(refused)
    }

    /// The state hash: the same value `maj render`, `maj verify` and the agent's server
    /// record for the same document.
    ///
    /// # Errors
    ///
    /// Returns a refusal when the model cannot canonicalise the document.
    pub fn state_hash(&self) -> Result<String, UiError> {
        self.project().state_hash().map_err(refused)
    }

    /// What the preferences should be, given the state.
    #[must_use]
    pub fn prefs(&self) -> Prefs {
        Prefs {
            lang: self.lang,
            zoom: self.zoom,
            draft: self.draft,
            tiled: self.tiled,
            transfer: self.transfer,
            panels: self.panels,
            page: self.view.page,
            snap: self.view.snap,
            link: self.view.link,
            loops: self.view.loops,
            left_w: self.view.left_w,
            right_w: self.view.right_w,
            timeline_h: self.view.timeline_h,
            media_split: self.view.media_split,
        }
    }

    /// Writes the preferences, because the arrangement changed.
    ///
    /// A failure here is deliberately silent: a preference is not worth interrupting an edit
    /// for, and the interface works without them. What is *not* silent is the arrangement
    /// itself, which lives in memory and is what the window reads.
    fn persist(&self) {
        let _ = self.prefs().save();
    }

    /// The file the document came from, if it came from one.
    #[must_use]
    pub fn path(&self) -> Option<PathBuf> {
        self.session().path().map(Path::to_path_buf)
    }

    /// Where a save would go.
    #[must_use]
    pub fn suggested_path(&self) -> PathBuf {
        self.path().unwrap_or_else(|| {
            let name = self.project().name().replace(' ', "-");
            PathBuf::from(format!("{name}.maj"))
        })
    }

    /// What the viewer is showing, for change detection.
    #[must_use]
    pub fn preview_key(&self) -> PreviewKey {
        let sequence = self.sequence_entity();
        PreviewKey {
            sequence: self.sequence,
            frame: self.playhead,
            width: sequence.map_or(0, |sequence| self.draft.scale(sequence.width())),
            height: sequence.map_or(0, |sequence| self.draft.scale(sequence.height())),
            tiled: self.tiled,
            transfer: self.transfer,
            revision: self.project().revision(),
        }
    }

    /// The frame the viewer should ask the renderer for.
    ///
    /// # Errors
    ///
    /// Returns a refusal when there is no sequence to render.
    pub fn preview_request(&self) -> Result<RenderRequest, UiError> {
        let sequence = self.sequence_entity().ok_or_else(UiError::no_sequence)?;
        Ok(RenderRequest::new(
            sequence.id(),
            self.playhead,
            self.draft.scale(sequence.width()),
            self.draft.scale(sequence.height()),
        ))
    }

    /// Renders the frame the viewer is showing.
    ///
    /// This is the same call `maj render` makes, with the same request and the same resolver,
    /// which is why a digest taken here and a digest taken from the command line agree. The
    /// window's viewer runs it on a worker rather than here; a test runs it directly.
    ///
    /// # Errors
    ///
    /// Returns a refusal when there is no sequence, or when the renderer refuses the request.
    pub fn render_preview(&self) -> Result<Preview, UiError> {
        let request = self.preview_request()?;
        let task = RenderTask {
            request,
            policy: RenderPolicy::Certified,
            grid: if self.tiled { TILED_GRID } else { (1, 1) },
        };
        let report = task
            .render(self.project(), &WorkspaceResolver::new())
            .map_err(refused)?;
        Ok(Preview {
            frame: report.frame,
            manifest: report.manifest,
            request,
        })
    }

    /// Renders one frame at an explicit size, for a still export and for tests.
    ///
    /// # Errors
    ///
    /// Returns a refusal when the sequence or the request is refused.
    pub fn render_frame_at(
        &self,
        frame: i64,
        width: u32,
        height: u32,
    ) -> Result<Framebuffer, UiError> {
        let sequence = self.sequence_entity().ok_or_else(UiError::no_sequence)?;
        render_frame(
            self.project(),
            RenderRequest::new(sequence.id(), frame, width, height),
            &WorkspaceResolver::new(),
        )
        .map_err(refused)
    }

    /// Records a note.
    pub fn note(&mut self, kind: NoteKind, text: impl Into<String>) {
        self.notes.push_back(Note {
            text: text.into(),
            kind,
        });
        while self.notes.len() > NOTE_LIMIT {
            self.notes.pop_front();
        }
    }

    /// Records a refusal so the interface can show it.
    pub fn note_refusal(&mut self, refusal: &UiError) {
        self.note(NoteKind::Error, refusal.to_string());
    }

    /// Forgets everything the interface has said.
    pub fn clear_notes(&mut self) {
        self.notes.clear();
    }

    /// The status line: what the interface says about the state of the work.
    ///
    /// Composed rather than formatted from a single translated sentence, so a language with a
    /// different word order only has to translate the labels.
    #[must_use]
    pub fn status_line(&self) -> String {
        let lang = self.lang;
        let mut parts = vec![format!(
            "{} {}",
            text(lang, Key::StatusFrame),
            self.playhead
        )];
        if let Some(sequence) = self.sequence_entity() {
            let mode = TimecodeMode::preferred_for(sequence.timebase());
            if let Ok(timecode) =
                Timecode::from_frames(self.playhead.max(0), sequence.timebase(), mode)
            {
                parts.push(timecode.to_string());
            }
            parts.push(format!("{}x{}", sequence.width(), sequence.height()));
        }
        parts.push(format!(
            "{} {}",
            text(lang, Key::StatusDraft),
            self.draft.key()
        ));
        parts.push(format!(
            "{} {}",
            text(lang, Key::StatusRevision),
            self.project().revision()
        ));
        parts.push(
            text(
                lang,
                if self.is_dirty() {
                    Key::StatusUnsaved
                } else {
                    Key::StatusSaved
                },
            )
            .to_owned(),
        );
        if let Ok(hash) = self.state_hash() {
            let short: String = hash.chars().take(8).collect();
            parts.push(format!("{} {short}", text(lang, Key::StatusDigest)));
        }
        if let Some(ms) = self.last_render_ms {
            parts.push(format!("{} {ms:.1} ms", text(lang, Key::StatusRenderTime)));
        }
        parts.push(format!(
            "{} {}",
            text(lang, Key::StatusLanguage),
            lang.key()
        ));
        parts.join(" · ")
    }

    /// Applies an intention.
    ///
    /// View intentions change what is displayed. Editing intentions become commands, and the
    /// commands of **one intention are applied as one edit**: they go into the log
    /// separately, in order, exactly as they always did, but they form a single undo step,
    /// and a refusal anywhere in them rolls back the whole intention.
    ///
    /// That matters because the model and the person count differently. Adding a title is
    /// one thing a person did and two commands in the document — an asset and a clip that
    /// uses it. Applied one at a time it would take two presses of undo to put back, and a
    /// refusal on the second would leave the first behind: a project holding half an edit
    /// nobody asked for, valid as far as the model is concerned and wrong as far as the
    /// person is concerned. [`Session::apply_group`] is what makes the two counts agree.
    ///
    /// # Errors
    ///
    /// Returns a refusal when the intention does not apply, or when the engine refused a
    /// command. A refusal is returned, never swallowed: what a person should see is the
    /// engine's own sentence. A refused intention has changed nothing.
    pub fn dispatch(&mut self, action: Action) -> Result<Report, UiError> {
        if let Some(report) = self.display(&action)? {
            return Ok(report);
        }
        let commands = {
            let context = Context::new(
                self.session().project(),
                self.sequence,
                self.selection,
                self.playhead,
            );
            action.commands(&context)?
        };
        if commands.is_empty() {
            return Ok(Report::empty());
        }
        let names: Vec<&'static str> = commands.iter().map(kind_of).collect();
        self.session_mut().apply_group(commands).map_err(refused)?;
        self.adopt_created();
        self.reconcile();
        let report = Report {
            commands: names,
            note: None,
        };
        Ok(report)
    }

    /// Applies a keyboard binding, resolving the parts that need the shell's own state.
    ///
    /// # Errors
    ///
    /// Returns whatever the intention it resolves to refuses.
    pub fn perform(&mut self, binding: Binding) -> Result<Report, UiError> {
        match binding {
            Binding::StepFrames(frames) => self.dispatch(Action::StepFrames(frames)),
            Binding::StepSeconds(seconds) => self.dispatch(Action::StepSeconds(seconds)),
            Binding::GoToStart => self.dispatch(Action::GoToStart),
            Binding::GoToEnd => self.dispatch(Action::GoToEnd),
            Binding::StepFramesAndSelect(frames) => {
                self.dispatch(Action::StepFrames(frames))?;
                self.select_under_playhead();
                Ok(Report::empty())
            }
            Binding::ZoomIn => self.dispatch(Action::ZoomBy(1.25)),
            Binding::ZoomOut => self.dispatch(Action::ZoomBy(0.8)),
            Binding::TrimStartAtPlayhead => {
                let to = self.playhead;
                self.dispatch(Action::TrimSelectedStart(to))
            }
            Binding::TrimEndAtPlayhead => {
                let to = self.playhead;
                self.dispatch(Action::TrimSelectedEnd(to))
            }
            Binding::MoveSelected(frames) => self.dispatch(Action::MoveSelected(frames)),
            Binding::MoveTrack(delta) => self.dispatch(Action::MoveSelectedToTrack(delta)),
            Binding::Split => self.dispatch(Action::SplitAtPlayhead),
            Binding::Remove => self.dispatch(Action::RemoveSelected),
            Binding::Duplicate => self.dispatch(Action::DuplicateSelected),
            Binding::CloseGap => self.dispatch(Action::CloseGap),
            Binding::RippleDelete => self.dispatch(Action::RippleDelete),
            Binding::InsertBlank => self.dispatch(Action::InsertBlank),
            // Keyframes resolve here for the same reason the edit points do: where a clip's keys
            // sit, and which of its channels have curves, are facts about the document that only
            // the shell is holding.
            Binding::StepKeyframe(step) => match self.keyframe_point(step) {
                Some(frame) => self.dispatch(Action::SeekTo(frame)),
                None => Err(UiError::NotApplicable(NO_FURTHER_KEYFRAME)),
            },
            Binding::ToggleKeyframes => {
                let channels = self.animated_channels();
                if channels.is_empty() {
                    return Err(UiError::NotApplicable(NOTHING_ANIMATED));
                }
                for channel in channels {
                    self.dispatch(Action::ToggleKeyframe(channel))?;
                }
                Ok(Report::empty())
            }
            Binding::ClearKeyframes => {
                let channels = self.animated_channels();
                if channels.is_empty() {
                    return Err(UiError::NotApplicable(NOTHING_ANIMATED));
                }
                for channel in channels {
                    self.dispatch(Action::ClearChannel(channel))?;
                }
                Ok(Report::empty())
            }
            // The two navigations resolve here because what counts as "the next cut" is a fact
            // about the sequence, and the shell is the only thing holding it. Neither appends a
            // command: looking along the cuts and selecting a clip are view state.
            Binding::StepEditPoint(step) => match self.edit_point(step) {
                Some(frame) => self.dispatch(Action::SeekTo(frame)),
                None => Err(UiError::NotApplicable(NO_FURTHER_EDIT)),
            },
            Binding::SelectAdjacent(step) => {
                let Some(clip) = self.adjacent_clip(step) else {
                    return Err(UiError::NotApplicable(NO_FURTHER_CLIP));
                };
                let start = self
                    .project()
                    .clip(clip)
                    .map_or(0, |clip| clip.timeline_start());
                self.dispatch(Action::SeekTo(start))?;
                self.dispatch(Action::SelectClip(clip))?;
                Ok(Report::empty())
            }
            Binding::ToggleEnabled => self.dispatch(Action::ToggleSelectedEnabled),
            Binding::Undo => self.dispatch(Action::Undo),
            Binding::Redo => self.dispatch(Action::Redo),
            Binding::Save => self.dispatch(Action::Save),
            Binding::Example => self.dispatch(Action::OpenExample),
            Binding::TogglePalette => {
                if self.palette.open {
                    self.dispatch(Action::ClosePalette)
                } else {
                    self.dispatch(Action::OpenPalette)
                }
            }
            Binding::Draft(draft) => self.dispatch(Action::SetDraft(draft)),
            Binding::ToggleTiled => self.dispatch(Action::SetTiled(!self.tiled)),
            Binding::ToggleLanguage => {
                let next = match self.lang {
                    Lang::English => Lang::Spanish,
                    Lang::Spanish => Lang::English,
                };
                self.dispatch(Action::SetLang(next))
            }
            Binding::AddVideoTrack => self.dispatch(Action::AddTrack(TrackKind::Video)),
            Binding::AddAudioTrack => self.dispatch(Action::AddTrack(TrackKind::Audio)),
            Binding::PlaceSelection => match self.media {
                Some(asset) => self.dispatch(Action::InsertMediaAtPlayhead(asset)),
                None => Err(UiError::NotApplicable(
                    "nothing is selected in the media pool",
                )),
            },
            // The bindings the window resolves against itself. A zoom that fits the whole
            // sequence into the lane depends on how wide the lane is, which is a fact about a
            // window and not about a document; an export still, an open and a save-as need a
            // path a person chooses, which is a dialog. The shell answers them by saying so,
            // rather than by guessing at a size or a file name — the window intercepts all of
            // them before they reach here, and this arm is what keeps the table honest if one
            // ever stops doing that.
            Binding::ZoomFit
            | Binding::ExportStill
            | Binding::Open
            | Binding::SaveAs
            | Binding::PlayPause
            | Binding::Shuttle(_)
            | Binding::ToggleLoop
            | Binding::CyclePage(_)
            | Binding::Page(_)
            | Binding::TogglePanel(_)
            | Binding::ToggleSnap
            | Binding::ToggleLink => Err(UiError::NotApplicable(NEEDS_WINDOW)),
        }
    }

    /// Runs an intention that changes only what is displayed, or that operates on the whole
    /// document rather than on the model's contents.
    ///
    /// Returns `None` for an intention the caller must translate into commands.
    fn display(&mut self, action: &Action) -> Result<Option<Report>, UiError> {
        let report = match action {
            Action::StepFrames(frames) => {
                self.seek(self.playhead.saturating_add(*frames));
                Report::empty()
            }
            Action::StepSeconds(seconds) => {
                let rate = self
                    .sequence_entity()
                    .map_or(24, |sequence| sequence.timebase().nominal_fps());
                let frames = i64::from(rate).saturating_mul(*seconds);
                self.seek(self.playhead.saturating_add(frames));
                Report::empty()
            }
            Action::SeekTo(frame) => {
                self.seek(*frame);
                Report::empty()
            }
            Action::GoToStart => {
                self.seek(0);
                Report::empty()
            }
            Action::GoToEnd => {
                let end = self.sequence_entity().map_or(0, sequence_end);
                self.seek(end);
                Report::empty()
            }
            Action::ZoomBy(factor) => {
                let zoom = self.zoom * factor;
                self.zoom = if zoom.is_finite() {
                    zoom.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1)
                } else {
                    self.zoom
                };
                Report::empty()
            }
            Action::TogglePanel(panel) => {
                self.panels = self.panels.toggled(*panel);
                self.view = self.view.with_panels(self.panels);
                self.persist();
                Report::empty()
            }
            Action::SetTitleText(text) => {
                self.studio.text = text.clone();
                Report::empty()
            }
            Action::SetTitleFace(face) => {
                self.studio.face = *face;
                Report::empty()
            }
            Action::SetTitleSize(size) => {
                // A size that is not a positive, finite number means "the sequence's own",
                // which is how a field a person has cleared asks for the default back.
                self.studio.size = if size.is_finite() && *size > 0.0 {
                    *size
                } else {
                    0.0
                };
                Report::empty()
            }
            Action::SetTitleAlign(align) => {
                self.studio.align = *align;
                Report::empty()
            }
            Action::SetPage(page) => {
                self.set_page(*page)?;
                Report::empty()
            }
            Action::CyclePage(step) => {
                let built: Vec<Page> = PAGES.into_iter().filter(|page| page.built()).collect();
                let current = built.iter().position(|page| *page == self.view.page);
                let count = built.len() as i64;
                let next = match (current, count) {
                    (Some(index), count) if count > 0 => {
                        ((index as i64 + step).rem_euclid(count)) as usize
                    }
                    _ => 0,
                };
                if let Some(page) = built.get(next).copied() {
                    self.set_page(page)?;
                }
                Report::empty()
            }
            Action::SetSplit(split, value) => {
                let clamped = (*value).clamp(SPLIT_RANGE.0, SPLIT_RANGE.1);
                self.view = self.view.with_width(*split, clamped);
                self.persist();
                Report::empty()
            }
            Action::ResetSplits => {
                self.view = self.view.reset();
                self.persist();
                Report::empty()
            }
            Action::SetSnap(value) => {
                self.view = self.view.with_switch(Switch::Snap, *value);
                self.persist();
                Report::empty()
            }
            Action::SetLink(value) => {
                self.view = self.view.with_switch(Switch::Link, *value);
                self.persist();
                Report::empty()
            }
            Action::SetLoops(value) => {
                self.view = self.view.with_switch(Switch::Loops, *value);
                self.persist();
                Report::empty()
            }
            Action::SetDraft(draft) => {
                self.draft = *draft;
                Report::empty()
            }
            Action::SetTiled(tiled) => {
                self.tiled = *tiled;
                Report::empty()
            }
            Action::SetTransfer(transfer) => {
                self.transfer = *transfer;
                Report::empty()
            }
            Action::SetLang(lang) => {
                self.lang = *lang;
                Report::empty()
            }
            Action::SelectSequence(sequence) => {
                if self.project().sequence(*sequence).is_none() {
                    return Err(UiError::no_sequence());
                }
                self.sequence = Some(*sequence);
                self.selection = None;
                self.seek(self.playhead);
                Report::empty()
            }
            Action::SelectClip(clip) => {
                if self.project().clip(*clip).is_none() {
                    return Err(UiError::no_selection());
                }
                self.selection = Some(*clip);
                self.sequence = self
                    .project()
                    .locate_clip(*clip)
                    .map(|(sequence, _, _)| sequence)
                    .or(self.sequence);
                Report::empty()
            }
            Action::SelectMedia(asset) => {
                if self.project().asset(*asset).is_none() {
                    return Err(UiError::NotApplicable("that media is not in the pool"));
                }
                self.media = Some(*asset);
                Report::empty()
            }
            Action::ClearSelection => {
                self.selection = None;
                Report::empty()
            }
            Action::OpenPalette => {
                self.palette.open = true;
                self.palette.row = 0;
                Report::empty()
            }
            Action::ClosePalette => {
                self.palette.open = false;
                self.palette.query.clear();
                self.palette.args.clear();
                Report::empty()
            }
            Action::SetPaletteQuery(query) => {
                self.palette.query = query.clone();
                self.palette.row = 0;
                Report::empty()
            }
            Action::SetPaletteRow(row) => {
                self.palette.row = *row;
                Report::empty()
            }
            Action::SetPaletteArg { field, value } => {
                self.palette.args.insert(field.clone(), value.clone());
                Report::empty()
            }
            Action::RunTool { name, args } => return self.run_tool(name, args).map(Some),
            Action::Undo => {
                let undone = self.session_mut().undo(1).map_err(refused)?;
                self.reconcile();
                Report {
                    commands: Vec::new(),
                    note: Some(if undone == 0 {
                        "nothing to undo".to_owned()
                    } else {
                        format!("undid {undone}")
                    }),
                }
            }
            Action::Redo => {
                let redone = self.session_mut().redo(1).map_err(refused)?;
                self.reconcile();
                Report {
                    commands: Vec::new(),
                    note: Some(if redone == 0 {
                        "nothing to redo".to_owned()
                    } else {
                        format!("redid {redone}")
                    }),
                }
            }
            Action::Save => return self.save().map(Some),
            Action::SaveAs(path) => return self.save_as(path).map(Some),
            Action::Open(path) => return self.open_into(path).map(Some),
            Action::OpenExample => return self.load_example().map(Some),
            Action::ExportStill(path) => return self.export_still(path).map(Some),
            _ => return Ok(None),
        };
        Ok(Some(report))
    }

    /// Replaces the document with the example project.
    ///
    /// The name is kept, because a person who asked for the example did not ask to rename
    /// the document they were working in.
    ///
    /// # Errors
    ///
    /// Returns whatever the engine refuses while the example is built, which a correct build
    /// never does.
    pub fn load_example(&mut self) -> Result<Report, UiError> {
        let example = Self::example(self.project().name())?;
        self.sessions = example.sessions;
        self.sequence = example.sequence;
        self.selection = None;
        self.media = None;
        self.playhead = example.playhead;
        self.saved_revision = example.saved_revision;
        self.reconcile();
        Ok(Report {
            commands: Vec::new(),
            note: Some(format!(
                "built the example: {} commands, {} assets",
                self.project().log().len(),
                self.project().assets().len()
            )),
        })
    }

    /// Writes the document where it came from.
    ///
    /// # Errors
    ///
    /// Refuses when the document has never been saved, because guessing a path is not the
    /// interface's decision to make.
    pub fn save(&mut self) -> Result<Report, UiError> {
        if self.path().is_none() {
            return Err(UiError::NotApplicable(
                "this document has no path yet: save it as something",
            ));
        }
        let (path, digest) = self.session_mut().save(None).map_err(refused)?;
        let revision = self.project().revision();
        self.saved_revision = revision;
        Ok(Report {
            commands: Vec::new(),
            note: Some(format!("saved {} · {digest}", path.display())),
        })
    }

    /// Writes the document to an explicit path and remembers it.
    ///
    /// # Errors
    ///
    /// Refuses when the document cannot be written.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<Report, UiError> {
        let (path, digest) = self
            .session_mut()
            .save(Some(path.as_ref()))
            .map_err(refused)?;
        let revision = self.project().revision();
        self.saved_revision = revision;
        Ok(Report {
            commands: Vec::new(),
            note: Some(format!("saved {} · {digest}", path.display())),
        })
    }

    /// Renders the frame under the playhead and writes it, with its manifest.
    ///
    /// A still is written at the sequence's own resolution and sixteen bits, exactly as
    /// `maj render` writes one, so the file and the command line can be compared byte for
    /// byte.
    ///
    /// # Errors
    ///
    /// Refuses when there is no sequence, when the renderer refuses, or when the file cannot
    /// be written.
    pub fn export_still(&mut self, path: impl AsRef<Path>) -> Result<Report, UiError> {
        let sequence = self.sequence_entity().ok_or_else(UiError::no_sequence)?;
        let request = RenderRequest::new(
            sequence.id(),
            self.playhead,
            sequence.width(),
            sequence.height(),
        );
        let task = RenderTask {
            request,
            policy: RenderPolicy::Certified,
            grid: if self.tiled { TILED_GRID } else { (1, 1) },
        };
        let report = task
            .render(self.project(), &WorkspaceResolver::new())
            .map_err(|error| {
                UiError::Refused(format!("{}: {error}", text(self.lang, Key::ErrorRender)))
            })?;
        let (written, manifest_path) = maj_export::write_frame_with_manifest(
            path.as_ref(),
            &report.frame,
            &report.manifest,
            maj_render::BitDepth::Sixteen,
            self.transfer,
        )
        .map_err(refused)?;
        Ok(Report {
            commands: Vec::new(),
            note: Some(format!(
                "wrote {} ({written} bytes) and {}",
                path.as_ref().display(),
                manifest_path.display()
            )),
        })
    }

    /// Runs one of the agent's tools against this session.
    ///
    /// The palette's rows are the same catalogue the Model Context Protocol server
    /// advertises, executed by the same function, against the same session a person is
    /// editing. That is the whole claim of the interface: the door an agent uses and the door
    /// a person uses are one door, and this method is where that stops being a slogan.
    ///
    /// # Errors
    ///
    /// Returns the tool's own refusal, carried through unchanged.
    pub fn run_tool(
        &mut self,
        name: &str,
        args: &BTreeMap<String, String>,
    ) -> Result<Report, UiError> {
        let mut arguments = Map::new();
        for (field, text) in args {
            arguments.insert(field.clone(), json_argument(text));
        }
        let session = self
            .sessions
            .remove(SESSION)
            .map_err(|error| UiError::Refused(error.to_string()))?;
        let mut registry = Sessions::new(SESSION.to_owned(), session);
        let outcome = maj_mcp::tools::call(&mut registry, name, &arguments);
        let session = registry
            .remove(SESSION)
            .map_err(|error| UiError::Refused(error.to_string()))?;
        self.sessions.insert(SESSION.to_owned(), session);
        let outcome = outcome.map_err(refused)?;
        self.adopt_created();
        self.reconcile();
        Ok(Report {
            commands: Vec::new(),
            note: Some(if outcome.is_error {
                format!("{name}: {}", outcome.text)
            } else {
                outcome.text.clone()
            }),
        })
    }

    /// Moves the playhead, clamped to the sequence's span.
    fn seek(&mut self, frame: i64) {
        let end = self.sequence_entity().map_or(0, sequence_end);
        self.playhead = frame.clamp(0, end.saturating_add(1).max(0));
    }

    /// Selects the clip under the playhead on the last video track that covers it.
    /// Every cut on the active sequence, in order and without repeats: the frames where a clip
    /// begins and where it ends.
    ///
    /// Only the picture tracks are walked, because those are the cuts a person sees in the
    /// monitor; an audio track's edges are not an edit point either eye can find.
    fn edit_points(&self) -> Vec<i64> {
        let Some(sequence) = self.sequence_entity() else {
            return Vec::new();
        };
        let mut frames: Vec<i64> = sequence
            .tracks()
            .iter()
            .filter(|track| track.kind() == TrackKind::Video)
            .flat_map(|track| track.clips())
            .flat_map(|clip| {
                let end = clip.timeline_start().saturating_add(clip.timeline_length());
                [clip.timeline_start(), end]
            })
            .collect();
        frames.sort_unstable();
        frames.dedup();
        frames
    }

    /// Every keyframe frame of the selected clip, in order and without repeats.
    ///
    /// The keys of every channel, not of one: the keyboard has no notion of a current channel,
    /// and a person stepping along keys is looking at where the picture changes rather than at
    /// which dial was turned.
    fn keyframe_points(&self) -> Vec<i64> {
        let mut frames: Vec<i64> = self
            .selected_keyframes()
            .into_iter()
            .map(|(_, frame)| frame)
            .collect();
        frames.sort_unstable();
        frames.dedup();
        frames
    }

    /// The next or previous keyframe, strictly after or before the playhead.
    fn keyframe_point(&self, step: i64) -> Option<i64> {
        let points = self.keyframe_points();
        if step > 0 {
            points.into_iter().find(|frame| *frame > self.playhead)
        } else {
            points
                .into_iter()
                .rev()
                .find(|frame| *frame < self.playhead)
        }
    }

    /// The channels of the selected clip that have a curve, in a fixed order.
    fn animated_channels(&self) -> Vec<AnimChannel> {
        let Some(clip) = self.selected_clip() else {
            return Vec::new();
        };
        let mut channels: Vec<AnimChannel> = clip
            .animation()
            .iter()
            .map(|animation| animation.channel())
            .collect();
        channels.sort_unstable();
        channels.dedup();
        channels
    }

    /// The next or previous cut, strictly after or before the playhead.
    fn edit_point(&self, step: i64) -> Option<i64> {
        let points = self.edit_points();
        if step > 0 {
            points.into_iter().find(|frame| *frame > self.playhead)
        } else {
            points
                .into_iter()
                .rev()
                .find(|frame| *frame < self.playhead)
        }
    }

    /// The next or previous clip, by where it starts.
    fn adjacent_clip(&self, step: i64) -> Option<ClipId> {
        let sequence = self.sequence_entity()?;
        let mut heads: Vec<(i64, ClipId)> = sequence
            .tracks()
            .iter()
            .filter(|track| track.kind() == TrackKind::Video)
            .flat_map(|track| track.clips())
            .map(|clip| (clip.timeline_start(), clip.id()))
            .collect();
        heads.sort_unstable();
        if step > 0 {
            heads
                .into_iter()
                .find(|(start, _)| *start > self.playhead)
                .map(|(_, id)| id)
        } else {
            heads
                .into_iter()
                .rev()
                .find(|(start, _)| *start < self.playhead)
                .map(|(_, id)| id)
        }
    }

    fn select_under_playhead(&mut self) {
        let Some(sequence) = self.sequence_entity() else {
            self.selection = None;
            return;
        };
        let frame = self.playhead;
        let found = sequence
            .tracks()
            .iter()
            .filter(|track| track.kind() == TrackKind::Video)
            .flat_map(|track| track.clips())
            .rev() // The last insertion is the topmost layer, which is what is on screen.
            .find(|clip| clip.covers(frame) && clip.is_enabled())
            .map(Clip::id);
        self.selection = found;
    }

    /// Adopts an entity a command just created, where adopting it is what the person meant.
    fn adopt_created(&mut self) {
        let sequences = self.project().sequences();
        if self
            .sequence
            .is_none_or(|id| self.project().sequence(id).is_none())
        {
            self.sequence = sequences.last().map(Sequence::id);
        }
    }

    /// Repairs the state after an edit: an active sequence that still exists, a selection
    /// that still exists, a playhead inside the sequence.
    fn reconcile(&mut self) {
        if self
            .sequence
            .is_none_or(|id| self.project().sequence(id).is_none())
        {
            self.sequence = self.project().sequences().first().map(Sequence::id);
        }
        if self
            .selection
            .is_some_and(|id| self.project().clip(id).is_none())
        {
            self.selection = None;
        }
        if self
            .media
            .is_some_and(|id| self.project().asset(id).is_none())
        {
            self.media = None;
        }
        let playhead = self.playhead;
        self.seek(playhead);
    }
}

/// The frame after the last one any track of a sequence holds.
#[must_use]
pub fn sequence_end(sequence: &Sequence) -> i64 {
    sequence
        .tracks()
        .iter()
        .filter_map(|track| track.end_frame())
        .max()
        .unwrap_or(0)
}

/// Turns a palette argument's text into a JSON value.
///
/// Argument fields are typed by the catalogue, which the tool decodes: an identifier, an
/// exact rational, a frame range, a switch, a name from a fixed set. The palette therefore
/// does not need to know what each field means — it passes the text through as JSON where
/// the text *is* JSON, and as a string where it is not, and the tool refuses anything it
/// cannot read with a message that names the field. That keeps one decoder in the project
/// instead of a second, weaker one in the interface.
#[must_use]
pub fn json_argument(text: &str) -> Value {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Value::Null;
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => value,
        Err(_) => Value::String(trimmed.to_owned()),
    }
}

/// Turns any error that can explain itself into a refusal with its own words.
fn refused(error: impl std::fmt::Display) -> UiError {
    UiError::Refused(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_shell_is_empty_and_saved() {
        let shell = Shell::new("empty");
        assert!(shell.sequence_entity().is_none());
        assert!(shell.selected_clip().is_none());
        assert_eq!(shell.playhead(), 0);
        assert!(!shell.is_dirty(), "a new document is not a modification");
        assert_eq!(shell.depths(), (0, 0));
    }

    #[test]
    fn the_example_project_builds_and_is_valid() {
        let shell = Shell::example("example").expect("the example builds");
        assert_eq!(shell.project().assets().len(), 3);
        assert_eq!(shell.project().sequences().len(), 1);
        assert!(
            shell.project().is_valid(),
            "the example must be a valid document"
        );
        let sequence = shell.sequence_entity().expect("an active sequence");
        assert_eq!(sequence.tracks().len(), 2);
        let video = sequence
            .tracks()
            .iter()
            .find(|track| track.kind() == TrackKind::Video)
            .expect("a video track");
        assert_eq!(video.clips().len(), 3);
        // Three clips of the whole of a 240-frame asset, one after another.
        assert!(
            video
                .clips()
                .iter()
                .all(|clip| clip.timeline_length() == 240)
        );
        assert_eq!(video.end_frame(), Some(720));
        assert!(shell.is_dirty(), "the example is a new, unsaved document");
    }

    #[test]
    fn a_view_action_changes_no_document_state() {
        let mut shell = Shell::example("example").expect("the example builds");
        let revision = shell.project().revision();
        shell.dispatch(Action::StepFrames(4)).expect("stepped");
        shell.dispatch(Action::ZoomBy(2.0)).expect("zoomed");
        shell
            .dispatch(Action::SetDraft(Draft::Quarter))
            .expect("drafted");
        shell.dispatch(Action::SetTiled(true)).expect("tiled");
        assert_eq!(shell.playhead(), 4);
        assert_eq!(shell.project().revision(), revision);
        assert_eq!(shell.draft(), Draft::Quarter);
        assert!(shell.tiled());
    }

    #[test]
    fn the_playhead_never_leaves_the_sequence() {
        let mut shell = Shell::example("example").expect("the example builds");
        shell.dispatch(Action::StepFrames(-50)).expect("stepped");
        assert_eq!(shell.playhead(), 0);
        shell.dispatch(Action::GoToEnd).expect("went to the end");
        assert_eq!(shell.playhead(), 720);
        shell.dispatch(Action::StepFrames(1_000)).expect("stepped");
        assert_eq!(
            shell.playhead(),
            721,
            "one frame past the end, and no further"
        );
    }

    #[test]
    fn the_zoom_is_bounded_and_never_becomes_a_nan() {
        let mut shell = Shell::new("empty");
        for _ in 0..40 {
            shell.dispatch(Action::ZoomBy(4.0)).expect("zoomed");
        }
        assert!(shell.zoom() <= ZOOM_RANGE.1);
        shell.dispatch(Action::ZoomBy(f64::NAN)).expect("ignored");
        assert!(shell.zoom().is_finite());
        assert!(shell.zoom() > 0.0);
    }

    #[test]
    fn an_intention_that_does_not_apply_is_refused_rather_than_approximated() {
        let mut shell = Shell::example("example").expect("the example builds");
        let revision = shell.project().revision();
        let refused = shell.dispatch(Action::SplitAtPlayhead);
        assert!(matches!(refused, Err(UiError::NotApplicable(_))));
        assert_eq!(
            shell.project().revision(),
            revision,
            "a refusal must not reach the document"
        );
    }

    #[test]
    fn the_palette_opens_closes_and_forgets_what_was_typed() {
        let mut shell = Shell::new("empty");
        shell.dispatch(Action::OpenPalette).expect("opened");
        assert!(shell.palette().open);
        shell
            .dispatch(Action::SetPaletteQuery("maj_".to_owned()))
            .expect("typed");
        shell
            .dispatch(Action::SetPaletteArg {
                field: "frame".to_owned(),
                value: "12".to_owned(),
            })
            .expect("typed");
        assert_eq!(shell.palette().query, "maj_");
        shell.dispatch(Action::ClosePalette).expect("closed");
        assert!(!shell.palette().open);
        assert!(shell.palette().query.is_empty());
        assert!(shell.palette().args.is_empty());
    }

    #[test]
    fn the_bindings_reach_the_state_they_need() {
        let mut shell = Shell::example("example").expect("the example builds");
        shell.perform(Binding::StepFrames(0)).expect("stepped");
        shell
            .perform(Binding::StepFramesAndSelect(1))
            .expect("stepped");
        assert_eq!(shell.playhead(), 1);
        assert!(
            shell.selected_clip().is_some(),
            "stepping onto a clip with Shift selects it"
        );
        let revision = shell.project().revision();
        shell.perform(Binding::ToggleLanguage).expect("switched");
        assert_eq!(shell.lang(), Lang::Spanish);
        assert_eq!(
            shell.project().revision(),
            revision,
            "a language is not an edit"
        );
        shell.perform(Binding::ToggleTiled).expect("toggled");
        assert!(shell.tiled());
        shell.perform(Binding::Draft(Draft::Half)).expect("chosen");
        assert_eq!(shell.draft(), Draft::Half);
    }

    #[test]
    fn trim_bindings_use_the_playhead() {
        let mut shell = Shell::example("example").expect("the example builds");
        shell.dispatch(Action::StepFrames(0)).expect("stepped");
        shell
            .perform(Binding::StepFramesAndSelect(10))
            .expect("stepped");
        shell
            .perform(Binding::TrimStartAtPlayhead)
            .expect("trimmed");
        let clip = shell.selected_clip().expect("still selected");
        assert_eq!(clip.timeline_start(), 10);
        assert_eq!(clip.source_range().start(), 10);
    }

    #[test]
    fn every_draft_scales_a_dimension_without_reaching_zero() {
        assert_eq!(Draft::Full.scale(320), 320);
        assert_eq!(Draft::Half.scale(321), 161);
        assert_eq!(Draft::Quarter.scale(1), 1);
    }

    #[test]
    fn the_preview_key_moves_with_everything_that_changes_the_pixels() {
        let mut shell = Shell::example("example").expect("the example builds");
        let base = shell.preview_key();
        shell.dispatch(Action::StepFrames(3)).expect("stepped");
        assert_ne!(base, shell.preview_key());
        let moved = shell.preview_key();
        shell
            .dispatch(Action::SetDraft(Draft::Half))
            .expect("drafted");
        assert_ne!(moved, shell.preview_key());
        let drafted = shell.preview_key();
        shell.dispatch(Action::SetTiled(true)).expect("tiled");
        assert_ne!(drafted, shell.preview_key());
        let tiled = shell.preview_key();
        shell
            .perform(Binding::StepFramesAndSelect(1))
            .expect("selected");
        shell
            .dispatch(Action::ToggleSelectedEnabled)
            .expect("disabled");
        assert_ne!(
            tiled,
            shell.preview_key(),
            "an edit re-renders the same frame"
        );
    }

    #[test]
    fn the_status_line_speaks_the_chosen_language() {
        let mut shell = Shell::example("example").expect("the example builds");
        let english = shell.status_line();
        assert!(english.contains("revision"));
        assert!(english.contains("language en"));
        shell
            .dispatch(Action::SetLang(Lang::Spanish))
            .expect("switched");
        let spanish = shell.status_line();
        assert!(spanish.contains("revisión"));
        assert!(spanish.contains("idioma es"));
        assert_ne!(english, spanish);
    }

    #[test]
    fn an_argument_field_is_json_when_it_is_json_and_text_when_it_is_not() {
        assert_eq!(json_argument("12"), Value::from(12));
        assert_eq!(json_argument("1/2"), Value::String("1/2".to_owned()));
        assert_eq!(json_argument("[0, 60]"), serde_json::json!([0, 60]));
        assert_eq!(json_argument("true"), Value::Bool(true));
        assert_eq!(
            json_argument("shot-01"),
            Value::String("shot-01".to_owned())
        );
        assert_eq!(json_argument("   "), Value::Null);
    }

    #[test]
    fn an_export_writes_a_still_and_its_manifest() {
        let directory = std::env::temp_dir().join("maj-ui-export");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let mut shell = Shell::example("example").expect("the example builds");
        let still = directory.join("frame.png");
        let report = shell.export_still(&still).expect("exported");
        assert!(still.is_file(), "the still was written");
        let manifest = maj_export::manifest::path_for(&still);
        assert!(manifest.is_file(), "the manifest was written");
        assert!(report.note.is_some(), "the report says what was written");
    }

    #[test]
    fn a_save_without_a_path_is_refused_rather_than_guessed() {
        let mut shell = Shell::example("example").expect("the example builds");
        assert!(matches!(
            shell.dispatch(Action::Save),
            Err(UiError::NotApplicable(_))
        ));
    }

    #[test]
    fn saving_clears_the_unsaved_state_and_survives_a_round_trip() {
        let directory = std::env::temp_dir().join("maj-ui-save");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        let path = directory.join("example.maj");
        let mut shell = Shell::example("example").expect("the example builds");
        assert!(shell.is_dirty());
        shell.dispatch(Action::SaveAs(path.clone())).expect("saved");
        assert!(!shell.is_dirty());
        let reopened = Shell::open(&path, Prefs::default()).expect("reopened");
        assert_eq!(
            reopened.project().state_hash().expect("a hash"),
            shell.project().state_hash().expect("a hash"),
            "a round trip through the document preserves the state"
        );
    }
}

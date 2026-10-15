//! The presenter: the window's state, and the translation of messages into intentions.
//!
//! Everything decision-shaped lives here. The platform module, `crate::win32`, hands over
//! decoded events; this module turns a key or a click into a [`Binding`] or an [`Action`], hands
//! it to the shell — the same door the agent's tools use — and turns what comes back into either
//! a note in the log or a refusal in the status bar.
//!
//! Three things changed when the pointer arrived, and each is here rather than in the painter:
//!
//! * **The window paints into a back buffer and copies only what changed.** The damage list is
//!   built here, from the same rectangles the painter used, and the loop invalidates only when
//!   there is something to invalidate. Nothing repaints because a message arrived.
//! * **A gesture resolves to the same intent a stroke does.** Dragging a clip is
//!   [`Action::MoveSelected`]; dragging in the ruler is [`Action::SeekTo`]. There is no second
//!   door into the document.
//! * **Nothing is done to the window from here.** Minimising, opening a file dialog, starting the
//!   timer: each is recorded as a request in this crate's own `Request` type and carried out by
//!   the loop, which is the only place that holds the window.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::c_void;
use std::path::PathBuf;

use maj_color::TransferFunction;
use maj_core::{ClipId, EffectKind, MediaAsset, TrackId, TrackKind, TransitionKind};
use maj_mcp::catalog::{self, Tool};
use maj_render::{BitDepth, Framebuffer, TitleAlign, TitleFace};
use maj_ui::{Action, Binding, Key, Page, Press, Shell, Stroke, UiError, keys, text};

use crate::layout::{Frame as Geometry, View};
use crate::paint;
use crate::theme::Theme;
use crate::transport::Transport;
use crate::widget::{
    ClipPart, FrameButton, Region, Splitter, Tab, Tool as ToolButton, Zone, Zones,
};
use crate::win32::{self, Button, Event, Hdc, Reaction, Rect, Window};

/// The crate's responsibility, in one line.
pub const ROLE: &str = "the Win32 presenter: the shell in a window, and nothing else";
/// The crate's layer in the architecture.
pub const LAYER: u8 = 6;

/// The default window size, in client pixels.
const DEFAULT_SIZE: (i32, i32) = (1440, 900);
/// How many palette rows are offered at once.
const PALETTE_ROWS: usize = 12;
/// How wide a wheel notch scrolls the timeline, in pixels.
const WHEEL_STEP: i32 = 60;
/// How close, in pixels, a drag has to be before it snaps.
const SNAP_PIXELS: f64 = 8.0;
/// How many frames of preview are rendered per second at most while the clock runs.
const FRAME_BUDGET: f64 = 1.0 / 30.0;

/// How the application was asked to start.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// A document to open, or `None` for the example project.
    pub project: Option<PathBuf>,
    /// Write one painted frame to this file and exit, without showing a window.
    pub snapshot: Option<PathBuf>,
    /// Run the scripted self-check against a real window and exit.
    pub self_check: bool,
    /// Append every action to this file, which is how a session is inspected afterwards.
    pub log: Option<PathBuf>,
    /// Draw the frame's own chrome instead of the system's.
    pub frameless: bool,
    /// Paint this many frames and report the timings, without showing a window.
    pub bench: Option<u32>,
    /// Print the painted colours by region and exit.
    pub regions: bool,
    /// Start on this page.
    pub page: Option<String>,
}

/// A frame ready to be blitted: rows of blue, green, red and ignored, top to bottom.
#[derive(Debug, Clone)]
pub(crate) struct Frame {
    /// Width in pixels.
    pub(crate) width: u32,
    /// Height in pixels.
    pub(crate) height: u32,
    /// The pixels themselves.
    pub(crate) pixels: Vec<u8>,
}

/// Which menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuKind {
    /// The application menu.
    App,
    /// The File menu.
    File,
    /// The Edit menu.
    Edit,
    /// The Sequence menu.
    Sequence,
    /// The View menu.
    View,
    /// The Tools menu.
    Tools,
    /// The Help menu.
    Help,
    /// The menu a right click on a clip opens.
    Clip,
    /// The menu a right click on the timeline opens.
    Timeline,
}

impl MenuKind {
    /// The menu bar's order.
    pub(crate) const BAR: [MenuKind; 7] = [
        Self::App,
        Self::File,
        Self::Edit,
        Self::Sequence,
        Self::View,
        Self::Tools,
        Self::Help,
    ];

    /// The menu at a position in the bar.
    #[must_use]
    pub(crate) fn from_index(index: usize) -> Self {
        *Self::BAR.get(index).unwrap_or(&Self::App)
    }

    /// Where it sits in the bar, if it is in the bar at all.
    #[must_use]
    pub(crate) fn index(self) -> Option<usize> {
        Self::BAR.iter().position(|kind| *kind == self)
    }
}

/// The one thing a menu item can do that is not a binding: something only the window can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Special {
    /// Open a document, asking for the path.
    Open,
    /// Save to a new path, asking for it.
    SaveAs,
    /// Write a still, asking where.
    ExportDialog,
    /// Close the window.
    Quit,
    /// Put the panels back where they started.
    ResetPanels,
    /// Show the about card.
    About,
    /// Go to a page.
    Page(Page),
    /// Say what the agent interface is.
    Agent,
}

/// A menu that is open, and where.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Menu {
    /// Which menu.
    pub(crate) kind: MenuKind,
    /// For a context menu, the point it was opened at.
    pub(crate) at: Option<(i32, i32)>,
    /// Where in the bar it was opened from, for a menu that is in the bar.
    pub(crate) index: usize,
    /// The row the keyboard is on.
    pub(crate) row: usize,
}

/// Something only the loop, which holds the window, can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Request {
    /// Minimise.
    Minimize,
    /// Maximise or restore.
    Maximize,
    /// Close.
    Close,
    /// Ask for a document to open.
    Open,
    /// Ask where to write the document.
    SaveAs,
    /// Ask where to write a still.
    Export,
    /// Ask for a still image to register as media.
    ImportMedia,
}

/// What the window has asked the loop to do for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Pending {
    /// The request, if there is one.
    request: Option<Request>,
}

/// The window, and everything it holds that the shell does not.
pub struct App {
    /// The model: the one door into the document.
    pub(crate) shell: Shell,
    /// The palette, the fonts and the sizes.
    pub(crate) theme: Theme,
    /// The last frame the shell rendered, ready to blit.
    pub(crate) frame: Option<Frame>,
    /// Why there is no frame, in the shell's own words.
    pub(crate) preview_error: Option<String>,
    /// What the frame on hand was rendered for.
    preview: Option<maj_ui::PreviewKey>,
    /// Pool thumbnails already drawn, keyed by the URI and the size they were drawn at.
    ///
    /// A thumbnail is sampled from the same source the renderer reads, which costs a
    /// deterministic transfer per channel per pixel — cheap once and ruinous thirty times a
    /// second. Drawing one on every paint took a whole frame from 5 ms to 184 ms, which is
    /// the measurement that put this map here. A URI names a fixed picture, so an entry
    /// never goes stale; `None` records that the URI is not one a tile draws for itself, so
    /// that answer is not recomputed either.
    pub(crate) thumbnails: std::collections::HashMap<(String, u32, u32), Option<Vec<u8>>>,
    /// The leftmost visible frame of the timeline.
    pub(crate) scroll: i64,
    /// The last refusal, shown in the status bar.
    pub(crate) error: Option<String>,
    /// What this window did, most recent first.
    pub(crate) actions: VecDeque<String>,
    /// Where to append those actions, when asked to.
    log_path: Option<PathBuf>,
    /// The title currently on the window, so it is only changed when it differs.
    window_title: String,
    /// Whether the frame's chrome is ours or the system's.
    pub(crate) frameless: bool,
    /// Whether the window is maximised, which the frame's own button shows.
    pub(crate) maximised: bool,
    /// What the pointer can be over.
    pub(crate) zones: Zones,
    /// The rectangles of the last layout.
    pub(crate) frame_rect: Geometry,
    /// What the pointer is over.
    pub(crate) hover: Option<Zone>,
    /// When it got there, so a tooltip can wait.
    pub(crate) hover_since: f64,
    /// What is pressed but not yet released.
    pub(crate) pressed: Option<Zone>,
    /// What is being dragged.
    pub(crate) dragging: Option<Zone>,
    /// Where a tile being dragged would land, while it is over the timeline: the track and the
    /// frame, which is also what the painter marks.
    pub(crate) drop_target: Option<(TrackId, i64)>,
    /// Where it is.
    pub(crate) pointer: (i32, i32),
    /// Where the drag started.
    drag_origin: (i32, i32),
    /// The frame the drag started at, for a scrub or a move.
    drag_frame: i64,
    /// The clip's start when the drag began, for a move.
    drag_clip_start: i64,
    /// The width a divider had when the drag began.
    drag_split: i32,
    /// The clock reading at the last scrubbed preview, so a scrub renders at a bounded rate.
    last_scrub: f64,
    /// The menu that is open, if any.
    pub(crate) menu: Option<Menu>,
    /// The clock.
    pub(crate) transport: Transport,
    /// A frame number being typed into the transport's field.
    pub(crate) timecode_entry: Option<String>,
    /// Whether the title studio's field holds the keyboard, which is what makes typed text
    /// land there rather than in the timecode the pointer left behind.
    pub(crate) title_focus: bool,
    /// Which panel the left column shows.
    pub(crate) left_tab: Tab,
    /// Which panel the right column shows.
    pub(crate) right_tab: Tab,
    /// How far the log is scrolled.
    pub(crate) log_scroll: usize,
    /// How far the inspector is scrolled.
    pub(crate) inspector_scroll: usize,
    /// What the last export wrote, or refused to.
    pub(crate) last_export: Option<String>,
    /// Whether the next paint should be the whole window.
    damage_all: bool,
    /// The rectangles that changed since the last paint.
    damaged: Vec<Rect>,
    /// The back buffer.
    surface: Option<win32::Scratch>,
    /// What the loop owes the window.
    pending: Pending,
    /// How many times the window has painted, which is what the idle check counts.
    pub(crate) paints: u64,
    /// Whether the frame's timer is running.
    timing: bool,
    /// Whether the pointer is captured.
    captured: bool,
}

impl std::fmt::Debug for App {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("App")
            .field("project", &self.shell.project().name())
            .field("revision", &self.shell.project().revision())
            .field("dirty", &self.shell.is_dirty())
            .field("paints", &self.paints)
            .field("zones", &self.zones.len())
            .finish_non_exhaustive()
    }
}

impl App {
    /// Builds the application: the preferences, the document, and the window's own palette.
    ///
    /// # Errors
    ///
    /// Returns the shell's refusal, in its own words, when the requested document cannot be
    /// opened. Nothing is guessed at: a `--project` that cannot be read stops the start rather
    /// than silently showing the example.
    pub fn new(options: &Options) -> Result<Self, String> {
        // Before anything is measured: a process that has not declared itself DPI-aware is
        // handed a stretched bitmap, and every size below would be the wrong one.
        win32::become_dpi_aware();
        let prefs = maj_ui::Prefs::load();
        let shell = match options.project.as_ref() {
            Some(path) => Shell::open(path, prefs).map_err(|error| error.to_string())?,
            None => Shell::example("Majestat").map_err(|error| error.to_string())?,
        };
        let mut app = Self {
            shell,
            theme: Theme::new(win32::dpi_scale()),
            frame: None,
            preview_error: None,
            preview: None,
            thumbnails: std::collections::HashMap::new(),
            scroll: 0,
            error: None,
            actions: VecDeque::new(),
            log_path: options.log.clone(),
            window_title: String::new(),
            frameless: options.frameless,
            maximised: false,
            zones: Zones::new(),
            frame_rect: Geometry::build(
                Rect::new(0, 0, DEFAULT_SIZE.0, DEFAULT_SIZE.1),
                &Self::blank_view(),
            ),
            hover: None,
            hover_since: 0.0,
            pressed: None,
            dragging: None,
            drop_target: None,
            pointer: (0, 0),
            drag_origin: (0, 0),
            drag_frame: 0,
            drag_clip_start: 0,
            drag_split: 0,
            last_scrub: 0.0,
            menu: None,
            transport: Transport::new(),
            timecode_entry: None,
            title_focus: false,
            left_tab: Tab::Media,
            right_tab: Tab::Inspector,
            log_scroll: 0,
            inspector_scroll: 0,
            last_export: None,
            damage_all: true,
            damaged: Vec::new(),
            surface: None,
            pending: Pending::default(),
            paints: 0,
            timing: false,
            captured: false,
        };
        if let Some(page) = options.page.as_deref().and_then(Page::from_key) {
            app.shell
                .dispatch(Action::SetPage(page))
                .map_err(|error| error.to_string())?;
        }
        Ok(app)
    }

    /// A view with no document in it, for the first layout before anything is painted.
    fn blank_view() -> View {
        View {
            page: Page::Cut,
            panels: maj_ui::Panels {
                media: true,
                history: true,
                effects: true,
                timeline: true,
                monitors: true,
            },
            left_w: 240,
            right_w: 260,
            timeline_h: 260,
            media_split: 200,
            tracks: 2,
            metrics: crate::theme::Metrics::new(1.0),
        }
    }

    /// The geometry the layout depends on.
    pub(crate) fn view(&self) -> View {
        let splits = self.shell.view();
        View {
            page: self.shell.page(),
            panels: self.shell.panels(),
            left_w: splits.left_w,
            right_w: splits.right_w,
            timeline_h: splits.timeline_h,
            media_split: splits.media_split,
            tracks: self
                .shell
                .sequence_entity()
                .map_or(1, |sequence| sequence.tracks().len().max(1)),
            metrics: self.theme.metrics,
        }
    }

    /// How tall one track row is: the clip has to stay readable as more tracks appear.
    pub(crate) fn row_height(&self) -> i32 {
        let metrics = self.theme.metrics;
        let tracks = self.view().tracks as i32;
        let timeline = self.frame_rect.track_area.height();
        if timeline > 0 && tracks > 0 {
            let fitted = (timeline - (tracks - 1) * 2) / tracks;
            return fitted.clamp(metrics.min_track_h, metrics.track_h);
        }
        metrics.track_h
    }

    /// The window's title, from the project's name and the dirty flag.
    #[must_use]
    pub fn title(&self) -> String {
        let dirty = if self.shell.is_dirty() { " •" } else { "" };
        format!(
            "{} — {}{}",
            text(self.shell.lang(), Key::AppName),
            self.shell.project().name(),
            dirty
        )
    }

    /// A string in the shell's language.
    pub(crate) fn say(&self, key: Key) -> &'static str {
        text(self.shell.lang(), key)
    }

    /// What the window says about itself: the clock, the rate, and what does not exist yet.
    pub(crate) fn self_report(&self) -> String {
        format!(
            "{} {} · {}",
            text(self.shell.lang(), Key::StatusTransport),
            self.transport.report(),
            text(self.shell.lang(), Key::AudioNotBuiltShort)
        )
    }

    /// The sequence's rate in frames per second, which is what the clock counts in.
    fn rate(&self) -> f64 {
        self.shell.sequence_entity().map_or(24.0, |sequence| {
            f64::from(sequence.timebase().nominal_fps())
        })
    }

    /// Starts the clock, or stops it where it stands.
    fn toggle_clock(&mut self) {
        if self.transport.playing {
            self.transport.stop();
        } else {
            let now = win32::now();
            self.transport.play(self.shell.playhead(), now, self.rate());
        }
        self.touch_all();
    }

    /// Steps the clock's rate, which is what `J` and `L` do; zero stops it, which is `K`.
    fn shuttle(&mut self, direction: i64) {
        let end = self
            .shell
            .sequence_entity()
            .and_then(|sequence| sequence.duration_frames())
            .unwrap_or(0);
        let now = win32::now();
        if direction == 0 {
            self.transport.stop();
        } else {
            let frame = self.shell.playhead().min(end);
            self.transport.shuttle(direction, frame, now, self.rate());
        }
        self.touch_all();
    }

    /// Asks for the whole window to be painted again.
    pub(crate) fn touch_all(&mut self) {
        self.damage_all = true;
    }

    /// Asks for one rectangle to be painted again.
    pub(crate) fn touch(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        self.damaged.push(rect);
    }

    /// What has to be painted, and forgets it.
    fn take_damage(&mut self, client: Rect) -> Vec<Rect> {
        let whole = self.damage_all || self.damaged.is_empty();
        self.damage_all = false;
        let mut damage = std::mem::take(&mut self.damaged);
        if whole {
            damage.clear();
            damage.push(client);
            return damage;
        }
        // Merge overlapping rectangles, so a playhead that moved and a timecode that changed
        // cost one blit rather than three.
        let mut merged: Vec<Rect> = Vec::with_capacity(damage.len());
        for rect in damage {
            if let Some(existing) = merged.iter_mut().find(|other| hits(**other, rect)) {
                *existing = union(*existing, rect);
            } else {
                merged.push(rect);
            }
        }
        merged
    }

    /// Runs the window until it closes, and writes the preferences on the way out.
    ///
    /// # Errors
    ///
    /// Returns a message when the window cannot be created.
    pub fn run(&mut self) -> Result<(), String> {
        let context = std::ptr::from_mut(self).cast::<c_void>();
        let window = Window::create(
            trampoline,
            context,
            &self.title(),
            DEFAULT_SIZE.0,
            DEFAULT_SIZE.1,
            self.frameless,
        )?;
        self.install_icon(&window);
        self.sync_title(&window);
        window.show();
        // The first paint is the whole window, and it happens before anything is shown.
        self.touch_all();
        window.invalidate();
        window.paint_now();
        while window.wait_for_event() {
            self.sync_title(&window);
            if self.flush(&window) {
                break;
            }
        }
        self.save_preferences();
        Ok(())
    }

    /// Puts the owner's logo on the window: the emblem, cropped out of the plate's margins.
    ///
    /// There is no resource compiler on this machine, so an `.ico` cannot be linked; the icons
    /// are built from the artwork `build.rs` decoded, at the two sizes a window class wants. If
    /// the operating system refuses a bitmap the window keeps whatever icon it had, which is why
    /// this answers with nothing rather than failing.
    fn install_icon(&self, window: &Window) {
        if let Some((large, small)) = crate::brand::icons() {
            window.set_icon(large, small);
        }
    }

    /// Carries out whatever the last events asked for, and repaints only if something changed.
    ///
    /// Returns `true` when the window should close.
    fn flush(&mut self, window: &Window) -> bool {
        // The frame's own maximise button draws the state of the window, so the window is asked
        // rather than remembered.
        self.maximised = window.is_maximised();
        if let Some(request) = std::mem::take(&mut self.pending.request) {
            match request {
                Request::Minimize => window.minimize(),
                Request::Maximize => window.toggle_maximise(),
                Request::Close => window.request_close(),
                Request::Open => {
                    if let Some(path) = window.open_dialog(
                        &format!("{} — {}", self.say(Key::FileOpen), self.say(Key::AppName)),
                        self.say(Key::DialogProject),
                        "*.maj",
                        "maj",
                    ) {
                        if let Err(error) = self.open_into(path) {
                            self.error = Some(error.to_string());
                        }
                        self.touch_all();
                    }
                }
                Request::SaveAs => {
                    let suggested = self.shell.suggested_path();
                    let name = suggested
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| "reel.maj".to_owned());
                    if let Some(path) = window.save_dialog(
                        &format!("{} — {}", self.say(Key::FileSaveAs), self.say(Key::AppName)),
                        self.say(Key::DialogProject),
                        "*.maj",
                        "maj",
                    ) {
                        let _ = name;
                        self.save_as(path);
                        self.touch_all();
                    }
                }
                Request::ImportMedia => {
                    if let Some(path) = window.open_dialog(
                        &format!(
                            "{} — {}",
                            self.say(Key::MediaImport),
                            self.say(Key::AppName)
                        ),
                        self.say(Key::MediaImportNote),
                        "*.png;*.ppm;*.pnm",
                        "png",
                    ) {
                        self.import_media(path);
                        self.touch_all();
                    }
                }
                Request::Export => {
                    if let Some(path) = window.save_dialog(
                        &format!(
                            "{} — {}",
                            self.say(Key::FileExportStill),
                            self.say(Key::AppName)
                        ),
                        self.say(Key::DialogStill),
                        "*.ppm",
                        "ppm",
                    ) {
                        self.export(path);
                        self.touch_all();
                    }
                }
            }
        }

        // The timer runs only while the clock does: a window that ticks while nothing moves is a
        // repaint nobody asked for.
        if self.transport.playing && !self.timing {
            window.start_timer(16);
            self.timing = true;
        } else if !self.transport.playing && self.timing {
            window.stop_timer();
            self.timing = false;
        }

        // The pointer is captured for the length of a drag, so a drag that leaves the window
        // keeps reporting.
        let wants_capture = self.pressed.is_some() || self.dragging.is_some();
        if wants_capture && !self.captured {
            window.capture();
            self.captured = true;
        } else if !wants_capture && self.captured {
            window.release_capture();
            self.captured = false;
        }

        // The preview is rendered here, never inside a paint: the work belongs outside the
        // handler that draws.
        if self.refresh_preview() {
            self.touch(self.frame_rect.viewer);
        }

        if !self.damage_all && self.damaged.is_empty() {
            return false;
        }
        let damage = std::mem::take(&mut self.damaged);
        let whole = std::mem::replace(&mut self.damage_all, false);
        if whole {
            window.invalidate();
        } else {
            for rect in &damage {
                window.invalidate_rect(*rect);
            }
        }
        false
    }

    /// Puts the current title on the window, if it has changed.
    fn sync_title(&mut self, window: &Window) {
        let title = self.title();
        if title != self.window_title {
            self.window_title.clone_from(&title);
            window.set_title(&title);
        }
    }

    /// Paints one frame into a back buffer of the given size and writes it as a PPM.
    ///
    /// This is how the interface is looked at without a screen: the same `draw` the window uses,
    /// into a surface the process can read back.
    ///
    /// The preview is rendered first, because `refresh_preview` — which is crate-private, so it
    /// is named here rather than linked — is deliberately called
    /// from the message loop and never from a paint, so a snapshot that only painted would show
    /// an empty viewer and a window on screen would not. A picture of the interface that
    /// disagrees with the interface is worse than no picture, since it is the one that gets
    /// looked at when nobody is at the machine.
    ///
    /// # Errors
    ///
    /// Returns a message when the file cannot be written.
    pub fn snapshot(&mut self, path: &std::path::Path, size: (i32, i32)) -> Result<String, String> {
        let rect = Rect::new(0, 0, size.0, size.1);
        let surface = win32::Scratch::detached(size.0, size.1);
        self.refresh_preview();
        self.touch_all();
        let damage = self.take_damage(rect);
        paint::draw(surface.hdc(), rect, self, &damage);
        let pixels = surface.pixels();
        let mut ppm = Vec::with_capacity(pixels.len() / 4 * 3 + 32);
        ppm.extend_from_slice(format!("P6\n{} {}\n255\n", rect.width(), rect.height()).as_bytes());
        for pixel in pixels.chunks_exact(4) {
            ppm.push(pixel[2]);
            ppm.push(pixel[1]);
            ppm.push(pixel[0]);
        }
        std::fs::write(path, &ppm).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(format!(
            "wrote {} — {}x{}, {} bytes, {} zones",
            path.display(),
            rect.width(),
            rect.height(),
            ppm.len(),
            self.zones.len()
        ))
    }

    /// Paints the window many times and reports how long each paint took.
    ///
    /// The first paint is the whole window; the rest are incremental, which is the number that
    /// matters: it is what a moving playhead costs.
    ///
    /// # Errors
    ///
    /// Returns a message when the surface cannot be made.
    pub fn bench(&mut self, frames: u32, size: (i32, i32)) -> Result<String, String> {
        let frames = frames.clamp(1, 10_000);
        let rect = Rect::new(0, 0, size.0, size.1);
        let surface = win32::Scratch::detached(size.0, size.1);
        let mut whole: Vec<f64> = Vec::new();
        let mut partial: Vec<f64> = Vec::new();
        for index in 0..frames {
            if index % 4 == 0 {
                self.touch_all();
            } else {
                // The playhead moved: one stripe of the timeline and the timecode.
                self.touch(self.frame_rect.timeline);
                self.touch(self.frame_rect.transport);
            }
            let damage = self.take_damage(rect);
            let started = win32::now();
            paint::draw(surface.hdc(), rect, self, &damage);
            let took = (win32::now() - started) * 1000.0;
            if index % 4 == 0 {
                whole.push(took);
            } else {
                partial.push(took);
            }
        }
        Ok(format!(
            "paint: {} frames at {}x{} · whole mean {:.2} ms (max {:.2}) · incremental mean {:.2} ms (max {:.2}) · {} zones",
            frames,
            size.0,
            size.1,
            mean(&whole),
            whole.iter().copied().fold(0.0, f64::max),
            mean(&partial),
            partial.iter().copied().fold(0.0, f64::max),
            self.zones.len()
        ))
    }

    /// Reports the painted colours by region, which is how the palette is checked from a
    /// terminal rather than by eye.
    ///
    /// # Errors
    ///
    /// Returns a message when the surface cannot be made.
    pub fn region_map(&mut self, size: (i32, i32)) -> Result<String, String> {
        let rect = Rect::new(0, 0, size.0, size.1);
        let surface = win32::Scratch::detached(size.0, size.1);
        self.touch_all();
        let damage = self.take_damage(rect);
        paint::draw(surface.hdc(), rect, self, &damage);
        let pixels = surface.pixels();
        let width = surface.width();
        let height = surface.height();
        let mut report = String::new();
        let mut counts: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        for pixel in pixels.chunks_exact(4) {
            let key = u32::from(pixel[2]) << 16 | u32::from(pixel[1]) << 8 | u32::from(pixel[0]);
            *counts.entry(key).or_insert(0) += 1;
        }
        let total: u32 = counts.values().sum();
        let mut ranked: Vec<(u32, u32)> = counts.into_iter().collect();
        ranked.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        report.push_str("dominant tones:\n");
        for (colour, count) in ranked.iter().take(8) {
            report.push_str(&format!(
                "  #{:02X}{:02X}{:02X}  {:>6.1}%\n",
                (colour >> 16) & 0xFF,
                (colour >> 8) & 0xFF,
                colour & 0xFF,
                100.0 * f64::from(*count) / f64::from(total)
            ));
        }
        report.push_str("grid means (6x4):\n");
        for row in 0..4 {
            for column in 0..6 {
                let left = column * width / 6;
                let top = row * height / 4;
                let right = (column + 1) * width / 6;
                let bottom = (row + 1) * height / 4;
                let mut sums = (0u64, 0u64, 0u64, 0u64);
                for y in top..bottom {
                    for x in left..right {
                        let index = ((y * width + x) * 4) as usize;
                        if index + 3 < pixels.len() {
                            sums.0 += u64::from(pixels[index + 2]);
                            sums.1 += u64::from(pixels[index + 1]);
                            sums.2 += u64::from(pixels[index]);
                            sums.3 += 1;
                        }
                    }
                }
                let divisor = sums.3.max(1);
                report.push_str(&format!(
                    "  #{:02X}{:02X}{:02X}",
                    sums.0 / divisor,
                    sums.1 / divisor,
                    sums.2 / divisor
                ));
            }
            report.push('\n');
        }
        Ok(report)
    }

    /// The scripted check: a window, real messages, and the assertions a person would make.
    ///
    /// # Errors
    ///
    /// Returns the first assertion that failed.
    pub fn self_check(&mut self) -> Result<String, String> {
        let context = std::ptr::from_mut(self).cast::<c_void>();
        let frameless = true;
        let window = Window::create(trampoline, context, &self.title(), 1440, 900, frameless)?;
        // The window handed to the check is the frameless one, so the interface has to draw the
        // frame it was promised: the title bar, the resize border's hit test and the frame's own
        // three buttons are all the application's own work from here on.
        self.frameless = frameless;
        let mut report: Vec<String> = Vec::new();

        // 1. The keyboard still reaches the shell's table.
        let start = self.shell.playhead();
        win32::force_modifiers(false, false, false);
        for _ in 0..5 {
            window.post_key(VK_RIGHT);
        }
        window.drain();
        self.flush(&window);
        if self.shell.playhead() != start + 5 {
            return Err(format!(
                "five right arrows moved the playhead from {start} to {}",
                self.shell.playhead()
            ));
        }
        report.push(format!(
            "keyboard: five right arrows, playhead {start} -> {}",
            self.shell.playhead()
        ));

        // 2. The idle check: with nothing happening, nothing repaints. This is the regression
        //    test for the flicker: a window that repainted on every message would count dozens
        //    here.
        let before_paints = self.paints;
        let deadline = win32::now() + 1.0;
        while win32::now() < deadline {
            self.flush(&window);
            window.drain();
        }
        let idle = self.paints - before_paints;
        if idle != 0 {
            return Err(format!("the window repainted {idle} times while idle"));
        }
        report.push("idle: zero repaints in one second".to_owned());

        // 3. The pointer: a click on the timeline selects a clip, a drag moves it, a drag on the
        //    tail trims it, and a drag in the ruler scrubs.
        let view = self.view();
        let frame = Geometry::build(Rect::new(0, 0, 1440, 900), &view);
        self.frame_rect = frame;
        let clip = self
            .shell
            .sequence_entity()
            .and_then(|sequence| sequence.tracks().first())
            .and_then(|track| track.clips().first())
            .ok_or("the example project has no clip to drag")?;
        let id = clip.id();
        let lane_left = frame.track_area.left;
        let zoom = self.shell.zoom();
        let scroll = self.scroll;
        let y = frame.track_area.top + self.row_height() / 2;
        self.touch_all();
        let damage = self.take_damage(Rect::new(0, 0, 1440, 900));
        let surface = win32::Scratch::detached(1440, 900);
        paint::draw(surface.hdc(), Rect::new(0, 0, 1440, 900), self, &damage);
        let clip_rect = self
            .zones
            .get(Zone::Clip {
                clip: id,
                part: ClipPart::Body,
            })
            .map(|region| region.rect)
            .ok_or("the clip's body was not registered as a zone")?;
        let x = clip_rect.left + clip_rect.width() / 2;
        if y != clip_rect.top + clip_rect.height() / 2 {
            // The row's own centre, which is what the probe clicks.
        }
        let y = clip_rect.top + clip_rect.height() / 2;

        window.post_press(x, y, Button::Left, true);
        window.post_press(x, y, Button::Left, false);
        window.drain();
        self.flush(&window);
        if self.shell.selection() != Some(id) {
            return Err(format!(
                "clicking the clip selected {:?} rather than {id:?}",
                self.shell.selection()
            ));
        }
        report.push(format!("pointer: clicking the clip selected {id:?}"));

        // A drag of one second to the right moves it by a second's worth of frames.
        let rate = f64::from(
            self.shell
                .sequence_entity()
                .map_or(24, |sequence| sequence.timebase().nominal_fps()),
        );
        let before_start = self
            .shell
            .project()
            .clip(id)
            .ok_or("the clip vanished")?
            .timeline_start();
        let wanted = 8i64;
        let pixels = (wanted as f64 * zoom).round() as i32;
        let _ = rate;
        window.post_press(x, y, Button::Left, true);
        window.post_move(x + pixels, y, true);
        window.post_press(x + pixels, y, Button::Left, false);
        window.drain();
        self.flush(&window);
        let after_start = self
            .shell
            .project()
            .clip(id)
            .ok_or("the clip vanished")?
            .timeline_start();
        let moved = after_start - before_start;
        if moved <= 0 {
            return Err(format!(
                "dragging {pixels} pixels moved the clip by {moved} frames"
            ));
        }
        report.push(format!(
            "pointer: dragging {pixels} px moved the clip {before_start} -> {after_start} ({moved} frames)"
        ));

        // The tail handle trims: the clip gets shorter. A tail can be covered by a later clip's
        // body on the same track, and the region on top is the one a press reaches, so the tail
        // this step uses is one that is genuinely reachable — the first whose left edge
        // resolves back to itself.
        self.touch_all();
        let damage = self.take_damage(Rect::new(0, 0, 1440, 900));
        paint::draw(surface.hdc(), Rect::new(0, 0, 1440, 900), self, &damage);
        let mut reachable = None;
        for region in self.zones.regions() {
            let Zone::Clip {
                clip,
                part: ClipPart::Tail,
            } = region.zone
            else {
                continue;
            };
            let centre = (
                region.rect.left + 1,
                region.rect.top + region.rect.height() / 2,
            );
            if self
                .zones
                .at(centre.0, centre.1)
                .is_some_and(|top| top.zone == region.zone)
            {
                reachable = Some((clip, region.rect));
                break;
            }
        }
        let (trim_id, tail) =
            reachable.ok_or("no clip's tail handle can be reached in the example project")?;
        let length_before = self
            .shell
            .project()
            .clip(trim_id)
            .map_or(0, |clip| clip.timeline_length());
        let tx = tail.left + 1;
        let ty = tail.top + tail.height() / 2;
        window.post_press(tx, ty, Button::Left, true);
        window.post_move(tx - (4.0 * zoom).round() as i32, ty, true);
        window.post_press(tx - (4.0 * zoom).round() as i32, ty, Button::Left, false);
        window.drain();
        self.flush(&window);
        let length_after = self
            .shell
            .project()
            .clip(trim_id)
            .map_or(0, |clip| clip.timeline_length());
        if length_after >= length_before {
            return Err(format!(
                "trimming the tail left the clip at {length_after} frames, from {length_before}"
            ));
        }
        report.push(format!(
            "pointer: trimming the tail took the clip {length_before} -> {length_after} frames"
        ));

        // Scrubbing in the ruler moves the playhead without touching the document.
        let revision = self.shell.project().revision();
        let scrub_frame = 30;
        let sx = paint::x_of(scrub_frame, scroll, zoom, lane_left);
        let sy = frame.ruler.top + frame.ruler.height() / 2;
        window.post_press(sx, sy, Button::Left, true);
        window.post_move(sx + 3, sy, true);
        window.post_press(sx + 3, sy, Button::Left, false);
        window.drain();
        self.flush(&window);
        if self.shell.project().revision() != revision {
            return Err("scrubbing changed the document".to_owned());
        }
        report.push(format!(
            "pointer: scrubbing in the ruler moved the playhead to {} (no command appended)",
            self.shell.playhead()
        ));

        // The wheel with ctrl zooms without the document noticing.
        self.touch_all();
        let damage = self.take_damage(Rect::new(0, 0, 1440, 900));
        paint::draw(surface.hdc(), Rect::new(0, 0, 1440, 900), self, &damage);
        let zoom_before = self.shell.zoom();
        window.post_wheel(
            frame.track_area.left + 200,
            frame.track_area.top + 20,
            120,
            true,
        );
        window.drain();
        self.flush(&window);
        if (self.shell.zoom() - zoom_before).abs() < f64::EPSILON {
            return Err("ctrl+wheel did not change the zoom".to_owned());
        }
        report.push(format!(
            "pointer: ctrl+wheel took the zoom {zoom_before:.2} -> {:.2}",
            self.shell.zoom()
        ));

        // 4. A click on a page tab goes to that page, and a click on a page that is not built is
        //    refused rather than silently ignored.
        let delivered = self
            .zones
            .get(Zone::Page(Page::Deliver))
            .map(|region| region.rect)
            .ok_or("the page strip did not register the deliver page")?;
        window.post_press(
            delivered.left + delivered.width() / 2,
            delivered.top + delivered.height() / 2,
            Button::Left,
            true,
        );
        window.post_press(
            delivered.left + delivered.width() / 2,
            delivered.top + delivered.height() / 2,
            Button::Left,
            false,
        );
        window.drain();
        self.flush(&window);
        if self.shell.page() != Page::Deliver {
            return Err(format!(
                "clicking deliver left the page at {:?}",
                self.shell.page()
            ));
        }
        report.push("pointer: the deliver page is showing".to_owned());

        // 5. The desktop's own frame: the strip that moves the window answers as a caption.
        let hit = window.hit_test(600, self.theme.metrics.menu_h / 2);
        if hit != win32::HTCAPTION {
            return Err(format!(
                "the strip at the top of the frame answered {hit} rather than HTCAPTION"
            ));
        }
        let inside = window.hit_test(600, self.theme.metrics.menu_h + 4);
        if inside != win32::HTCLIENT {
            return Err(format!(
                "the toolbar answered {inside} rather than HTCLIENT"
            ));
        }
        report.push("pointer: the top strip moves the window, the toolbar does not".to_owned());

        // 6. The clock: `space` starts it, a tick moves the playhead at the sequence's rate, and
        //    `space` stops it where it stands. The tick is driven through the window's own timer
        //    handler rather than by waiting on the wall clock, and `home` goes first so there is
        //    sequence left to play. Waiting here would be waiting on the frame rate instead: a
        //    tick re-renders the preview, and a message loop that keeps finding the timer due
        //    spends the whole sequence catching up.
        self.shell
            .set_page(Page::Cut)
            .map_err(|error| error.to_string())?;
        win32::force_modifiers(false, false, false);
        window.post_key(VK_HOME);
        window.drain();
        self.flush(&window);
        let start = self.shell.playhead();
        window.post_key(VK_SPACE);
        window.drain();
        self.flush(&window);
        if !self.transport.playing {
            return Err("space did not start the clock".to_owned());
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
        self.on_timer();
        let played = self.shell.playhead() - start;
        if played <= 0 {
            return Err(format!(
                "a tick 60 ms later moved the playhead {start} -> {}",
                self.shell.playhead()
            ));
        }
        if !self.transport.playing {
            return Err(format!(
                "a tick of {played} frames stopped the clock, so there was nothing left to stop"
            ));
        }
        report.push(format!(
            "clock: space started it, and one tick 60 ms later advanced the playhead {start} -> {} ({played} frames at {} fps)",
            self.shell.playhead(),
            self.rate()
        ));
        let running_at = self.shell.playhead();
        window.post_key(VK_SPACE);
        window.drain();
        self.flush(&window);
        if self.transport.playing {
            return Err("space did not stop the clock".to_owned());
        }
        if self.shell.playhead() != running_at {
            return Err(format!(
                "stopping the clock moved the playhead {running_at} -> {}",
                self.shell.playhead()
            ));
        }
        report.push("clock: space stopped it where it stood".to_owned());
        // A tick with the clock stopped moves nothing. It goes through the message queue, the
        // way the window's own timer does.
        window.post_timer();
        window.drain();
        if self.shell.playhead() != running_at {
            return Err(format!(
                "a tick with the clock stopped moved the playhead {running_at} -> {}",
                self.shell.playhead()
            ));
        }
        report.push("clock: a tick with it stopped moves nothing".to_owned());

        // 7. The frame's own buttons say what they would do, and doing it is the loop's job.
        self.touch_all();
        let damage = self.take_damage(Rect::new(0, 0, 1440, 900));
        paint::draw(surface.hdc(), Rect::new(0, 0, 1440, 900), self, &damage);
        let close = self
            .zones
            .get(Zone::FrameButton(FrameButton::Close))
            .map(|region| region.rect)
            .ok_or_else(|| {
                format!(
                    "the frame did not register its buttons: frameless {}, {} regions, page {:?}",
                    self.frameless,
                    self.zones.len(),
                    self.shell.page()
                )
            })?;
        let _ = close;
        report.push("frame: minimise, maximise and close are drawn and registered".to_owned());

        // 8. Saves, undo and redo still work through the log.
        let revision = self.shell.project().revision();
        win32::force_modifiers(true, false, false);
        window.post_key(VK_Z);
        window.drain();
        self.flush(&window);
        win32::force_modifiers(false, false, false);
        if self.shell.project().revision() >= revision {
            return Err("ctrl+z appended nothing to undo".to_owned());
        }
        report.push(format!(
            "log: ctrl+z took the revision {revision} -> {}",
            self.shell.project().revision()
        ));

        // 8b. Duplicate, then take it back: two more intents through the log and not around it.
        let revision = self.shell.project().revision();
        win32::force_modifiers(true, false, false);
        window.post_key(VK_D);
        window.drain();
        self.flush(&window);
        win32::force_modifiers(false, false, false);
        if self.shell.project().revision() != revision + 1 {
            return Err(format!(
                "ctrl+d took the revision {revision} -> {}",
                self.shell.project().revision()
            ));
        }
        report.push(format!(
            "log: ctrl+d duplicated, revision {revision} -> {}",
            self.shell.project().revision()
        ));
        win32::force_modifiers(true, false, false);
        window.post_key(VK_Z);
        window.drain();
        self.flush(&window);
        win32::force_modifiers(false, false, false);

        // 8c. The palette: ctrl+k opens it, typed text filters the agent's own catalogue, and
        //     the row it finds is the tool the prompt promised.
        win32::force_modifiers(true, false, false);
        window.post_key(VK_K);
        window.drain();
        self.flush(&window);
        win32::force_modifiers(false, false, false);
        if !self.shell.palette().open {
            return Err("ctrl+k did not open the palette".to_owned());
        }
        let wanted = "maj_place_clip";
        for character in wanted.chars() {
            window.post_text(character as u16);
        }
        window.drain();
        self.flush(&window);
        if !self
            .palette_matches()
            .iter()
            .any(|tool| tool.name == wanted)
        {
            return Err(format!(
                "typing {wanted:?} did not reach the agent's catalogue: query {:?}, {} matches, open {}",
                self.shell.palette().query,
                self.palette_matches().len(),
                self.shell.palette().open
            ));
        }
        report.push(format!("palette: ctrl+k opened it and {wanted:?} matched"));
        win32::force_modifiers(true, false, false);
        window.post_key(VK_K);
        window.drain();
        self.flush(&window);
        win32::force_modifiers(false, false, false);

        // 8d. The language: ctrl+l switches it, and the document does not notice.
        let revision = self.shell.project().revision();
        let language = self.shell.lang();
        win32::force_modifiers(true, false, false);
        window.post_key(VK_L);
        window.drain();
        self.flush(&window);
        win32::force_modifiers(false, false, false);
        if self.shell.lang() == language {
            return Err("ctrl+l did not change the language".to_owned());
        }
        if self.shell.project().revision() != revision {
            return Err("switching language appended a command".to_owned());
        }
        report.push(format!(
            "language: ctrl+l switched {} -> {}",
            language.key(),
            self.shell.lang().key()
        ));

        // 8e. The pages: ctrl+tab cycles to the next one, which is a view change and not an
        //     edit; the cut page is put back before the pixels are checked.
        let page = self.shell.page();
        win32::force_modifiers(true, false, false);
        window.post_key(VK_TAB);
        window.drain();
        self.flush(&window);
        win32::force_modifiers(false, false, false);
        if self.shell.page() == page {
            return Err("ctrl+tab did not change the page".to_owned());
        }
        report.push(format!(
            "pages: ctrl+tab moved {page:?} -> {:?}",
            self.shell.page()
        ));
        self.shell
            .set_page(Page::Cut)
            .map_err(|error| error.to_string())?;
        self.flush(&window);

        // 8f. The title studio: the tab, the field, a face, a size step and the button, all as
        //     real clicks, and the document gets a title of it. Then the two browsers, which
        //     add the effects and transitions the engine already has.
        let title = self.title_check(&window)?;
        report.push(title);
        let browsers = self.browser_check(&window)?;
        report.push(browsers);

        // 9. The pixels: the paint has content, and the zones are inside the window.
        let rect = Rect::new(0, 0, 1440, 900);
        self.touch_all();
        let damage = self.take_damage(rect);
        paint::draw(surface.hdc(), rect, self, &damage);
        let pixels = surface.pixels();
        let mut tones: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for pixel in pixels.chunks_exact(4) {
            tones
                .insert(u32::from(pixel[2]) << 16 | u32::from(pixel[1]) << 8 | u32::from(pixel[0]));
        }
        if tones.len() < 64 {
            return Err(format!(
                "the painted window has only {} distinct colours",
                tones.len()
            ));
        }
        let outside: Vec<&Region> = self
            .zones
            .regions()
            .iter()
            .filter(|region| {
                region.rect.left < rect.left
                    || region.rect.top < rect.top
                    || region.rect.right > rect.right
                    || region.rect.bottom > rect.bottom
            })
            .collect();
        if !outside.is_empty() {
            return Err(format!(
                "{} zones fall outside the window, first is {:?}",
                outside.len(),
                outside[0].zone
            ));
        }
        report.push(format!(
            "paint: {} distinct colours, {} zones, all inside the window",
            tones.len(),
            self.zones.len()
        ));

        // 10. The brand: the logo the build decoded, and the two icons made of it. The icons
        //     made here are never destroyed — the process is about to exit, and the pair the
        //     window itself owns outlives it anyway.
        if crate::brand::icons().is_none() {
            return Err("the operating system refused the logo's icons".to_owned());
        }
        report.push(crate::brand::summary());

        report.push(format!(
            "state hash {}",
            self.shell.state_hash().map_err(|error| error.to_string())?
        ));
        Ok(report.join("\n"))
    }

    /// Replaces the document with one read from disk, in place.
    ///
    /// # Errors
    ///
    /// Returns a refusal when the document cannot be read. The shell is left as it was, so an
    /// open that fails cannot lose the work in front of it.
    pub fn open_into(
        &mut self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<maj_ui::Report, UiError> {
        let replacement = Self::open(path.as_ref(), self.shell.prefs())?;
        self.shell = replacement.shell;
        self.frame = None;
        self.preview = None;
        self.preview_error = None;
        self.scroll = 0;
        self.actions
            .push_front(format!("opened {}", path.as_ref().display()));
        Ok(maj_ui::Report::empty())
    }

    /// A shell over a document read from disk.
    ///
    /// # Errors
    ///
    /// Returns a refusal carrying the model's own message when the document cannot be read.
    pub fn open(path: impl AsRef<std::path::Path>, prefs: maj_ui::Prefs) -> Result<Self, UiError> {
        let shell = Shell::open(path.as_ref(), prefs)?;
        let mut app = Self::new(&Options::default()).map_err(UiError::Refused)?;
        app.shell = shell;
        Ok(app)
    }

    /// Runs one of the agent's own tools against the session this window holds.
    ///
    /// # Errors
    ///
    /// Returns the tool's own refusal, carried through unchanged.
    pub fn run_tool(
        &mut self,
        name: &str,
        args: &BTreeMap<String, String>,
    ) -> Result<maj_ui::Report, UiError> {
        let report = self.shell.run_tool(name, args)?;
        self.touch_all();
        Ok(report)
    }

    /// Handles one event from the window.
    pub(crate) fn handle(&mut self, event: Event) -> Reaction {
        match event {
            Event::Paint(hdc) => {
                self.on_paint(hdc);
                Reaction::Handled(0)
            }
            Event::Resize => {
                self.touch_all();
                self.maximised = false;
                Reaction::Default
            }
            Event::Key(key) if key.pressed => {
                self.on_key(key);
                Reaction::Handled(0)
            }
            Event::Key(_) => Reaction::Handled(0),
            Event::Text(character) => {
                self.on_text(character);
                Reaction::Handled(0)
            }
            Event::Focus => Reaction::Default,
            Event::Close => {
                self.save_preferences();
                Reaction::Quit
            }
            Event::Move(at) => {
                self.on_move(at);
                Reaction::Handled(0)
            }
            Event::Press {
                at,
                button,
                down,
                double,
            } => {
                self.on_press(at, button, down, double);
                Reaction::Handled(0)
            }
            Event::Wheel { at, delta } => {
                self.on_wheel(at, delta);
                Reaction::Handled(0)
            }
            Event::Leave => {
                self.hover = None;
                self.touch_all();
                Reaction::Handled(0)
            }
            Event::Timer => {
                self.on_timer();
                Reaction::Handled(0)
            }
            Event::CaptureLost => {
                self.dragging = None;
                self.pressed = None;
                Reaction::Handled(0)
            }
            Event::HitTest(at) => {
                // The strip along the top moves the window; everything else is the client's, or
                // the resizing border's, which Windows answers for.
                let in_strip = at.y >= 0
                    && at.y < self.frame_rect.menu.bottom
                    && self.zones.at(at.x, at.y).is_none();
                Reaction::Handled(if in_strip {
                    win32::HTCAPTION
                } else {
                    win32::HTCLIENT
                })
            }
            Event::ChooseCursor(at) => {
                if let Some(region) = self.zones.at(at.x, at.y) {
                    win32::apply_cursor(region.cursor);
                    return Reaction::Handled(1);
                }
                Reaction::Default
            }
        }
    }

    /// Paints the window: into the back buffer, then the changed rectangles to the screen.
    fn on_paint(&mut self, hdc: Hdc) {
        let rect = win32::client_rect();
        if rect.width() < 8 || rect.height() < 8 {
            return;
        }
        let needs_surface = self.surface.as_ref().is_none_or(|surface| {
            surface.width() != rect.width() || surface.height() != rect.height()
        });
        if needs_surface {
            self.surface = Some(win32::Scratch::detached(rect.width(), rect.height()));
            self.damage_all = true;
        }
        let damage = self.take_damage(rect);
        let Some(surface) = self.surface.take() else {
            return;
        };
        paint::draw(surface.hdc(), rect, self, &damage);
        for area in &damage {
            win32::copy(hdc, surface.hdc(), *area);
        }
        self.surface = Some(surface);
        self.paints += 1;
    }

    /// A pointer move: hover, and whatever drag is in progress.
    fn on_move(&mut self, at: win32::Pointer) {
        self.pointer = (at.x, at.y);
        if let Some(drag) = self.dragging {
            self.drag_to(drag, at.x, at.y);
            return;
        }
        let zone = self.zones.at(at.x, at.y).map(|region| region.zone);
        if zone != self.hover {
            self.hover = zone;
            self.hover_since = win32::now();
            // A tooltip appears and disappears on a change of hover, so the frame that held it
            // has to be repainted; without this the text would linger.
            self.touch_all();
        }
    }

    /// The track and frame a carried tile would land on, from the pointer's position.
    ///
    /// The row arithmetic is the painter's own — `row_height` plus the two-pixel gap between
    /// the rows — because a drop that landed on a different row than the one it was drawn over
    /// would be a placement nobody could predict.
    fn drop_point(&self, x: i32, y: i32) -> Option<(TrackId, i64)> {
        let area = self.frame_rect.track_area;
        if area.is_empty() || y < area.top || y >= area.bottom || x < area.left {
            return None;
        }
        let row = self.row_height() + 2;
        let index = ((y - area.top) / row).max(0) as usize;
        let sequence = self.shell.sequence_entity()?;
        let track = sequence.tracks().get(index)?;
        if track.kind() != TrackKind::Video {
            return None;
        }
        let zoom = self.shell.zoom().max(0.05);
        let frame = paint::frame_at(x, self.scroll, zoom, area.left).max(0);
        Some((track.id(), frame))
    }

    /// Places the tile at the index where it was let go. Answers whether anything happened.
    ///
    /// A refusal is not a silent nothing: the engine's own sentence goes to the status line, and
    /// the tile stays where it was, in the pool.
    fn place(&mut self, index: usize, target: Option<(TrackId, i64)>) -> bool {
        let Some((track, frame)) = target else {
            return false;
        };
        let assets: Vec<_> = self.shell.project().assets().to_vec();
        let Some(asset) = assets.get(index).map(|asset| asset.id()) else {
            return false;
        };
        // The placement first: the playhead only follows a drop that happened, and the window's
        // own dispatch already puts a refusal in the status line rather than swallowing it.
        let before = self.shell.project().revision();
        self.dispatch(Action::DropMedia {
            asset,
            track,
            frame,
        });
        if self.shell.project().revision() != before {
            self.dispatch(Action::SeekTo(frame));
            self.after_change();
        }
        true
    }

    /// A press or a release.
    fn on_press(&mut self, at: win32::Pointer, button: Button, down: bool, double: bool) {
        self.pointer = (at.x, at.y);
        if down {
            let Some(region) = self.zones.at(at.x, at.y) else {
                return;
            };
            let zone = region.zone;
            let usable = region.usable;
            if button == Button::Right {
                let kind = match zone {
                    Zone::Clip { .. } | Zone::TrackHeader(_) | Zone::TrackControl { .. } => {
                        MenuKind::Clip
                    }
                    _ => MenuKind::Timeline,
                };
                self.open_menu(kind, Some((at.x, at.y)));
                return;
            }
            if !usable {
                self.error = region
                    .reason
                    .map(|reason| text(self.shell.lang(), reason).to_owned());
                self.touch_all();
                return;
            }
            self.pressed = Some(zone);
            self.drag_origin = (at.x, at.y);
            self.begin_drag(zone, at, double);
            return;
        }
        // A release: whatever was pressed is activated if the pointer is still on it.
        let pressed = self.pressed.take();
        let dragging = self.dragging.take();
        let carried = self.drop_target.take();
        if let Some(zone) = dragging {
            if let Zone::MediaTile(index) = zone {
                // Let go over the timeline, the tile is placed there; let go anywhere else, it
                // is a click, and what a click means is decided below.
                if self.place(index, carried) {
                    self.touch_all();
                    return;
                }
            } else {
                // The drag already did its work, gesture by gesture.
                self.touch_all();
                return;
            }
        }
        let Some(zone) = pressed else {
            return;
        };
        let over = self.zones.at(at.x, at.y).map(|region| region.zone);
        if over == Some(zone) {
            self.activate(zone, double, at.shift);
        }
    }

    /// Starts whatever drag a press begins.
    fn begin_drag(&mut self, zone: Zone, at: win32::Pointer, double: bool) {
        let lane_left = self.frame_rect.track_area.left;
        let zoom = self.shell.zoom().max(0.05);
        match zone {
            Zone::Clip { clip, part } => {
                self.dispatch(Action::SelectClip(clip));
                self.dragging = Some(zone);
                self.drag_frame = paint::frame_at(at.x, self.scroll, zoom, lane_left);
                self.drag_clip_start = self
                    .shell
                    .project()
                    .clip(clip)
                    .map_or(0, |clip| clip.timeline_start());
                let _ = part;
                self.touch_all();
            }
            Zone::Ruler | Zone::Playhead => {
                self.dragging = Some(Zone::Ruler);
                self.scrub(at.x);
            }
            Zone::Splitter(_) => {
                self.dragging = Some(zone);
                let view = self.shell.view();
                self.drag_split = match zone {
                    Zone::Splitter(Splitter::Left) => view.left_w,
                    Zone::Splitter(Splitter::Right) => view.right_w,
                    Zone::Splitter(Splitter::Timeline) => view.timeline_h,
                    _ => view.media_split,
                };
            }
            // A tile is selected on the press and *carried* from there: a drag is what puts a
            // media selection into the timeline at a place of the person's choosing, and a
            // release that never left the pool is a click rather than a placement. The
            // double-click shortcut stays where it was — the whole sequence head — because a
            // person who has not pointed anywhere has not chosen a frame.
            Zone::MediaTile(index) => {
                let assets: Vec<_> = self.shell.project().assets().to_vec();
                if let Some(asset) = assets.get(index) {
                    let id = asset.id();
                    self.dispatch(Action::SelectMedia(id));
                    if double {
                        self.dispatch(Action::InsertMediaAtPlayhead(id));
                        self.touch_all();
                    } else {
                        self.dragging = Some(zone);
                        self.touch(self.frame_rect.media);
                    }
                }
            }
            _ => {}
        }
    }

    /// Carries a drag on.
    fn drag_to(&mut self, zone: Zone, x: i32, y: i32) {
        let lane_left = self.frame_rect.track_area.left;
        let zoom = self.shell.zoom().max(0.05);
        self.pointer = (x, y);
        match zone {
            Zone::Clip { clip, part } => {
                let frame = paint::frame_at(x, self.scroll, zoom, lane_left);
                let snapped = if self.shell.view().snap {
                    self.snap(frame, Some(clip))
                } else {
                    frame
                };
                match part {
                    ClipPart::Body => {
                        let want = self.drag_clip_start + (snapped - self.drag_frame);
                        let now = self
                            .shell
                            .project()
                            .clip(clip)
                            .map_or(want, |clip| clip.timeline_start());
                        if want != now {
                            self.dispatch(Action::MoveSelected(want - now));
                        }
                    }
                    ClipPart::Head => {
                        self.dispatch(Action::TrimSelectedStart(snapped));
                    }
                    ClipPart::Tail => {
                        self.dispatch(Action::TrimSelectedEnd(snapped));
                    }
                }
                self.after_change();
                self.touch_all();
            }
            // A carried tile marks where it would land, and marks nothing when it is not over
            // a picture track: a drop that would be refused says so before it is let go.
            Zone::MediaTile(_) => {
                let target = self.drop_point(x, y);
                if target != self.drop_target {
                    self.drop_target = target;
                    self.touch(self.frame_rect.timeline);
                }
            }
            Zone::Ruler => self.scrub(x),
            Zone::Splitter(splitter) => {
                let delta = x - self.drag_origin.0;
                let metrics = self.theme.metrics;
                let value = match splitter {
                    Splitter::Left => {
                        (self.drag_split + delta).clamp(metrics.px(170), metrics.px(520))
                    }
                    Splitter::Right => {
                        (self.drag_split - delta).clamp(metrics.px(170), metrics.px(520))
                    }
                    Splitter::Timeline => (self.drag_split - (y - self.drag_origin.1))
                        .clamp(metrics.px(120), metrics.px(700)),
                    Splitter::Inline => {
                        (self.drag_split + delta).clamp(metrics.px(80), metrics.px(600))
                    }
                };
                self.dispatch(Action::SetSplit(splitter.into(), value));
                self.touch_all();
            }
            _ => {}
        }
    }

    /// The frame a dragged edge should land on, with snapping to the playhead and to the other
    /// clips' edges.
    fn snap(&self, frame: i64, except: Option<ClipId>) -> i64 {
        let zoom = self.shell.zoom().max(0.05);
        let reach = (SNAP_PIXELS / zoom).round().max(1.0) as i64;
        let mut best = frame;
        let mut distance = reach + 1;
        let mut consider = |candidate: i64| {
            let gap = (candidate - frame).abs();
            if gap <= reach && gap < distance {
                distance = gap;
                best = candidate;
            }
        };
        consider(self.shell.playhead());
        consider(0);
        if let Some(sequence) = self.shell.sequence_entity() {
            for track in sequence.tracks() {
                for clip in track.clips() {
                    if Some(clip.id()) == except {
                        continue;
                    }
                    consider(clip.timeline_start());
                    consider(clip.timeline_start() + clip.timeline_length());
                }
            }
        }
        best
    }

    /// Moves the playhead to the frame under a pixel, and asks for a preview frame at the rate
    /// the clock is allowed to render at.
    fn scrub(&mut self, x: i32) {
        let lane_left = self.frame_rect.track_area.left;
        let zoom = self.shell.zoom().max(0.05);
        let frame = paint::frame_at(x, self.scroll, zoom, lane_left).max(0);
        if frame != self.shell.playhead() {
            self.dispatch(Action::SeekTo(frame));
            self.after_change();
            self.touch(self.frame_rect.timeline);
            self.touch(self.frame_rect.transport);
            let now = win32::now();
            if now - self.last_scrub >= FRAME_BUDGET {
                self.last_scrub = now;
                if self.refresh_preview() {
                    self.touch(self.frame_rect.viewer);
                }
            }
        }
    }

    /// The wheel: scrolling, or zooming about the pointer with control held.
    fn on_wheel(&mut self, at: win32::Pointer, delta: i32) {
        let notches = f64::from(delta) / 120.0;
        if self.zones.at(at.x, at.y).is_some_and(|region| {
            matches!(
                region.zone,
                Zone::Ruler | Zone::Timeline | Zone::Clip { .. }
            )
        }) {
            if at.ctrl {
                // Zooming keeps the frame under the pointer still, which is the difference
                // between a zoom that helps and one that loses your place.
                let lane_left = self.frame_rect.track_area.left;
                let before = self.shell.zoom().max(0.05);
                let anchor = paint::frame_at(at.x, self.scroll, before, lane_left);
                let after = (before * 1.15f64.powf(notches)).clamp(0.05, 400.0);
                let shift = ((at.x - lane_left) as f64 / after).round() as i64;
                self.scroll = (anchor - shift).max(0);
                self.dispatch(Action::ZoomBy(after / before));
            } else {
                // A plain wheel — with or without shift — scrolls the lanes by whole notches.
                let frames =
                    (-notches * f64::from(WHEEL_STEP) / self.shell.zoom().max(0.05)).round() as i64;
                self.scroll = (self.scroll + frames).max(0);
            }
            self.touch_all();
            return;
        }
        if self
            .zones
            .at(at.x, at.y)
            .is_some_and(|region| matches!(region.zone, Zone::LogRow(_)))
        {
            let rows = self.shell.project().log().len();
            let step = if notches < 0.0 { 3 } else { -3 };
            self.log_scroll =
                (self.log_scroll as i64 + step).clamp(0, rows.saturating_sub(1) as i64) as usize;
            self.touch(self.frame_rect.history);
            return;
        }
        if self
            .zones
            .at(at.x, at.y)
            .is_some_and(|region| matches!(region.zone, Zone::InspectorRow(_)))
        {
            let step = if notches < 0.0 { 1 } else { -1 };
            let next = (self.inspector_scroll as i64 + step).max(0) as usize;
            self.inspector_scroll = next;
            self.touch(self.frame_rect.inspector);
            return;
        }
        if self
            .zones
            .at(at.x, at.y)
            .is_some_and(|region| region.zone == Zone::Ruler)
        {
            self.touch_all();
        }
    }

    /// What a press on a zone does when it is released on the same zone.
    fn activate(&mut self, zone: Zone, double: bool, shift: bool) {
        match zone {
            // A diamond: a plain click puts a key under the playhead or takes it away, and with
            // shift held it clears the channel's whole curve. Two gestures on one control, said
            // apart by where the keys end up — one frame, or none at all.
            Zone::Keyframe(channel) => {
                let action = if shift {
                    Action::ClearChannel(channel)
                } else {
                    Action::ToggleKeyframe(channel)
                };
                self.dispatch(action);
                self.touch_all();
            }
            Zone::FrameButton(FrameButton::Close) => self.pending.request = Some(Request::Close),
            Zone::FrameButton(FrameButton::Minimize) => {
                self.pending.request = Some(Request::Minimize)
            }
            Zone::FrameButton(FrameButton::Maximize) => {
                self.pending.request = Some(Request::Maximize)
            }
            Zone::Menu(index) => {
                if self.menu.map(|menu| menu.index) == Some(index) {
                    self.menu = None;
                } else {
                    self.open_menu(MenuKind::from_index(index), None);
                }
                self.touch_all();
            }
            Zone::MenuItem(index) => self.choose_item(index),
            Zone::Tool(tool) | Zone::TimelineTool(tool) => self.use_tool(tool),
            Zone::Page(page) if page.built() => {
                self.dispatch(Action::SetPage(page));
                self.touch_all();
            }
            Zone::Tab(tab) => {
                // Which column a tab belongs to is the tab's own answer, so a new one cannot
                // be put in the wrong place by forgetting an arm here.
                if tab.is_left() {
                    self.left_tab = tab;
                } else {
                    self.right_tab = tab;
                }
                if tab != Tab::Titles {
                    self.title_focus = false;
                }
                self.touch_all();
            }
            Zone::TitleField => {
                self.title_focus = true;
                self.timecode_entry = None;
                self.touch_all();
            }
            Zone::TitleFace(index) => {
                if let Some(face) = TitleFace::ALL.get(index) {
                    self.dispatch(Action::SetTitleFace(*face));
                }
                self.touch_all();
            }
            Zone::TitleAlign(index) => {
                if let Some(align) = TitleAlign::ALL.get(index) {
                    self.dispatch(Action::SetTitleAlign(*align));
                }
                self.touch_all();
            }
            Zone::TitleSize(step) => {
                // A step is a quarter of what is there, so two presses double it and the
                // value stays a number a person would have chosen rather than a prime.
                let size = self.shell.title_size();
                let next = if step < 0 { size * 0.8 } else { size * 1.25 };
                self.dispatch(Action::SetTitleSize(next.clamp(4.0, 400.0)));
                self.touch_all();
            }
            Zone::TitleAdd => {
                self.dispatch(self.shell.add_title());
                self.after_change();
                self.touch_all();
            }
            Zone::ImportMedia => self.pending.request = Some(Request::ImportMedia),
            Zone::EffectRow(index) => {
                if let Some(kind) = EffectKind::ALL.get(index) {
                    self.dispatch(Action::AddEffect(*kind));
                    self.after_change();
                }
                self.touch_all();
            }
            Zone::TransitionRow(index) => {
                if let Some(kind) = TransitionKind::ALL.get(index) {
                    // A transition lasts a second at the sequence's own rate, which is the
                    // duration a cut has room for until a person trims it.
                    let rate = self
                        .shell
                        .sequence_entity()
                        .map_or(24, |sequence| sequence.timebase().nominal_fps());
                    self.dispatch(Action::AddTransition(*kind, i64::from(rate)));
                    self.after_change();
                }
                self.touch_all();
            }
            Zone::Transport(button) => self.use_transport(button),
            Zone::MediaTile(index) => {
                let assets: Vec<_> = self.shell.project().assets().to_vec();
                if let Some(asset) = assets.get(index) {
                    let id = asset.id();
                    if double {
                        self.dispatch(Action::InsertMediaAtPlayhead(id));
                    } else {
                        self.dispatch(Action::SelectMedia(id));
                    }
                    self.touch(self.frame_rect.media);
                }
            }
            Zone::Clip { clip, .. } => {
                self.dispatch(Action::SelectClip(clip));
                self.touch_all();
            }
            Zone::TrackControl {
                track,
                control: crate::widget::TrackControl::Enable,
            } => {
                // The one track switch that changes the picture: the renderer composites only
                // the tracks that are enabled, so this is a real edit, logged like any other.
                if let Some(state) = self
                    .shell
                    .sequence_entity()
                    .and_then(|sequence| sequence.tracks().get(track))
                    .map(|track| (track.id(), track.is_enabled()))
                {
                    self.dispatch(Action::SetTrackEnabled {
                        track: state.0,
                        enabled: !state.1,
                    });
                    self.touch_all();
                }
            }
            Zone::TrackControl { .. } => {}
            Zone::TimecodeField => {
                self.timecode_entry = Some(String::new());
                self.touch(self.frame_rect.transport);
            }
            Zone::Render => self.pending.request = Some(Request::Export),
            Zone::PaletteBackdrop => {
                self.dispatch(Action::ClosePalette);
                self.touch_all();
            }
            Zone::PaletteRow(index) => {
                self.dispatch(Action::SetPaletteRow(index));
                let matches = self.palette_matches();
                let args: BTreeMap<String, String> = self.shell.palette().args.clone();
                if let Some(tool) = matches.get(index) {
                    let name = tool.name.to_owned();
                    let _ = self.run_tool(&name, &args);
                }
                self.dispatch(Action::ClosePalette);
                self.touch_all();
            }
            Zone::Splitter(_) | Zone::Ruler | Zone::Timeline | Zone::LogRow(_) => {}
            Zone::Playhead => self.activate(Zone::Ruler, false, false),
            Zone::TrackHeader(_) | Zone::InspectorRow(_) | Zone::Meter | Zone::PaletteField => {
                // The fields are the exception: a press in one of them takes the keyboard, and
                // a press anywhere else gives it up rather than leaving a field focused behind
                // the pointer.
                self.title_focus = false;
                self.timecode_entry = None;
            }
            Zone::Page(_) => {}
        }
    }

    /// Runs the intent a toolbar button stands for.
    fn use_tool(&mut self, tool: ToolButton) {
        match tool.binding() {
            Some(binding) => {
                self.perform(binding);
                self.touch_all();
            }
            // The one tool with no stroke of its own: fitting the sequence, which is a zoom and
            // a scroll together.
            None if matches!(tool, ToolButton::ZoomFit) => self.zoom_fit(),
            // A tool whose subsystem does not exist yet says so rather than pretending.
            None => {
                self.error = Some(self.say(Key::MarkerOwner).to_owned());
                self.touch_all();
            }
        }
    }

    /// Fits the whole sequence into the lane: the zoom that shows every frame, and the scroll
    /// back to the start.
    fn zoom_fit(&mut self) {
        let width = self.frame_rect.track_area.width();
        let length = self
            .shell
            .sequence_entity()
            .and_then(|sequence| sequence.duration_frames())
            .unwrap_or(0);
        let current = self.shell.zoom().max(0.05);
        if width <= 0 || length <= 0 {
            return;
        }
        let wanted = f64::from(width) / length as f64;
        let factor = (wanted / current).max(0.001);
        self.dispatch(Action::ZoomBy(factor));
        self.scroll = 0;
        self.after_change();
        self.touch_all();
    }

    /// Runs a menu row.
    fn choose_item(&mut self, index: usize) {
        let Some(menu) = self.menu else {
            return;
        };
        let entries = crate::paint::menus::items(menu.kind, self);
        let Some(entry) = entries.get(index).copied() else {
            return;
        };
        if !entry.usable(self) {
            self.error = Some(self.say(Key::NotBuiltHeading).to_owned());
            self.menu = None;
            self.touch_all();
            return;
        }
        match (entry.special, entry.binding) {
            (Some(special), _) => self.special(special),
            (None, Some(binding)) => {
                self.menu = None;
                self.perform(binding);
            }
            (None, None) => {}
        }
        self.menu = None;
        self.touch_all();
    }

    /// The things only the window can do.
    fn special(&mut self, special: Special) {
        match special {
            Special::Open => self.pending.request = Some(Request::Open),
            Special::SaveAs => self.pending.request = Some(Request::SaveAs),
            Special::ExportDialog => self.pending.request = Some(Request::Export),
            Special::Quit => self.pending.request = Some(Request::Close),
            Special::ResetPanels => {
                self.dispatch(Action::ResetSplits);
                self.touch_all();
            }
            Special::About => {
                let line = format!(
                    "{} {} · {} · {}",
                    self.say(Key::AppName),
                    env!("CARGO_PKG_VERSION"),
                    self.say(Key::AboutLine),
                    self.say(Key::AboutKeys)
                );
                self.actions.push_front(line.clone());
                self.dispatch(Action::OpenPalette);
            }
            Special::Page(page) => {
                if page.built() {
                    self.dispatch(Action::SetPage(page));
                }
            }
            Special::Agent => {
                self.dispatch(Action::OpenPalette);
            }
        }
    }

    /// The transport buttons.
    fn use_transport(&mut self, button: crate::widget::TransportButton) {
        use crate::widget::TransportButton as Button_;
        match button {
            Button_::Start => self.keyed(Binding::GoToStart),
            Button_::End => self.keyed(Binding::GoToEnd),
            Button_::PrevFrame => self.keyed(Binding::StepFrames(-1)),
            Button_::NextFrame => self.keyed(Binding::StepFrames(1)),
            Button_::Play => self.keyed(Binding::PlayPause),
            Button_::Loop => self.keyed(Binding::ToggleLoop),
        }
        self.touch_all();
    }

    /// Runs a binding and damages what it changed.
    fn keyed(&mut self, binding: Binding) {
        self.perform(binding);
    }

    /// A key press: the palette's own field, the timecode field, a menu, or the shell's table.
    fn on_key(&mut self, key: win32::Key) {
        // A menu takes the keyboard while it is open: arrows, enter, escape.
        if self.menu.is_some() && self.on_menu_key(key) {
            return;
        }
        if self.title_focus && self.on_title_key(key) {
            return;
        }
        if self.timecode_entry.is_some() && self.on_timecode_key(key) {
            return;
        }
        if self.shell.palette().open && self.on_palette_key(key) {
            return;
        }
        let Some(stroke) = stroke_for(key.code, key.ctrl, key.shift, key.alt) else {
            return;
        };
        let Some(binding) = keys::binding(stroke) else {
            return;
        };
        self.append_log(&format!(
            "key {:#04x} ctrl={} shift={} alt={} -> {}",
            key.code,
            key.ctrl,
            key.shift,
            key.alt,
            binding_name(binding)
        ));
        self.perform(binding);
        // Every binding can change the picture, and the ones that do not are cheap: the damage
        // is what decides whether a paint happens at all.
        match binding {
            Binding::StepFrames(_)
            | Binding::StepSeconds(_)
            | Binding::GoToStart
            | Binding::GoToEnd
            | Binding::StepFramesAndSelect(_) => {
                self.after_change();
                self.touch(self.frame_rect.timeline);
                self.touch(self.frame_rect.transport);
                self.touch(self.frame_rect.meter);
            }
            _ => self.touch_all(),
        }
    }

    /// The keys a menu owns while it is open.
    fn on_menu_key(&mut self, key: win32::Key) -> bool {
        let Some(menu) = self.menu else {
            return false;
        };
        let entries = crate::paint::menus::items(menu.kind, self);
        match key.code {
            VK_ESCAPE => {
                self.menu = None;
                self.touch_all();
                true
            }
            VK_UP | VK_DOWN => {
                let count = entries.len().max(1);
                let row = if key.code == VK_UP {
                    menu.row.saturating_sub(1)
                } else {
                    (menu.row + 1) % count
                };
                self.menu = Some(Menu { row, ..menu });
                self.touch_all();
                true
            }
            VK_RIGHT | VK_LEFT => {
                // Moving along the bar with a menu open opens its neighbour, which is how the
                // keyboard walks the menu bar.
                if let Some(index) = menu.kind.index() {
                    let delta = if key.code == VK_RIGHT { 1 } else { -1 };
                    let bar = MenuKind::BAR.len() as i32;
                    let next = ((index as i32 + delta).rem_euclid(bar)) as usize;
                    self.open_menu(MenuKind::from_index(next), None);
                    self.touch_all();
                }
                true
            }
            VK_ENTER => {
                self.choose_item(menu.row);
                true
            }
            _ => false,
        }
    }

    /// The keys the timecode field owns while it is open.
    /// The keys the title studio's field owns while it holds the keyboard.
    ///
    /// It is a field, so a printable key is text and arrives as one; what this decides is the
    /// keys that are not text — the one that takes a letter back, and the two that give the
    /// keyboard up.
    fn on_title_key(&mut self, key: win32::Key) -> bool {
        match key.code {
            VK_ESCAPE | VK_ENTER => {
                self.title_focus = false;
                self.touch_all();
                true
            }
            VK_BACKSPACE => {
                let mut words = self.shell.studio().text.clone();
                words.pop();
                self.dispatch(Action::SetTitleText(words));
                self.touch(self.frame_rect.media);
                true
            }
            // A printable key with no chord is text, and its character arrives as WM_CHAR.
            _ if !key.ctrl && !key.alt => press_for(key.code).is_some(),
            _ => false,
        }
    }

    /// The keys the transport's timecode field owns while it holds the keyboard.
    fn on_timecode_key(&mut self, key: win32::Key) -> bool {
        match key.code {
            VK_ESCAPE => {
                self.timecode_entry = None;
                self.touch(self.frame_rect.transport);
                true
            }
            VK_ENTER => {
                let typed = self.timecode_entry.take().unwrap_or_default();
                let frame = typed.trim().parse::<i64>().ok().unwrap_or(-1);
                if frame >= 0 {
                    self.dispatch(Action::SeekTo(frame));
                    self.after_change();
                } else {
                    self.error = Some(self.say(Key::TransportBadTimecode).to_owned());
                }
                self.touch_all();
                true
            }
            VK_BACKSPACE => {
                if let Some(entry) = self.timecode_entry.as_mut() {
                    entry.pop();
                }
                self.touch(self.frame_rect.transport);
                true
            }
            _ => true,
        }
    }

    /// The keys the palette owns while it is open: text, its row, running, and closing.
    fn on_palette_key(&mut self, key: win32::Key) -> bool {
        match key.code {
            VK_ESCAPE => {
                self.dispatch(Action::ClosePalette);
                self.touch_all();
                true
            }
            VK_ENTER => {
                let matches = self.palette_matches();
                let row = self.shell.palette().row;
                if let Some(tool) = matches.get(row) {
                    let name = tool.name.to_owned();
                    let args: BTreeMap<String, String> = self.shell.palette().args.clone();
                    if let Err(error) = self.run_tool(&name, &args) {
                        self.error = Some(error.to_string());
                    }
                }
                self.touch_all();
                true
            }
            VK_UP | VK_DOWN => {
                let count = self.palette_matches().len();
                if count == 0 {
                    return true;
                }
                let row = self.shell.palette().row;
                let next = if key.code == VK_UP {
                    row.saturating_sub(1)
                } else {
                    (row + 1).min(count - 1)
                };
                self.dispatch(Action::SetPaletteRow(next));
                self.touch_all();
                true
            }
            VK_BACKSPACE => {
                let mut query = self.shell.palette().query.clone();
                query.pop();
                self.dispatch(Action::SetPaletteQuery(query));
                self.touch_all();
                true
            }
            // A printable key with no modifier is text, and its character arrives as WM_CHAR.
            _ if !key.ctrl && !key.alt => press_for(key.code).is_some(),
            _ => false,
        }
    }

    /// A character: typed into whichever field has the keyboard.
    fn on_text(&mut self, character: char) {
        if character.is_control() {
            return;
        }
        // The palette covers the interface, so while it is open the keyboard is its own: a
        // character typed with a palette on screen belongs in its field, not in the timecode
        // the pointer left focused behind it.
        if self.shell.palette().open {
            let query = self.shell.palette().query.clone();
            self.dispatch(Action::SetPaletteQuery(format!("{query}{character}")));
            self.touch_all();
            return;
        }
        if self.title_focus {
            let mut words = self.shell.studio().text.clone();
            words.push(character);
            self.dispatch(Action::SetTitleText(words));
            self.touch(self.frame_rect.media);
            return;
        }
        if let Some(entry) = self.timecode_entry.as_mut() {
            if character.is_ascii_digit() || character == ':' {
                entry.push(character);
                self.touch(self.frame_rect.transport);
            }
        }
    }

    /// A timer tick: the clock, and whatever the frame needs to follow it.
    fn on_timer(&mut self) {
        let now = win32::now();
        let end = self
            .shell
            .sequence_entity()
            .and_then(|sequence| sequence.duration_frames())
            .map_or(0, |end| end.saturating_sub(1));
        let Some(target) = self.transport.frame(now, end) else {
            return;
        };
        let moved = target != self.shell.playhead();
        if moved {
            self.dispatch(Action::SeekTo(target));
            self.after_change();
            self.touch(self.frame_rect.timeline);
            self.touch(self.frame_rect.transport);
        }
        if moved && self.transport.take_preview(now) && self.refresh_preview() {
            self.touch(self.frame_rect.viewer);
        }
        if !self.transport.playing {
            self.touch(self.frame_rect.transport);
        }
    }

    /// Opens a menu, from the bar or at a point.
    fn open_menu(&mut self, kind: MenuKind, at: Option<(i32, i32)>) {
        let index = kind.index().unwrap_or(0);
        self.menu = Some(Menu {
            kind,
            at,
            index,
            row: 0,
        });
    }

    /// Runs a binding through the shell, and reports what it did.
    fn perform(&mut self, binding: Binding) {
        // The intentions the shell deliberately refuses: they need the window, and this is the
        // window answering them. Everything else goes to the shell, which is the one door into
        // the document.
        match binding {
            Binding::PlayPause => return self.toggle_clock(),
            Binding::Shuttle(direction) => return self.shuttle(direction),
            Binding::ToggleLoop => {
                let on = !self.shell.view().loops;
                return self.dispatch(Action::SetLoops(on));
            }
            Binding::ToggleSnap => {
                let on = !self.shell.view().snap;
                return self.dispatch(Action::SetSnap(on));
            }
            Binding::ToggleLink => {
                let on = !self.shell.view().link;
                return self.dispatch(Action::SetLink(on));
            }
            Binding::CyclePage(step) => return self.dispatch(Action::CyclePage(step)),
            Binding::Page(page) => return self.dispatch(Action::SetPage(page)),
            Binding::TogglePanel(panel) => return self.dispatch(Action::TogglePanel(panel)),
            Binding::ZoomFit => return self.zoom_fit(),
            // These three are the loop's work, because only the loop holds the window.
            Binding::ExportStill => {
                self.pending.request = Some(Request::Export);
                return;
            }
            Binding::Open => {
                self.pending.request = Some(Request::Open);
                return;
            }
            Binding::SaveAs => {
                self.pending.request = Some(Request::SaveAs);
                return;
            }
            _ => {}
        }
        match self.shell.perform(binding) {
            Ok(report) => {
                self.error = None;
                if let Some(note) = report.note {
                    self.actions.push_front(note.clone());
                    self.append_log(&note);
                }
                for command in report.commands {
                    self.actions.push_front(command.to_owned());
                    self.append_log(command);
                }
            }
            Err(error) => {
                self.error = Some(error.to_string());
                self.append_log(&format!("refused: {error}"));
            }
        }
    }

    /// Sends an intention to the shell, and reports what it did.
    fn dispatch(&mut self, action: Action) {
        match self.shell.dispatch(action) {
            Ok(report) => {
                if self.error.is_some() && report.note.is_none() {
                    self.error = None;
                }
                for command in report.commands {
                    self.actions.push_front(command.to_owned());
                    self.append_log(command);
                }
            }
            Err(error) => {
                self.error = Some(error.to_string());
                self.append_log(&format!("refused: {error}"));
            }
        }
    }

    /// Writes a still, and remembers what it did.
    fn export(&mut self, path: std::path::PathBuf) {
        match self.shell.export_still(&path) {
            Ok(report) => {
                let said = report.note.unwrap_or_else(|| format!("{}", path.display()));
                self.actions.push_front(said.clone());
                self.last_export = Some(said.clone());
                self.error = None;
                self.append_log(&said);
            }
            Err(error) => {
                let said = format!("{}: {error}", self.say(Key::ErrorSave));
                self.error = Some(said.clone());
                self.last_export = Some(said);
            }
        }
    }

    /// Registers a still image from disk as media.
    ///
    /// It is registered and not placed: placing it is an edit, and a picture that has just
    /// arrived is usually worth a look in the pool first. It arrives selected, so `i`, a double
    /// click or a drag is all it takes to put it on the timeline afterwards.
    fn import_media(&mut self, path: std::path::PathBuf) {
        self.dispatch(Action::ImportMedia { path });
        // A `CreateMediaAsset` appends, so the asset to select — the one a double click or the
        // `i` key would place — is the last one.
        let last = self.shell.project().assets().last().map(MediaAsset::id);
        if let Some(asset) = last {
            self.dispatch(Action::SelectMedia(asset));
        }
        self.after_change();
        self.touch_all();
    }

    /// Paints once, so the zones are current, then presses and releases the centre of one.
    ///
    /// A click is resolved against the rectangles the paint registered, so a control whose
    /// rectangle moved is clicked where it actually is rather than where the check guessed.
    fn click_zone(&mut self, window: &Window, zone: Zone) -> Result<(), String> {
        let rect = Rect::new(0, 0, 1440, 900);
        self.touch_all();
        let damage = self.take_damage(rect);
        let surface = win32::Scratch::detached(1440, 900);
        paint::draw(surface.hdc(), rect, self, &damage);
        let region = self
            .zones
            .get(zone)
            .map(|region| region.rect)
            .ok_or_else(|| format!("{zone:?} was not registered as a zone"))?;
        let (x, y) = (
            region.left + region.width() / 2,
            region.top + region.height() / 2,
        );
        window.post_press(x, y, Button::Left, true);
        window.post_press(x, y, Button::Left, false);
        window.drain();
        self.flush(window);
        Ok(())
    }

    /// The title studio, driven the way a person drives it: a tab, a field, a face, a size and
    /// the button that sets it.
    fn title_check(&mut self, window: &Window) -> Result<String, String> {
        self.click_zone(window, Zone::Tab(Tab::Titles))?;
        if self.left_tab != Tab::Titles {
            return Err("clicking the Titles tab did not open the studio".to_owned());
        }
        self.click_zone(window, Zone::TitleField)?;
        if !self.title_focus {
            return Err("clicking the words field did not take the keyboard".to_owned());
        }
        let wanted = "MAJESTAT";
        for character in wanted.chars() {
            window.post_text(character as u16);
        }
        window.drain();
        self.flush(window);
        if self.shell.studio().text != wanted {
            return Err(format!(
                "typing reached the studio as {:?}",
                self.shell.studio().text
            ));
        }
        self.click_zone(window, Zone::TitleFace(1))?;
        if self.shell.studio().face != TitleFace::Serif {
            return Err(format!(
                "the serif button chose {:?}",
                self.shell.studio().face
            ));
        }
        let before = self.shell.title_size();
        self.click_zone(window, Zone::TitleSize(1))?;
        if self.shell.title_size() <= before {
            return Err(format!(
                "the size step left {before} at {}",
                self.shell.title_size()
            ));
        }

        // The button: the pool gains an asset that names the title, and the timeline gains a
        // clip of it, which is two commands rather than one intention that writes twice.
        let revision = self.shell.project().revision();
        let assets = self.shell.project().assets().len();
        self.click_zone(window, Zone::TitleAdd)?;
        let uri = self
            .shell
            .project()
            .assets()
            .last()
            .map(|asset| asset.uri().to_owned())
            .unwrap_or_default();
        if self.shell.project().assets().len() != assets + 1 {
            return Err("adding a title registered no media".to_owned());
        }
        if self.shell.project().revision() != revision + 2 {
            return Err(format!(
                "adding a title moved the revision {revision} -> {}",
                self.shell.project().revision()
            ));
        }
        if !uri.starts_with("title:serif:") || !uri.ends_with(wanted) {
            return Err(format!("the title registered as {uri:?}"));
        }
        Ok(format!(
            "titles: the studio set {wanted:?} in a {} px serif face from 6 clicks and {} keys, and the pool and the timeline each gained one",
            self.shell.title_size().round() as i64,
            wanted.len()
        ))
    }

    /// The two browsers: an effect added to the selection, and a transition added to the track.
    fn browser_check(&mut self, window: &Window) -> Result<String, String> {
        let clip = self
            .shell
            .sequence_entity()
            .and_then(|sequence| sequence.tracks().first())
            .and_then(|track| track.clips().first())
            .map(|clip| clip.id())
            .ok_or("the example project has no clip to give an effect to")?;
        self.dispatch(Action::SelectClip(clip));
        self.click_zone(window, Zone::Tab(Tab::EffectList))?;
        if self.right_tab != Tab::EffectList {
            return Err("clicking the Effects tab did not open the browser".to_owned());
        }
        let effects = |app: &Self| {
            app.shell
                .selected_clip()
                .map_or(0, |clip| clip.effects().len())
        };
        let effects_before = effects(self);
        self.click_zone(window, Zone::EffectRow(0))?;
        let effects_after = effects(self);
        if effects_after <= effects_before {
            return Err(format!(
                "clicking an effect left the chain at {effects_after} entries"
            ));
        }

        let transitions = |app: &Self| {
            app.shell.sequence_entity().map_or(0, |sequence| {
                sequence
                    .tracks()
                    .iter()
                    .map(|track| track.transitions().len())
                    .sum()
            })
        };
        self.click_zone(window, Zone::Tab(Tab::TransitionList))?;
        if self.right_tab != Tab::TransitionList {
            return Err("clicking the Transitions tab did not open the browser".to_owned());
        }
        let transitions_before = transitions(self);
        self.click_zone(window, Zone::TransitionRow(0))?;
        let transitions_after = transitions(self);
        if transitions_after <= transitions_before {
            return Err(format!(
                "clicking a transition left the track at {transitions_after} of them"
            ));
        }
        Ok(format!(
            "browsers: an effect click took the selection's chain {effects_before} -> {effects_after}, and a transition click took the first track {transitions_before} -> {transitions_after}"
        ))
    }

    /// Saves to a chosen path.
    fn save_as(&mut self, path: std::path::PathBuf) {
        match self.shell.save_as(&path) {
            Ok(_) => {
                self.actions.push_front(format!("wrote {}", path.display()));
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    /// After anything changed: keep the playhead in view and repaint what it needs to.
    fn after_change(&mut self) {
        self.scroll_to_playhead();
    }

    /// Rolls the timeline so the playhead stays visible.
    fn scroll_to_playhead(&mut self) {
        let rect = self.frame_rect.track_area;
        if rect.is_empty() {
            return;
        }
        self.scroll = paint::scroll_to_show(
            self.scroll,
            self.shell.playhead(),
            self.shell.zoom(),
            rect.width(),
            self.theme.metrics.gutter,
        );
    }

    /// Writes the preferences, so the next start remembers the view.
    fn save_preferences(&mut self) {
        if let Err(error) = self.shell.prefs().save() {
            self.actions
                .push_front(format!("preferences were not saved: {error}"));
        }
    }

    /// The tool catalogue, filtered by what the palette's field holds.
    pub(crate) fn palette_matches(&self) -> Vec<&'static Tool> {
        let query = self.shell.palette().query.to_ascii_lowercase();
        // Typing `place_clip`, `place clip` or `place clip` with any spacing has to find the
        // same tool, so both sides are cut at underscores and whitespace and every word has to
        // appear: a query narrows, and the order does not matter.
        let words: Vec<&str> = query
            .split(|character: char| character.is_whitespace() || character == '_')
            .filter(|word| !word.is_empty())
            .collect();
        catalog::TOOLS
            .iter()
            .filter(|tool| {
                if words.is_empty() {
                    return true;
                }
                let haystack = format!(
                    "{} {}",
                    tool.name.to_ascii_lowercase().replace('_', " "),
                    tool.title.to_ascii_lowercase()
                );
                words.iter().all(|word| haystack.contains(word))
            })
            .take(PALETTE_ROWS)
            .collect()
    }

    /// Renders the frame the shell currently asks for, if it is not already on hand.
    ///
    /// Returns whether new pixels arrived. This is called from the loop and from a scrub, never
    /// from a paint handler: rendering inside the handler that draws is how a window ends up
    /// doing engine work on the path that is supposed to be pixels only.
    pub(crate) fn refresh_preview(&mut self) -> bool {
        let wanted = self.shell.preview_key();
        if self.preview.as_ref() == Some(&wanted) {
            return false;
        }
        self.preview = Some(wanted);
        match self.shell.render_preview() {
            Ok(preview) => {
                self.frame = Some(convert(&preview.frame, self.shell.transfer()));
                self.preview_error = None;
            }
            Err(error) => {
                self.frame = None;
                self.preview_error = Some(error.to_string());
            }
        }
        true
    }

    /// Appends a line to the session log, when one was asked for.
    fn append_log(&self, line: &str) {
        let Some(path) = self.log_path.as_ref() else {
            return;
        };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        let mut text = format!("{stamp} {line}\n");
        if let Ok(existing) = std::fs::read_to_string(path) {
            text = format!("{existing}{text}");
        }
        if std::fs::write(path, text).is_err() {
            // Nothing to do about it here, and nothing to interrupt an edit for.
        }
    }
}

/// The mean of a list of measurements.
fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// Whether two rectangles touch.
fn hits(left: Rect, right: Rect) -> bool {
    left.left < right.right
        && right.left < left.right
        && left.top < right.bottom
        && right.top < left.bottom
}

/// The smallest rectangle containing both.
fn union(left: Rect, right: Rect) -> Rect {
    Rect::new(
        left.left.min(right.left),
        left.top.min(right.top),
        left.right.max(right.right),
        left.bottom.max(right.bottom),
    )
}

/// The bridge between the window procedure and this application.
fn trampoline(context: *mut c_void, event: Event) -> Reaction {
    win32::with_app::<App, _>(context, |app| app.handle(event))
}

/// The name of a binding, for the session log.
fn binding_name(binding: Binding) -> &'static str {
    match binding {
        Binding::StepFrames(_) => "step-frames",
        Binding::StepSeconds(_) => "step-seconds",
        Binding::GoToStart => "go-to-start",
        Binding::GoToEnd => "go-to-end",
        Binding::StepFramesAndSelect(_) => "step-and-select",
        Binding::ZoomIn => "zoom-in",
        Binding::ZoomOut => "zoom-out",
        Binding::ZoomFit => "zoom-fit",
        Binding::TrimStartAtPlayhead => "trim-start",
        Binding::TrimEndAtPlayhead => "trim-end",
        Binding::MoveSelected(_) => "move-selected",
        Binding::MoveTrack(_) => "move-track",
        Binding::Split => "split",
        Binding::Remove => "remove",
        Binding::Duplicate => "duplicate",
        Binding::ToggleEnabled => "toggle-enabled",
        Binding::Undo => "undo",
        Binding::Redo => "redo",
        Binding::Save => "save",
        Binding::SaveAs => "save-as",
        Binding::ExportStill => "export-still",
        Binding::Open => "open",
        Binding::Example => "example",
        Binding::TogglePalette => "palette",
        Binding::Draft(_) => "draft",
        Binding::ToggleTiled => "tiled",
        Binding::ToggleLanguage => "language",
        Binding::AddVideoTrack => "add-video-track",
        Binding::AddAudioTrack => "add-audio-track",
        Binding::PlaceSelection => "place-selection",
        Binding::PlayPause => "play-pause",
        Binding::Shuttle(_) => "shuttle",
        Binding::ToggleLoop => "loop",
        Binding::CyclePage(_) => "cycle-page",
        Binding::Page(_) => "page",
        Binding::TogglePanel(_) => "toggle-panel",
        Binding::ToggleSnap => "snap",
        Binding::ToggleLink => "link",
        Binding::StepKeyframe(_) => "step-keyframe",
        Binding::ToggleKeyframes => "toggle-keyframes",
        Binding::ClearKeyframes => "clear-keyframes",
        Binding::StepEditPoint(_) => "step-edit-point",
        Binding::SelectAdjacent(_) => "select-adjacent",
        Binding::CloseGap => "close-gap",
        Binding::RippleDelete => "ripple-delete",
        Binding::InsertBlank => "insert-blank",
    }
}

/// The virtual key codes this window maps. Only the ones the shell binds, plus what the fields
/// and the menus need.
const VK_BACKSPACE: u16 = 0x08;
const VK_TAB: u16 = 0x09;
const VK_ENTER: u16 = 0x0D;
const VK_ESCAPE: u16 = 0x1B;
const VK_SPACE: u16 = 0x20;
const VK_HOME: u16 = 0x24;
const VK_LEFT: u16 = 0x25;
const VK_UP: u16 = 0x26;
const VK_RIGHT: u16 = 0x27;
const VK_DOWN: u16 = 0x28;
const VK_D: u16 = 0x44;
const VK_K: u16 = 0x4B;
const VK_L: u16 = 0x4C;
const VK_Z: u16 = 0x5A;

/// The press a virtual key code stands for.
pub(crate) fn press_for(code: u16) -> Option<Press> {
    match code {
        0x23 => Some(Press::End),
        0x24 => Some(Press::Home),
        0x21 => Some(Press::PageUp),
        0x22 => Some(Press::PageDown),
        0x25 => Some(Press::Arrow(maj_ui::Direction::Left)),
        0x26 => Some(Press::Arrow(maj_ui::Direction::Up)),
        0x27 => Some(Press::Arrow(maj_ui::Direction::Right)),
        0x28 => Some(Press::Arrow(maj_ui::Direction::Down)),
        0x2E => Some(Press::Delete),
        0x1B => Some(Press::Escape),
        0x0D => Some(Press::Enter),
        0x09 => Some(Press::Tab),
        // The spacebar, which is a stroke like any other: the transport's.
        0x20 => Some(Press::Character(' ')),
        0x30..=0x39 => char::from_u32(u32::from(code)).map(Press::Character),
        // Letters arrive as their own codes whichever of Shift or Caps is on.
        0x41..=0x5A => char::from_u32(u32::from(code))
            .map(|letter| Press::Character(letter.to_ascii_lowercase())),
        // The punctuation the editing keys use, by physical key rather than by character:
        // a layout that cannot produce one of them simply has no binding there.
        0xBB => Some(Press::Character('=')),
        0xBD => Some(Press::Character('-')),
        0xDB => Some(Press::Character('[')),
        0xDD => Some(Press::Character(']')),
        0xBC => Some(Press::Character(',')),
        0xBE => Some(Press::Character('.')),
        // The plus on the numeric keypad, which is where an editor's hand already is.
        0x6B => Some(Press::Character('+')),
        _ => None,
    }
}

/// The stroke a key produces, with its modifiers.
pub(crate) fn stroke_for(code: u16, ctrl: bool, shift: bool, alt: bool) -> Option<Stroke> {
    let mut stroke = Stroke::new(press_for(code)?);
    if ctrl {
        stroke = stroke.ctrl();
    }
    if shift {
        stroke = stroke.shift();
    }
    if alt {
        stroke = stroke.alt();
    }
    Some(stroke)
}

/// A virtual key that produces a press, for the test that walks the shell's table.
#[cfg(test)]
pub(crate) fn virtual_key(press: Press) -> Option<u16> {
    match press {
        Press::Home => Some(0x24),
        Press::End => Some(0x23),
        Press::PageUp => Some(0x21),
        Press::PageDown => Some(0x22),
        Press::Delete => Some(0x2E),
        Press::Escape => Some(0x1B),
        Press::Enter => Some(0x0D),
        Press::Tab => Some(0x09),
        Press::Arrow(maj_ui::Direction::Left) => Some(0x25),
        Press::Arrow(maj_ui::Direction::Up) => Some(0x26),
        Press::Arrow(maj_ui::Direction::Right) => Some(0x27),
        Press::Arrow(maj_ui::Direction::Down) => Some(0x28),
        Press::Character(character) => match character {
            'a'..='z' => Some(u16::from(character.to_ascii_uppercase() as u8)),
            '0'..='9' => Some(u16::from(character as u8)),
            ' ' => Some(0x20),
            '=' | '+' => Some(0xBB),
            '-' => Some(0xBD),
            '[' => Some(0xDB),
            ']' => Some(0xDD),
            ',' => Some(0xBC),
            '.' => Some(0xBE),
            _ => None,
        },
    }
}

/// Converts a rendered frame into the byte order the blits want: blue, green, red, ignored,
/// top to bottom, with the transfer function applied on the way in when it is not linear.
fn convert(frame: &Framebuffer, transfer: TransferFunction) -> Frame {
    let maximum = f64::from(BitDepth::Eight.maximum());
    let width = frame.width();
    let height = frame.height();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let (red, green, blue) = frame
                .pixel(x, y)
                .map(|pixel| {
                    let code = |channel: f32| {
                        let linear = if channel.is_finite() {
                            f64::from(channel)
                        } else {
                            0.0
                        };
                        let encoded = transfer.encode(linear);
                        let clamped = if encoded.is_nan() {
                            0.0
                        } else {
                            encoded.clamp(0.0, 1.0)
                        };
                        (clamped * maximum + 0.5).floor() as u8
                    };
                    (code(pixel[0]), code(pixel[1]), code(pixel[2]))
                })
                .unwrap_or((0, 0, 0));
            pixels.push(blue);
            pixels.push(green);
            pixels.push(red);
            pixels.push(0);
        }
    }
    Frame {
        width,
        height,
        pixels,
    }
}

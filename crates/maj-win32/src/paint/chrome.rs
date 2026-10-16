//! The window's own chrome: the menu bar, the toolbar, the page strip, the status line.
//!
//! Every one of these registers the regions a pointer can land on, in painting order, so the
//! hit-test table and the picture cannot disagree.

use maj_ui::{Key, Page};

use crate::app::App;
use crate::glyphs::{self, Glyph};
use crate::layout::Frame;
use crate::widget::{FrameButton, Region, Tool, Zone};
use crate::win32::{self, Align, Colour, CursorKind, Hdc, Rect};

/// The menus, in the order they sit in the bar.
pub(crate) const MENUS: [Key; 7] = [
    Key::MenuApp,
    Key::MenuFile,
    Key::MenuEdit,
    Key::MenuSequence,
    Key::MenuView,
    Key::MenuTools,
    Key::MenuHelp,
];

/// The tools the toolbar shows, with a break before each group.
const TOOLS: &[(Tool, bool)] = &[
    (Tool::Undo, false),
    (Tool::Redo, true),
    (Tool::Split, false),
    (Tool::TrimIn, false),
    (Tool::TrimOut, false),
    (Tool::Remove, false),
    (Tool::Duplicate, true),
    (Tool::Marker, false),
    (Tool::Snap, false),
    (Tool::Link, true),
    (Tool::ZoomOut, false),
    (Tool::ZoomIn, false),
    (Tool::ZoomFit, true),
    (Tool::Draft, false),
    (Tool::Tiled, false),
    (Tool::Palette, true),
    (Tool::AddVideoTrack, false),
    (Tool::AddAudioTrack, false),
    (Tool::Export, true),
    (Tool::Example, false),
];

/// The page strip: every page, in the order the reference lists them.
pub(crate) const PAGES: [Page; 7] = [
    Page::Media,
    Page::Cut,
    Page::Edit,
    Page::Fusion,
    Page::Color,
    Page::Fairlight,
    Page::Deliver,
];

/// Draws a button's background for its state, and answers with the ink to draw in.
fn face(hdc: Hdc, rect: Rect, app: &App, zone: Zone, active: bool, usable: bool) -> Colour {
    let theme = &app.theme;
    let radius = theme.metrics.radius;
    if !usable {
        return theme.faint;
    }
    if app.pressed == Some(zone) {
        win32::rounded(hdc, rect, radius, theme.chrome_low, None);
        return theme.accent;
    }
    if app.hover == Some(zone) {
        win32::rounded(hdc, rect, radius, theme.chrome_high, None);
        return theme.ink;
    }
    if active {
        win32::rounded(
            hdc,
            rect,
            radius,
            theme.chrome_high,
            Some(theme.accent_deep),
        );
        return theme.accent;
    }
    theme.dim
}

/// The menu bar: the brand, the menus, the project's name, and — when the frame is ours — the
/// three buttons that minimise, maximise and close.
pub(crate) fn menu_bar(hdc: Hdc, frame: &Frame, app: &mut App) {
    let rect = frame.menu;
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    win32::gradient(hdc, rect, theme.chrome, theme.panel);
    win32::fill_colour(
        hdc,
        Rect::new(rect.left, rect.bottom - 1, rect.right, rect.bottom),
        theme.separator,
    );

    // The owner's logo, on the bar, where the reference frame this interface follows also puts
    // it — set in the accent rather than stamped on its own white plate, because a plate on a
    // dark bar reads as a sticker on the window instead of as the application's mark. The
    // ground is the top of the gradient just drawn, which is where the mark sits.
    let side = (rect.height() - m.px(6)).max(m.px(8));
    let mark = Rect::new(
        rect.left + m.pad,
        rect.top + m.px(3),
        rect.left + m.pad + side,
        rect.top + m.px(3) + side,
    );
    crate::brand::seal_tinted(hdc, mark, theme.accent, theme.chrome);
    let name = app.say(Key::AppName);
    let name_x = mark.right + m.px(4);
    let name_w = win32::text_width(hdc, name, &theme.heading_font);
    win32::text(
        hdc,
        Rect::new(name_x, rect.top, name_x + name_w + m.px(8), rect.bottom),
        name,
        theme.ink,
        &theme.heading_font,
        Align::Left,
    );

    let open = app.menu.map(|menu| menu.kind);
    let mut x = name_x + name_w + m.px(10);
    for (index, key) in MENUS.iter().enumerate() {
        let label = app.say(*key);
        let width = win32::text_width(hdc, label, &theme.body_font) + m.px(16);
        let item = Rect::new(x, rect.top + m.px(3), x + width, rect.bottom - m.px(3));
        if item.right > rect.right - m.px(150) {
            break;
        }
        let is_open = open == Some(crate::app::MenuKind::from_index(index));
        let ink = face(hdc, item, app, Zone::Menu(index), is_open, true);
        win32::text(
            hdc,
            item,
            label,
            if is_open { theme.accent } else { ink },
            &theme.body_font,
            Align::Centre,
        );
        app.zones.push(Region::new(
            Zone::Menu(index),
            item,
            CursorKind::Arrow,
            *key,
        ));
        x = item.right;
    }

    // The right side: the project, and the frame's own buttons when the frame is ours.
    let mut right = rect.right - m.pad;
    if app.frameless {
        for button in [
            FrameButton::Close,
            FrameButton::Maximize,
            FrameButton::Minimize,
        ] {
            let width = m.frame_button_w.min(rect.width() / 6);
            let item = Rect::new(right - width, rect.top, right, rect.bottom);
            let (glyph, key) = match button {
                FrameButton::Close => (Glyph::Close, Key::FrameClose),
                FrameButton::Maximize => (
                    if app.maximised {
                        Glyph::Restore
                    } else {
                        Glyph::Maximize
                    },
                    Key::FrameMaximize,
                ),
                FrameButton::Minimize => (Glyph::Minimize, Key::FrameMinimize),
            };
            let zone = Zone::FrameButton(button);
            let ink = if app.hover == Some(zone) {
                win32::fill_colour(hdc, item, theme.chrome_high);
                if button == FrameButton::Close {
                    theme.error
                } else {
                    theme.ink
                }
            } else {
                theme.dim
            };
            glyphs::draw(
                hdc,
                Rect::new(
                    item.left + m.px(14),
                    item.top + m.px(7),
                    item.right - m.px(14),
                    item.bottom - m.px(7),
                ),
                glyph,
                ink,
            );
            app.zones
                .push(Region::new(zone, item, CursorKind::Arrow, key));
            right = item.left;
        }
    }
    let project = app.shell.project().name().to_owned();
    let page = app.say(page_label(app.shell.page()));
    let dirty = if app.shell.is_dirty() { " •" } else { "" };
    let summary = format!("{project}{dirty} · {page}");
    let width = win32::text_width(hdc, &summary, &theme.small_font) + m.px(16);
    win32::text(
        hdc,
        Rect::new(
            (right - width - m.px(6)).max(x),
            rect.top,
            right - m.px(6),
            rect.bottom,
        ),
        &summary,
        theme.dim,
        &theme.small_font,
        Align::Right,
    );
}

/// The label of a page.
#[must_use]
pub(crate) const fn page_label(page: Page) -> Key {
    match page {
        Page::Media => Key::PageMedia,
        Page::Cut => Key::PageCut,
        Page::Edit => Key::PageEdit,
        Page::Fusion => Key::PageFusion,
        Page::Color => Key::PageColor,
        Page::Fairlight => Key::PageFairlight,
        Page::Deliver => Key::PageDeliver,
    }
}

/// The toolbar: the editing verbs as buttons, each greyed out when it would refuse.
pub(crate) fn toolbar(hdc: Hdc, frame: &Frame, app: &mut App) {
    let rect = frame.toolbar;
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    win32::fill_colour(hdc, rect, theme.panel);
    win32::fill_colour(
        hdc,
        Rect::new(rect.left, rect.bottom - 1, rect.right, rect.bottom),
        theme.border,
    );
    let size = (m.toolbar_h - m.px(10)).max(m.px(16));
    let mut x = rect.left + m.pad;
    let has_selection = app.shell.selection().is_some();
    for (tool, breaks_before) in TOOLS {
        if *breaks_before {
            let separator = Rect::new(
                x + m.px(2),
                rect.top + m.px(6),
                x + m.px(3),
                rect.bottom - m.px(6),
            );
            win32::fill_colour(hdc, separator, theme.border);
            x += m.px(7);
        }
        let item = Rect::new(
            x,
            rect.top + (rect.height() - size) / 2,
            x + size,
            rect.top + (rect.height() + size) / 2,
        );
        if item.right > rect.right - m.px(8) {
            break;
        }
        let usable = tool_usable(*tool, app);
        let active = tool_active(*tool, app);
        let ink = face(hdc, item, app, Zone::Tool(*tool), active, usable);
        glyphs::draw(hdc, item.inset(m.px(3)), tool.glyph(), ink);
        app.zones.push(
            Region::new(Zone::Tool(*tool), item, CursorKind::Point, tool.label())
                .hinted(tool.hint()),
        );
        x = item.right + m.px(2);
    }

    // The right end of the toolbar says what the timeline is doing rather than holding buttons:
    // the zoom, and how many frames are on screen.
    let zoom = format!("{:.0}%", app.shell.zoom() * 100.0);
    let width = win32::text_width(hdc, &zoom, &theme.mono_font) + m.px(16);
    win32::text(
        hdc,
        Rect::new(
            rect.right - width,
            rect.top,
            rect.right - m.px(4),
            rect.bottom,
        ),
        &zoom,
        theme.dim,
        &theme.mono_font,
        Align::Right,
    );
    let _ = has_selection;
}

/// Whether a tool would do anything right now.
fn tool_usable(tool: Tool, app: &App) -> bool {
    let (undo, redo) = app.shell.depths();
    let has_selection = app.shell.selection().is_some();
    match tool {
        Tool::Undo => undo > 0,
        Tool::Redo => redo > 0,
        Tool::Split | Tool::TrimIn | Tool::TrimOut | Tool::Remove | Tool::Duplicate => {
            has_selection
        }
        // A marker anchors to a sequence, so with no sequence open there is nowhere to put one.
        Tool::Marker => app.shell.sequence_entity().is_some(),
        _ => true,
    }
}

/// Whether a tool is switched on, for the ones that are switches.
fn tool_active(tool: Tool, app: &App) -> bool {
    match tool {
        Tool::Snap => app.shell.view().snap,
        Tool::Link => app.shell.view().link,
        Tool::Tiled => app.shell.tiled(),
        _ => false,
    }
}

/// The page strip: seven pages, three of which are built.
pub(crate) fn pages(hdc: Hdc, frame: &Frame, app: &mut App) {
    let rect = frame.pages;
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    win32::gradient(hdc, rect, theme.panel, theme.chrome);
    win32::fill_colour(
        hdc,
        Rect::new(rect.left, rect.top, rect.right, rect.top + 1),
        theme.separator,
    );
    let current = app.shell.page();

    // How wide the whole strip wants to be.
    let widths: Vec<i32> = PAGES
        .iter()
        .map(|page| win32::text_width(hdc, app.say(page_label(*page)), &theme.body_font) + m.px(58))
        .collect();
    let total: i32 = widths.iter().sum();
    let mut x = rect.left + (rect.width() - total).max(0) / 2;
    for (index, page) in PAGES.iter().enumerate() {
        let item = Rect::new(
            x,
            rect.top + m.px(2),
            x + widths[index],
            rect.bottom - m.px(3),
        );
        if item.right > rect.right {
            break;
        }
        let built = page.built();
        let active = *page == current;
        let zone = Zone::Page(*page);
        if built && app.hover == Some(zone) {
            win32::rounded(hdc, item, m.radius, theme.chrome_high, None);
        }
        let icon = Rect::new(
            item.left + m.px(10),
            item.top + m.px(7),
            item.left + m.px(24),
            item.bottom - m.px(7),
        );
        let ink = if !built {
            theme.faint
        } else if active {
            theme.accent
        } else {
            theme.ink
        };
        glyphs::draw(hdc, icon, page_glyph(*page), ink);
        win32::text(
            hdc,
            Rect::new(
                icon.right + m.px(4),
                item.top,
                item.right - m.px(8),
                item.bottom,
            ),
            app.say(page_label(*page)),
            ink,
            &theme.body_font,
            Align::Left,
        );
        if active {
            win32::fill_colour(
                hdc,
                Rect::new(
                    item.left + m.px(6),
                    rect.bottom - m.px(4),
                    item.right - m.px(6),
                    rect.bottom - m.px(2),
                ),
                theme.accent,
            );
        }
        let region = Region::new(zone, item, CursorKind::Point, page_label(*page));
        // A page whose engine does not exist yet says which phase owns it instead of doing
        // nothing when it is clicked.
        let region = if built {
            region
        } else {
            region.refused(page_owner(*page))
        };
        app.zones.push(region);
        x = item.right;
    }

    // The mark and the name sit in the left corner, the way the reference frame this interface
    // follows carries them, and the language switch follows them like the chip it is.
    let corner = (rect.height() - m.px(8)).max(m.px(10));
    let seal = Rect::new(
        rect.left + m.px(8),
        rect.top + (rect.height() - corner) / 2,
        rect.left + m.px(8) + corner,
        rect.top + (rect.height() - corner) / 2 + corner,
    );
    // Set in the accent, on the middle of this strip's gradient — `panel_alt` is exactly
    // halfway between the two colours it runs through, and the mark is centred in it.
    crate::brand::seal_tinted(hdc, seal, theme.accent, theme.panel_alt);
    let name = app.say(Key::AppName);
    let name_w = win32::text_width(hdc, name, &theme.body_font);
    win32::text(
        hdc,
        Rect::new(
            seal.right + m.px(6),
            rect.top,
            seal.right + m.px(6) + name_w + m.px(4),
            rect.bottom,
        ),
        name,
        theme.dim,
        &theme.body_font,
        Align::Left,
    );
    let chip_left = seal.right + m.px(10) + name_w;
    let chip = Rect::new(
        chip_left,
        rect.top + m.px(6),
        chip_left + m.px(42),
        rect.bottom - m.px(7),
    );
    let ink = face(hdc, chip, app, Zone::Tool(Tool::Palette), false, false);
    win32::text(
        hdc,
        chip,
        app.shell.lang().key(),
        ink,
        &theme.small_font,
        Align::Centre,
    );
}

/// The picture a page's tab carries.
#[must_use]
pub(crate) const fn page_glyph(page: Page) -> Glyph {
    match page {
        Page::Media => Glyph::ListView,
        Page::Cut => Glyph::Split,
        Page::Edit => Glyph::Film,
        Page::Fusion => Glyph::Keyframe,
        Page::Color => Glyph::Sliders,
        Page::Fairlight => Glyph::Wave,
        Page::Deliver => Glyph::Marker,
    }
}

/// The phase that owns a page that is not built.
#[must_use]
pub(crate) const fn page_owner(page: Page) -> Key {
    match page {
        Page::Edit => Key::PageOwnerEdit,
        Page::Fusion => Key::PageOwnerFusion,
        Page::Color => Key::PageOwnerColor,
        Page::Fairlight => Key::PageOwnerFairlight,
        // A page that is built has no owner to name; the refusal is never drawn for it.
        _ => Key::NotBuiltHeading,
    }
}

/// The status bar: what the shell says, and what happened last.
pub(crate) fn status(hdc: Hdc, frame: &Frame, app: &mut App) {
    let rect = frame.status;
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    win32::fill_colour(hdc, rect, theme.chrome);
    win32::fill_colour(
        hdc,
        Rect::new(rect.left, rect.top, rect.right, rect.top + 1),
        theme.separator,
    );
    let line = app.shell.status_line();
    win32::text(
        hdc,
        Rect::new(
            rect.left + m.pad,
            rect.top,
            rect.left + rect.width() * 2 / 3,
            rect.bottom,
        ),
        &line,
        theme.dim,
        &theme.small_font,
        Align::Left,
    );
    let (said, colour) = match app.error.as_ref() {
        Some(error) => (error.clone(), theme.error),
        None => (app.self_report(), theme.faint),
    };
    win32::text(
        hdc,
        Rect::new(
            rect.left + rect.width() / 3,
            rect.top,
            rect.right - m.pad,
            rect.bottom,
        ),
        &said,
        colour,
        &theme.small_font,
        Align::Right,
    );
}

//! The drawing: the whole interface, painted onto a device context.
//!
//! Nothing here owns state or decides anything. It reads the shell and the window's own view
//! state, it writes pixels, and it records what is where so the pointer can be resolved against
//! the same rectangles it drew.
//!
//! Two rules shape it. **Every panel is drawn from the layout's rectangles**, so a panel cannot
//! be painted somewhere the hit test does not know about. **A panel that the damage does not
//! touch is not drawn at all**: the back buffer still holds the previous frame, so a playhead
//! moving by one frame costs a stripe of the timeline rather than the whole window.

mod chrome;
pub(crate) mod menus;
mod panels;
mod timeline;
mod viewer;

pub(crate) use timeline::{frame_at, scroll_to_show, x_of};

use maj_ui::{Key, Page, text};

use crate::app::App;
use crate::layout::Frame;
use crate::widget::{Region, Splitter, Zone};
use crate::win32::{self, Align, CursorKind, Hdc, Rect};

/// Paints the interface, into the back buffer, for the rectangles that changed.
pub(crate) fn draw(hdc: Hdc, rect: Rect, app: &mut App, damage: &[Rect]) {
    app.zones.clear();
    let frame = Frame::build(rect, &app.view());
    app.frame_rect = frame;
    let whole = damage.is_empty() || damage.contains(&rect);
    let wanted = |area: Rect| whole || damage.iter().any(|part| overlaps(*part, area));

    // The floor, for the parts that changed. The rest of the buffer is already right.
    for area in damage {
        win32::fill_colour(hdc, *area, app.theme.ground);
    }
    if damage.is_empty() {
        win32::fill_colour(hdc, rect, app.theme.ground);
        chrome::menu_bar(hdc, &frame, app);
        chrome::toolbar(hdc, &frame, app);
        chrome::pages(hdc, &frame, app);
        chrome::status(hdc, &frame, app);
    } else {
        if wanted(frame.menu) {
            chrome::menu_bar(hdc, &frame, app);
        }
        if wanted(frame.toolbar) {
            chrome::toolbar(hdc, &frame, app);
        }
        if wanted(frame.pages) {
            chrome::pages(hdc, &frame, app);
        }
        if wanted(frame.status) {
            chrome::status(hdc, &frame, app);
        }
    }

    match app.shell.page() {
        Page::Media => {
            if wanted(frame.grid) {
                panels::media_page(hdc, &frame, app);
            }
            if wanted(frame.viewer) {
                viewer::viewer(hdc, frame.viewer, app);
            }
            if wanted(frame.transport) {
                viewer::transport(hdc, frame.transport, app);
            }
            if wanted(frame.meter) {
                viewer::meter(hdc, frame.meter, app);
            }
            if wanted(frame.inspector) {
                panels::right_column(hdc, frame.inspector, app);
            }
        }
        Page::Deliver => {
            if wanted(frame.export) {
                panels::deliver_page(hdc, &frame, app);
            }
            if wanted(frame.centre) {
                viewer::viewer(hdc, frame.centre, app);
            }
            if wanted(frame.right) {
                panels::right_column(hdc, frame.right, app);
            }
        }
        Page::Cut => {
            if wanted(frame.left) {
                panels::left_column(hdc, frame.left, app);
            }
            if wanted(frame.right) {
                panels::right_column(hdc, frame.right, app);
            }
            if wanted(frame.viewer) {
                viewer::viewer(hdc, frame.viewer, app);
            }
            if wanted(frame.transport) {
                viewer::transport(hdc, frame.transport, app);
            }
            if wanted(frame.meter) {
                viewer::meter(hdc, frame.meter, app);
            }
            if wanted(frame.timeline) {
                timeline::timeline(hdc, &frame, app);
            }
        }
        // The pages whose engines are not built: the shape of the window, and a card that says
        // which phase owns them. No imitation of a panel that cannot work.
        page => not_built(hdc, frame.body, app, page),
    }

    splitters(hdc, &frame, app);
    if app.shell.palette().open {
        menus::palette(hdc, rect, app);
    }
    if app.menu.is_some() {
        menus::dropdown(hdc, rect, app);
    }
    menus::tooltip(hdc, rect, app);
}

/// Whether two rectangles share a pixel.
fn overlaps(left: Rect, right: Rect) -> bool {
    left.left < right.right
        && right.left < left.right
        && left.top < right.bottom
        && right.top < left.bottom
}

/// The dividers a pointer can drag.
fn splitters(hdc: Hdc, frame: &Frame, app: &mut App) {
    let inline = Rect::new(
        app.frame_rect.inspector.left,
        app.frame_rect.inspector.bottom,
        app.frame_rect.inspector.right,
        app.frame_rect.mixer.top,
    );
    for (splitter, rect) in [
        (Splitter::Left, frame.split_left),
        (Splitter::Right, frame.split_right),
        (Splitter::Timeline, frame.split_timeline),
        (Splitter::Inline, inline),
    ] {
        if rect.is_empty() {
            continue;
        }
        let vertical = matches!(splitter, Splitter::Timeline);
        let hovered = app.hover == Some(Zone::Splitter(splitter))
            || app.dragging == Some(Zone::Splitter(splitter));
        win32::fill_colour(
            hdc,
            rect,
            if hovered {
                app.theme.accent_deep
            } else {
                app.theme.ground
            },
        );
        app.zones.push(Region::new(
            Zone::Splitter(splitter),
            rect,
            if vertical {
                CursorKind::Vertical
            } else {
                CursorKind::Horizontal
            },
            Key::SplitterResize,
        ));
    }
}

/// The card a page shows while the phase that builds it has not happened.
fn not_built(hdc: Hdc, rect: Rect, app: &mut App, page: Page) {
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    win32::fill_colour(hdc, rect, theme.ground);
    let width = (rect.width() - m.px(80)).clamp(m.px(280), m.px(560));
    let height = m.px(150);
    let left = rect.left + (rect.width() - width) / 2;
    let top = rect.top + (rect.height() - height) / 2;
    let card = Rect::new(left, top, left + width, top + height);
    win32::rounded(hdc, card, m.radius, theme.panel, Some(theme.border));
    let icon = Rect::new(
        card.left + m.px(24),
        card.top + m.px(24),
        card.left + m.px(60),
        card.top + m.px(60),
    );
    win32::rounded(hdc, icon, m.radius, theme.chrome, None);
    crate::glyphs::draw(
        hdc,
        icon.inset(m.px(8)),
        chrome::page_glyph(page),
        theme.faint,
    );
    win32::text(
        hdc,
        Rect::new(
            icon.right + m.px(12),
            card.top + m.px(24),
            card.right - m.px(16),
            card.top + m.px(48),
        ),
        text(app.shell.lang(), chrome::page_label(page)),
        theme.ink,
        &theme.heading_font,
        Align::Left,
    );
    win32::text(
        hdc,
        Rect::new(
            icon.right + m.px(12),
            card.top + m.px(46),
            card.right - m.px(16),
            card.top + m.px(66),
        ),
        text(app.shell.lang(), Key::NotBuiltHeading),
        theme.faint,
        &theme.small_font,
        Align::Left,
    );
    win32::text_block(
        hdc,
        Rect::new(
            card.left + m.px(24),
            card.top + m.px(76),
            card.right - m.px(24),
            card.bottom - m.px(16),
        ),
        text(app.shell.lang(), chrome::page_owner(page)),
        theme.dim,
        &theme.body_font,
    );
}

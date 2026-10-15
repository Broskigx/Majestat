//! The viewer, its transport bar, and the meters beside it.
//!
//! The frame is the one the certified renderer produced, scaled with the smoother stretch mode
//! so a downscaled preview looks like a picture. When there is no frame the panel says why, in
//! the shell's own words, rather than showing an empty black rectangle.

use maj_ui::{Key, Timecode, TimecodeMode, text};

use crate::app::App;
use crate::glyphs;
use crate::widget::{Region, TransportButton, Zone};
use crate::win32::{self, Align, CursorKind, Hdc, Rect};

/// The buttons of the transport bar, in order.
const BUTTONS: [TransportButton; 6] = [
    TransportButton::Start,
    TransportButton::PrevFrame,
    TransportButton::Play,
    TransportButton::NextFrame,
    TransportButton::End,
    TransportButton::Loop,
];

/// The viewer: the frame, letterboxed, with the facts a monitor carries.
pub(crate) fn viewer(hdc: Hdc, rect: Rect, app: &mut App) {
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    win32::fill_colour(hdc, rect, theme.well);
    let heading = Rect::new(rect.left, rect.top, rect.right, rect.top + m.heading_h);
    win32::gradient(hdc, heading, theme.chrome, theme.panel);
    win32::text(
        hdc,
        Rect::new(
            heading.left + m.px(8),
            heading.top,
            heading.right,
            heading.bottom,
        ),
        text(app.shell.lang(), Key::PanelProgram),
        theme.dim,
        &theme.body_font,
        Align::Left,
    );
    if let Some(sequence) = app.shell.sequence_entity() {
        let label = format!(
            "{} · {}x{}",
            sequence.name(),
            sequence.width(),
            sequence.height()
        );
        win32::text(
            hdc,
            Rect::new(
                heading.left + m.px(90),
                heading.top,
                heading.right - m.px(8),
                heading.bottom,
            ),
            &label,
            theme.faint,
            &theme.small_font,
            Align::Right,
        );
    }
    let frame = Rect::new(
        rect.left + 1,
        heading.bottom + 1,
        rect.right - 1,
        rect.bottom - 1,
    );
    win32::fill_colour(hdc, frame, theme.ground);

    let Some(picture) = app.frame.as_ref() else {
        let message = app
            .preview_error
            .clone()
            .unwrap_or_else(|| text(app.shell.lang(), Key::ViewerEmpty).to_owned());
        // Centred in the matte, wrapped, so a refusal reads as a sentence rather than as a
        // squeezed line at the top of a black rectangle.
        let box_ = Rect::new(
            frame.left + frame.width() / 6,
            frame.top + frame.height() / 2 - m.px(30),
            frame.right - frame.width() / 6,
            frame.top + frame.height() / 2 + m.px(30),
        );
        let colour = if app.preview_error.is_some() {
            theme.error
        } else {
            theme.faint
        };
        win32::text_block(hdc, box_, &message, colour, &theme.body_font);
        return;
    };

    let (width, height) = (picture.width as i32, picture.height as i32);
    let available = frame.inset(m.px(6));
    let scale = (f64::from(available.width()) / f64::from(width))
        .min(f64::from(available.height()) / f64::from(height))
        .max(0.01);
    let drawn_w = (f64::from(width) * scale).round() as i32;
    let drawn_h = (f64::from(height) * scale).round() as i32;
    let left = available.left + (available.width() - drawn_w) / 2;
    let top = available.top + (available.height() - drawn_h) / 2;
    let target = Rect::new(left, top, left + drawn_w, top + drawn_h);
    win32::blit_smooth(hdc, target, picture.width, picture.height, &picture.pixels);
    win32::frame(hdc, target, &theme.border_brush());

    let draft = match app.shell.draft() {
        maj_ui::Draft::Full => Key::DraftFull,
        maj_ui::Draft::Half => Key::DraftHalf,
        maj_ui::Draft::Quarter => Key::DraftQuarter,
    };
    let path = if app.shell.tiled() {
        Key::MonitorTiled
    } else {
        Key::MonitorTransfer
    };
    let label = format!(
        "{} · {}",
        text(app.shell.lang(), draft),
        text(app.shell.lang(), path)
    );
    win32::text(
        hdc,
        Rect::new(
            frame.left + m.px(8),
            frame.top + m.px(4),
            frame.right,
            frame.top + m.px(4) + m.row_h,
        ),
        &label,
        theme.faint,
        &theme.small_font,
        Align::Left,
    );
    if let Some(ms) = app.shell.last_render_ms() {
        let render = format!("{ms:.1} ms");
        win32::text(
            hdc,
            Rect::new(
                frame.left,
                frame.top + m.px(4),
                frame.right - m.px(8),
                frame.top + m.px(4) + m.row_h,
            ),
            &render,
            theme.faint,
            &theme.mono_font,
            Align::Right,
        );
    }
}

/// The timecode readout: `HH:MM:SS:FF` at the sequence's rate, or the field while it is being
/// typed into.
pub(crate) fn timecode(app: &App) -> String {
    if let Some(entry) = app.timecode_entry.as_ref() {
        return format!("{entry}_");
    }
    let Some(sequence) = app.shell.sequence_entity() else {
        return app.shell.playhead().to_string();
    };
    let mode = TimecodeMode::preferred_for(sequence.timebase());
    match Timecode::from_frames(app.shell.playhead().max(0), sequence.timebase(), mode) {
        Ok(timecode) => timecode.to_string(),
        Err(_) => app.shell.playhead().to_string(),
    }
}

/// The transport bar: the usual buttons, the timecode, and how the clock is doing.
pub(crate) fn transport(hdc: Hdc, rect: Rect, app: &mut App) {
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    win32::fill_colour(hdc, rect, theme.panel);
    win32::fill_colour(
        hdc,
        Rect::new(rect.left, rect.top, rect.right, rect.top + 1),
        theme.separator,
    );

    let size = (rect.height() - m.px(10)).min(m.px(28)).max(m.px(16));
    let gap = m.px(3);
    let total = (size + gap) * BUTTONS.len() as i32 - gap;
    let mut x = rect.left + (rect.width() - total) / 2;
    for button in BUTTONS {
        let item = Rect::new(
            x,
            rect.top + (rect.height() - size) / 2,
            x + size,
            rect.top + (rect.height() + size) / 2,
        );
        let zone = Zone::Transport(button);
        let active = matches!(button, TransportButton::Loop) && app.transport.loops;
        let playing = app.transport.playing;
        let ink = if app.pressed == Some(zone) {
            win32::rounded(hdc, item, m.radius, theme.chrome_low, None);
            theme.accent
        } else if app.hover == Some(zone) {
            win32::rounded(hdc, item, m.radius, theme.chrome_high, None);
            theme.ink
        } else if active || (button == TransportButton::Play && playing) {
            win32::rounded(
                hdc,
                item,
                m.radius,
                theme.chrome_high,
                Some(theme.accent_deep),
            );
            theme.accent
        } else {
            theme.dim
        };
        if button == TransportButton::Play && playing {
            win32::fill_colour(hdc, item, theme.chrome_high);
        }
        glyphs::draw(hdc, item.inset(m.px(4)), button.glyph(playing), ink);
        app.zones.push(
            Region::new(zone, item, CursorKind::Point, button.label(playing)).hinted(button.hint()),
        );
        x = item.right + gap;
    }

    // The timecode: a field, so a person can type a frame number into it.
    let value = timecode(app);
    let width = win32::text_width(hdc, "00:00:00:00", &theme.mono_font) + m.px(22);
    let field = Rect::new(
        rect.right - width - m.px(8),
        rect.top + m.px(5),
        rect.right - m.px(8),
        rect.bottom - m.px(5),
    );
    if field.left > x {
        let typing = app.timecode_entry.is_some();
        win32::rounded(
            hdc,
            field,
            m.radius,
            if typing {
                theme.chrome_low
            } else {
                theme.chrome
            },
            Some(if typing { theme.accent } else { theme.border }),
        );
        win32::text(
            hdc,
            field,
            &value,
            if typing { theme.accent } else { theme.ink },
            &theme.mono_font,
            Align::Centre,
        );
        app.zones.push(Region::new(
            Zone::TimecodeField,
            field,
            CursorKind::Text,
            Key::TransportTimecode,
        ));
    }

    // And what the clock is: the shell's own sentence about what does not exist, plus the rate.
    let report = app.self_report();
    let note = Rect::new(
        rect.left + m.px(8),
        rect.top,
        (field.left - m.px(8)).max(x),
        rect.bottom,
    );
    if note.width() > m.px(40) {
        win32::text(
            hdc,
            note,
            &report,
            theme.faint,
            &theme.small_font,
            Align::Left,
        );
    }
}

/// The meters: two columns of ticks that stand at rest, with the reason why.
pub(crate) fn meter(hdc: Hdc, rect: Rect, app: &mut App) {
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    win32::fill_colour(hdc, rect, theme.panel);
    win32::fill_colour(
        hdc,
        Rect::new(rect.left, rect.top, rect.left + 1, rect.bottom),
        theme.separator,
    );
    let inner = rect.inset(m.px(6));
    if inner.is_empty() {
        return;
    }
    let column_w = (inner.width() - m.px(4)) / 2;
    for (index, channel) in ["L", "R"].iter().enumerate() {
        let left = inner.left + (index as i32) * (column_w + m.px(4));
        let column = Rect::new(left, inner.top, left + column_w, inner.bottom - m.px(16));
        if column.is_empty() {
            continue;
        }
        win32::rounded(hdc, column, m.px(2), theme.well, Some(theme.border));
        // The scale marks, from -60 to 0 in equal steps, so the column reads as a meter that is
        // at rest rather than as an empty box.
        for step in 0..=10 {
            let y = column.bottom - (column.height() * step) / 10;
            let long = step % 5 == 0;
            win32::fill_colour(
                hdc,
                Rect::new(
                    column.left + 1,
                    y,
                    column.left + if long { m.px(7) } else { m.px(4) },
                    y + 1,
                ),
                theme.border,
            );
        }
        win32::text(
            hdc,
            Rect::new(
                column.left,
                column.bottom,
                column.right,
                column.bottom + m.px(16),
            ),
            channel,
            theme.faint,
            &theme.small_font,
            Align::Centre,
        );
    }
    app.zones.push(Region::new(
        Zone::Meter,
        rect,
        CursorKind::Arrow,
        Key::AudioNotBuilt,
    ));
}

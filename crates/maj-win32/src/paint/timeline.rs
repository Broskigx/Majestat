//! The timeline: its tool strip, the ruler, the track headers, the clips and the playhead.
//!
//! The ruler and the lanes share one mapping from a frame to a pixel and back. Clips are drawn
//! with a band along the top and a gradient body rather than as flat blocks, and a selected clip
//! carries the two grab handles a pointer can take hold of.

use maj_core::TrackKind;
use maj_ui::{Key, Timecode, TimecodeMode, text};

use crate::app::App;
use crate::glyphs::{self, Glyph};
use crate::layout::Frame;
use crate::widget::{ClipPart, Region, Tool, TrackControl, Zone};
use crate::win32::{self, Align, CursorKind, Hdc, Rect};

/// The tools the timeline's own strip carries, and where a divider sits before them.
const TOOLS: &[(Tool, bool)] = &[
    (Tool::Split, false),
    (Tool::TrimIn, false),
    (Tool::TrimOut, false),
    (Tool::Remove, true),
    (Tool::Snap, false),
    (Tool::Link, true),
    (Tool::ZoomOut, false),
    (Tool::ZoomIn, false),
    (Tool::ZoomFit, true),
    (Tool::Marker, false),
];

/// How many pixels from a clip's edge the trim handles are.
const HANDLE: i32 = 6;

/// The pixel a frame sits at.
#[must_use]
pub(crate) fn x_of(frame: i64, scroll: i64, zoom: f64, lane_left: i32) -> i32 {
    lane_left + ((frame - scroll) as f64 * zoom).round() as i32
}

/// The frame a pixel is at.
#[must_use]
pub(crate) fn frame_at(x: i32, scroll: i64, zoom: f64, lane_left: i32) -> i64 {
    let zoom = zoom.max(0.01);
    scroll + ((f64::from(x - lane_left) / zoom).round() as i64)
}

/// Rolls the view so the playhead stays inside it.
pub(crate) fn scroll_to_show(
    scroll: i64,
    playhead: i64,
    zoom: f64,
    width: i32,
    gutter: i32,
) -> i64 {
    let usable = f64::from((width - gutter).max(1));
    let zoom = zoom.max(0.05);
    let margin = 48.0 / zoom;
    let left_edge = scroll as f64 + margin;
    let right_edge = scroll as f64 + usable / zoom - margin;
    let frame = playhead as f64;
    if frame < left_edge {
        (frame - margin).max(0.0) as i64
    } else if frame > right_edge {
        (frame - (usable / zoom) + margin).max(0.0) as i64
    } else {
        scroll
    }
}

/// Paints the whole timeline.
pub(crate) fn timeline(hdc: Hdc, frame: &Frame, app: &mut App) {
    let rect = frame.timeline;
    if rect.is_empty() {
        return;
    }
    let m = app.theme.metrics;
    let split = win32::clip(hdc, rect);
    win32::fill_colour(hdc, rect, app.theme.panel);
    win32::frame(hdc, rect, &app.theme.border_brush());
    // The tool strip is painted before the palette is borrowed for the rest of the frame, so
    // the two borrows never overlap.
    tools(hdc, frame, app);
    let theme = &app.theme;

    let Some(sequence) = app.shell.sequence_entity() else {
        win32::text(
            hdc,
            Rect::new(
                frame.ruler.left + m.px(8),
                frame.ruler.top,
                frame.ruler.right,
                frame.ruler.bottom,
            ),
            text(app.shell.lang(), Key::PanelTimeline),
            theme.faint,
            &theme.body_font,
            Align::Left,
        );
        win32::unclip(hdc, split);
        return;
    };
    let timebase = sequence.timebase();
    let fps = timebase.numerator() as f64 / timebase.denominator() as f64;
    let zoom = app.shell.zoom().clamp(0.05, 400.0);
    let scroll = app.scroll;
    ruler(hdc, frame, app, timebase, fps, zoom, scroll);
    gutter(hdc, frame, app);
    lanes(hdc, frame, app, timebase, zoom, scroll);
    win32::unclip(hdc, split);
}

/// The timeline's own tool strip.
fn tools(hdc: Hdc, frame: &Frame, app: &mut App) {
    let rect = frame.timeline_tools;
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
    let name = app
        .shell
        .sequence_entity()
        .map(|sequence| sequence.name().to_owned())
        .unwrap_or_else(|| text(app.shell.lang(), Key::PanelTimeline).to_owned());
    let name_w = win32::text_width(hdc, &name, &theme.small_font) + m.px(14);
    win32::text(
        hdc,
        Rect::new(
            rect.left + m.px(8),
            rect.top,
            rect.left + 8 + name_w,
            rect.bottom,
        ),
        &name,
        theme.ink,
        &theme.small_font,
        Align::Left,
    );
    let size = (rect.height() - m.px(8)).max(m.px(14));
    let mut x = rect.left + name_w + m.px(8);
    for (tool, breaks_before) in TOOLS {
        if *breaks_before {
            win32::fill_colour(
                hdc,
                Rect::new(
                    x + m.px(2),
                    rect.top + m.px(5),
                    x + m.px(3),
                    rect.bottom - m.px(5),
                ),
                theme.border,
            );
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
        let zone = Zone::TimelineTool(*tool);
        let active = matches!(tool, Tool::Snap) && app.shell.view().snap
            || matches!(tool, Tool::Link) && app.shell.view().link;
        let usable = if matches!(tool, Tool::Marker) {
            // The model has no marker entity until M6; the button is drawn, and refuses.
            false
        } else {
            !matches!(
                tool,
                Tool::Split | Tool::TrimIn | Tool::TrimOut | Tool::Remove
            ) || app.shell.selection().is_some()
        };
        let ink = if app.pressed == Some(zone) {
            win32::rounded(hdc, item, m.radius, theme.chrome_low, None);
            theme.accent
        } else if app.hover == Some(zone) {
            win32::rounded(hdc, item, m.radius, theme.chrome_high, None);
            theme.ink
        } else if active {
            win32::rounded(
                hdc,
                item,
                m.radius,
                theme.chrome_high,
                Some(theme.accent_deep),
            );
            theme.accent
        } else if usable {
            theme.dim
        } else {
            theme.faint
        };
        glyphs::draw(hdc, item.inset(m.px(3)), tool.glyph(), ink);
        let region = Region::new(zone, item, CursorKind::Point, tool.label()).hinted(tool.hint());
        app.zones.push(if usable {
            region
        } else {
            region.refused(Key::NothingSelected)
        });
        x = item.right + m.px(2);
    }
}

/// The ruler: a tick and a timecode every step.
fn ruler(
    hdc: Hdc,
    frame: &Frame,
    app: &mut App,
    timebase: maj_time::Timebase,
    fps: f64,
    zoom: f64,
    scroll: i64,
) {
    let rect = frame.ruler;
    let lane = Rect::new(
        frame.track_area.left,
        rect.top,
        frame.track_area.right,
        rect.bottom,
    );
    if lane.is_empty() {
        return;
    }
    let theme = &app.theme;
    win32::fill_colour(hdc, rect, theme.ground);
    win32::fill_colour(
        hdc,
        Rect::new(rect.left, rect.bottom - 1, rect.right, rect.bottom),
        theme.separator,
    );
    let mode = TimecodeMode::preferred_for(timebase);
    let step = step_for(zoom, fps);
    let first = scroll - scroll.rem_euclid(step);
    let mut frame_number = first;
    while frame_number <= scroll + i64::from(lane.width()) {
        let x = x_of(frame_number, scroll, zoom, lane.left);
        if x >= lane.left && x < lane.right + 60 {
            win32::fill_colour(
                hdc,
                Rect::new(x, rect.bottom - theme.metrics.px(8), x + 1, rect.bottom - 1),
                theme.border,
            );
            if let Ok(timecode) = Timecode::from_frames(frame_number, timebase, mode) {
                win32::text(
                    hdc,
                    Rect::new(
                        x + theme.metrics.px(3),
                        rect.top,
                        (x + theme.metrics.px(80)).min(lane.right),
                        rect.bottom - 4,
                    ),
                    &timecode.to_string(),
                    theme.faint,
                    &theme.mono_font,
                    Align::Left,
                );
            }
        }
        frame_number += step;
    }
    app.zones.push(Region::new(
        Zone::Ruler,
        lane,
        CursorKind::Cross,
        Key::TransportScrub,
    ));
    // The playhead's head, which is what a person grabs to scrub.
    let x = x_of(app.shell.playhead(), scroll, zoom, lane.left);
    if x >= lane.left - 4 && x <= lane.right + 4 {
        win32::polygon(
            hdc,
            Rect::new(x - 6, rect.top + 2, x + 6, rect.top + 13),
            &[(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)],
            theme.accent,
        );
        app.zones.push(Region::new(
            Zone::Playhead,
            Rect::new(x - 6, rect.top, x + 7, rect.bottom),
            CursorKind::Horizontal,
            Key::TransportScrub,
        ));
    }
}

/// The track headers: the name, the kind, and the controls each track has.
fn gutter(hdc: Hdc, frame: &Frame, app: &mut App) {
    let rect = frame.gutter;
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    win32::fill_colour(hdc, rect, theme.chrome);
    win32::fill_colour(
        hdc,
        Rect::new(rect.right - 1, rect.top, rect.right, rect.bottom),
        theme.border,
    );
    // The tracks are read out into owned rows before anything is drawn, so the borrow of the
    // document ends here and the painter below is free to record its regions.
    let tracks: Vec<(TrackKind, bool, String, usize, i64)> = app
        .shell
        .sequence_entity()
        .map_or_else(Vec::new, |sequence| {
            sequence
                .tracks()
                .iter()
                .map(|track| {
                    (
                        track.kind(),
                        track.is_enabled(),
                        track.name().to_owned(),
                        track.clip_count(),
                        track.end_frame().unwrap_or(0),
                    )
                })
                .collect()
        });
    let row_h = app.row_height();
    for (index, (kind, enabled, name, clip_count, end_frame)) in tracks.iter().enumerate() {
        let theme = &app.theme;
        let top = frame.track_area.top + (index as i32) * (row_h + 2);
        let row = Rect::new(rect.left, top, rect.right - 1, top + row_h);
        if row.bottom > rect.bottom || row.top < rect.top {
            continue;
        }
        let colour = match kind {
            TrackKind::Audio => theme.audio_top,
            TrackKind::Video => theme.video_top,
        };
        win32::fill_colour(
            hdc,
            Rect::new(row.left, row.top, row.left + m.px(3), row.bottom),
            colour,
        );
        if app.hover == Some(Zone::TrackHeader(index)) {
            win32::fill_colour(hdc, row, theme.chrome_high);
        }
        win32::text(
            hdc,
            Rect::new(
                row.left + m.px(8),
                row.top,
                row.right - m.px(58),
                row.top + row_h / 2 + 2,
            ),
            name,
            if *enabled { theme.ink } else { theme.faint },
            &theme.body_font,
            Align::Left,
        );
        let kind_key = match kind {
            TrackKind::Video => Key::TrackKindVideo,
            TrackKind::Audio => Key::TrackKindAudio,
        };
        let facts = format!(
            "{} · {} {}f",
            text(app.shell.lang(), kind_key),
            clip_count,
            end_frame
        );
        win32::text(
            hdc,
            Rect::new(
                row.left + m.px(8),
                row.top + row_h / 2,
                row.right - m.px(58),
                row.bottom,
            ),
            &facts,
            theme.faint,
            &theme.small_font,
            Align::Left,
        );
        app.zones.push(Region::new(
            Zone::TrackHeader(index),
            row,
            CursorKind::Point,
            Key::TrackHeader,
        ));
        controls(hdc, app, row, index, *kind, *enabled);
    }
}

/// The enable, lock and mute controls of one track.
fn controls(hdc: Hdc, app: &mut App, row: Rect, index: usize, kind: TrackKind, enabled: bool) {
    let theme = &app.theme;
    let m = theme.metrics;
    let size = m.px(16);
    let mut x = row.right - m.px(4) - size * 3 - m.px(8);
    let top = row.top + (row.height() - size) / 2;
    let controls = [
        (
            TrackControl::Enable,
            if enabled { Glyph::Eye } else { Glyph::EyeOff },
            enabled,
        ),
        (TrackControl::Lock, Glyph::Unlock, true),
        (
            TrackControl::Mute,
            if kind == TrackKind::Audio {
                Glyph::Speaker
            } else {
                Glyph::Muted
            },
            true,
        ),
    ];
    for (control, glyph, on) in controls {
        let item = Rect::new(x, top, x + size, top + size);
        let zone = Zone::TrackControl {
            track: index,
            control,
        };
        // Only the eye does anything yet: the engine renders a track only when it is enabled,
        // so that switch is real. Locking needs the editorial operations to obey it (M6) and
        // mute needs an audio model (M9), so those two say whose they are.
        let usable = control == TrackControl::Enable;
        let ink = if !usable {
            theme.faint
        } else if !on {
            theme.error
        } else if app.hover == Some(zone) {
            theme.accent
        } else {
            theme.dim
        };
        if usable && app.hover == Some(zone) {
            win32::rounded(hdc, item, m.radius, theme.chrome_high, None);
        }
        glyphs::draw(hdc, item.inset(m.px(2)), glyph, ink);
        let label = match control {
            TrackControl::Enable => Key::TrackEnable,
            TrackControl::Lock => Key::TrackLock,
            TrackControl::Mute => Key::TrackMute,
        };
        let region =
            Region::new(zone, item, CursorKind::Point, label).hinted(Some(match control {
                TrackControl::Enable => "alt+e",
                TrackControl::Lock => "",
                TrackControl::Mute => "",
            }));
        let region = if usable {
            region
        } else {
            region.refused(match control {
                TrackControl::Lock => Key::TrackLockOwner,
                _ => Key::TrackMuteOwner,
            })
        };
        app.zones.push(region);
        x = item.right;
    }
}

/// The lanes: one row per track, with its clips.
fn lanes(
    hdc: Hdc,
    frame: &Frame,
    app: &mut App,
    timebase: maj_time::Timebase,
    zoom: f64,
    scroll: i64,
) {
    let area = frame.track_area;
    if area.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    let row_h = app.row_height();
    let selected = app.shell.selection();
    // The selection's keys, read once: the marks are drawn clip by clip, and asking the document
    // the same question once per clip would be the same answer more often.
    let keys = app.shell.selected_keyframes();
    let Some(sequence) = app.shell.sequence_entity() else {
        return;
    };
    win32::fill_colour(hdc, area, theme.ground);

    for (index, track) in sequence.tracks().iter().enumerate() {
        let top = area.top + (index as i32) * (row_h + 2);
        let row = Rect::new(area.left, top, area.right, top + row_h);
        if row.top > area.bottom {
            break;
        }
        if index % 2 == 1 {
            win32::fill_colour(hdc, row, theme.panel);
        }
        app.zones.push(Region::new(
            Zone::Timeline,
            row,
            CursorKind::Arrow,
            Key::PanelTimeline,
        ));
        for clip in track.clips() {
            let start = clip.timeline_start();
            let length = clip.timeline_length();
            let left = x_of(start, scroll, zoom, area.left) + 1;
            let right = x_of(start + length, scroll, zoom, area.left) - 1;
            if right < area.left || left > area.right {
                continue;
            }
            let body = Rect::new(
                left.max(area.left),
                row.top + 2,
                right.min(area.right),
                row.bottom - 2,
            );
            if body.width() < 3 {
                continue;
            }
            let chosen = selected == Some(clip.id());
            let (fill, band) = match track.kind() {
                TrackKind::Audio => (theme.audio, theme.audio_top),
                TrackKind::Video => (theme.video, theme.video_top),
            };
            if !clip.is_enabled() {
                win32::rounded(
                    hdc,
                    body,
                    m.radius,
                    theme.off,
                    Some(if chosen { theme.accent } else { theme.border }),
                );
            } else {
                win32::rounded(
                    hdc,
                    body,
                    m.radius,
                    fill,
                    Some(if chosen { theme.accent } else { theme.border }),
                );
                win32::fill_colour(
                    hdc,
                    Rect::new(
                        body.left + 1,
                        body.top + 1,
                        body.right - 1,
                        body.top + m.px(4),
                    ),
                    band,
                );
            }
            if chosen {
                // A wash over whatever the clip already is, so it stays legible as a video or an
                // audio clip while it is the selection.
                theme.wash(hdc, body, theme.selection, 48);
            }
            if track.kind() == TrackKind::Audio {
                hatch(hdc, body, theme.border);
                if body.width() > m.px(120) {
                    win32::text(
                        hdc,
                        Rect::new(
                            body.left + m.px(6),
                            body.top + (body.height() - m.row_h) / 2,
                            body.right - m.px(4),
                            body.top + (body.height() + m.row_h) / 2,
                        ),
                        text(app.shell.lang(), Key::AudioNotBuiltShort),
                        theme.faint,
                        &theme.small_font,
                        Align::Left,
                    );
                }
            } else if body.width() > m.px(34) {
                win32::text(
                    hdc,
                    Rect::new(
                        body.left + m.px(6),
                        body.top + m.px(3),
                        body.right - m.px(4),
                        body.top + m.px(17),
                    ),
                    clip.name(),
                    if clip.is_enabled() {
                        theme.ink
                    } else {
                        theme.faint
                    },
                    &theme.small_font,
                    Align::Left,
                );
                if body.width() > m.px(110) {
                    if let Ok(timecode) = Timecode::from_frames(
                        length,
                        timebase,
                        TimecodeMode::preferred_for(timebase),
                    ) {
                        win32::text(
                            hdc,
                            Rect::new(
                                body.left + m.px(6),
                                body.bottom - m.px(15),
                                body.right - m.px(4),
                                body.bottom - 2,
                            ),
                            &timecode.to_string(),
                            theme.faint,
                            &theme.mono_font,
                            Align::Left,
                        );
                    }
                }
            }
            if chosen {
                handles(hdc, app, body);
                // Every key the clip holds, on the clip's own foot: the frames where the picture
                // starts to move. Only the selection's keys are drawn — the inspector and the
                // keyboard both speak about one clip at a time, and a mark nobody can act on is
                // noise rather than information.
                for (_, at) in &keys {
                    let x = x_of(*at, scroll, zoom, area.left);
                    if x <= body.left || x >= body.right {
                        continue;
                    }
                    glyphs::draw(
                        hdc,
                        Rect::new(
                            x - m.px(4),
                            body.bottom - m.px(9),
                            x + m.px(4),
                            body.bottom - m.px(1),
                        ),
                        Glyph::Keyframe,
                        theme.accent,
                    );
                }
            }
            // The whole body moves the clip; the edges trim it. The edges are registered last so
            // that they win where they overlap the body.
            app.zones.push(Region::new(
                Zone::Clip {
                    clip: clip.id(),
                    part: ClipPart::Body,
                },
                body,
                CursorKind::Move,
                Key::ClipBody,
            ));
            let head = Rect::new(
                body.left,
                body.top,
                (body.left + HANDLE.min(body.width() / 3)).max(body.left + 2),
                body.bottom,
            );
            let tail = Rect::new(
                (body.right - HANDLE.min(body.width() / 3)).min(body.right - 2),
                body.top,
                body.right,
                body.bottom,
            );
            app.zones.push(Region::new(
                Zone::Clip {
                    clip: clip.id(),
                    part: ClipPart::Head,
                },
                head,
                CursorKind::Horizontal,
                Key::ClipTrimHead,
            ));
            app.zones.push(Region::new(
                Zone::Clip {
                    clip: clip.id(),
                    part: ClipPart::Tail,
                },
                tail,
                CursorKind::Horizontal,
                Key::ClipTrimTail,
            ));
        }

        // A tile being carried over the timeline says where it would land: a wash across the row
        // it would join, and a line at the frame it would start on. Drawn after the row's clips,
        // so it reads as a target rather than as one more clip.
        if app.dragging.is_some()
            && let Some((drop_track, drop_frame)) = app.drop_target
            && drop_track == track.id()
        {
            let x = x_of(drop_frame, scroll, zoom, area.left);
            if x >= area.left - 1 && x <= area.right + 1 {
                theme.wash(hdc, row, theme.accent, 30);
                win32::fill_colour(
                    hdc,
                    Rect::new(x - m.px(1), row.top, x + m.px(1), row.bottom),
                    theme.accent,
                );
                glyphs::draw(
                    hdc,
                    Rect::new(
                        x + m.px(2),
                        row.top + m.px(2),
                        x + m.px(14),
                        row.top + m.px(14),
                    ),
                    Glyph::Plus,
                    theme.accent,
                );
            }
        }
    }

    // The playhead, over everything.
    let x = x_of(app.shell.playhead(), scroll, zoom, area.left);
    if x >= area.left - 1 && x <= area.right + 1 {
        win32::line(
            hdc,
            (x, frame.ruler.top + 6),
            (x, area.bottom),
            &app.theme.playhead_pen(),
        );
    }
}

/// The two grab handles a selected clip shows.
fn handles(hdc: Hdc, app: &App, body: Rect) {
    let theme = &app.theme;
    let width = HANDLE.min(body.width() / 3).max(2);
    win32::fill_colour(
        hdc,
        Rect::new(body.left, body.top, body.left + width, body.bottom),
        theme.accent,
    );
    win32::fill_colour(
        hdc,
        Rect::new(body.right - width, body.top, body.right, body.bottom),
        theme.accent,
    );
}

/// The diagonal hatch that says a lane has no samples behind it.
fn hatch(hdc: Hdc, rect: Rect, colour: crate::win32::Colour) {
    let step = 8;
    let mut offset = -rect.height();
    while offset < rect.width() {
        let from = (rect.left + offset, rect.bottom);
        let to = (rect.left + offset + rect.height(), rect.top);
        win32::line(hdc, from, to, &crate::win32::Pen::solid(colour, 1));
        offset += step;
    }
}

/// A readable tick spacing: the smallest rung of the ladder that leaves room for a label.
fn step_for(zoom: f64, frames_per_second: f64) -> i64 {
    let second = frames_per_second.max(1.0);
    let ladder = [
        1.0,
        2.0,
        5.0,
        10.0,
        second,
        second * 2.0,
        second * 5.0,
        second * 10.0,
        second * 30.0,
        second * 60.0,
        second * 300.0,
        second * 600.0,
    ];
    for candidate in ladder {
        if candidate * zoom >= 96.0 {
            return candidate.round().max(1.0) as i64;
        }
    }
    (second * 3600.0) as i64
}

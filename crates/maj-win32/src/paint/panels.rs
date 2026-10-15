//! The panels: the pool, the log, the inspector, the mixer, and the media page's grid.
//!
//! Each panel is headed by tabs rather than by a bare title, so the left column and the right
//! column can each hold two panels without a divider between them, which is what the reference
//! does and what keeps the chrome thin.

use maj_core::{EffectKind, TransitionKind};
use maj_render::{PictureResolver, TitleAlign, TitleFace};
use maj_ui::{AnimChannel, Key, text};

use crate::app::App;
use crate::glyphs::{self, Glyph};
use crate::layout::Frame;
use crate::widget::{Region, Tab, Zone};
use crate::win32::{self, Align, Colour, CursorKind, Hdc, Rect};

/// Draws a panel with tabs at its head and answers with the body's rectangle.
///
/// The tabs wrap onto as many rows as they need. A column that can be dragged narrow must
/// still reach every panel in it, and a tab that silently falls off the end of a single row is
/// a panel a person cannot open — so the strip grows downwards instead, and the body is what is
/// left below it.
fn titled(hdc: Hdc, rect: Rect, app: &mut App, tabs: &[(Tab, Key)], active: Tab) -> Rect {
    if rect.is_empty() {
        return rect;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    // The tabs are laid out before anything is drawn, because how many rows they need is what
    // decides how tall the heading is.
    let mut placed: Vec<(Tab, Key, Rect)> = Vec::with_capacity(tabs.len());
    let mut row = 0_i32;
    let mut x = rect.left + m.px(2);
    for (tab, key) in tabs {
        let width =
            win32::text_width(hdc, text(app.shell.lang(), *key), &theme.body_font) + m.px(18);
        if x > rect.left + m.px(2) && x + width > rect.right - m.px(2) {
            row += 1;
            x = rect.left + m.px(2);
        }
        let top = rect.top + row * m.heading_h;
        placed.push((
            *tab,
            *key,
            Rect::new(x, top + m.px(2), x + width, top + m.heading_h - m.px(2)),
        ));
        x += width + m.px(2);
    }
    let heading = Rect::new(
        rect.left,
        rect.top,
        rect.right,
        rect.top + (row + 1) * m.heading_h,
    );
    win32::gradient(hdc, heading, theme.chrome, theme.panel);
    win32::frame(hdc, rect, &theme.border_brush());
    win32::fill_colour(
        hdc,
        Rect::new(rect.left, heading.bottom, rect.right, heading.bottom + 1),
        theme.separator,
    );
    for (tab, key, item) in placed {
        let chosen = tab == active;
        let zone = Zone::Tab(tab);
        if chosen {
            win32::fill_colour(
                hdc,
                Rect::new(
                    item.left + m.px(4),
                    item.bottom,
                    item.right - m.px(4),
                    item.bottom + m.px(1),
                ),
                theme.accent,
            );
        } else if app.hover == Some(zone) {
            win32::rounded(hdc, item, m.radius, theme.chrome_high, None);
        }
        win32::text(
            hdc,
            item,
            text(app.shell.lang(), key),
            if chosen { theme.accent } else { theme.dim },
            &theme.body_font,
            Align::Centre,
        );
        app.zones
            .push(Region::new(zone, item, CursorKind::Point, key));
    }
    Rect::new(
        rect.left + 1,
        heading.bottom + 1,
        rect.right - 1,
        rect.bottom - 1,
    )
}

/// One line of a list: a left label, a right value, and a state.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    /// What it says.
    pub(crate) left: String,
    /// Its value.
    pub(crate) right: String,
    /// The ink when the row is not otherwise marked.
    pub(crate) colour: Colour,
    /// Whether it is the selected row.
    pub(crate) chosen: bool,
}

/// Draws rows into a list area, clipped to what fits.
fn rows(
    hdc: Hdc,
    rect: Rect,
    values: &[Row],
    app: &mut App,
    offset: usize,
    zone_of: impl Fn(usize) -> Option<Zone>,
) {
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    let capacity = (rect.height() / m.row_h).max(0) as usize;
    for (line, row) in values.iter().take(capacity).enumerate() {
        let top = rect.top + (line as i32) * m.row_h;
        let item = Rect::new(rect.left, top, rect.right, top + m.row_h);
        let zone = zone_of(offset + line);
        if row.chosen {
            win32::fill_colour(hdc, item, theme.chrome_high);
            win32::fill_colour(
                hdc,
                Rect::new(item.left, item.top, item.left + m.px(2), item.bottom),
                theme.accent,
            );
        } else if zone.is_some() && app.hover == zone {
            win32::fill_colour(hdc, item, theme.panel_alt);
        }
        win32::text(
            hdc,
            Rect::new(
                item.left + m.px(6),
                item.top,
                item.right - m.px(96),
                item.bottom,
            ),
            &row.left,
            row.colour,
            &theme.body_font,
            Align::Left,
        );
        if !row.right.is_empty() {
            win32::text(
                hdc,
                Rect::new(
                    item.right - m.px(92),
                    item.top,
                    item.right - m.px(6),
                    item.bottom,
                ),
                &row.right,
                theme.faint,
                &theme.mono_font,
                Align::Right,
            );
        }
        if let Some(zone) = zone {
            app.zones.push(Region::new(
                zone,
                item,
                CursorKind::Point,
                Key::PanelHistory,
            ));
        }
    }
}

/// The left column: the pool, the title studio, or the log.
pub(crate) fn left_column(hdc: Hdc, rect: Rect, app: &mut App) {
    let active = app.left_tab;
    let body = titled(
        hdc,
        rect,
        app,
        &[
            (Tab::Media, Key::PanelMedia),
            (Tab::Titles, Key::TabTitles),
            (Tab::History, Key::PanelHistory),
        ],
        active,
    );
    match active {
        Tab::History => log_body(hdc, body, app),
        Tab::Titles => studio_body(hdc, body, app),
        _ => pool_body(hdc, body, app),
    }
}

/// The right column: the inspector, the two browsers and the mixer.
pub(crate) fn right_column(hdc: Hdc, rect: Rect, app: &mut App) {
    let active = app.right_tab;
    let body = titled(
        hdc,
        rect,
        app,
        &[
            (Tab::Inspector, Key::PanelInspector),
            (Tab::EffectList, Key::BrowserEffects),
            (Tab::TransitionList, Key::BrowserTransitions),
            (Tab::Mixer, Key::PanelMixer),
        ],
        active,
    );
    match active {
        Tab::Mixer => mixer_body(hdc, body, app),
        Tab::EffectList => effect_list_body(hdc, body, app),
        Tab::TransitionList => transition_list_body(hdc, body, app),
        _ => inspector_body(hdc, body, app),
    }
}

/// The resolver the pool asks about a URI, which is the one the renderer uses.
///
/// Asking it rather than matching on the extension here is what keeps the pool's word and the
/// render manifest's word the same word.
const RESOLVER: maj_render::WorkspaceResolver = maj_render::WorkspaceResolver::new();

/// Kinds a tile can draw for itself: the pictures the engine builds out of the URI, with no
/// file behind them.
///
/// A still on disk is deliberately absent. Drawing one means decoding it, and decoding it on
/// every paint of every tile is engine work on the path that is supposed to be pixels — the
/// same reason the viewer renders from the message loop and not from `draw`. When the cache
/// exists (M10) this is the list that grows.
const DRAWABLE: [&str; 3] = ["solid", "generated", "title"];

/// A tile-sized picture of a URI, or `None` when drawing one would cost a file read.
///
/// The samples come from the same [`maj_render::PictureSource`] the renderer would use, so a
/// tile is a small version of what the viewer shows rather than a second idea of it. Code
/// values are what a screen wants, and the source hands back scene-linear light, so each
/// sample goes through the same transfer the viewer encodes with.
fn thumbnail(uri: &str, width: u32, height: u32) -> Option<Vec<u8>> {
    if !DRAWABLE.contains(&RESOLVER.kind(uri)) {
        return None;
    }
    let source = RESOLVER.resolve(uri, (width, height), 0).ok()?;
    let (source_w, source_h) = (f32::from(source.width() as u16), source.height() as f32);
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        for column in 0..width {
            // Sample at the centre of each tile pixel, mapped across the source.
            let x = (column as f32 + 0.5) / width as f32 * source_w;
            let y = (row as f32 + 0.5) / height as f32 * source_h;
            let pixel = source.sample(x, y);
            let code = |channel: f32| -> u8 {
                maj_color::TransferFunction::Srgb
                    .encode(f64::from(channel.clamp(0.0, 1.0)))
                    .mul_add(255.0, 0.5) as u8
            };
            // BGRA, top-down.
            pixels.push(code(pixel[2]));
            pixels.push(code(pixel[1]));
            pixels.push(code(pixel[0]));
            pixels.push(255);
        }
    }
    Some(pixels)
}

/// The media pool: one tile per asset, with the honest note about decoding.
pub(crate) fn pool_body(hdc: Hdc, rect: Rect, app: &mut App) {
    if rect.is_empty() {
        return;
    }
    let m = app.theme.metrics;
    let assets: Vec<(String, String, bool, maj_core::MediaAssetId, String)> = app
        .shell
        .project()
        .assets()
        .iter()
        .map(|asset| {
            (
                asset.name().to_owned(),
                format!("{}f · {}", asset.duration_frames(), asset.timebase()),
                asset.has_video(),
                asset.id(),
                asset.uri().to_owned(),
            )
        })
        .collect();

    // Every tile's well is the same size, so the thumbnails are made — or found already made
    // — once, before the paint starts and before the theme is borrowed. Sampling one costs a
    // deterministic transfer per channel per pixel, which is a fine price to pay once and an
    // impossible one to pay on every paint: see `App::thumbnails`.
    let well_size = (
        (m.tile_w - 2 * m.px(3)).max(1) as u32,
        (m.tile_h - m.px(24) - m.px(3)).max(1) as u32,
    );
    for (_, _, _, _, uri) in &assets {
        app.thumbnails
            .entry((uri.clone(), well_size.0, well_size.1))
            .or_insert_with(|| thumbnail(uri, well_size.0, well_size.1));
    }

    let theme = &app.theme;
    let selected = app.shell.media_selection();
    // The grid stops above the footer: the button that brings a picture in from disk and the
    // note that says what this build decodes are not things a tile may grow into.
    let grid = Rect::new(
        rect.left,
        rect.top,
        rect.right,
        (rect.bottom - m.px(62)).max(rect.top),
    );
    let columns = ((grid.width() - m.pad) / (m.tile_w + m.pad)).max(1);
    for (index, (name, detail, has_video, id, uri)) in assets.iter().enumerate() {
        let column = index as i32 % columns;
        let line = index as i32 / columns;
        let left = grid.left + m.pad + column * (m.tile_w + m.pad);
        let top = grid.top + m.pad + line * (m.tile_h + m.pad);
        let tile = Rect::new(left, top, left + m.tile_w, top + m.tile_h);
        if tile.bottom > grid.bottom {
            break;
        }
        let zone = Zone::MediaTile(index);
        let chosen = selected == Some(*id);
        let hovered = app.hover == Some(zone);
        win32::rounded(
            hdc,
            tile,
            m.radius,
            if hovered {
                theme.chrome_high
            } else {
                theme.panel_alt
            },
            Some(if chosen { theme.accent } else { theme.border }),
        );
        let well = Rect::new(
            tile.left + m.px(3),
            tile.top + m.px(3),
            tile.right - m.px(3),
            tile.bottom - m.px(24),
        );
        win32::fill_colour(hdc, well, theme.well);
        // A tile shows the picture it *is*. For the forms the engine builds from the URI
        // itself — a flat field, a chart, a title — that is a real thumbnail, made once above.
        // For anything else it is the kind of media and the word the engine itself would put
        // in a render manifest, so the pool cannot claim "not decoded" about a still it reads
        // perfectly well. A thumbnail of a file on disk would mean decoding it; that waits
        // for the cache (M10).
        match app
            .thumbnails
            .get(&(uri.clone(), well_size.0, well_size.1))
            .and_then(Option::as_ref)
        {
            Some(pixels) => win32::blit_smooth(hdc, well, well_size.0, well_size.1, pixels),
            None => {
                let picture = if *has_video { Glyph::Film } else { Glyph::Wave };
                glyphs::draw(
                    hdc,
                    Rect::new(
                        well.left + well.width() / 3,
                        well.top + well.height() / 5,
                        well.right - well.width() / 3,
                        well.bottom - well.height() / 5,
                    ),
                    picture,
                    theme.border,
                );
                let kind = RESOLVER.kind(uri);
                let label = if kind == maj_render::Placeholder::UNDECODABLE {
                    text(app.shell.lang(), Key::MediaUndecodedShort)
                } else {
                    kind
                };
                win32::text(
                    hdc,
                    Rect::new(
                        well.left,
                        well.bottom - m.px(14),
                        well.right,
                        well.bottom - m.px(2),
                    ),
                    label,
                    theme.faint,
                    &theme.small_font,
                    Align::Centre,
                );
            }
        }
        win32::text(
            hdc,
            Rect::new(
                tile.left + m.px(5),
                well.bottom,
                tile.right - m.px(5),
                well.bottom + m.px(11),
            ),
            name,
            if chosen { theme.accent } else { theme.ink },
            &theme.small_font,
            Align::Left,
        );
        win32::text(
            hdc,
            Rect::new(
                tile.left + m.px(5),
                well.bottom + m.px(11),
                tile.right - m.px(5),
                tile.bottom - m.px(2),
            ),
            detail,
            theme.faint,
            &theme.small_font,
            Align::Left,
        );
        app.zones.push(
            Region::new(zone, tile, CursorKind::Point, Key::MediaInsert).hinted(Some("doble clic")),
        );
    }
    // The button that brings a picture in from disk, and the honest note about what this build
    // can decode underneath it.
    let button = Rect::new(
        rect.left + m.px(4),
        (rect.bottom - m.px(58)).max(rect.top),
        rect.right - m.px(4),
        (rect.bottom - m.px(34)).max(rect.top + m.px(20)),
    );
    let hovered = app.hover == Some(Zone::ImportMedia);
    panel_button(
        hdc,
        button,
        app,
        text(app.shell.lang(), Key::MediaImport),
        hovered,
        true,
    );
    app.zones.push(
        Region::new(
            Zone::ImportMedia,
            button,
            CursorKind::Point,
            Key::MediaImport,
        )
        .hinted(Some("png / ppm")),
    );
    let footer = Rect::new(
        rect.left + m.px(4),
        (rect.bottom - m.px(30)).max(rect.top),
        rect.right - m.px(4),
        rect.bottom - m.px(4),
    );
    win32::text_block(
        hdc,
        footer,
        text(app.shell.lang(), Key::MediaUndecoded),
        theme.faint,
        &theme.small_font,
    );
}

/// A button inside a panel: a rounded box with a label, which the caller gives a zone.
fn panel_button(hdc: Hdc, rect: Rect, app: &App, label: &str, chosen: bool, usable: bool) {
    let theme = &app.theme;
    let m = theme.metrics;
    win32::rounded(
        hdc,
        rect,
        m.radius,
        if chosen {
            theme.accent
        } else if !usable {
            theme.panel
        } else {
            theme.panel_alt
        },
        Some(if chosen { theme.accent } else { theme.border }),
    );
    win32::text(
        hdc,
        rect,
        label,
        if !usable {
            theme.faint
        } else if chosen {
            theme.panel
        } else {
            theme.ink
        },
        &theme.body_font,
        Align::Centre,
    );
}

/// The border a form field is drawn with: the accent when it holds the keyboard.
fn field_border(app: &App, focused: bool) -> Colour {
    if focused {
        app.theme.accent
    } else {
        app.theme.border
    }
}

/// The title studio: the words, the face, the cap height and the alignment.
///
/// It is a form rather than a list, because a title is the one thing a person *composes* here
/// instead of picking: the words are theirs. What the form produces is a media asset whose URI
/// names the face, the cap height, the colour, the alignment and the words, so the whole of a
/// title is visible in the log and a document stays a function of its commands. The last line
/// shows that URI, which is the seam between this form and the renderer.
pub(crate) fn studio_body(hdc: Hdc, rect: Rect, app: &mut App) {
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    let lang = app.shell.lang();
    let left = rect.left + m.pad;
    let right = rect.right - m.pad;
    let mut y = rect.top + m.pad;

    // The words.
    win32::text(
        hdc,
        Rect::new(left, y, right, y + m.px(11)),
        text(lang, Key::TitleWords),
        theme.dim,
        &theme.small_font,
        Align::Left,
    );
    y += m.px(12);
    let field = Rect::new(left, y, right, y + m.px(22));
    win32::rounded(
        hdc,
        field,
        m.radius,
        theme.well,
        Some(field_border(app, app.title_focus)),
    );
    let words = app.shell.studio().text.clone();
    let inner = Rect::new(
        field.left + m.px(5),
        field.top,
        field.right - m.px(5),
        field.bottom,
    );
    if words.is_empty() && !app.title_focus {
        win32::text(
            hdc,
            inner,
            text(lang, Key::TitleEmpty),
            theme.faint,
            &theme.small_font,
            Align::Left,
        );
    } else {
        win32::text(hdc, inner, &words, theme.ink, &theme.body_font, Align::Left);
        if app.title_focus {
            let caret = (inner.left + win32::text_width(hdc, &words, &theme.body_font) + 1)
                .min(inner.right - 1);
            win32::fill_colour(
                hdc,
                Rect::new(
                    caret,
                    field.top + m.px(3),
                    caret + 1,
                    field.bottom - m.px(3),
                ),
                theme.accent,
            );
        }
    }
    app.zones.push(
        Region::new(Zone::TitleField, field, CursorKind::Text, Key::TitleWords)
            .hinted(Some("escribe")),
    );
    y = field.bottom + m.px(8);

    // The face.
    win32::text(
        hdc,
        Rect::new(left, y, right, y + m.px(11)),
        text(lang, Key::TitleFace),
        theme.dim,
        &theme.small_font,
        Align::Left,
    );
    y += m.px(12);
    let gap = m.px(4);
    let each = ((right - left) - gap * 3) / 4;
    let faces = TitleFace::ALL;
    for (index, face) in faces.iter().enumerate() {
        let item = Rect::new(
            left + (each + gap) * index as i32,
            y,
            left + each * (index as i32 + 1) + gap * index as i32,
            y + m.px(20),
        );
        let chosen = app.shell.studio().face == *face;
        panel_button(hdc, item, app, text(lang, face_label(*face)), chosen, true);
        app.zones.push(Region::new(
            Zone::TitleFace(index),
            item,
            CursorKind::Point,
            face_label(*face),
        ));
    }
    y += m.px(26);

    // The cap height, as a number and two buttons rather than as a field: the value is in
    // pixels of the frame, and a step is easier to press than a number is to type.
    win32::text(
        hdc,
        Rect::new(left, y, right, y + m.px(11)),
        text(lang, Key::TitleCap),
        theme.dim,
        &theme.small_font,
        Align::Left,
    );
    y += m.px(12);
    let step = Rect::new(left, y, left + m.px(20), y + m.px(20));
    panel_button(hdc, step, app, "-", false, true);
    app.zones.push(Region::new(
        Zone::TitleSize(-1),
        step,
        CursorKind::Point,
        Key::TitleCap,
    ));
    let more = Rect::new(right - m.px(20), y, right, y + m.px(20));
    panel_button(hdc, more, app, "+", false, true);
    app.zones.push(Region::new(
        Zone::TitleSize(1),
        more,
        CursorKind::Point,
        Key::TitleCap,
    ));
    let value = format!("{:.0} px", app.shell.title_size());
    win32::text(
        hdc,
        Rect::new(step.right, y, more.left, y + m.px(20)),
        &value,
        theme.ink,
        &theme.mono_font,
        Align::Centre,
    );
    y += m.px(26);

    // The alignment.
    win32::text(
        hdc,
        Rect::new(left, y, right, y + m.px(11)),
        text(lang, Key::TitleAlign),
        theme.dim,
        &theme.small_font,
        Align::Left,
    );
    y += m.px(12);
    let align_each = ((right - left) - gap * 2) / 3;
    for (index, align) in TitleAlign::ALL.iter().enumerate() {
        let item = Rect::new(
            left + (align_each + gap) * index as i32,
            y,
            left + align_each * (index as i32 + 1) + gap * index as i32,
            y + m.px(20),
        );
        let chosen = app.shell.studio().align == *align;
        panel_button(
            hdc,
            item,
            app,
            text(lang, align_label(*align)),
            chosen,
            true,
        );
        app.zones.push(Region::new(
            Zone::TitleAlign(index),
            item,
            CursorKind::Point,
            align_label(*align),
        ));
    }
    y += m.px(26);

    // The button that sets it.
    let add = Rect::new(left, y, right, y + m.px(24));
    let ready = !words.trim().is_empty();
    panel_button(hdc, add, app, text(lang, Key::TitleAdd), ready, ready);
    let region = Region::new(Zone::TitleAdd, add, CursorKind::Point, Key::TitleAdd);
    app.zones.push(if ready {
        region
    } else {
        region.refused(Key::TitleEmpty)
    });
    y = add.bottom + m.px(8);

    // What the renderer will be told, and what the faces are.
    win32::text(
        hdc,
        Rect::new(left, y, right, y + m.px(11)),
        text(lang, Key::TitleUri),
        theme.dim,
        &theme.small_font,
        Align::Left,
    );
    y += m.px(12);
    win32::text_block(
        hdc,
        Rect::new(left, y, right, rect.bottom - m.px(4)),
        &app.shell.title_uri(),
        theme.faint,
        &theme.mono_font,
    );
}

/// A browser: every kind the engine has, one per row, clicked to add it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Browser {
    /// Every effect, added to the selection.
    Effects,
    /// Every transition, added at the playhead.
    Transitions,
}

impl Browser {
    /// The rows: what each kind is called, its stable name, and whether it can be added now.
    fn rows(self) -> Vec<(Key, &'static str, bool)> {
        match self {
            Self::Effects => EffectKind::ALL
                .into_iter()
                .map(|kind| (effect_label(kind), kind.key(), true))
                .collect(),
            Self::Transitions => TransitionKind::ALL
                .into_iter()
                .map(|kind| (transition_label(kind), kind.key(), true))
                .collect(),
        }
    }

    /// What clicking a row does, in one sentence.
    const fn note(self) -> Key {
        match self {
            Self::Effects => Key::EffectListNote,
            Self::Transitions => Key::TransitionListNote,
        }
    }

    /// The zone of one row.
    const fn zone(self, index: usize) -> Zone {
        match self {
            Self::Effects => Zone::EffectRow(index),
            Self::Transitions => Zone::TransitionRow(index),
        }
    }

    /// Whether a kind can be added at all right now.
    ///
    /// An effect needs a clip to go on; a transition needs a sequence, and whether it fits is
    /// the engine's decision rather than the browser's, so the row is offered and the refusal
    /// is the engine's own sentence.
    fn usable(self, app: &App) -> bool {
        match self {
            Self::Effects => app.shell.selection().is_some(),
            Self::Transitions => app.shell.sequence_entity().is_some(),
        }
    }
}

/// Draws a browser.
fn browser_body(hdc: Hdc, rect: Rect, app: &mut App, browser: Browser) {
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    let lang = app.shell.lang();
    let note_h = m.px(40);
    let list = Rect::new(
        rect.left,
        rect.top,
        rect.right,
        (rect.bottom - note_h).max(rect.top),
    );
    let usable = browser.usable(app);
    let capacity = (list.height() / m.row_h).max(0) as usize;
    for (index, (label, key, _)) in browser.rows().iter().enumerate().take(capacity) {
        let top = list.top + (index as i32) * m.row_h;
        let item = Rect::new(list.left, top, list.right, top + m.row_h);
        let zone = browser.zone(index);
        if app.hover == Some(zone) && usable {
            win32::fill_colour(hdc, item, theme.panel_alt);
        }
        win32::text(
            hdc,
            Rect::new(
                item.left + m.px(6),
                item.top,
                item.right - m.px(96),
                item.bottom,
            ),
            text(lang, *label),
            if usable { theme.ink } else { theme.faint },
            &theme.body_font,
            Align::Left,
        );
        win32::text(
            hdc,
            Rect::new(
                item.right - m.px(92),
                item.top,
                item.right - m.px(6),
                item.bottom,
            ),
            key,
            theme.faint,
            &theme.mono_font,
            Align::Right,
        );
        let region = Region::new(zone, item, CursorKind::Point, *label);
        app.zones.push(if usable {
            region
        } else {
            region.refused(Key::NothingSelected)
        });
    }
    win32::text_block(
        hdc,
        Rect::new(
            rect.left + m.px(6),
            rect.bottom - note_h,
            rect.right - m.px(6),
            rect.bottom - m.px(4),
        ),
        text(lang, browser.note()),
        theme.faint,
        &theme.small_font,
    );
}

/// Every effect, to add to the selection.
pub(crate) fn effect_list_body(hdc: Hdc, rect: Rect, app: &mut App) {
    browser_body(hdc, rect, app, Browser::Effects);
}

/// Every transition, to add at the playhead.
pub(crate) fn transition_list_body(hdc: Hdc, rect: Rect, app: &mut App) {
    browser_body(hdc, rect, app, Browser::Transitions);
}

/// What an effect is called in this interface.
pub(crate) const fn effect_label(kind: EffectKind) -> Key {
    match kind {
        EffectKind::Exposure => Key::EffectExposure,
        EffectKind::Brightness => Key::EffectBrightness,
        EffectKind::Contrast => Key::EffectContrast,
        EffectKind::Saturation => Key::EffectSaturation,
        EffectKind::Gamma => Key::EffectGamma,
        EffectKind::HueRotate => Key::EffectHueRotate,
        EffectKind::Blur => Key::EffectBlur,
        EffectKind::Sharpen => Key::EffectSharpen,
        EffectKind::Vignette => Key::EffectVignette,
        EffectKind::Grain => Key::EffectGrain,
    }
}

/// What a transition is called in this interface.
pub(crate) const fn transition_label(kind: TransitionKind) -> Key {
    match kind {
        TransitionKind::CrossDissolve => Key::TransitionCrossDissolve,
        TransitionKind::DipToBlack => Key::TransitionDipToBlack,
        TransitionKind::DipToWhite => Key::TransitionDipToWhite,
        TransitionKind::WipeLeft => Key::TransitionWipeLeft,
        TransitionKind::WipeRight => Key::TransitionWipeRight,
        TransitionKind::WipeUp => Key::TransitionWipeUp,
        TransitionKind::WipeDown => Key::TransitionWipeDown,
        TransitionKind::Circle => Key::TransitionCircle,
    }
}

/// What a face is called in this interface.
pub(crate) const fn face_label(face: TitleFace) -> Key {
    match face {
        TitleFace::Sans => Key::FaceSans,
        TitleFace::Serif => Key::FaceSerif,
        TitleFace::Mono => Key::FaceMono,
        TitleFace::Condensed => Key::FaceCondensed,
    }
}

/// What an alignment is called in this interface.
pub(crate) const fn align_label(align: TitleAlign) -> Key {
    match align {
        TitleAlign::Left => Key::AlignLeft,
        TitleAlign::Centre => Key::AlignCentre,
        TitleAlign::Right => Key::AlignRight,
    }
}

/// The command log: what was applied, by whom, and the shell's own notes on top of it.
pub(crate) fn log_body(hdc: Hdc, rect: Rect, app: &mut App) {
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let mut values: Vec<Row> = Vec::new();
    for note in app.shell.notes().iter().rev() {
        values.push(Row {
            left: note.text.clone(),
            right: String::new(),
            colour: if note.kind == maj_ui::NoteKind::Error {
                theme.error
            } else {
                theme.accent
            },
            chosen: false,
        });
    }
    for (index, entry) in app.shell.project().log().iter().enumerate().rev() {
        values.push(Row {
            left: format!("{:>4}  {}", index + 1, maj_ui::kind_of(entry.command())),
            right: entry.author().to_owned(),
            colour: theme.ink,
            chosen: false,
        });
    }
    let scroll = app.log_scroll.min(values.len().saturating_sub(1));
    let shown: Vec<Row> = values.into_iter().skip(scroll).collect();
    rows(hdc, rect.inset(2), &shown, app, scroll, |index| {
        Some(Zone::LogRow(index))
    });
}

/// The inspector: the selected clip, in full.
pub(crate) fn inspector_body(hdc: Hdc, rect: Rect, app: &mut App) {
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let Some(clip) = app.shell.selected_clip() else {
        win32::text_block(
            hdc,
            rect.inset(theme.metrics.px(8)),
            text(app.shell.lang(), Key::InspectorEmpty),
            theme.faint,
            &theme.small_font,
        );
        return;
    };
    let lang = app.shell.lang();
    let mut values = vec![Row {
        left: clip.name().to_owned(),
        right: format!("{}f", clip.timeline_length()),
        colour: theme.accent,
        chosen: true,
    }];
    if let Some(range) = clip.timeline_range() {
        values.push(Row {
            left: text(lang, Key::EffectsRange).to_owned(),
            right: format!("{}–{}", range.start(), range.end()),
            colour: theme.dim,
            chosen: false,
        });
    }
    values.push(Row {
        left: text(lang, Key::EffectsSpeed).to_owned(),
        right: clip.speed().to_string(),
        colour: theme.dim,
        chosen: false,
    });
    values.push(Row {
        left: text(lang, Key::EffectsOpacity).to_owned(),
        right: clip.opacity().to_string(),
        colour: theme.dim,
        chosen: false,
    });
    values.push(Row {
        left: text(lang, Key::EffectsBlend).to_owned(),
        right: format!("{:?}", clip.blend()),
        colour: theme.dim,
        chosen: false,
    });
    values.push(Row {
        left: text(lang, Key::EffectsTransform).to_owned(),
        right: format!("{:?}", clip.transform()),
        colour: theme.dim,
        chosen: false,
    });
    values.push(Row {
        left: text(lang, Key::EffectsChain).to_owned(),
        right: clip.effects().len().to_string(),
        colour: theme.faint,
        chosen: false,
    });
    for effect in clip.effects() {
        values.push(Row {
            left: format!("  {:?}", effect.kind()),
            right: if effect.is_enabled() {
                "on".to_owned()
            } else {
                "off".to_owned()
            },
            colour: if effect.is_enabled() {
                theme.ink
            } else {
                theme.faint
            },
            chosen: false,
        });
    }
    values.push(Row {
        left: text(lang, Key::EffectsMasks).to_owned(),
        right: clip.masks().len().to_string(),
        colour: theme.faint,
        chosen: false,
    });
    for mask in clip.masks() {
        values.push(Row {
            left: format!("  {:?}", mask.shape()),
            right: String::new(),
            colour: theme.dim,
            chosen: false,
        });
    }
    // The keyframe rows are drawn **first**, above the rows that describe the clip, because a
    // diamond is the one control here a person reaches for and a panel whose switches are below
    // the fold is a panel whose switches do not exist.
    let m = theme.metrics;
    let channels: [(AnimChannel, Key); 6] = [
        (AnimChannel::Opacity, Key::EffectsOpacity),
        (AnimChannel::X, Key::InspectorPositionX),
        (AnimChannel::Y, Key::InspectorPositionY),
        (AnimChannel::ScaleX, Key::InspectorScaleX),
        (AnimChannel::ScaleY, Key::InspectorScaleY),
        (AnimChannel::Rotation, Key::InspectorRotation),
    ];
    let mut top = rect.top + m.px(4);
    win32::text(
        hdc,
        Rect::new(
            rect.left + m.px(4),
            top,
            rect.right - m.px(4),
            top + m.row_h,
        ),
        text(lang, Key::InspectorKeyframes),
        theme.faint,
        &theme.heading_font,
        Align::Left,
    );
    top += m.row_h;
    for (channel, label) in channels {
        if top + m.row_h > rect.bottom {
            break;
        }
        let row = Rect::new(rect.left, top, rect.right, top + m.row_h);
        let diamond = Rect::new(
            row.left + m.px(5),
            row.top + m.px(6),
            row.left + m.px(16),
            row.top + m.px(17),
        );
        // Filled when a key sits on the frame under the playhead, outlined when the channel has
        // a curve but nothing here, and hollow when it has no curve at all.
        let (animated, keyed) = app.shell.keyframe_state(channel);
        let colour = if keyed {
            theme.accent
        } else if animated {
            theme.ink
        } else {
            theme.faint
        };
        glyphs::draw(hdc, diamond, Glyph::Keyframe, colour);
        win32::text(
            hdc,
            Rect::new(
                row.left + m.px(22),
                row.top,
                row.right - m.px(4),
                row.bottom,
            ),
            text(lang, label),
            if keyed { theme.ink } else { theme.dim },
            &theme.body_font,
            Align::Left,
        );
        let value = app
            .shell
            .channel_value_at_playhead(channel)
            .map_or_else(String::new, |value| value.to_string());
        win32::text(
            hdc,
            Rect::new(
                row.left + m.px(22),
                row.top,
                row.right - m.px(4),
                row.bottom,
            ),
            &value,
            if animated { theme.ink } else { theme.faint },
            &theme.mono_font,
            Align::Right,
        );
        // The switch is registered over the whole row, so a person does not have to hit eleven
        // pixels to use it, and the tooltip says what it will do in the state it is in.
        let said = match (animated, keyed) {
            (_, true) => Key::InspectorKeyRemove,
            (true, false) => Key::InspectorKeyAdd,
            (false, false) => Key::InspectorKeyClear,
        };
        app.zones.push(
            Region::new(Zone::Keyframe(channel), row, CursorKind::Point, said)
                .hinted(Some("shift")),
        );
        top += m.row_h;
    }

    // Whatever is left goes to the rows that describe the clip.
    let scroll = app.inspector_scroll.min(values.len().saturating_sub(1));
    let shown: Vec<Row> = values.into_iter().skip(scroll).collect();
    rows(
        hdc,
        Rect::new(rect.left, top, rect.right, rect.bottom).inset(2),
        &shown,
        app,
        scroll,
        |index| Some(Zone::InspectorRow(index)),
    );
}

/// The mixer, which is honest about what it cannot do.
pub(crate) fn mixer_body(hdc: Hdc, rect: Rect, app: &mut App) {
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    // The fader wells are drawn empty rather than at a level nobody set.
    let mut x = rect.left + m.px(12);
    while x + m.px(18) < rect.right - m.px(12) {
        let well = Rect::new(x, rect.top + m.px(10), x + m.px(8), rect.bottom - m.px(70));
        win32::rounded(hdc, well, m.px(3), theme.well, Some(theme.border));
        win32::fill_colour(
            hdc,
            Rect::new(well.left, well.bottom - m.px(3), well.right, well.bottom),
            theme.border,
        );
        x += m.px(20);
    }
    win32::text_block(
        hdc,
        Rect::new(
            rect.left + m.px(8),
            rect.bottom - m.px(64),
            rect.right - m.px(8),
            rect.bottom - m.px(6),
        ),
        text(app.shell.lang(), Key::AudioNotBuilt),
        theme.faint,
        &theme.small_font,
    );
    app.zones.push(Region::new(
        Zone::Meter,
        rect,
        CursorKind::Arrow,
        Key::AudioNotBuilt,
    ));
}

/// The media page: the pool across the width, with bigger tiles.
pub(crate) fn media_page(hdc: Hdc, frame: &Frame, app: &mut App) {
    let rect = frame.grid;
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    win32::fill_colour(hdc, rect, theme.panel);
    win32::frame(hdc, rect, &theme.border_brush());
    let heading = Rect::new(
        rect.left + 1,
        rect.top + 1,
        rect.right - 1,
        rect.top + 1 + m.heading_h,
    );
    win32::gradient(hdc, heading, theme.chrome, theme.panel);
    win32::text(
        hdc,
        Rect::new(
            heading.left + m.px(8),
            heading.top,
            heading.right,
            heading.bottom,
        ),
        text(app.shell.lang(), Key::PanelMedia),
        theme.accent,
        &theme.heading_font,
        Align::Left,
    );
    let count = format!("{}", app.shell.project().assets().len());
    win32::text(
        hdc,
        Rect::new(
            heading.right - m.px(40),
            heading.top,
            heading.right - m.px(8),
            heading.bottom,
        ),
        &count,
        theme.dim,
        &theme.mono_font,
        Align::Right,
    );
    // The grid reuses the pool's own drawing with a wider tile, by giving it the room it needs.
    pool_body(
        hdc,
        Rect::new(
            rect.left + 1,
            heading.bottom + 1,
            rect.right - 1,
            rect.bottom - 1,
        ),
        app,
    );
}

/// The deliver page's settings column.
pub(crate) fn deliver_page(hdc: Hdc, frame: &Frame, app: &mut App) {
    let rect = frame.export;
    if rect.is_empty() {
        return;
    }
    let theme = &app.theme;
    let m = theme.metrics;
    win32::fill_colour(hdc, rect, theme.panel);
    win32::frame(hdc, rect, &theme.border_brush());
    let heading = Rect::new(
        rect.left + 1,
        rect.top + 1,
        rect.right - 1,
        rect.top + 1 + m.heading_h,
    );
    win32::gradient(hdc, heading, theme.chrome, theme.panel);
    win32::text(
        hdc,
        Rect::new(
            heading.left + m.px(8),
            heading.top,
            heading.right,
            heading.bottom,
        ),
        text(app.shell.lang(), Key::PageDeliver),
        theme.accent,
        &theme.heading_font,
        Align::Left,
    );
    let mut body = Rect::new(
        rect.left + m.px(8),
        heading.bottom + m.px(4),
        rect.right - m.px(8),
        rect.bottom - m.px(8),
    );
    let facts: Vec<(Key, String)> = {
        let mut facts = vec![
            (
                Key::ExportWhat,
                text(app.shell.lang(), Key::ExportStillsOnly).to_owned(),
            ),
            (Key::StatusFrame, app.shell.playhead().to_string()),
            (
                Key::StatusRevision,
                app.shell.project().revision().to_string(),
            ),
        ];
        if let Some(sequence) = app.shell.sequence_entity() {
            facts.push((
                Key::ExportSize,
                format!("{}x{}", sequence.width(), sequence.height()),
            ));
            facts.push((Key::ExportRate, sequence.timebase().to_string()));
        }
        if let Ok(hash) = app.shell.state_hash() {
            facts.push((Key::StatusDigest, hash.chars().take(12).collect()));
        }
        facts.push((
            Key::ExportPath,
            app.shell.suggested_path().display().to_string(),
        ));
        facts
    };
    for (key, value) in &facts {
        win32::text(
            hdc,
            Rect::new(body.left, body.top, body.right, body.top + m.row_h),
            text(app.shell.lang(), *key),
            theme.faint,
            &theme.small_font,
            Align::Left,
        );
        win32::text(
            hdc,
            Rect::new(body.left, body.top, body.right, body.top + m.row_h),
            value,
            theme.ink,
            &theme.mono_font,
            Align::Right,
        );
        body = Rect::new(body.left, body.top + m.row_h + 2, body.right, body.bottom);
        if body.is_empty() {
            break;
        }
    }
    // The button that writes the still: the one thing this page can actually do.
    let button = Rect::new(
        body.left,
        body.top + m.px(10),
        body.left + m.px(190),
        body.top + m.px(10) + m.px(30),
    );
    let hovered = app.hover == Some(Zone::Render);
    win32::rounded(
        hdc,
        button,
        m.radius,
        if hovered {
            theme.accent
        } else {
            theme.chrome_high
        },
        Some(theme.accent),
    );
    win32::text(
        hdc,
        button,
        text(app.shell.lang(), Key::DeliverRender),
        if hovered { theme.chrome } else { theme.accent },
        &theme.body_font,
        Align::Centre,
    );
    app.zones.push(Region::new(
        Zone::Render,
        button,
        CursorKind::Point,
        Key::DeliverRender,
    ));
    if let Some(last) = app.last_export.as_ref() {
        win32::text_block(
            hdc,
            Rect::new(
                body.left,
                button.bottom + m.px(8),
                body.right,
                (button.bottom + m.px(60)).min(body.bottom),
            ),
            last,
            theme.dim,
            &theme.small_font,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pool_draws_what_it_can_and_names_what_it_cannot() {
        // The tile used to say "not decoded (M2)" about everything, including the flat fields
        // and charts the engine builds out of the URI and draws perfectly well. A pool that
        // calls a picture undecodable is a pool nobody can believe about the one that really
        // is, so the two answers are pinned here against the resolver that decides them.
        for uri in [
            "solid:11,24,30",
            "generated:bars",
            "title:sans:24:ffffff:centre:HI",
        ] {
            let drawn = thumbnail(uri, 24, 16).unwrap_or_else(|| panic!("{uri} is drawable"));
            assert_eq!(drawn.len(), 24 * 16 * 4, "{uri}");
            assert!(
                drawn.chunks_exact(4).all(|pixel| pixel[3] == 255),
                "a tile is opaque: {uri}"
            );
            assert_ne!(
                RESOLVER.kind(uri),
                maj_render::Placeholder::UNDECODABLE,
                "{uri} must not be called undecodable"
            );
        }

        // A still on disk is readable and deliberately not drawn, because decoding one per
        // tile is engine work the paint path may not do until there is a cache (M10). It is
        // named for its container, which is true, rather than for a decoder that is missing.
        assert!(thumbnail("shot.png", 24, 16).is_none());
        assert_eq!(
            RESOLVER.kind("missing.png"),
            maj_render::Placeholder::OFFLINE
        );

        // And a movie is the one thing the label was ever right about.
        assert!(thumbnail("shot.mov", 24, 16).is_none());
        assert_eq!(
            RESOLVER.kind("shot.mov"),
            maj_render::Placeholder::UNDECODABLE
        );
    }

    #[test]
    fn a_flat_field_thumbnails_to_one_colour() {
        // The simplest picture there is, so the sampling and the transfer can be checked
        // without a shape to reason about: every pixel of a solid must come out the same, and
        // it must not come out black, which is what an unencoded scene-linear value looks like.
        let drawn = thumbnail("solid:11,24,30", 8, 8).expect("a flat field is drawable");
        let first: &[u8] = &drawn[0..4];
        assert!(
            drawn.chunks_exact(4).all(|pixel| pixel == first),
            "a flat field is flat"
        );
        assert!(
            first[0] > 16 && first[1] > 16,
            "scene-linear light must be encoded for a screen, not blitted raw: {first:?}"
        );
    }
}

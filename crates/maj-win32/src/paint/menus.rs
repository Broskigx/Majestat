//! The menus. Drawn here rather than asked of the system, so that they are in the interface's
//! own palette, are reachable by keyboard, and can be measured by a test.
//!
//! A menu is a table of items; an item is either a binding — the same one the keyboard uses — or
//! one of the few actions that belong to the window itself, like opening a file dialog. Nothing
//! in the table decides anything about the document.

use maj_ui::{Binding, Key, Page, text};

use crate::app::{App, MenuKind, Special};
use crate::glyphs::{self, Glyph};
use crate::widget::{Region, Zone};
use crate::win32::{self, Align, CursorKind, Hdc, Rect};

/// One row of a menu.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Item {
    /// What it says.
    pub(crate) label: Key,
    /// The binding it runs, when it is a keyboard-intent.
    pub(crate) binding: Option<Binding>,
    /// What the window itself does, when it is not a binding.
    pub(crate) special: Option<Special>,
    /// The stroke, for the right-hand column.
    pub(crate) hint: Option<&'static str>,
    /// Whether it is a switch, and whether it is on.
    pub(crate) checked: Option<bool>,
    /// A divider above this row.
    pub(crate) after_gap: bool,
}

impl Item {
    /// A row that runs a binding.
    pub(crate) const fn bound(label: Key, binding: Binding, hint: &'static str) -> Self {
        Self {
            label,
            binding: Some(binding),
            special: None,
            hint: Some(hint),
            checked: None,
            after_gap: false,
        }
    }

    /// A row that runs something only the window can do.
    pub(crate) const fn special(label: Key, special: Special, hint: Option<&'static str>) -> Self {
        Self {
            label,
            binding: None,
            special: Some(special),
            hint,
            checked: None,
            after_gap: false,
        }
    }

    /// The same row, as a switch.
    pub(crate) const fn checked(label: Key, binding: Binding, on: bool) -> Self {
        Self {
            label,
            binding: Some(binding),
            special: None,
            hint: None,
            checked: Some(on),
            after_gap: false,
        }
    }

    /// The same row, with a divider above it.
    #[must_use]
    pub(crate) const fn after(mut self) -> Self {
        self.after_gap = true;
        self
    }

    /// Whether the row can be chosen at all.
    pub(crate) fn usable(&self, app: &App) -> bool {
        match self.special {
            Some(Special::Quit) | Some(Special::Open) => true,
            Some(Special::SaveAs) | Some(Special::ExportDialog) => true,
            Some(Special::Page(page)) => page.built(),
            _ => match self.binding {
                Some(Binding::Undo) => app.shell.depths().0 > 0,
                Some(Binding::Redo) => app.shell.depths().1 > 0,
                Some(Binding::Split)
                | Some(Binding::Remove)
                | Some(Binding::Duplicate)
                | Some(Binding::TrimStartAtPlayhead)
                | Some(Binding::TrimEndAtPlayhead)
                | Some(Binding::ToggleEnabled)
                | Some(Binding::CloseGap)
                | Some(Binding::RippleDelete)
                | Some(Binding::InsertBlank) => app.shell.selection().is_some(),
                _ => true,
            },
        }
    }
}

/// The items of a menu.
#[must_use]
pub(crate) fn items(kind: MenuKind, app: &App) -> Vec<Item> {
    let view = app.shell.view();
    match kind {
        MenuKind::App => vec![
            Item::bound(Key::FileExample, Binding::Example, "ctrl+alt+n"),
            Item::bound(Key::ViewPalette, Binding::TogglePalette, "ctrl+k"),
            Item::special(Key::FileQuit, Special::Quit, None).after(),
            Item::special(Key::HelpAbout, Special::About, None),
        ],
        MenuKind::File => vec![
            Item::bound(Key::FileExample, Binding::Example, "ctrl+alt+n"),
            Item::special(Key::FileOpen, Special::Open, None).after(),
            Item::bound(Key::FileSave, Binding::Save, "ctrl+s"),
            Item::special(Key::FileSaveAs, Special::SaveAs, None),
            Item::special(Key::FileExportStill, Special::ExportDialog, Some("alt+x")).after(),
            Item::bound(Key::MenuLanguage, Binding::ToggleLanguage, "ctrl+l"),
        ],
        MenuKind::Edit => vec![
            Item::bound(Key::EditUndo, Binding::Undo, "ctrl+z"),
            Item::bound(Key::EditRedo, Binding::Redo, "ctrl+shift+z"),
            Item::bound(Key::EditSplit, Binding::Split, "s").after(),
            Item::bound(Key::EditRemove, Binding::Remove, "supr"),
            Item::bound(Key::EditDuplicate, Binding::Duplicate, "ctrl+d"),
            Item::bound(Key::EditToggleEnabled, Binding::ToggleEnabled, "e"),
            Item::bound(Key::EditRippleDelete, Binding::RippleDelete, "shift+supr"),
            Item::bound(Key::EditCloseGap, Binding::CloseGap, "g").after(),
            Item::bound(Key::EditInsertBlank, Binding::InsertBlank, "shift+g"),
            Item::bound(Key::EditPreviousEdit, Binding::StepEditPoint(-1), ",").after(),
            Item::bound(Key::EditNextEdit, Binding::StepEditPoint(1), "."),
            Item::bound(
                Key::EditSelectPrevious,
                Binding::SelectAdjacent(-1),
                "shift+,",
            ),
            Item::bound(Key::EditSelectNext, Binding::SelectAdjacent(1), "shift+."),
        ],
        MenuKind::Sequence => vec![
            Item::bound(Key::EditAddVideoTrack, Binding::AddVideoTrack, "n"),
            Item::bound(Key::EditAddAudioTrack, Binding::AddAudioTrack, "shift+n"),
            Item::bound(Key::ViewCyclePage, Binding::CyclePage(1), "ctrl+tab").after(),
            Item::special(Key::PageMedia, Special::Page(Page::Media), Some("alt+1")),
            Item::special(Key::PageCut, Special::Page(Page::Cut), Some("alt+2")),
            Item::special(
                Key::PageDeliver,
                Special::Page(Page::Deliver),
                Some("alt+3"),
            ),
        ],
        MenuKind::View => {
            let panels = app.shell.panels();
            let mut items = vec![
                Item::checked(
                    Key::PanelMedia,
                    Binding::TogglePanel(maj_ui::Panel::Media),
                    panels.media,
                ),
                Item::checked(
                    Key::PanelHistory,
                    Binding::TogglePanel(maj_ui::Panel::History),
                    panels.history,
                ),
                Item::checked(
                    Key::PanelEffects,
                    Binding::TogglePanel(maj_ui::Panel::Effects),
                    panels.effects,
                ),
                Item::checked(
                    Key::PanelTimeline,
                    Binding::TogglePanel(maj_ui::Panel::Timeline),
                    panels.timeline,
                ),
                Item::checked(
                    Key::PanelProgram,
                    Binding::TogglePanel(maj_ui::Panel::Monitors),
                    panels.monitors,
                )
                .after(),
                Item::checked(Key::ViewSnap, Binding::ToggleSnap, view.snap),
                Item::checked(Key::ViewLink, Binding::ToggleLink, view.link),
                Item::checked(Key::TransportLoop, Binding::ToggleLoop, view.loops),
                Item::checked(Key::MonitorTiled, Binding::ToggleTiled, app.shell.tiled()).after(),
                Item::bound(
                    Key::DraftFull,
                    Binding::Draft(maj_ui::Draft::Full),
                    "ctrl+1",
                ),
                Item::bound(
                    Key::DraftHalf,
                    Binding::Draft(maj_ui::Draft::Half),
                    "ctrl+2",
                ),
                Item::bound(
                    Key::DraftQuarter,
                    Binding::Draft(maj_ui::Draft::Quarter),
                    "ctrl+3",
                ),
                Item::bound(Key::ViewZoomIn, Binding::ZoomIn, "+").after(),
                Item::bound(Key::ViewZoomOut, Binding::ZoomOut, "-"),
                Item::bound(Key::ViewZoomFit, Binding::ZoomFit, "alt+0"),
                Item::special(Key::ViewResetPanels, Special::ResetPanels, None),
            ];
            items.push(Item::special(Key::ViewLayout, Special::About, None));
            items
        }
        MenuKind::Tools => vec![
            Item::bound(Key::ViewPalette, Binding::TogglePalette, "ctrl+k"),
            Item::bound(Key::FileExample, Binding::Example, "ctrl+alt+n"),
            Item::special(Key::ToolsAgent, Special::Agent, None).after(),
        ],
        MenuKind::Help => vec![Item::special(Key::HelpAbout, Special::About, None)],
        MenuKind::Clip => vec![
            Item::bound(Key::EditSplit, Binding::Split, "s"),
            Item::bound(Key::EditDuplicate, Binding::Duplicate, "ctrl+d"),
            Item::bound(Key::EditRemove, Binding::Remove, "supr"),
            Item::bound(Key::EditToggleEnabled, Binding::ToggleEnabled, "e").after(),
            Item::bound(Key::ViewTrimIn, Binding::TrimStartAtPlayhead, "["),
            Item::bound(Key::ViewTrimOut, Binding::TrimEndAtPlayhead, "]"),
        ],
        MenuKind::Timeline => vec![
            Item::bound(Key::EditAddVideoTrack, Binding::AddVideoTrack, "n"),
            Item::bound(Key::EditAddAudioTrack, Binding::AddAudioTrack, "shift+n").after(),
            Item::bound(Key::ViewZoomIn, Binding::ZoomIn, "+"),
            Item::bound(Key::ViewZoomOut, Binding::ZoomOut, "-"),
            Item::bound(Key::ViewZoomFit, Binding::ZoomFit, "alt+0"),
        ],
    }
}

/// Draws the menu that is open, at the place it belongs, and registers its rows.
pub(crate) fn dropdown(hdc: Hdc, client: Rect, app: &mut App) {
    let Some(open) = app.menu else {
        return;
    };
    let theme = &app.theme;
    let m = theme.metrics;
    let entries = items(open.kind, app);
    if entries.is_empty() {
        return;
    }
    let mut width = m.px(150);
    let mut height = m.px(8);
    for entry in &entries {
        let mut line =
            win32::text_width(hdc, text(app.shell.lang(), entry.label), &theme.body_font)
                + m.px(48);
        if let Some(hint) = entry.hint {
            line += win32::text_width(hdc, hint, &theme.small_font) + m.px(20);
        }
        width = width.max(line);
        height += m.row_h + m.px(4);
        if entry.after_gap {
            height += m.px(7);
        }
    }
    let (left, top) = match open.at {
        Some(point) => point,
        None => match app.zones.get(Zone::Menu(open.index)) {
            Some(region) => (region.rect.left, region.rect.bottom),
            None => (client.left + m.px(8), app.frame_rect.toolbar.bottom),
        },
    };
    let panel = Rect::new(
        left.min(client.right - width - m.px(4))
            .max(client.left + m.px(2)),
        top.min(client.bottom - height - m.px(4))
            .max(client.top + m.px(2)),
        left.min(client.right - width - m.px(4))
            .max(client.left + m.px(2))
            + width,
        top.min(client.bottom - height - m.px(4))
            .max(client.top + m.px(2))
            + height,
    );
    app.menu = Some(open);
    // A soft shadow: three rounded rectangles stepping outwards, which is enough to lift the
    // panel off the interface without an alpha channel.
    for step in (1..=3).rev() {
        win32::rounded(hdc, panel.inset(-step), m.radius + step, theme.shadow, None);
    }
    win32::rounded(hdc, panel, m.radius, theme.chrome, Some(theme.separator));

    let mut y = panel.top + m.px(4);
    for (index, entry) in entries.iter().enumerate() {
        if entry.after_gap {
            win32::fill_colour(
                hdc,
                Rect::new(
                    panel.left + m.px(6),
                    y + m.px(3),
                    panel.right - m.px(6),
                    y + m.px(4),
                ),
                theme.border,
            );
            y += m.px(7);
        }
        let row = Rect::new(panel.left + m.px(2), y, panel.right - m.px(2), y + m.row_h);
        let usable = entry.usable(app);
        let zone = Zone::MenuItem(index);
        if usable && app.hover == Some(zone) {
            win32::rounded(hdc, row, m.px(2), theme.chrome_high, None);
        }
        if let Some(on) = entry.checked {
            if on {
                glyphs::draw(
                    hdc,
                    Rect::new(
                        row.left + m.px(4),
                        row.top + m.px(3),
                        row.left + m.px(16),
                        row.bottom - m.px(3),
                    ),
                    Glyph::Check,
                    theme.accent,
                );
            }
        }
        let ink = if usable {
            if app.hover == Some(zone) {
                theme.ink
            } else {
                theme.dim
            }
        } else {
            theme.faint
        };
        win32::text(
            hdc,
            Rect::new(
                row.left + m.px(22),
                row.top,
                row.right - m.px(70),
                row.bottom,
            ),
            text(app.shell.lang(), entry.label),
            ink,
            &theme.body_font,
            Align::Left,
        );
        if let Some(hint) = entry.hint {
            win32::text(
                hdc,
                Rect::new(
                    row.right - m.px(68),
                    row.top,
                    row.right - m.px(8),
                    row.bottom,
                ),
                hint,
                theme.faint,
                &theme.small_font,
                Align::Right,
            );
        }
        let region = Region::new(zone, row, CursorKind::Point, entry.label);
        app.zones.push(if usable {
            region
        } else {
            region.refused(match entry.special {
                Some(Special::Page(page)) => crate::paint::chrome::page_owner(page),
                _ => Key::NotBuiltHeading,
            })
        });
        y = row.bottom + m.px(4);
    }
}

/// The command palette: the agent's own catalogue, in a person's hands.
pub(crate) fn palette(hdc: Hdc, client: Rect, app: &mut App) {
    let theme = &app.theme;
    let m = theme.metrics;
    // The dim behind it: the whole window, washed, so the palette is plainly on top.
    app.theme.wash(hdc, client, theme.shadow, 150);

    let width = (client.width() - m.px(80)).clamp(m.px(360), m.px(720));
    let height = (client.height() - m.px(120)).clamp(m.px(200), m.px(460));
    let left = client.left + (client.width() - width) / 2;
    let top = client.top + (client.height() - height) / 3;
    let panel = Rect::new(left, top, left + width, top + height);
    for step in (1..=4).rev() {
        win32::rounded(hdc, panel.inset(-step), m.radius + step, theme.shadow, None);
    }
    win32::rounded(hdc, panel, m.radius, theme.chrome, Some(theme.accent));

    let field = Rect::new(
        panel.left + m.px(8),
        panel.top + m.px(8),
        panel.right - m.px(8),
        panel.top + m.px(8) + m.px(30),
    );
    win32::rounded(hdc, field, m.radius, theme.ground, Some(theme.border));
    let palette = app.shell.palette();
    let typed = if palette.query.is_empty() {
        text(app.shell.lang(), Key::PaletteSearch).to_owned()
    } else {
        format!("{}▌", palette.query)
    };
    win32::text(
        hdc,
        field.inset(m.px(8)),
        &typed,
        if palette.query.is_empty() {
            theme.faint
        } else {
            theme.ink
        },
        &theme.body_font,
        Align::Left,
    );
    app.zones.push(Region::new(
        Zone::PaletteField,
        field,
        CursorKind::Text,
        Key::PaletteSearch,
    ));

    let list = Rect::new(
        panel.left + m.px(8),
        field.bottom + m.px(6),
        panel.right - m.px(8),
        panel.bottom - m.px(34),
    );
    let matches = app.palette_matches();
    if matches.is_empty() {
        win32::text_block(
            hdc,
            list,
            text(app.shell.lang(), Key::PaletteEmpty),
            theme.faint,
            &theme.body_font,
        );
    } else {
        let capacity = (list.height() / m.row_h).max(1) as usize;
        for (index, tool) in matches.iter().take(capacity).enumerate() {
            let row = Rect::new(
                list.left,
                list.top + (index as i32) * m.row_h,
                list.right,
                list.top + (index as i32 + 1) * m.row_h,
            );
            let chosen = index == palette.row;
            let zone = Zone::PaletteRow(index);
            if chosen {
                win32::rounded(
                    hdc,
                    row,
                    m.px(2),
                    theme.chrome_high,
                    Some(theme.accent_deep),
                );
            } else if app.hover == Some(zone) {
                win32::rounded(hdc, row, m.px(2), theme.chrome_low, None);
            }
            let name = tool.name.trim_start_matches("maj_").replace('_', " ");
            win32::text(
                hdc,
                Rect::new(
                    row.left + m.px(8),
                    row.top,
                    row.right - m.px(120),
                    row.bottom,
                ),
                &name,
                if chosen { theme.accent } else { theme.ink },
                &theme.body_font,
                Align::Left,
            );
            win32::text(
                hdc,
                Rect::new(
                    row.right - m.px(116),
                    row.top,
                    row.right - m.px(8),
                    row.bottom,
                ),
                tool.command.unwrap_or("read"),
                theme.faint,
                &theme.small_font,
                Align::Right,
            );
            app.zones
                .push(Region::new(zone, row, CursorKind::Point, Key::PaletteRun));
        }
    }

    let footer = Rect::new(
        panel.left + m.px(8),
        panel.bottom - m.px(30),
        panel.right - m.px(8),
        panel.bottom - m.px(6),
    );
    let hint = match matches.get(palette.row) {
        Some(tool) => {
            let mut parts: Vec<String> = Vec::new();
            for field in tool.fields.iter().take(4) {
                let value = palette.args.get(field.name).cloned().unwrap_or_default();
                parts.push(format!("{}={value}", field.name));
            }
            format!(
                "{} · {}",
                text(app.shell.lang(), Key::PaletteArguments),
                parts.join(" ")
            )
        }
        None => text(app.shell.lang(), Key::PaletteRun).to_owned(),
    };
    win32::text(
        hdc,
        footer,
        &hint,
        theme.dim,
        &theme.small_font,
        Align::Left,
    );
    // The whole panel is a region, so a click inside it does not close it; the backdrop is the
    // whole client, registered first, and the panel is registered over it.
    app.zones.push(Region::new(
        Zone::PaletteBackdrop,
        panel,
        CursorKind::Arrow,
        Key::PaletteClose,
    ));
}

/// The tooltip: the name of whatever the pointer has been resting on, and its stroke.
pub(crate) fn tooltip(hdc: Hdc, client: Rect, app: &mut App) {
    let Some(zone) = app.hover else {
        return;
    };
    if app.menu.is_some() {
        return;
    }
    let rested = crate::win32::now() - app.hover_since;
    if rested < 0.45 {
        return;
    }
    let Some(region) = app.zones.get(zone) else {
        return;
    };
    let theme = &app.theme;
    let m = theme.metrics;
    let mut label = text(app.shell.lang(), region.label).to_owned();
    if let Some(reason) = region.reason {
        label = format!("{label}\n{}", text(app.shell.lang(), reason));
    }
    if let Some(hint) = region.hint {
        if !hint.is_empty() {
            label = format!("{label}   {hint}");
        }
    }
    let width = win32::text_width(hdc, &label, &theme.small_font) + m.px(18);
    let left = (app.pointer.0 + m.px(14)).min(client.right - width - m.px(4));
    let top = (app.pointer.1 + m.px(18)).min(client.bottom - m.px(28));
    let box_ = Rect::new(
        left.max(client.left + 2),
        top,
        left.max(client.left + 2) + width,
        top + m.px(24),
    );
    win32::rounded(hdc, box_, m.radius, theme.chrome_low, Some(theme.separator));
    win32::text(
        hdc,
        box_.inset(m.px(6)),
        &label,
        theme.ink,
        &theme.small_font,
        Align::Left,
    );
}

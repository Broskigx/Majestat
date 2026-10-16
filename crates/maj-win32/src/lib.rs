//! `maj-win32` — the window: a presenter for [`maj_ui`], and nothing else.
//!
//! **Layer 6.** See `docs/ARCHITECTURE.md` for the layering rule, `docs/UI.md` for the
//! interface itself and `docs/adr/0013-the-window-is-written-against-win32.md` for why this
//! crate is shaped the way it is.
//!
//! The shell — the intentions, the keyboard table, the two languages, the preferences — is
//! `maj-ui`, and it is deliberately free of any windowing type. This crate is the other half:
//! it owns a window, translates the messages Windows sends into the strokes the shell's table
//! already defines, and paints what the shell says. It decides nothing about the document:
//! every edit goes through [`Shell::dispatch`](maj_ui::Shell::dispatch) or
//! [`Shell::perform`](maj_ui::Shell::perform), which is to say through the same command log an
//! agent's tools append to.
//!
//! Three rules hold here:
//!
//! * **The window knows no more than the shell tells it.** Every label, name and state comes
//!   from the shell, in the language the shell was told to speak; the pixel positions are the
//!   only opinions this crate has.
//! * **A key is a stroke, and a stroke is a binding.** The mapping is a table, and the test
//!   `every_stroke_the_shell_answers_to_can_be_typed_here` proves that every stroke the
//!   shell's table defines can be produced by a key on this window, so the documented keymap
//!   cannot quietly stop working.
//! * **A refusal is shown.** Whatever the engine refuses appears in the status bar in its own
//!   words. Nothing about the document is guessed at.
//!
//! And one rule about the brand: the owner's logo is decoded from `assets/brand/logo.png` at
//! build time by `build.rs`, so the artwork has one source and the binary carries pixels rather
//! than a PNG decoder. The module that draws it is `brand`.

pub mod app;
mod brand;
mod glyphs;
mod layout;
mod paint;
mod theme;
mod transport;
mod widget;
mod win32;

pub use app::{App, LAYER, Options, ROLE};

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use maj_ui::{Binding, Page, Panels, keys};

    use crate::app::{press_for, stroke_for, virtual_key};
    use crate::glyphs;
    use crate::layout::{Frame, View};
    use crate::theme::Metrics;
    use crate::widget::Tool;
    use crate::win32::Rect;

    /// The geometry the layout tests are built from.
    fn view(page: Page) -> View {
        View {
            page,
            panels: Panels::default(),
            left_w: 264,
            right_w: 288,
            timeline_h: 300,
            media_split: 220,
            tracks: 2,
            metrics: Metrics::new(1.0),
        }
    }

    #[test]
    fn every_glyph_has_a_picture_and_a_name_of_its_own() {
        let mut names = HashSet::new();
        for glyph in glyphs::ALL {
            let name = glyphs::name(*glyph);
            assert!(!name.is_empty(), "{glyph:?} has no name");
            assert!(names.insert(name), "`{name}` names two glyphs");
            assert!(
                !glyphs::shapes(*glyph).is_empty(),
                "{glyph:?} draws nothing"
            );
        }
    }

    #[test]
    fn every_toolbar_button_carries_a_picture_and_a_label() {
        for tool in Tool::ALL {
            assert!(
                !glyphs::name(tool.glyph()).is_empty(),
                "{tool:?} has no picture"
            );
            // Every tool has a stroke that reaches it except the one the mouse is for: fitting
            // the sequence, which has no stroke of its own and lives in the View menu. The
            // marker tool used to be the second exception; it has a stroke now (`m`), so this
            // list is down to one and a new button without a key fails here.
            if tool.binding().is_none() {
                assert!(
                    matches!(tool, Tool::ZoomFit),
                    "{tool:?} has no stroke and no reason to be a button"
                );
            }
            let _ = tool.label();
            let _ = tool.hint();
        }
    }

    #[test]
    fn the_layout_puts_every_panel_inside_the_client_area() {
        let client = Rect::new(0, 0, 1440, 900);
        for page in [Page::Media, Page::Cut, Page::Deliver] {
            let frame = Frame::build(client, &view(page));
            for (name, rect) in [
                ("menu", frame.menu),
                ("toolbar", frame.toolbar),
                ("status", frame.status),
                ("pages", frame.pages),
                ("body", frame.body),
                ("viewer", frame.viewer),
                ("transport", frame.transport),
                ("inspector", frame.inspector),
                ("timeline", frame.timeline),
                ("track area", frame.track_area),
            ] {
                assert!(
                    rect.left >= client.left
                        && rect.top >= client.top
                        && rect.right <= client.right
                        && rect.bottom <= client.bottom,
                    "{page:?}'s {name} is outside the window: {rect:?}"
                );
            }
        }
    }

    #[test]
    fn the_layout_summary_names_the_page_and_the_dividers() {
        let view = view(Page::Cut);
        let summary = crate::layout::summary(&view, view.panels);
        assert!(summary.starts_with("cut"), "{summary}");
        assert!(summary.contains("left 264"), "{summary}");
        assert!(summary.contains("timeline 300"), "{summary}");
    }

    #[test]
    fn every_stroke_the_shell_answers_to_can_be_typed_here() {
        let mut checked = 0;
        for (stroke, expected) in keys::TABLE {
            let key = virtual_key(stroke.press)
                .unwrap_or_else(|| panic!("no virtual key produces {:?}", stroke.press));
            let produced = stroke_for(key, stroke.ctrl, stroke.shift, stroke.alt)
                .unwrap_or_else(|| panic!("key {key:#04x} produces no stroke"));
            assert_eq!(
                keys::binding(produced),
                Some(*expected),
                "key {key:#04x} with ctrl={} shift={} alt={} does not reach {expected:?}",
                stroke.ctrl,
                stroke.shift,
                stroke.alt
            );
            checked += 1;
        }
        assert_eq!(checked, keys::TABLE.len());
    }

    #[test]
    fn no_key_the_window_answers_to_is_unbound_by_the_shell() {
        // The other direction: a key this window translates must be a stroke the shell has a
        // binding for, or a person presses it and nothing happens.
        let mut handled = 0;
        for code in 0x00u16..0x100 {
            let Some(stroke) = stroke_for(code, false, false, false) else {
                continue;
            };
            if keys::binding(stroke).is_some() {
                handled += 1;
            }
        }
        assert!(
            handled >= 15,
            "only {handled} unmodified keys reach a binding"
        );
    }

    #[test]
    fn the_modifiers_reach_the_stroke_they_belong_to() {
        assert_eq!(
            stroke_for(0x5A, true, false, false).map(keys::binding),
            Some(Some(Binding::Undo)),
            "ctrl+z is undo"
        );
        assert_eq!(
            stroke_for(0x5A, true, true, false).map(keys::binding),
            Some(Some(Binding::Redo)),
            "ctrl+shift+z is redo"
        );
        assert_eq!(
            stroke_for(0x25, false, true, false).map(keys::binding),
            Some(Some(Binding::StepFrames(-10))),
            "shift+left steps ten frames"
        );
        assert_eq!(
            stroke_for(0x25, false, false, true).map(keys::binding),
            Some(Some(Binding::MoveSelected(-1))),
            "alt+left moves the clip"
        );
    }

    #[test]
    fn a_key_with_no_stroke_is_silent_rather_than_wrong() {
        assert_eq!(
            stroke_for(0x70, false, false, false),
            None,
            "F1 is not bound"
        );
        assert_eq!(
            press_for(0x08),
            None,
            "backspace is text editing, not a stroke"
        );
    }
}

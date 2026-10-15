//! Preferences: what the interface remembers, kept deliberately outside the document.
//!
//! The `.maj` file is a description of an edit and nothing else. Which language a person
//! reads, how far the timeline is zoomed and which panels are open are facts about a
//! workstation, not about the work — putting them in the document would make the same edit
//! hash differently on two machines, which is exactly the property the engine exists to
//! protect. So they live in a small JSON file beside the user's configuration.
//!
//! Nothing here can fail in a way a person has to care about. A missing file, an unreadable
//! file, a truncated file and a file full of nonsense all produce the defaults, because a
//! preference is not worth refusing to start over; and a preference that is out of range —
//! a zoom of zero, a NaN — is repaired on the way in for the same reason.

use std::path::{Path, PathBuf};

use maj_color::TransferFunction;
use serde::{Deserialize, Serialize};

use crate::action::{Draft, Panels};
use crate::i18n::Lang;
use crate::view::Page;

/// The file name preferences are stored under.
pub const FILE_NAME: &str = "ui.json";

/// The environment variable that overrides the configuration directory.
pub const CONFIG_DIR_VAR: &str = "MAJ_CONFIG_DIR";

/// How far the timeline is zoomed, in pixels per frame, when nothing says otherwise.
pub const DEFAULT_ZOOM: f64 = 4.0;

/// The narrowest and widest timeline zoom, in pixels per frame.
///
/// A floor keeps a long sequence from turning into a single pixel and a ceiling keeps a
/// single frame from being wider than the window; both are shown and both can be raised.
pub const ZOOM_RANGE: (f64, f64) = (0.05, 400.0);

/// What the interface remembers between sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Prefs {
    /// The language the interface speaks.
    pub lang: Lang,
    /// Timeline zoom, in pixels per frame.
    pub zoom: f64,
    /// Preview resolution.
    pub draft: Draft,
    /// Whether the viewer renders through tiles.
    pub tiled: bool,
    /// The curve the viewer's code values are drawn through.
    pub transfer: TransferFunction,
    /// Which panels are open.
    pub panels: Panels,
    /// The page that was showing.
    pub page: Page,
    /// Whether a dragged edge snaps to the playhead and to other clips.
    pub snap: bool,
    /// Whether selecting a clip selects everything linked to it.
    pub link: bool,
    /// Whether the transport starts again when it reaches the end.
    pub loops: bool,
    /// The left column's width, in pixels at the interface's own scale.
    pub left_w: i32,
    /// The right column's width.
    pub right_w: i32,
    /// The timeline's height.
    pub timeline_h: i32,
    /// How much of a column its first panel takes.
    pub media_split: i32,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            lang: Lang::English,
            zoom: DEFAULT_ZOOM,
            draft: Draft::Full,
            tiled: false,
            transfer: TransferFunction::Srgb,
            panels: Panels::default(),
            page: Page::Cut,
            snap: true,
            link: true,
            loops: false,
            left_w: 264,
            right_w: 288,
            timeline_h: 300,
            media_split: 220,
        }
    }
}

impl Prefs {
    /// Repairs a preference file's contents into the range the interface can honour.
    #[must_use]
    pub fn sanitised(mut self) -> Self {
        if !self.zoom.is_finite() {
            self.zoom = DEFAULT_ZOOM;
        }
        self.zoom = self.zoom.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        // A divider out of range, or a page whose engine does not exist, is repaired rather
        // than honoured, by the same code the layout uses: the two cannot disagree about what
        // the interface can show.
        let repaired = crate::view::View {
            page: self.page,
            panels: self.panels,
            snap: self.snap,
            link: self.link,
            loops: self.loops,
            left_w: self.left_w,
            right_w: self.right_w,
            timeline_h: self.timeline_h,
            media_split: self.media_split,
        }
        .sanitised();
        self.page = repaired.page;
        self.left_w = repaired.left_w;
        self.right_w = repaired.right_w;
        self.timeline_h = repaired.timeline_h;
        self.media_split = repaired.media_split;
        self
    }

    /// The window's arrangement, as the layout wants it.
    #[must_use]
    pub fn view(&self) -> crate::view::View {
        crate::view::View {
            page: self.page,
            panels: self.panels,
            snap: self.snap,
            link: self.link,
            loops: self.loops,
            left_w: self.left_w,
            right_w: self.right_w,
            timeline_h: self.timeline_h,
            media_split: self.media_split,
        }
    }

    /// Reads preferences, falling back to the defaults for everything that does not parse.
    #[must_use]
    pub fn load() -> Self {
        match Self::path() {
            Some(path) => Self::load_from(path),
            None => Self::default(),
        }
    }

    /// Reads preferences from an explicit path.
    ///
    /// A file that is missing, unreadable, truncated or nonsense gives the defaults. That is
    /// deliberate: refusing to open the interface because a preferences file is corrupt
    /// would be the wrong trade, and there is nothing in here that cannot be set again in a
    /// few seconds.
    #[must_use]
    pub fn load_from(path: impl AsRef<Path>) -> Self {
        let Ok(text) = std::fs::read_to_string(path.as_ref()) else {
            return Self::default();
        };
        serde_json::from_str::<Self>(&text)
            .map(Self::sanitised)
            .unwrap_or_default()
    }

    /// Writes preferences to the conventional location, creating the directory if needed.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O error. A caller that cannot write preferences may carry
    /// on: the interface works without them.
    pub fn save(&self) -> std::io::Result<Option<PathBuf>> {
        let Some(path) = Self::path() else {
            return Ok(None);
        };
        self.save_to(&path)?;
        Ok(Some(path))
    }

    /// Writes preferences to an explicit path.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O error, including one from creating the parent directory.
    pub fn save_to(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.clone().sanitised())
            .map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }

    /// The file preferences live in, when a configuration directory can be found.
    #[must_use]
    pub fn path() -> Option<PathBuf> {
        Self::config_dir().map(|directory| directory.join(FILE_NAME))
    }

    /// The directory preferences live in.
    ///
    /// The order is deliberate: an explicit override first, then the platform's convention,
    /// then the Unix convention, and finally nothing at all rather than a guess at a path
    /// nobody asked for.
    #[must_use]
    pub fn config_dir() -> Option<PathBuf> {
        Self::config_dir_with(|name| std::env::var(name).ok())
    }

    /// The directory preferences live in, with the environment supplied by the caller.
    #[must_use]
    pub fn config_dir_with(lookup: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
        if let Some(directory) = lookup(CONFIG_DIR_VAR).filter(|value| !value.trim().is_empty()) {
            return Some(PathBuf::from(directory));
        }
        if let Some(app_data) = lookup("APPDATA").filter(|value| !value.trim().is_empty()) {
            return Some(PathBuf::from(app_data).join("Majestat"));
        }
        if let Some(config) = lookup("XDG_CONFIG_HOME").filter(|value| !value.trim().is_empty()) {
            return Some(PathBuf::from(config).join("majestat"));
        }
        let home = lookup("HOME")
            .or_else(|| lookup("USERPROFILE"))
            .filter(|value| !value.trim().is_empty())?;
        Some(PathBuf::from(home).join(".config").join("majestat"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("maj-ui-prefs-{name}"));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        directory
    }

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let directory = scratch("missing");
        assert_eq!(
            Prefs::load_from(directory.join("nothing.json")),
            Prefs::default()
        );
    }

    #[test]
    fn nonsense_gives_the_defaults_rather_than_a_refusal() {
        let directory = scratch("nonsense");
        let path = directory.join(FILE_NAME);
        std::fs::write(&path, "{ this is not json").expect("written");
        assert_eq!(Prefs::load_from(&path), Prefs::default());

        std::fs::write(&path, r#"{"zoom": "four"}"#).expect("written");
        assert_eq!(Prefs::load_from(&path), Prefs::default());

        std::fs::write(&path, r#"{"unknown_field": 1}"#).expect("written");
        assert_eq!(Prefs::load_from(&path), Prefs::default());
    }

    #[test]
    fn a_round_trip_preserves_every_field() {
        let directory = scratch("round-trip");
        let path = directory.join(FILE_NAME);
        let prefs = Prefs {
            lang: Lang::Spanish,
            zoom: 12.5,
            draft: Draft::Quarter,
            tiled: true,
            transfer: TransferFunction::Rec709,
            panels: Panels {
                media: false,
                history: true,
                effects: false,
                timeline: true,
                monitors: true,
            },
            page: Page::Media,
            snap: false,
            link: false,
            loops: true,
            left_w: 300,
            right_w: 320,
            timeline_h: 260,
            media_split: 180,
        };
        prefs.save_to(&path).expect("saved");
        assert_eq!(Prefs::load_from(&path), prefs);
    }

    #[test]
    fn a_partial_file_keeps_the_fields_it_names() {
        let directory = scratch("partial");
        let path = directory.join(FILE_NAME);
        std::fs::write(&path, r#"{"lang":"es","zoom":9.0}"#).expect("written");
        let prefs = Prefs::load_from(&path);
        assert_eq!(prefs.lang, Lang::Spanish);
        assert_eq!(prefs.zoom, 9.0);
        assert_eq!(prefs.draft, Prefs::default().draft);
        assert_eq!(prefs.panels, Prefs::default().panels);
    }

    #[test]
    fn a_divider_or_a_page_that_cannot_be_honoured_is_repaired() {
        let path = scratch("repair").join(FILE_NAME);
        std::fs::write(
            &path,
            r#"{"page":"color","left_w":90000,"right_w":-4,"timeline_h":0,"media_split":12000}"#,
        )
        .expect("written");
        let prefs = Prefs::load_from(&path);
        assert_eq!(
            prefs.page,
            Page::Cut,
            "a page with no engine is not honoured"
        );
        for value in [
            prefs.left_w,
            prefs.right_w,
            prefs.timeline_h,
            prefs.media_split,
        ] {
            assert!(
                (crate::view::SPLIT_RANGE.0..=crate::view::SPLIT_RANGE.1).contains(&value),
                "{value} is outside the range the layout can honour"
            );
        }
    }

    #[test]
    fn the_preferences_and_the_view_agree_about_the_arrangement() {
        let prefs = Prefs {
            page: Page::Deliver,
            snap: false,
            left_w: 333,
            ..Prefs::default()
        };
        let view = prefs.view();
        assert_eq!(view.page, Page::Deliver);
        assert!(!view.snap);
        assert_eq!(view.left_w, 333);
        assert_eq!(view.panels, prefs.panels);
    }

    #[test]
    fn a_zoom_that_cannot_be_honoured_is_repaired() {
        for (given, expected) in [
            (f64::NAN, DEFAULT_ZOOM),
            (0.0, ZOOM_RANGE.0),
            (-4.0, ZOOM_RANGE.0),
            (1.0e9, ZOOM_RANGE.1),
        ] {
            let repaired = Prefs {
                zoom: given,
                ..Prefs::default()
            }
            .sanitised();
            assert_eq!(repaired.zoom, expected, "zoom {given}");
        }
    }

    #[test]
    fn the_configuration_directory_prefers_the_override() {
        let lookup = |name: &str| match name {
            CONFIG_DIR_VAR => Some("/custom".to_owned()),
            "APPDATA" => Some("/appdata".to_owned()),
            _ => None,
        };
        assert_eq!(
            Prefs::config_dir_with(lookup),
            Some(PathBuf::from("/custom"))
        );

        let lookup = |name: &str| match name {
            "APPDATA" => Some("/appdata".to_owned()),
            "HOME" => Some("/home/someone".to_owned()),
            _ => None,
        };
        assert_eq!(
            Prefs::config_dir_with(lookup),
            Some(PathBuf::from("/appdata").join("Majestat"))
        );
    }

    #[test]
    fn the_configuration_directory_falls_back_to_unix_convention() {
        let lookup = |name: &str| match name {
            "XDG_CONFIG_HOME" => Some("/xdg".to_owned()),
            _ => None,
        };
        assert_eq!(
            Prefs::config_dir_with(lookup),
            Some(PathBuf::from("/xdg").join("majestat"))
        );

        let lookup = |name: &str| match name {
            "HOME" => Some("/home/someone".to_owned()),
            _ => None,
        };
        assert_eq!(
            Prefs::config_dir_with(lookup),
            Some(PathBuf::from("/home/someone/.config").join("majestat"))
        );

        let empty = |_: &str| None;
        assert_eq!(Prefs::config_dir_with(empty), None);
    }

    #[test]
    fn an_empty_override_is_ignored_rather_than_taken_literally() {
        let lookup = |name: &str| match name {
            CONFIG_DIR_VAR => Some("   ".to_owned()),
            "HOME" => Some("/home/someone".to_owned()),
            _ => None,
        };
        assert_eq!(
            Prefs::config_dir_with(lookup),
            Some(PathBuf::from("/home/someone/.config").join("majestat"))
        );
    }
}

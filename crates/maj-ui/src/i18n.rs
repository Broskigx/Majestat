//! The interface's two languages.
//!
//! Every string the interface itself says lives here, in both languages. The table is a pair
//! of exhaustive `match`es rather than a lookup of pairs, which means the compiler refuses to
//! build until a new key has been written in *both* languages — a missing translation is a
//! build error, not a blank label a person finds in a screenshot.
//!
//! The language is stored by its short name (`en`, `es`) rather than by its variant name, so
//! a preferences file that a person opens by hand reads the way a person would write it.
//!
//! What is deliberately **not** translated: the engine's own words. A refusal from the model
//! reads exactly as `maj-core` wrote it, and a command's kind is its own snake_case name from
//! the log, in every language. Paraphrasing those would be a second place for the truth to
//! live, and the two copies would drift apart at the first schema change.

/// A language the interface speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    /// English, the language of the repository.
    English,
    /// Spanish.
    Spanish,
}

/// Every language, in the order the switcher lists them.
pub const LANGS: [Lang; 2] = [Lang::English, Lang::Spanish];

impl Lang {
    /// The short stable name, used in preferences and in the status bar.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Spanish => "es",
        }
    }

    /// Reads a language back from its short name.
    #[must_use]
    pub fn from_key(value: &str) -> Option<Self> {
        LANGS.into_iter().find(|lang| lang.key() == value)
    }
}

impl serde::Serialize for Lang {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.key())
    }
}

impl<'de> serde::Deserialize<'de> for Lang {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::from_key(&value)
            .ok_or_else(|| serde::de::Error::custom(format!("`{value}` is not a language")))
    }
}

/// One string the interface says. Names are the *label*, never the sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// The application's own name, which is the same in both languages.
    AppName,
    /// The application menu.
    MenuApp,
    /// The File menu.
    MenuFile,
    /// The Edit menu.
    MenuEdit,
    /// The Sequence menu.
    MenuSequence,
    /// The View menu.
    MenuView,
    /// The Tools menu.
    MenuTools,
    /// The Help menu.
    MenuHelp,
    /// The language submenu.
    MenuLanguage,
    /// New project.
    FileNew,
    /// Open a document.
    FileOpen,
    /// Save.
    FileSave,
    /// Save to a new path.
    FileSaveAs,
    /// Build the demo project out of real commands.
    FileExample,
    /// Write the frame under the playhead as a still, with its manifest.
    FileExportStill,
    /// Quit.
    FileQuit,
    /// Undo.
    EditUndo,
    /// Redo.
    EditRedo,
    /// Remove the selected clip.
    EditRemove,
    /// Duplicate the selected clip.
    EditDuplicate,
    /// Split the selected clip at the playhead.
    EditSplit,
    /// Put the playhead on the previous edit point.
    EditPreviousEdit,
    /// Put the playhead on the next edit point.
    EditNextEdit,
    /// Select the previous clip.
    EditSelectPrevious,
    /// Select the next clip.
    EditSelectNext,
    /// Close the gap in front of the selected clip.
    EditCloseGap,
    /// Remove the selected clip and close the gap it leaves.
    EditRippleDelete,
    /// Open a one-second gap at the playhead.
    EditInsertBlank,
    /// Add a video track.
    EditAddVideoTrack,
    /// Add an audio track.
    EditAddAudioTrack,
    /// The command palette.
    ViewPalette,
    /// Zoom in.
    ViewZoomIn,
    /// Zoom out.
    ViewZoomOut,
    /// Step one frame back.
    ViewStepBack,
    /// Step one frame forward.
    ViewStepForward,
    /// Go to the start.
    ViewGoToStart,
    /// Go to the end.
    ViewGoToEnd,
    /// The media pool panel.
    PanelMedia,
    /// The history panel.
    PanelHistory,
    /// The source monitor.
    PanelSource,
    /// The program monitor.
    PanelProgram,
    /// The effect controls panel.
    PanelEffects,
    /// The timeline panel.
    PanelTimeline,
    /// The audio panel.
    PanelAudio,
    /// Draft resolution: the whole frame.
    DraftFull,
    /// Draft resolution: half.
    DraftHalf,
    /// Draft resolution: a quarter.
    DraftQuarter,
    /// The tiled rendering switch.
    MonitorTiled,
    /// The transfer curve selector.
    MonitorTransfer,
    /// The frame number.
    StatusFrame,
    /// The timecode.
    StatusTimecode,
    /// The project revision.
    StatusRevision,
    /// The undo and redo depths.
    StatusHistory,
    /// The state digest.
    StatusDigest,
    /// The preview's render time.
    StatusRenderTime,
    /// The label that says the preview is not full resolution.
    StatusDraft,
    /// The state of the document: saved or not.
    StatusSaved,
    /// The state of the document: changed since the last save.
    StatusUnsaved,
    /// The language switcher's own label.
    StatusLanguage,
    /// The palette's search box.
    PaletteSearch,
    /// The palette's argument entry.
    PaletteArguments,
    /// Run the row under the selection.
    PaletteRun,
    /// Close the palette.
    PaletteClose,
    /// The palette found nothing.
    PaletteEmpty,
    /// The audio panel's honest placeholder.
    AudioNotBuilt,
    /// The media panel's honest placeholder.
    MediaUndecoded,
    /// The playback statement: there is no engine yet.
    PlaybackNotBuilt,
    /// The exporter's honest placeholder.
    ExportStillsOnly,
    /// A label for the clip's opacity.
    EffectsOpacity,
    /// The heading over the inspector's keyframe rows.
    InspectorKeyframes,
    /// The horizontal position's name in the inspector.
    InspectorPositionX,
    /// The vertical position's name in the inspector.
    InspectorPositionY,
    /// The horizontal scale's name in the inspector.
    InspectorScaleX,
    /// The vertical scale's name in the inspector.
    InspectorScaleY,
    /// The rotation's name in the inspector.
    InspectorRotation,
    /// What a diamond does when there is no key under the playhead.
    InspectorKeyAdd,
    /// What it does when there is one.
    InspectorKeyRemove,
    /// What it does with shift held.
    InspectorKeyClear,
    /// A label for the clip's blend mode.
    EffectsBlend,
    /// A label for the clip's speed.
    EffectsSpeed,
    /// The transform group.
    EffectsTransform,
    /// The effect chain group.
    EffectsChain,
    /// The mask group.
    EffectsMasks,
    /// The transition group.
    EffectsTransitions,
    /// The label of a newly added media asset.
    MediaNewName,
    /// The label of the first video track.
    TrackVideoName,
    /// The label of the first audio track.
    TrackAudioName,
    /// The name a new sequence is given.
    SequenceNewName,
    /// The refusal shown when a document cannot be read.
    ErrorOpen,
    /// The refusal shown when a document cannot be written.
    ErrorSave,
    /// The refusal shown when a frame cannot be rendered.
    ErrorRender,
    /// A heading for the panel that states what an engine part cannot do yet.
    NotBuiltHeading,
    /// The media page's tab.
    PageMedia,
    /// The cut page's tab.
    PageCut,
    /// The edit page's tab.
    PageEdit,
    /// The fusion page's tab.
    PageFusion,
    /// The colour page's tab.
    PageColor,
    /// The fairlight page's tab.
    PageFairlight,
    /// The deliver page's tab.
    PageDeliver,
    /// What the edit page says while M6 owns its operations.
    PageOwnerEdit,
    /// What the fusion page says while M4 owns its graph.
    PageOwnerFusion,
    /// What the colour page says while M5 owns its colour management.
    PageOwnerColor,
    /// What the fairlight page says while M9 owns its mixer.
    PageOwnerFairlight,
    /// Move to the next page.
    ViewCyclePage,
    /// Fit the whole sequence into the lane.
    ViewZoomFit,
    /// Snap a dragged edge to the playhead and to other clips.
    ViewSnap,
    /// Keep the selection linked while editing.
    ViewLink,
    /// Put the dividers back where they started.
    ViewResetPanels,
    /// The layout group.
    ViewLayout,
    /// Trim the head of the selection to the playhead.
    ViewTrimIn,
    /// Trim the tail of the selection to the playhead.
    ViewTrimOut,
    /// The preview resolution switcher.
    ViewDraft,
    /// Enable or disable the selected clip.
    EditToggleEnabled,
    /// The agent's tool catalogue, as the palette.
    ToolsAgent,
    /// The about card.
    HelpAbout,
    /// What the about card says the application is.
    AboutLine,
    /// Where the keys are written down.
    AboutKeys,
    /// Minimise the window.
    FrameMinimize,
    /// Maximise or restore the window.
    FrameMaximize,
    /// Close the window.
    FrameClose,
    /// The title of the dialog that asks for a document.
    DialogProject,
    /// The title of the dialog that asks where to write a still.
    DialogStill,
    /// The status bar's label for the clock.
    StatusTransport,
    /// The short statement that there is no audio engine yet.
    AudioNotBuiltShort,
    /// The short statement that media is not decoded yet.
    MediaUndecodedShort,
    /// What the monitor says with no sequence to show.
    ViewerEmpty,
    /// What a control says when nothing is selected.
    NothingSelected,
    /// What the inspector says when nothing is selected.
    InspectorEmpty,
    /// What a divider says under the pointer.
    SplitterResize,
    /// The transport: the first frame.
    TransportStart,
    /// The transport: the frame after the last one.
    TransportEnd,
    /// The transport: start the clock.
    TransportPlay,
    /// The transport: stop it.
    TransportPause,
    /// The transport: start again at the end.
    TransportLoop,
    /// The transport's own timecode field.
    TransportTimecode,
    /// The refusal for a frame number that is not one.
    TransportBadTimecode,
    /// Dragging in the ruler.
    TransportScrub,
    /// A track's header.
    TrackHeader,
    /// A video track.
    TrackKindVideo,
    /// An audio track.
    TrackKindAudio,
    /// The track's enable switch.
    TrackEnable,
    /// The track's mute switch.
    TrackMute,
    /// The track's lock switch.
    TrackLock,
    /// Why the mute switch does nothing yet.
    TrackMuteOwner,
    /// Why the lock switch does nothing yet.
    TrackLockOwner,
    /// The body of a clip: dragging it moves the clip.
    ClipBody,
    /// The head of a clip: dragging it trims.
    ClipTrimHead,
    /// The tail of a clip: dragging it trims.
    ClipTrimTail,
    /// A media tile, placed at the playhead.
    MediaInsert,
    /// The inspector tab.
    PanelInspector,
    /// The mixer tab, which has no engine behind it.
    PanelMixer,
    /// The deliver page's render button.
    DeliverRender,
    /// The deliver page's summary: what would be written.
    ExportWhat,
    /// The deliver page's summary: at what size.
    ExportSize,
    /// The deliver page's summary: at what rate.
    ExportRate,
    /// The deliver page's summary: where.
    ExportPath,
    /// The label of the effect controls' numeric range.
    EffectsRange,
    /// The toolbar's marker tool, which the model cannot hold yet.
    MarkerTool,
    /// Why the marker tool does nothing: the phase that owns the entity.
    MarkerOwner,
    /// The left column's tab for the title studio.
    TabTitles,
    /// The right column's tab for the list of effects.
    BrowserEffects,
    /// The right column's tab for the list of transitions.
    BrowserTransitions,
    /// The pool's button for bringing a still in from disk.
    MediaImport,
    /// What importing a still does, in one sentence.
    MediaImportNote,
    /// The title studio's field: the words.
    TitleWords,
    /// The title studio's row of faces.
    TitleFace,
    /// The title studio's field: the cap height.
    TitleCap,
    /// The title studio's row of alignments.
    TitleAlign,
    /// The studio's button: set the words and put them on the timeline.
    TitleAdd,
    /// The caption over the URI the studio would hand the renderer.
    TitleUri,
    /// What the faces are, and what they are not.
    TitleFaces,
    /// What the studio says when the field is empty.
    TitleEmpty,
    /// A face: even weight, square proportions.
    FaceSans,
    /// A face: thinner strokes with serifs.
    FaceSerif,
    /// A face: one advance for every glyph.
    FaceMono,
    /// A face: heavy strokes at a little over half width.
    FaceCondensed,
    /// An alignment: against the left margin.
    AlignLeft,
    /// An alignment: centred.
    AlignCentre,
    /// An alignment: against the right margin.
    AlignRight,
    /// An effect: exposure.
    EffectExposure,
    /// An effect: brightness.
    EffectBrightness,
    /// An effect: contrast.
    EffectContrast,
    /// An effect: saturation.
    EffectSaturation,
    /// An effect: gamma.
    EffectGamma,
    /// An effect: hue rotation.
    EffectHueRotate,
    /// An effect: blur.
    EffectBlur,
    /// An effect: sharpen.
    EffectSharpen,
    /// An effect: vignette.
    EffectVignette,
    /// An effect: film grain.
    EffectGrain,
    /// A transition: cross dissolve.
    TransitionCrossDissolve,
    /// A transition: dip to black.
    TransitionDipToBlack,
    /// A transition: dip to white.
    TransitionDipToWhite,
    /// A transition: wipe left.
    TransitionWipeLeft,
    /// A transition: wipe right.
    TransitionWipeRight,
    /// A transition: wipe up.
    TransitionWipeUp,
    /// A transition: wipe down.
    TransitionWipeDown,
    /// A transition: a circle.
    TransitionCircle,
    /// What clicking an effect in the browser does.
    EffectListNote,
    /// What clicking a transition in the browser does.
    TransitionListNote,
}

/// The English string for a key.
#[must_use]
pub const fn en(key: Key) -> &'static str {
    match key {
        Key::AppName => "Majestat",
        Key::MenuApp => "Majestat",
        Key::MenuFile => "File",
        Key::MenuEdit => "Edit",
        Key::MenuSequence => "Sequence",
        Key::MenuView => "View",
        Key::MenuTools => "Tools",
        Key::MenuHelp => "Help",
        Key::MenuLanguage => "Language",
        Key::FileNew => "New",
        Key::FileOpen => "Open…",
        Key::FileSave => "Save",
        Key::FileSaveAs => "Save as…",
        Key::FileExample => "New from example",
        Key::FileExportStill => "Export still…",
        Key::FileQuit => "Quit",
        Key::EditUndo => "Undo",
        Key::EditRedo => "Redo",
        Key::EditRemove => "Remove clip",
        Key::EditDuplicate => "Duplicate clip",
        Key::EditSplit => "Split at playhead",
        Key::EditPreviousEdit => "Previous edit point",
        Key::EditNextEdit => "Next edit point",
        Key::EditSelectPrevious => "Select previous clip",
        Key::EditSelectNext => "Select next clip",
        Key::EditCloseGap => "Close gap",
        Key::EditRippleDelete => "Ripple delete",
        Key::EditInsertBlank => "Insert blank",
        Key::EditAddVideoTrack => "Add video track",
        Key::EditAddAudioTrack => "Add audio track",
        Key::ViewPalette => "Command palette",
        Key::ViewZoomIn => "Zoom in",
        Key::ViewZoomOut => "Zoom out",
        Key::ViewStepBack => "Previous frame",
        Key::ViewStepForward => "Next frame",
        Key::ViewGoToStart => "Go to start",
        Key::ViewGoToEnd => "Go to end",
        Key::PanelMedia => "Media pool",
        Key::PanelHistory => "History",
        Key::PanelSource => "Source",
        Key::PanelProgram => "Program",
        Key::PanelEffects => "Effect controls",
        Key::PanelTimeline => "Timeline",
        Key::PanelAudio => "Audio",
        Key::DraftFull => "Full",
        Key::DraftHalf => "Half",
        Key::DraftQuarter => "Quarter",
        Key::MonitorTiled => "Tiled",
        Key::MonitorTransfer => "Curve",
        Key::StatusFrame => "frame",
        Key::StatusTimecode => "timecode",
        Key::StatusRevision => "revision",
        Key::StatusHistory => "history",
        Key::StatusDigest => "digest",
        Key::StatusRenderTime => "render",
        Key::StatusDraft => "draft",
        Key::StatusSaved => "saved",
        Key::StatusUnsaved => "unsaved",
        Key::StatusLanguage => "language",
        Key::PaletteSearch => "Search commands and tools",
        Key::PaletteArguments => "Arguments",
        Key::PaletteRun => "Run",
        Key::PaletteClose => "Close",
        Key::PaletteEmpty => "Nothing matches",
        Key::AudioNotBuilt => "No audio engine yet: sound is M9, and this panel is not pretending.",
        Key::MediaUndecoded => {
            "This build decodes stills only. Media it cannot read draws a labelled placeholder — offline, damaged or undecodable — and the manifest says which: that is M2."
        }
        Key::PlaybackNotBuilt => {
            "There is no playback engine yet, so there is no play button: that is M3."
        }
        Key::ExportStillsOnly => {
            "The exporter writes PNG and PPM with a manifest. Video encoding is M8."
        }
        Key::EffectsOpacity => "Opacity",
        Key::InspectorKeyframes => "Keyframes",
        Key::InspectorPositionX => "Position X",
        Key::InspectorPositionY => "Position Y",
        Key::InspectorScaleX => "Scale X",
        Key::InspectorScaleY => "Scale Y",
        Key::InspectorRotation => "Rotation",
        Key::InspectorKeyAdd => "Put a keyframe on this frame",
        Key::InspectorKeyRemove => "Take the keyframe off this frame",
        Key::InspectorKeyClear => "Clear this channel's keyframes",
        Key::EffectsBlend => "Blend",
        Key::EffectsSpeed => "Speed",
        Key::EffectsTransform => "Transform",
        Key::EffectsChain => "Effects",
        Key::EffectsMasks => "Masks",
        Key::EffectsTransitions => "Transitions",
        Key::MediaNewName => "Media",
        Key::TrackVideoName => "V",
        Key::TrackAudioName => "A",
        Key::SequenceNewName => "Sequence",
        Key::ErrorOpen => "That document could not be opened",
        Key::ErrorSave => "That document could not be written",
        Key::ErrorRender => "That frame could not be rendered",
        Key::NotBuiltHeading => "Not built yet",
        Key::PageMedia => "Media",
        Key::PageCut => "Cut",
        Key::PageEdit => "Edit",
        Key::PageFusion => "Fusion",
        Key::PageColor => "Color",
        Key::PageFairlight => "Fairlight",
        Key::PageDeliver => "Deliver",
        Key::PageOwnerEdit => {
            "The editorial operations this page shows are M6, and they are not here yet."
        }
        Key::PageOwnerFusion => "The node graph this page shows is M4, and it is not here yet.",
        Key::PageOwnerColor => {
            "The colour management this page shows is M5, and it is not here yet."
        }
        Key::PageOwnerFairlight => "The mixer this page shows is M9, and it is not here yet.",
        Key::ViewCyclePage => "Next page",
        Key::ViewZoomFit => "Fit the sequence",
        Key::ViewSnap => "Snap to edges",
        Key::ViewLink => "Link the selection",
        Key::ViewResetPanels => "Reset the panels",
        Key::ViewLayout => "Layout",
        Key::ViewTrimIn => "Trim the head to the playhead",
        Key::ViewTrimOut => "Trim the tail to the playhead",
        Key::ViewDraft => "Preview resolution",
        Key::EditToggleEnabled => "Enable or disable the clip",
        Key::ToolsAgent => "The agent's tools",
        Key::HelpAbout => "About Majestat",
        Key::AboutLine => "A deterministic editing engine, one command log, two ways in.",
        Key::AboutKeys => "Every key is listed in docs/UI.md.",
        Key::FrameMinimize => "Minimise",
        Key::FrameMaximize => "Maximise",
        Key::FrameClose => "Close",
        Key::DialogProject => "Choose a document",
        Key::DialogStill => "Choose where to write the still",
        Key::StatusTransport => "transport",
        Key::AudioNotBuiltShort => "no audio engine (M9)",
        Key::MediaUndecodedShort => "not decoded (M2)",
        Key::ViewerEmpty => "There is no sequence to show.",
        Key::NothingSelected => "Nothing is selected.",
        Key::InspectorEmpty => "Select a clip to see its controls.",
        Key::SplitterResize => "Drag to resize",
        Key::TransportStart => "Go to start",
        Key::TransportEnd => "Go to end",
        Key::TransportPlay => "Play",
        Key::TransportPause => "Pause",
        Key::TransportLoop => "Play again at the end",
        Key::TransportTimecode => "Timecode",
        Key::TransportBadTimecode => "That is neither a frame number nor a timecode.",
        Key::TransportScrub => "Scrub",
        Key::TrackHeader => "Track",
        Key::TrackKindVideo => "Video",
        Key::TrackKindAudio => "Audio",
        Key::TrackEnable => "Enable the track",
        Key::TrackMute => "Mute the track",
        Key::TrackLock => "Lock the track",
        Key::TrackMuteOwner => "There is no audio model yet: muting is M9.",
        Key::TrackLockOwner => "Nothing enforces a lock yet: that is M6.",
        Key::ClipBody => "Drag to move the clip",
        Key::ClipTrimHead => "Drag to trim the head",
        Key::ClipTrimTail => "Drag to trim the tail",
        Key::MediaInsert => "Insert at the playhead",
        Key::PanelInspector => "Inspector",
        Key::PanelMixer => "Mixer",
        Key::DeliverRender => "Render a still",
        Key::ExportWhat => "What",
        Key::ExportSize => "Size",
        Key::ExportRate => "Rate",
        Key::ExportPath => "To",
        Key::EffectsRange => "Range",
        Key::MarkerTool => "Add marker",
        Key::MarkerOwner => "Markers arrive with M6: the model has no marker entity yet.",
        Key::TabTitles => "Titles",
        Key::BrowserEffects => "Effects",
        Key::BrowserTransitions => "Transitions",
        Key::MediaImport => "Add media…",
        Key::MediaImportNote => {
            "A still on disk — PNG or PPM — is registered, decoded and placed like any other shot. A movie is not read yet: that is M2."
        }
        Key::TitleWords => "Words",
        Key::TitleFace => "Face",
        Key::TitleCap => "Cap height",
        Key::TitleAlign => "Align",
        Key::TitleAdd => "Add a title at the playhead",
        Key::TitleUri => "What the renderer is told",
        Key::TitleFaces => {
            "Four faces are built into the renderer. No font file is read, so the same words in the same face are the same pixels on every machine."
        }
        Key::TitleEmpty => "Type the words: a title with no words is not a title.",
        Key::FaceSans => "Sans",
        Key::FaceSerif => "Serif",
        Key::FaceMono => "Mono",
        Key::FaceCondensed => "Condensed",
        Key::AlignLeft => "Left",
        Key::AlignCentre => "Centre",
        Key::AlignRight => "Right",
        Key::EffectExposure => "Exposure",
        Key::EffectBrightness => "Brightness",
        Key::EffectContrast => "Contrast",
        Key::EffectSaturation => "Saturation",
        Key::EffectGamma => "Gamma",
        Key::EffectHueRotate => "Hue",
        Key::EffectBlur => "Blur",
        Key::EffectSharpen => "Sharpen",
        Key::EffectVignette => "Vignette",
        Key::EffectGrain => "Grain",
        Key::TransitionCrossDissolve => "Cross dissolve",
        Key::TransitionDipToBlack => "Dip to black",
        Key::TransitionDipToWhite => "Dip to white",
        Key::TransitionWipeLeft => "Wipe left",
        Key::TransitionWipeRight => "Wipe right",
        Key::TransitionWipeUp => "Wipe up",
        Key::TransitionWipeDown => "Wipe down",
        Key::TransitionCircle => "Circle",
        Key::EffectListNote => {
            "Click one to add it to the selected clip. The parameters belong to the engine, and a kind it cannot run is refused rather than approximated."
        }
        Key::TransitionListNote => {
            "Click one to add it at the playhead, between the clip under it and whatever comes before."
        }
    }
}

/// The Spanish string for a key.
#[must_use]
pub const fn es(key: Key) -> &'static str {
    match key {
        Key::AppName => "Majestat",
        Key::MenuApp => "Majestat",
        Key::MenuFile => "Archivo",
        Key::MenuEdit => "Edición",
        Key::MenuSequence => "Secuencia",
        Key::MenuView => "Ver",
        Key::MenuTools => "Herramientas",
        Key::MenuHelp => "Ayuda",
        Key::MenuLanguage => "Idioma",
        Key::FileNew => "Nuevo",
        Key::FileOpen => "Abrir…",
        Key::FileSave => "Guardar",
        Key::FileSaveAs => "Guardar como…",
        Key::FileExample => "Nuevo desde el ejemplo",
        Key::FileExportStill => "Exportar fotograma…",
        Key::FileQuit => "Salir",
        Key::EditUndo => "Deshacer",
        Key::EditRedo => "Rehacer",
        Key::EditRemove => "Quitar clip",
        Key::EditDuplicate => "Duplicar clip",
        Key::EditSplit => "Dividir en el cursor",
        Key::EditPreviousEdit => "Punto de edición anterior",
        Key::EditNextEdit => "Punto de edición siguiente",
        Key::EditSelectPrevious => "Seleccionar el clip anterior",
        Key::EditSelectNext => "Seleccionar el clip siguiente",
        Key::EditCloseGap => "Cerrar el hueco",
        Key::EditRippleDelete => "Borrar y cerrar el hueco",
        Key::EditInsertBlank => "Insertar un hueco",
        Key::EditAddVideoTrack => "Añadir pista de vídeo",
        Key::EditAddAudioTrack => "Añadir pista de audio",
        Key::ViewPalette => "Paleta de comandos",
        Key::ViewZoomIn => "Acercar",
        Key::ViewZoomOut => "Alejar",
        Key::ViewStepBack => "Fotograma anterior",
        Key::ViewStepForward => "Fotograma siguiente",
        Key::ViewGoToStart => "Ir al principio",
        Key::ViewGoToEnd => "Ir al final",
        Key::PanelMedia => "Medios",
        Key::PanelHistory => "Historial",
        Key::PanelSource => "Origen",
        Key::PanelProgram => "Programa",
        Key::PanelEffects => "Controles de efecto",
        Key::PanelTimeline => "Línea de tiempo",
        Key::PanelAudio => "Audio",
        Key::DraftFull => "Completa",
        Key::DraftHalf => "Mitad",
        Key::DraftQuarter => "Cuarto",
        Key::MonitorTiled => "Teselado",
        Key::MonitorTransfer => "Curva",
        Key::StatusFrame => "fotograma",
        Key::StatusTimecode => "código de tiempo",
        Key::StatusRevision => "revisión",
        Key::StatusHistory => "historial",
        Key::StatusDigest => "resumen",
        Key::StatusRenderTime => "render",
        Key::StatusDraft => "borrador",
        Key::StatusSaved => "guardado",
        Key::StatusUnsaved => "sin guardar",
        Key::StatusLanguage => "idioma",
        Key::PaletteSearch => "Buscar comandos y herramientas",
        Key::PaletteArguments => "Argumentos",
        Key::PaletteRun => "Ejecutar",
        Key::PaletteClose => "Cerrar",
        Key::PaletteEmpty => "Nada coincide",
        Key::AudioNotBuilt => {
            "Todavía no hay motor de audio: el sonido es M9, y este panel no lo finge."
        }
        Key::MediaUndecoded => {
            "Esta compilación solo decodifica imágenes fijas. Lo que no puede leer dibuja un \
             marcador etiquetado — sin conexión, dañado o sin decodificador — y el manifiesto \
             dice cuál: eso es M2."
        }
        Key::PlaybackNotBuilt => {
            "Todavía no hay motor de reproducción, así que no hay botón de reproducir: eso es M3."
        }
        Key::ExportStillsOnly => {
            "El exportador escribe PNG y PPM con manifiesto. La codificación de vídeo es M8."
        }
        Key::EffectsOpacity => "Opacidad",
        Key::InspectorKeyframes => "Fotogramas clave",
        Key::InspectorPositionX => "Posición X",
        Key::InspectorPositionY => "Posición Y",
        Key::InspectorScaleX => "Escala X",
        Key::InspectorScaleY => "Escala Y",
        Key::InspectorRotation => "Rotación",
        Key::InspectorKeyAdd => "Poner un fotograma clave en este fotograma",
        Key::InspectorKeyRemove => "Quitar el fotograma clave de este fotograma",
        Key::InspectorKeyClear => "Borrar los fotogramas clave de este canal",
        Key::EffectsBlend => "Fusión",
        Key::EffectsSpeed => "Velocidad",
        Key::EffectsTransform => "Transformación",
        Key::EffectsChain => "Efectos",
        Key::EffectsMasks => "Máscaras",
        Key::EffectsTransitions => "Transiciones",
        Key::MediaNewName => "Medio",
        Key::TrackVideoName => "V",
        Key::TrackAudioName => "A",
        Key::SequenceNewName => "Secuencia",
        Key::ErrorOpen => "No se pudo abrir ese documento",
        Key::ErrorSave => "No se pudo escribir ese documento",
        Key::ErrorRender => "No se pudo dibujar ese fotograma",
        Key::NotBuiltHeading => "Aún sin construir",
        Key::PageMedia => "Medios",
        Key::PageCut => "Montaje",
        Key::PageEdit => "Edición",
        Key::PageFusion => "Fusión",
        Key::PageColor => "Color",
        Key::PageFairlight => "Fairlight",
        Key::PageDeliver => "Entrega",
        Key::PageOwnerEdit => {
            "Las operaciones de montaje que muestra esta página son M6, y todavía no están."
        }
        Key::PageOwnerFusion => {
            "El grafo de nodos que muestra esta página es M4, y todavía no está."
        }
        Key::PageOwnerColor => {
            "La gestión de color que muestra esta página es M5, y todavía no está."
        }
        Key::PageOwnerFairlight => "El mezclador que muestra esta página es M9, y todavía no está.",
        Key::ViewCyclePage => "Página siguiente",
        Key::ViewZoomFit => "Ajustar la secuencia",
        Key::ViewSnap => "Imán en los bordes",
        Key::ViewLink => "Vincular la selección",
        Key::ViewResetPanels => "Restaurar los paneles",
        Key::ViewLayout => "Disposición",
        Key::ViewTrimIn => "Recortar el inicio al cursor",
        Key::ViewTrimOut => "Recortar el final al cursor",
        Key::ViewDraft => "Resolución de la vista",
        Key::EditToggleEnabled => "Activar o desactivar el clip",
        Key::ToolsAgent => "Las herramientas del agente",
        Key::HelpAbout => "Acerca de Majestat",
        Key::AboutLine => {
            "Un motor de edición determinista, un solo registro de comandos, dos puertas."
        }
        Key::AboutKeys => "Todas las teclas están en docs/UI.md.",
        Key::FrameMinimize => "Minimizar",
        Key::FrameMaximize => "Maximizar",
        Key::FrameClose => "Cerrar",
        Key::DialogProject => "Elige un documento",
        Key::DialogStill => "Elige dónde escribir el fotograma",
        Key::StatusTransport => "transporte",
        Key::AudioNotBuiltShort => "sin motor de audio (M9)",
        Key::MediaUndecodedShort => "sin decodificar (M2)",
        Key::ViewerEmpty => "No hay ninguna secuencia que mostrar.",
        Key::NothingSelected => "No hay nada seleccionado.",
        Key::InspectorEmpty => "Selecciona un clip para ver sus controles.",
        Key::SplitterResize => "Arrastra para redimensionar",
        Key::TransportStart => "Ir al principio",
        Key::TransportEnd => "Ir al final",
        Key::TransportPlay => "Reproducir",
        Key::TransportPause => "Pausar",
        Key::TransportLoop => "Repetir al final",
        Key::TransportTimecode => "Código de tiempo",
        Key::TransportBadTimecode => "Eso no es ni un fotograma ni un código de tiempo.",
        Key::TransportScrub => "Desplazar",
        Key::TrackHeader => "Pista",
        Key::TrackKindVideo => "Vídeo",
        Key::TrackKindAudio => "Audio",
        Key::TrackEnable => "Activar la pista",
        Key::TrackMute => "Silenciar la pista",
        Key::TrackLock => "Bloquear la pista",
        Key::TrackMuteOwner => "Todavía no hay modelo de audio: silenciar es M9.",
        Key::TrackLockOwner => "Nada hace cumplir el bloqueo todavía: eso es M6.",
        Key::ClipBody => "Arrastra para mover el clip",
        Key::ClipTrimHead => "Arrastra para recortar el inicio",
        Key::ClipTrimTail => "Arrastra para recortar el final",
        Key::MediaInsert => "Insertar en el cursor",
        Key::PanelInspector => "Inspector",
        Key::PanelMixer => "Mezclador",
        Key::DeliverRender => "Escribir un fotograma",
        Key::ExportWhat => "Qué",
        Key::ExportSize => "Tamaño",
        Key::ExportRate => "Cadencia",
        Key::ExportPath => "Destino",
        Key::EffectsRange => "Rango",
        Key::MarkerTool => "Añadir marcador",
        Key::MarkerOwner => "Los marcadores llegan con M6: el modelo aún no los tiene.",
        Key::TabTitles => "Títulos",
        Key::BrowserEffects => "Efectos",
        Key::BrowserTransitions => "Transiciones",
        Key::MediaImport => "Agregar medio…",
        Key::MediaImportNote => {
            "Una imagen fija en disco — PNG o PPM — se registra, se decodifica y se coloca como cualquier otro plano. Una película aún no se lee: eso es M2."
        }
        Key::TitleWords => "Palabras",
        Key::TitleFace => "Fuente",
        Key::TitleCap => "Altura",
        Key::TitleAlign => "Alineación",
        Key::TitleAdd => "Añadir un título en el cabezal",
        Key::TitleUri => "Lo que se le dice al renderizador",
        Key::TitleFaces => {
            "Las cuatro fuentes son propias del renderizador. No se lee ningún archivo de fuentes, así que las mismas palabras en la misma fuente son los mismos píxeles en cualquier máquina."
        }
        Key::TitleEmpty => "Escribe las palabras: un título sin palabras no es un título.",
        Key::FaceSans => "Paloseco",
        Key::FaceSerif => "Con serifas",
        Key::FaceMono => "Monoespaciada",
        Key::FaceCondensed => "Condensada",
        Key::AlignLeft => "Izquierda",
        Key::AlignCentre => "Centro",
        Key::AlignRight => "Derecha",
        Key::EffectExposure => "Exposición",
        Key::EffectBrightness => "Brillo",
        Key::EffectContrast => "Contraste",
        Key::EffectSaturation => "Saturación",
        Key::EffectGamma => "Gama",
        Key::EffectHueRotate => "Tono",
        Key::EffectBlur => "Desenfoque",
        Key::EffectSharpen => "Nitidez",
        Key::EffectVignette => "Viñeta",
        Key::EffectGrain => "Grano",
        Key::TransitionCrossDissolve => "Disolvencia",
        Key::TransitionDipToBlack => "Fundido a negro",
        Key::TransitionDipToWhite => "Fundido a blanco",
        Key::TransitionWipeLeft => "Barrido a la izquierda",
        Key::TransitionWipeRight => "Barrido a la derecha",
        Key::TransitionWipeUp => "Barrido hacia arriba",
        Key::TransitionWipeDown => "Barrido hacia abajo",
        Key::TransitionCircle => "Círculo",
        Key::EffectListNote => {
            "Haz clic para añadirlo al clip seleccionado. Los parámetros son del motor, y un tipo que no puede ejecutar se rechaza en lugar de aproximarse."
        }
        Key::TransitionListNote => {
            "Haz clic para añadirlo en el cabezal, entre el clip que hay debajo y lo que venga antes."
        }
    }
}

/// The string for a key in a language.
#[must_use]
pub const fn text(lang: Lang, key: Key) -> &'static str {
    match lang {
        Lang::English => en(key),
        Lang::Spanish => es(key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every key, so a test can walk the whole table. Rust has no reflection, so the list is
    /// written out; the exhaustive `match`es above are what stops it from going stale without
    /// a build failure, and `every_key_is_listed` is what keeps this list honest.
    const ALL: &[Key] = &[
        Key::AppName,
        Key::MenuApp,
        Key::MenuFile,
        Key::MenuEdit,
        Key::MenuSequence,
        Key::MenuView,
        Key::MenuTools,
        Key::MenuHelp,
        Key::MenuLanguage,
        Key::FileNew,
        Key::FileOpen,
        Key::FileSave,
        Key::FileSaveAs,
        Key::FileExample,
        Key::FileExportStill,
        Key::FileQuit,
        Key::EditUndo,
        Key::EditRedo,
        Key::EditRemove,
        Key::EditDuplicate,
        Key::EditSplit,
        Key::EditPreviousEdit,
        Key::EditNextEdit,
        Key::EditSelectPrevious,
        Key::EditSelectNext,
        Key::EditCloseGap,
        Key::EditRippleDelete,
        Key::EditInsertBlank,
        Key::EditAddVideoTrack,
        Key::EditAddAudioTrack,
        Key::ViewPalette,
        Key::ViewZoomIn,
        Key::ViewZoomOut,
        Key::ViewStepBack,
        Key::ViewStepForward,
        Key::ViewGoToStart,
        Key::ViewGoToEnd,
        Key::PanelMedia,
        Key::PanelHistory,
        Key::PanelSource,
        Key::PanelProgram,
        Key::PanelEffects,
        Key::PanelTimeline,
        Key::PanelAudio,
        Key::DraftFull,
        Key::DraftHalf,
        Key::DraftQuarter,
        Key::MonitorTiled,
        Key::MonitorTransfer,
        Key::StatusFrame,
        Key::StatusTimecode,
        Key::StatusRevision,
        Key::StatusHistory,
        Key::StatusDigest,
        Key::StatusRenderTime,
        Key::StatusDraft,
        Key::StatusSaved,
        Key::StatusUnsaved,
        Key::StatusLanguage,
        Key::PaletteSearch,
        Key::PaletteArguments,
        Key::PaletteRun,
        Key::PaletteClose,
        Key::PaletteEmpty,
        Key::AudioNotBuilt,
        Key::MediaUndecoded,
        Key::PlaybackNotBuilt,
        Key::ExportStillsOnly,
        Key::EffectsOpacity,
        Key::InspectorKeyframes,
        Key::InspectorPositionX,
        Key::InspectorPositionY,
        Key::InspectorScaleX,
        Key::InspectorScaleY,
        Key::InspectorRotation,
        Key::InspectorKeyAdd,
        Key::InspectorKeyRemove,
        Key::InspectorKeyClear,
        Key::EffectsBlend,
        Key::EffectsSpeed,
        Key::EffectsTransform,
        Key::EffectsChain,
        Key::EffectsMasks,
        Key::EffectsTransitions,
        Key::MediaNewName,
        Key::TrackVideoName,
        Key::TrackAudioName,
        Key::SequenceNewName,
        Key::ErrorOpen,
        Key::ErrorSave,
        Key::ErrorRender,
        Key::NotBuiltHeading,
        Key::PageMedia,
        Key::PageCut,
        Key::PageEdit,
        Key::PageFusion,
        Key::PageColor,
        Key::PageFairlight,
        Key::PageDeliver,
        Key::PageOwnerEdit,
        Key::PageOwnerFusion,
        Key::PageOwnerColor,
        Key::PageOwnerFairlight,
        Key::ViewCyclePage,
        Key::ViewZoomFit,
        Key::ViewSnap,
        Key::ViewLink,
        Key::ViewResetPanels,
        Key::ViewLayout,
        Key::ViewTrimIn,
        Key::ViewTrimOut,
        Key::ViewDraft,
        Key::EditToggleEnabled,
        Key::ToolsAgent,
        Key::HelpAbout,
        Key::AboutLine,
        Key::AboutKeys,
        Key::FrameMinimize,
        Key::FrameMaximize,
        Key::FrameClose,
        Key::DialogProject,
        Key::DialogStill,
        Key::StatusTransport,
        Key::AudioNotBuiltShort,
        Key::MediaUndecodedShort,
        Key::ViewerEmpty,
        Key::NothingSelected,
        Key::InspectorEmpty,
        Key::SplitterResize,
        Key::TransportStart,
        Key::TransportEnd,
        Key::TransportPlay,
        Key::TransportPause,
        Key::TransportLoop,
        Key::TransportTimecode,
        Key::TransportBadTimecode,
        Key::TransportScrub,
        Key::TrackHeader,
        Key::TrackKindVideo,
        Key::TrackKindAudio,
        Key::TrackEnable,
        Key::TrackMute,
        Key::TrackLock,
        Key::TrackMuteOwner,
        Key::TrackLockOwner,
        Key::ClipBody,
        Key::ClipTrimHead,
        Key::ClipTrimTail,
        Key::MediaInsert,
        Key::PanelInspector,
        Key::PanelMixer,
        Key::DeliverRender,
        Key::ExportWhat,
        Key::ExportSize,
        Key::ExportRate,
        Key::ExportPath,
        Key::EffectsRange,
        Key::MarkerTool,
        Key::MarkerOwner,
        Key::TabTitles,
        Key::BrowserEffects,
        Key::BrowserTransitions,
        Key::MediaImport,
        Key::MediaImportNote,
        Key::TitleWords,
        Key::TitleFace,
        Key::TitleCap,
        Key::TitleAlign,
        Key::TitleAdd,
        Key::TitleUri,
        Key::TitleFaces,
        Key::TitleEmpty,
        Key::FaceSans,
        Key::FaceSerif,
        Key::FaceMono,
        Key::FaceCondensed,
        Key::AlignLeft,
        Key::AlignCentre,
        Key::AlignRight,
        Key::EffectExposure,
        Key::EffectBrightness,
        Key::EffectContrast,
        Key::EffectSaturation,
        Key::EffectGamma,
        Key::EffectHueRotate,
        Key::EffectBlur,
        Key::EffectSharpen,
        Key::EffectVignette,
        Key::EffectGrain,
        Key::TransitionCrossDissolve,
        Key::TransitionDipToBlack,
        Key::TransitionDipToWhite,
        Key::TransitionWipeLeft,
        Key::TransitionWipeRight,
        Key::TransitionWipeUp,
        Key::TransitionWipeDown,
        Key::TransitionCircle,
        Key::EffectListNote,
        Key::TransitionListNote,
    ];

    #[test]
    fn every_key_is_listed_once() {
        let mut seen = std::collections::HashSet::new();
        for key in ALL {
            assert!(seen.insert(*key), "{key:?} is listed twice");
        }
        assert_eq!(seen.len(), ALL.len());
    }

    #[test]
    fn no_string_is_empty_in_either_language() {
        for key in ALL {
            for lang in LANGS {
                assert!(
                    !text(lang, *key).trim().is_empty(),
                    "{key:?} is empty in {lang:?}"
                );
            }
        }
    }

    #[test]
    fn the_spanish_table_is_not_the_english_one() {
        // Not every string differs — "Majestat" and "Audio" are the same word — but these do,
        // so a table that accidentally returns English from `es` fails here.
        for key in [
            Key::MenuFile,
            Key::MenuEdit,
            Key::FileSave,
            Key::EditUndo,
            Key::PanelTimeline,
            Key::StatusUnsaved,
            Key::ViewPalette,
        ] {
            assert_ne!(en(key), es(key), "{key:?} is untranslated");
        }
    }

    #[test]
    fn language_keys_round_trip() {
        for lang in LANGS {
            assert_eq!(Lang::from_key(lang.key()), Some(lang));
        }
        assert_eq!(Lang::from_key("fr"), None);
    }
}

//! The tool catalogue: the surface a client sees, declared as data.
//!
//! # Why this is one table instead of forty functions
//!
//! A tool has four faces: the name a model calls, the JSON Schema a client reads, the
//! arguments the server accepts, and the example in the description. Written by hand four
//! times they drift, and the drift is invisible until a model calls a tool with an argument
//! the server has never heard of. So all four come from one [`Tool`] value: [`input_schema`]
//! renders the schema from the same [`Field`] list that [`crate::args`] walks to decode the
//! arguments, and the examples below are compiled into the descriptions *and* executed by the
//! tests, which is the only reason to trust them.
//!
//! # Two kinds of field
//!
//! A tool's fields are either arguments *of the command* — `sequence`, `clip`, `enabled` —
//! or a whole entity the client is describing, which is why [`FieldKind::Object`] and
//! [`FieldKind::List`] carry field lists of their own. Nesting is the reason a single
//! `maj_create_sequence` call can hand over a sequence with its tracks in one go.
//!
//! # What is deliberately not here
//!
//! No tool takes a float. Every number a client sends is resolved to an exact
//! [`Rational`](maj_time::Rational), because the render is a pure function of the log and a
//! log that stores `0.1` stores a rounding nobody can reproduce. Decimals are still
//! *accepted* — `0.5` becomes `1/2` — but they are converted on arrival.

use serde_json::{Map, Value, json};

use maj_core::{
    AnimChannel, BlendMode, EffectKind, Interpolation, MaskShape, TrackKind, TransitionKind,
};

/// The name a client calls, and the shape of its arguments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tool {
    /// The name, as it appears in `tools/list`. Always prefixed `maj_`.
    pub name: &'static str,
    /// A short human title, for a client that shows one.
    pub title: &'static str,
    /// What the tool does, in the imperative, including what it will refuse.
    pub description: &'static str,
    /// Every argument, in the order a reader wants them.
    pub fields: &'static [Field],
    /// The variant of [`Command`](maj_core::Command) this tool appends, when it appends one.
    ///
    /// `None` for the session, read and render tools, which change no project state or
    /// change it through another tool.
    pub command: Option<&'static str>,
    /// Whether the tool only reads. Advertised to the client as `readOnlyHint`.
    pub read_only: bool,
    /// Whether the tool can remove something. Advertised as `destructiveHint`.
    pub destructive: bool,
    /// A worked call, as JSON arguments. Executed by the tests, quoted in the description.
    pub example: &'static str,
}

/// One argument, or one field of an entity a client is describing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field {
    /// The JSON key.
    pub name: &'static str,
    /// What the value must be.
    pub kind: FieldKind,
    /// Whether the client must send it.
    pub required: bool,
    /// What it is for, in one sentence.
    pub doc: &'static str,
    /// JSON text for the value supplied when the field is absent, when it is optional.
    pub default: Option<&'static str>,
}

/// What a field's value must be.
///
/// The variants are the handful of shapes the model actually uses: identifiers, exact
/// rationals, frame ranges, switches, whole numbers, names from a fixed set, nested objects,
/// lists, and the two parameter forms of an effect chain. There is no number-of-bits variant
/// because a client should not be choosing the width of an integer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldKind {
    /// An entity identifier, allocated by the engine when the client omits it.
    Id {
        /// The kind of entity, for the allocation message and the description.
        kind: &'static str,
    },
    /// An entity identifier that may be absent, in which case the field is sent as null.
    OptionalId {
        /// The kind of entity.
        kind: &'static str,
    },
    /// An exact rational: `1`, `0.5`, `"1/2"` or `{"num":1,"den":2}`.
    Rational,
    /// A frame range: `[start, end)` as `[12, 60]`, or `{"start":12,"end":60}`.
    Range,
    /// A switch.
    Bool,
    /// A whole number.
    Integer,
    /// Free text.
    Text,
    /// One of a fixed set of names.
    Names(&'static [&'static str]),
    /// A list of names from a fixed set.
    NameList(&'static [&'static str]),
    /// A nested object.
    Object(&'static [Field]),
    /// A list of nested objects.
    List(&'static [Field]),
    /// An effect's parameter set: a map of key to value, or a list of `{key, value}`.
    Params,
    /// A single parameter value: a number, a string, a switch, a colour, or the tagged form.
    Param,
}

impl Field {
    /// A field the client must send.
    #[must_use]
    pub const fn required(name: &'static str, kind: FieldKind, doc: &'static str) -> Self {
        Self {
            name,
            kind,
            required: true,
            doc,
            default: None,
        }
    }

    /// A required entity identifier: a reference to something that already exists.
    #[must_use]
    pub const fn id(name: &'static str, kind: &'static str, doc: &'static str) -> Self {
        Self::required(name, FieldKind::Id { kind }, doc)
    }

    /// The identifier of the entity being described, which the engine allocates when the
    /// client leaves it out.
    ///
    /// Not `required`, because a client never has to think of a number: the whole field is
    /// absent from the schema's required list and present in the description as something the
    /// server will do for you.
    #[must_use]
    pub const fn allocated(name: &'static str, kind: &'static str, doc: &'static str) -> Self {
        Self {
            name,
            kind: FieldKind::Id { kind },
            required: false,
            doc,
            default: None,
        }
    }

    /// A field with a value the engine supplies when it is absent.
    ///
    /// The default is JSON, and it is what the engine actually sends, so the description a
    /// client reads and the behaviour it gets cannot disagree.
    #[must_use]
    pub const fn optional(
        name: &'static str,
        kind: FieldKind,
        doc: &'static str,
        default: &'static str,
    ) -> Self {
        Self {
            name,
            kind,
            required: false,
            doc,
            default: Some(default),
        }
    }

    /// An optional field whose meaning when absent is not a value the engine can state as
    /// JSON — "the end of the list", "where it was opened". The rule goes in the
    /// description instead.
    #[must_use]
    pub const fn free_optional(name: &'static str, kind: FieldKind, doc: &'static str) -> Self {
        Self {
            name,
            kind,
            required: false,
            doc,
            default: None,
        }
    }
}

/// The names of the track kinds, as the model spells them.
pub const TRACK_KINDS: &[&str] = &["video", "audio"];

/// The blend modes of the certified path.
pub const BLEND_MODES: &[&str] = &[
    "normal",
    "add",
    "multiply",
    "screen",
    "overlay",
    "darken",
    "lighten",
    "difference",
    "hard_light",
    "soft_light",
];

/// The mask shapes the renderer knows.
pub const MASK_SHAPES: &[&str] = &["rectangle", "ellipse"];

/// The transitions the renderer knows.
pub const TRANSITION_KINDS: &[&str] = &[
    "cross_dissolve",
    "dip_to_black",
    "dip_to_white",
    "wipe_left",
    "wipe_right",
    "wipe_up",
    "wipe_down",
    "circle",
];

/// The interpolation a keyframe uses towards the next one.
pub const INTERPOLATIONS: &[&str] = &["hold", "linear"];

/// The channels that can be animated, matching the clip's own properties.
pub const CHANNELS: &[&str] = &["opacity", "x", "y", "scale_x", "scale_y", "rotation"];

/// The effects the certified path implements.
pub const EFFECT_KINDS: &[&str] = &[
    "exposure",
    "brightness",
    "contrast",
    "saturation",
    "gamma",
    "hue_rotate",
    "blur",
    "sharpen",
    "vignette",
    "grain",
];

/// The transfer functions an encoded still may be written in.
pub const TRANSFER_FUNCTIONS: &[&str] = &["linear", "srgb", "rec709", "pq", "hlg"];

/// The session a call applies to, when a server holds more than one project.
const SESSION: Field = Field::free_optional(
    "session",
    FieldKind::Text,
    "Which open project to act on. Default: the session this server started with.",
);

/// Where in a list something lands, when the end of it is the natural place.
const INDEX_APPEND: Field = Field::free_optional(
    "index",
    FieldKind::Integer,
    "Where in the list it lands, counting from zero. Default: the end of the list.",
);

/// The owning sequence of a clip-level command.
const SEQUENCE: Field = Field::id(
    "sequence",
    "sequence",
    "The sequence the change happens in.",
);

/// The sequence, by identifier or by name, when a read may name either.
const SEQUENCE_REF: Field = Field::id("sequence", "sequence", "The sequence to read.");

/// The clip a command changes.
const CLIP: Field = Field::id("clip", "clip", "The clip to change.");

/// A track of the sequence.
const TRACK: Field = Field::id("track", "track", "The track to change.");

/// Position in a list.
const INDEX: Field = Field::required(
    "index",
    FieldKind::Integer,
    "Where to insert, counting from zero. In a list that already has that many entries or more.",
);

/// A whole entity, described inline.
const fn entity(name: &'static str, fields: &'static [Field], doc: &'static str) -> Field {
    Field::required(name, FieldKind::Object(fields), doc)
}

/// The seven properties of a transform, all optional except the scale.
const TRANSFORM_SHAPE: &[Field] = &[
    Field::optional(
        "x",
        FieldKind::Rational,
        "Horizontal offset, in frame widths.",
        "0",
    ),
    Field::optional(
        "y",
        FieldKind::Rational,
        "Vertical offset, in frame heights.",
        "0",
    ),
    Field::optional("scale_x", FieldKind::Rational, "Horizontal scale.", "1"),
    Field::optional("scale_y", FieldKind::Rational, "Vertical scale.", "1"),
    Field::optional("rotation", FieldKind::Rational, "Rotation in degrees.", "0"),
    Field::optional(
        "anchor_x",
        FieldKind::Rational,
        "Horizontal anchor, as a fraction of the source frame.",
        "0.5",
    ),
    Field::optional(
        "anchor_y",
        FieldKind::Rational,
        "Vertical anchor, as a fraction of the source frame.",
        "0.5",
    ),
];

/// One keyframe.
const KEYFRAME_SHAPE: &[Field] = &[
    Field::required(
        "offset",
        FieldKind::Integer,
        "Frames from the clip's first timeline frame.",
    ),
    Field::required("value", FieldKind::Param, "The animated value."),
    Field::optional(
        "interpolation",
        FieldKind::Names(INTERPOLATIONS),
        "How the value travels to the next keyframe.",
        "\"linear\"",
    ),
];

/// One animated channel.
const ANIMATION_SHAPE: &[Field] = &[
    Field::required("channel", FieldKind::Names(CHANNELS), "Which channel."),
    Field::required(
        "keys",
        FieldKind::List(KEYFRAME_SHAPE),
        "Every keyframe, ordered by offset. An empty list removes the animation.",
    ),
];

/// One mask.
const MASK_SHAPE: &[Field] = &[
    Field::allocated(
        "id",
        "mask",
        "Identifier. Omit and the engine allocates one.",
    ),
    Field::required(
        "shape",
        FieldKind::Names(MASK_SHAPES),
        "Rectangle or ellipse.",
    ),
    Field::optional(
        "center_x",
        FieldKind::Rational,
        "Horizontal centre, as a fraction of the frame.",
        "0.5",
    ),
    Field::optional(
        "center_y",
        FieldKind::Rational,
        "Vertical centre, as a fraction of the frame.",
        "0.5",
    ),
    Field::optional(
        "width",
        FieldKind::Rational,
        "Width, as a fraction of the frame.",
        "1",
    ),
    Field::optional(
        "height",
        FieldKind::Rational,
        "Height, as a fraction of the frame.",
        "1",
    ),
    Field::optional("rotation", FieldKind::Rational, "Rotation in degrees.", "0"),
    Field::optional(
        "feather",
        FieldKind::Rational,
        "Edge softness, as a fraction of the size.",
        "0",
    ),
    Field::optional(
        "opacity",
        FieldKind::Rational,
        "Coverage where the mask applies.",
        "1",
    ),
    Field::optional(
        "inverted",
        FieldKind::Bool,
        "Cover outside the shape instead.",
        "false",
    ),
];

/// One effect instance.
const EFFECT_SHAPE: &[Field] = &[
    Field::allocated(
        "id",
        "effect",
        "Identifier. Omit and the engine allocates one.",
    ),
    Field::required(
        "kind",
        FieldKind::Names(EFFECT_KINDS),
        "Which built-in effect.",
    ),
    Field::optional(
        "enabled",
        FieldKind::Bool,
        "Whether it runs at all.",
        "true",
    ),
    Field::optional(
        "params",
        FieldKind::Params,
        "Parameters, as a map of key to value or a list of `{key, value}`. Omit for the \
         effect's declared defaults; a key outside the effect's schema is refused.",
        "{}",
    ),
];

/// One transition between two clips of a track.
const TRANSITION_SHAPE: &[Field] = &[
    Field::allocated(
        "id",
        "transition",
        "Identifier. Omit and the engine allocates one.",
    ),
    Field::required(
        "kind",
        FieldKind::Names(TRANSITION_KINDS),
        "Which transition.",
    ),
    Field::required("at", FieldKind::Integer, "First timeline frame it covers."),
    Field::required(
        "duration",
        FieldKind::Integer,
        "Frames it lasts. Must be positive.",
    ),
    Field::optional(
        "from",
        FieldKind::OptionalId { kind: "clip" },
        "The outgoing clip, when the transition names one.",
        "null",
    ),
    Field::optional(
        "to",
        FieldKind::OptionalId { kind: "clip" },
        "The incoming clip, when the transition names one.",
        "null",
    ),
];

/// One registered media asset.
const ASSET_SHAPE: &[Field] = &[
    Field::allocated(
        "id",
        "asset",
        "Identifier. Omit and the engine allocates one.",
    ),
    Field::required("name", FieldKind::Text, "Name shown in the media pool."),
    Field::required(
        "uri",
        FieldKind::Text,
        "Where the picture comes from. `generated:<label>` and `solid:<r>,<g>,<b>` are \
         procedural and need no file; a path ending in `.ppm` is read from disk; anything \
         else renders as a labelled chart rather than failing.",
    ),
    Field::required(
        "timebase",
        FieldKind::Rational,
        "Frames per second of the source, exactly: 24000/1001 for 23.976.",
    ),
    Field::required(
        "duration_frames",
        FieldKind::Integer,
        "Length in source frames, at that timebase.",
    ),
    Field::optional(
        "has_video",
        FieldKind::Bool,
        "Whether it carries a picture.",
        "true",
    ),
    Field::optional(
        "has_audio",
        FieldKind::Bool,
        "Whether it carries sound.",
        "false",
    ),
];

/// One clip on a track.
const CLIP_SHAPE: &[Field] = &[
    Field::allocated(
        "id",
        "clip",
        "Identifier. Omit and the engine allocates one.",
    ),
    Field::id("asset_id", "asset", "The media this clip reads."),
    Field::required("name", FieldKind::Text, "Name of the clip."),
    Field::required(
        "source_range",
        FieldKind::Range,
        "Half-open range of source frames, as `[start, end)`.",
    ),
    Field::required(
        "timeline_start",
        FieldKind::Integer,
        "First timeline frame the clip occupies.",
    ),
    Field::optional(
        "speed",
        FieldKind::Rational,
        "Playback rate. Must be positive.",
        "1",
    ),
    Field::optional(
        "enabled",
        FieldKind::Bool,
        "Whether it contributes to the frame.",
        "true",
    ),
    Field::optional(
        "transform",
        FieldKind::Object(TRANSFORM_SHAPE),
        "Position, scale, rotation and anchor. Omit for the identity.",
        "{}",
    ),
    Field::optional("opacity", FieldKind::Rational, "Overall opacity.", "1"),
    Field::optional(
        "blend",
        FieldKind::Names(BLEND_MODES),
        "How it meets the layers below it.",
        "\"normal\"",
    ),
    Field::optional(
        "effects",
        FieldKind::List(EFFECT_SHAPE),
        "Effect chain, in application order.",
        "[]",
    ),
    Field::optional(
        "animation",
        FieldKind::List(ANIMATION_SHAPE),
        "Animated channels. At most one entry per channel.",
        "[]",
    ),
    Field::optional(
        "masks",
        FieldKind::List(MASK_SHAPE),
        "Masks, multiplied into the alpha.",
        "[]",
    ),
];

/// One track.
const TRACK_SHAPE: &[Field] = &[
    Field::allocated(
        "id",
        "track",
        "Identifier. Omit and the engine allocates one.",
    ),
    Field::required("kind", FieldKind::Names(TRACK_KINDS), "Video or audio."),
    Field::required("name", FieldKind::Text, "Name of the track, such as `V1`."),
    Field::optional(
        "enabled",
        FieldKind::Bool,
        "Whether the track contributes.",
        "true",
    ),
    Field::optional(
        "locked",
        FieldKind::Bool,
        "Whether editing it is refused.",
        "false",
    ),
    Field::optional(
        "muted",
        FieldKind::Bool,
        "Whether its sound is silenced.",
        "false",
    ),
    Field::optional(
        "clips",
        FieldKind::List(CLIP_SHAPE),
        "Clips, in timeline order.",
        "[]",
    ),
    Field::optional(
        "transitions",
        FieldKind::List(TRANSITION_SHAPE),
        "Transitions, in timeline order.",
        "[]",
    ),
];

/// One sequence.
const SEQUENCE_SHAPE: &[Field] = &[
    Field::allocated(
        "id",
        "sequence",
        "Identifier. Omit and the engine allocates one.",
    ),
    Field::required("name", FieldKind::Text, "Name of the sequence."),
    Field::required("timebase", FieldKind::Rational, "Frame rate, exactly."),
    Field::required("width", FieldKind::Integer, "Frame width in pixels."),
    Field::required("height", FieldKind::Integer, "Frame height in pixels."),
    Field::required(
        "sample_rate",
        FieldKind::Integer,
        "Audio sample rate in hertz.",
    ),
    Field::optional(
        "tracks",
        FieldKind::List(TRACK_SHAPE),
        "Tracks, bottom-first, which is the order they composite in.",
        "[]",
    ),
];

/// Every tool this server implements, in a stable order: the editing surface first, then the
/// session, then the reads, then the renderer.
pub static TOOLS: &[Tool] = &[
    // ---- media ----
    Tool {
        name: "maj_create_media_asset",
        title: "Create media asset",
        description: "Register media in the project, at the end of the media pool unless \
                      `index` says otherwise. Registering is not importing: nothing is read \
                      from disk, and the URI may be procedural.",
        fields: &[
            SESSION,
            INDEX_APPEND,
            entity("asset", ASSET_SHAPE, "The media to register."),
        ],
        command: Some("create_media_asset"),
        read_only: false,
        destructive: false,
        example: r#"{"asset":{"name":"shot-01","uri":"generated:shot-01","timebase":"24000/1001","duration_frames":240,"has_video":true,"has_audio":false}}"#,
    },
    Tool {
        name: "maj_insert_media_asset",
        title: "Insert media asset",
        description: "Register media at an explicit position in the media pool, shifting the \
                      entries at and after it up by one.",
        fields: &[
            SESSION,
            INDEX,
            entity("asset", ASSET_SHAPE, "The media to register."),
        ],
        command: Some("insert_media_asset"),
        read_only: false,
        destructive: false,
        example: r#"{"index":0,"asset":{"name":"shot-02","uri":"generated:shot-02","timebase":"24","duration_frames":96,"has_video":true,"has_audio":false}}"#,
    },
    Tool {
        name: "maj_remove_media_asset",
        title: "Remove media asset",
        description: "Remove media from the pool. Refused while a clip still refers to it, \
                      which is the point: the refusal is the check.",
        fields: &[SESSION, Field::id("id", "asset", "The media to remove.")],
        command: Some("remove_media_asset"),
        read_only: false,
        destructive: true,
        example: r#"{"id":1}"#,
    },
    // ---- sequences ----
    Tool {
        name: "maj_create_sequence",
        title: "Create sequence",
        description: "Create a sequence, at the end of the project unless `index` says \
                      otherwise. Tracks may be nested in the call, which is how a timeline is \
                      set up in one step.",
        fields: &[
            SESSION,
            INDEX_APPEND,
            entity("sequence", SEQUENCE_SHAPE, "The sequence to create."),
        ],
        command: Some("create_sequence"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":{"name":"Reel A","timebase":"24000/1001","width":1920,"height":1080,"sample_rate":48000,"tracks":[{"kind":"video","name":"V1"},{"kind":"audio","name":"A1"}]}}"#,
    },
    Tool {
        name: "maj_insert_sequence",
        title: "Insert sequence",
        description: "Create a sequence at an explicit position in the project, shifting the \
                      sequences at and after it up by one.",
        fields: &[
            SESSION,
            INDEX,
            entity("sequence", SEQUENCE_SHAPE, "The sequence to create."),
        ],
        command: Some("insert_sequence"),
        read_only: false,
        destructive: false,
        example: r#"{"index":0,"sequence":{"name":"Reel B","timebase":"24","width":1920,"height":1080,"sample_rate":48000}}"#,
    },
    Tool {
        name: "maj_remove_sequence",
        title: "Remove sequence",
        description: "Remove a sequence and everything in it. Undo restores the sequence, its \
                      tracks, its clips and their positions exactly.",
        fields: &[
            SESSION,
            Field::id("id", "sequence", "The sequence to remove."),
        ],
        command: Some("remove_sequence"),
        read_only: false,
        destructive: true,
        example: r#"{"id":1}"#,
    },
    // ---- tracks ----
    Tool {
        name: "maj_create_track",
        title: "Create track",
        description: "Add a track to a sequence, at the end unless `index` says otherwise. \
                      Tracks composite bottom-first, so index zero is the bottom layer.",
        fields: &[
            SESSION,
            SEQUENCE,
            INDEX_APPEND,
            entity("track", TRACK_SHAPE, "The track to create."),
        ],
        command: Some("create_track"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"track":{"kind":"video","name":"V2"}}"#,
    },
    Tool {
        name: "maj_remove_track",
        title: "Remove track",
        description: "Remove a track and every clip and transition on it. Undo restores the \
                      track, its clips and their positions in one step.",
        fields: &[SESSION, SEQUENCE, TRACK],
        command: Some("remove_track"),
        read_only: false,
        destructive: true,
        example: r#"{"sequence":1,"track":3}"#,
    },
    Tool {
        name: "maj_set_track_enabled",
        title: "Enable or disable track",
        description: "Turn a whole track on or off. A disabled track stays on the sequence \
                      and is skipped by the renderer, which is how two versions of a scene \
                      are compared without removing either.",
        fields: &[
            SESSION,
            SEQUENCE,
            TRACK,
            Field::required("enabled", FieldKind::Bool, "Whether the track contributes."),
        ],
        command: Some("set_track_enabled"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"track":3,"enabled":false}"#,
    },
    // ---- clips ----
    Tool {
        name: "maj_append_clip",
        title: "Append clip",
        description: "Append a clip to the end of a track. The clip's identifier is allocated \
                      unless you supply one; see `maj_place_clip` for the common case of \
                      placing media on a track chosen by kind.",
        fields: &[
            SESSION,
            SEQUENCE,
            TRACK,
            entity("clip", CLIP_SHAPE, "The clip to append."),
        ],
        command: Some("append_clip"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"track":2,"clip":{"asset_id":1,"name":"shot-01","source_range":[0,48],"timeline_start":0}}"#,
    },
    Tool {
        name: "maj_insert_clip",
        title: "Insert clip",
        description: "Insert a clip at an explicit position on a track, shifting the clips at \
                      and after it later. This is an insert edit, not an overwrite.",
        fields: &[
            SESSION,
            SEQUENCE,
            TRACK,
            INDEX,
            entity("clip", CLIP_SHAPE, "The clip to insert."),
        ],
        command: Some("insert_clip"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"track":2,"index":0,"clip":{"asset_id":1,"name":"pickup","source_range":[12,36],"timeline_start":0}}"#,
    },
    Tool {
        name: "maj_remove_clip",
        title: "Remove clip",
        description: "Remove a clip from its track, leaving a gap. Undo restores it at the \
                      exact position it came from.",
        fields: &[SESSION, SEQUENCE, CLIP],
        command: Some("remove_clip"),
        read_only: false,
        destructive: true,
        example: r#"{"sequence":1,"clip":4}"#,
    },
    Tool {
        name: "maj_trim_clip",
        title: "Trim clip",
        description: "Set a clip's source range and timeline position in one command. The \
                      range is half-open: `[start, end)`.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            Field::required(
                "source_range",
                FieldKind::Range,
                "The new half-open source range.",
            ),
            Field::required(
                "timeline_start",
                FieldKind::Integer,
                "The new first timeline frame.",
            ),
        ],
        command: Some("trim_clip"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"source_range":[12,60],"timeline_start":24}"#,
    },
    Tool {
        name: "maj_move_clip",
        title: "Move clip",
        description: "Move a clip along its track by setting its first timeline frame. Use \
                      `maj_reparent_clip` to move it to another track.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            Field::required(
                "timeline_start",
                FieldKind::Integer,
                "The new first timeline frame.",
            ),
        ],
        command: Some("move_clip"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"timeline_start":120}"#,
    },
    Tool {
        name: "maj_reparent_clip",
        title: "Move clip to another track",
        description: "Move a clip to another track of the same sequence, at an explicit \
                      position in that track's clip list.",
        fields: &[SESSION, SEQUENCE, CLIP, TRACK, INDEX],
        command: Some("reparent_clip"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"track":5,"index":0}"#,
    },
    Tool {
        name: "maj_set_clip_speed",
        title: "Set clip speed",
        description: "Retime a clip. The rate is exact: `\"1/2\"` is half speed and lasts \
                      twice as long, and nothing about it is rounded.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            Field::required(
                "speed",
                FieldKind::Rational,
                "Playback rate. Must be positive.",
            ),
        ],
        command: Some("set_clip_speed"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"speed":"1/2"}"#,
    },
    Tool {
        name: "maj_set_clip_enabled",
        title: "Enable or disable clip",
        description: "Turn a clip on or off. A disabled clip stays on the timeline and is \
                      skipped by the renderer, which is how a cut is compared against its \
                      alternative without deleting either.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            Field::required("enabled", FieldKind::Bool, "Whether the clip contributes."),
        ],
        command: Some("set_clip_enabled"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"enabled":false}"#,
    },
    Tool {
        name: "maj_set_clip_transform",
        title: "Set clip transform",
        description: "Position, scale, rotate and re-anchor a clip. Omitted properties take \
                      their identity value, so a scale of a half is `{\"scale_x\":\"1/2\",\"scale_y\":\"1/2\"}`.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            entity("transform", TRANSFORM_SHAPE, "The new transform."),
        ],
        command: Some("set_clip_transform"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"transform":{"scale_x":"1/2","scale_y":"1/2"}}"#,
    },
    Tool {
        name: "maj_set_clip_opacity",
        title: "Set clip opacity",
        description: "Set how much of the clip reaches the frame before it is composited.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            Field::required(
                "opacity",
                FieldKind::Rational,
                "Opacity, normally between 0 and 1.",
            ),
        ],
        command: Some("set_clip_opacity"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"opacity":"1/2"}"#,
    },
    Tool {
        name: "maj_set_clip_blend",
        title: "Set clip blend mode",
        description: "Set how the clip meets the layers beneath it.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            Field::required("blend", FieldKind::Names(BLEND_MODES), "The blend mode."),
        ],
        command: Some("set_clip_blend"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"blend":"screen"}"#,
    },
    // ---- effects ----
    Tool {
        name: "maj_add_effect",
        title: "Add effect",
        description: "Add a built-in effect to a clip's chain at a position. Effects run in \
                      list order, so the position is part of the look.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            INDEX_APPEND,
            entity("effect", EFFECT_SHAPE, "The effect to add."),
        ],
        command: Some("add_effect"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"effect":{"kind":"blur","params":{"radius":6}}}"#,
    },
    Tool {
        name: "maj_remove_effect",
        title: "Remove effect",
        description: "Remove one effect from a clip's chain. The rest of the chain keeps its \
                      order, and undo puts the effect back where it was.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            Field::id("effect", "effect", "The effect instance to remove."),
        ],
        command: Some("remove_effect"),
        read_only: false,
        destructive: true,
        example: r#"{"sequence":1,"clip":4,"effect":7}"#,
    },
    Tool {
        name: "maj_set_effect_enabled",
        title: "Enable or disable effect",
        description: "Bypass an effect without removing it, keeping its parameters.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            Field::id("effect", "effect", "The effect instance to change."),
            Field::required("enabled", FieldKind::Bool, "Whether the effect runs."),
        ],
        command: Some("set_effect_enabled"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"effect":7,"enabled":false}"#,
    },
    Tool {
        name: "maj_set_effect_param",
        title: "Set effect parameter",
        description: "Set one parameter of one effect. A key the effect does not declare is \
                      refused, so a typo cannot silently do nothing. Read the effect's schema \
                      from the project before guessing a key.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            Field::id("effect", "effect", "The effect instance to change."),
            Field::required("key", FieldKind::Text, "The parameter's key."),
            Field::required("value", FieldKind::Param, "The new value."),
        ],
        command: Some("set_effect_param"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"effect":7,"key":"radius","value":3}"#,
    },
    // ---- masks ----
    Tool {
        name: "maj_add_mask",
        title: "Add mask",
        description: "Add a mask to a clip at a position in its mask list. The mask multiplies \
                      into the layer's alpha, so a mask with no feather is a hard cut-out.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            INDEX_APPEND,
            entity("mask", MASK_SHAPE, "The mask to add."),
        ],
        command: Some("add_mask"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"mask":{"shape":"ellipse","center_x":"1/2","center_y":"1/2","width":"1/4","height":"1/4","feather":"1/50"}}"#,
    },
    Tool {
        name: "maj_remove_mask",
        title: "Remove mask",
        description: "Remove one mask from a clip, leaving the other masks and everything \
                      else about the clip untouched.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            Field::id("mask", "mask", "The mask to remove."),
        ],
        command: Some("remove_mask"),
        read_only: false,
        destructive: true,
        example: r#"{"sequence":1,"clip":4,"mask":8}"#,
    },
    Tool {
        name: "maj_set_mask",
        title: "Set mask",
        description: "Replace a mask's geometry in place. The mask keeps its identifier, which \
                      is what makes the change an edit to a mask rather than a new one.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            Field::id("mask", "mask", "The mask to replace."),
            entity(
                "value",
                MASK_SHAPE,
                "The new mask, with the same identifier.",
            ),
        ],
        command: Some("set_mask"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"mask":8,"value":{"shape":"rectangle","width":"1/2","height":"1/2"}}"#,
    },
    // ---- animation ----
    Tool {
        name: "maj_set_channel_keys",
        title: "Set channel keyframes",
        description: "Replace every keyframe of one animated channel. The list is complete, not \
                      incremental. An empty list removes the animation, which is exactly \
                      reversible.",
        fields: &[
            SESSION,
            SEQUENCE,
            CLIP,
            Field::required(
                "channel",
                FieldKind::Names(CHANNELS),
                "Which channel to animate.",
            ),
            Field::required(
                "keys",
                FieldKind::List(KEYFRAME_SHAPE),
                "Every keyframe, ordered by offset.",
            ),
        ],
        command: Some("set_channel_keys"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"clip":4,"channel":"opacity","keys":[{"offset":0,"value":0},{"offset":12,"value":1}]}"#,
    },
    // ---- transitions ----
    Tool {
        name: "maj_add_transition",
        title: "Add transition",
        description: "Add a transition to a track at a position in its list. `at` is the first \
                      timeline frame it covers and `duration` the number of frames, which must \
                      be positive.",
        fields: &[
            SESSION,
            SEQUENCE,
            TRACK,
            INDEX_APPEND,
            entity("transition", TRANSITION_SHAPE, "The transition to add."),
        ],
        command: Some("add_transition"),
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"track":2,"transition":{"kind":"cross_dissolve","at":48,"duration":24,"from":4,"to":5}}"#,
    },
    Tool {
        name: "maj_remove_transition",
        title: "Remove transition",
        description: "Remove one transition from a track, leaving the clips it joined exactly \
                      where they are.",
        fields: &[
            SESSION,
            SEQUENCE,
            TRACK,
            Field::id("transition", "transition", "The transition to remove."),
        ],
        command: Some("remove_transition"),
        read_only: false,
        destructive: true,
        example: r#"{"sequence":1,"track":2,"transition":9}"#,
    },
    // ---- session ----
    Tool {
        name: "maj_project_state",
        title: "Read project state",
        description: "The revision, both content hashes, the counts, and the undo depth. Start \
                      here: the revision is the handle every other answer is relative to.",
        fields: &[SESSION],
        command: None,
        read_only: true,
        destructive: false,
        example: r#"{}"#,
    },
    Tool {
        name: "maj_timeline",
        title: "Read the timeline",
        description: "Every sequence, track and clip with its identifiers, ranges and \
                      positions, plus each clip's effect chain. This is the read to make \
                      before an edit that names an identifier.",
        fields: &[SESSION],
        command: None,
        read_only: true,
        destructive: false,
        example: r#"{}"#,
    },
    Tool {
        name: "maj_clip_detail",
        title: "Read one clip",
        description: "One clip in full: transform, opacity, blend, effects with their \
                      parameters valid ranges, masks and keyframes.",
        fields: &[SESSION, CLIP],
        command: None,
        read_only: true,
        destructive: false,
        example: r#"{"clip":4}"#,
    },
    Tool {
        name: "maj_history",
        title: "Read the command log",
        description: "The log as applied, newest last: position, author and command, plus how \
                      many steps can still be undone. Everything the project is, an agent can \
                      read here.",
        fields: &[
            SESSION,
            Field::optional(
                "limit",
                FieldKind::Integer,
                "How many entries to return.",
                "50",
            ),
            Field::free_optional("author", FieldKind::Text, "Only entries by this author."),
        ],
        command: None,
        read_only: true,
        destructive: false,
        example: r#"{"limit":10}"#,
    },
    Tool {
        name: "maj_undo",
        title: "Undo",
        description: "Revert the most recent commands, exactly, including list positions. \
                      Returns how many steps were taken; asking for more than exist is not an \
                      error and undoes what there is.",
        fields: &[
            SESSION,
            Field::optional(
                "count",
                FieldKind::Integer,
                "How many commands to revert.",
                "1",
            ),
        ],
        command: None,
        read_only: false,
        destructive: false,
        example: r#"{"count":1}"#,
    },
    Tool {
        name: "maj_redo",
        title: "Redo",
        description: "Re-apply commands that were undone. Redo is unavailable once a new edit \
                      has been made, which is the log's own ordering rule.",
        fields: &[
            SESSION,
            Field::optional(
                "count",
                FieldKind::Integer,
                "How many commands to re-apply.",
                "1",
            ),
        ],
        command: None,
        read_only: false,
        destructive: false,
        example: r#"{"count":1}"#,
    },
    Tool {
        name: "maj_open_project",
        title: "Open a project",
        description: "Load a `.maj` document into a session. The undo stack starts empty, \
                      because inverses are session state and fabricating them after a load \
                      would be a lie.",
        fields: &[
            SESSION,
            Field::required("path", FieldKind::Text, "Path to the `.maj` document."),
        ],
        command: None,
        read_only: false,
        destructive: false,
        example: r#"{"path":"demo.maj"}"#,
    },
    Tool {
        name: "maj_new_project",
        title: "Start a project",
        description: "Replace a session with an empty project of the given name.",
        fields: &[
            SESSION,
            Field::required("name", FieldKind::Text, "Name of the project."),
        ],
        command: None,
        read_only: false,
        destructive: true,
        example: r#"{"name":"untitled"}"#,
    },
    Tool {
        name: "maj_save_project",
        title: "Save a project",
        description: "Write the document to disk, and report the hash that was written. \
                      Without `path`, it is saved where it was opened.",
        fields: &[
            SESSION,
            Field::free_optional(
                "path",
                FieldKind::Text,
                "Where to write it. Default: where it was opened.",
            ),
        ],
        command: None,
        read_only: false,
        destructive: false,
        example: r#"{"path":"out/reel-a.maj"}"#,
    },
    Tool {
        name: "maj_replay_check",
        title: "Check the log explains the state",
        description: "Rebuild the project from its own command log and compare the result with \
                      the live document. A mismatch means something changed the state without \
                      logging it, which is the one failure this architecture refuses to \
                      tolerate.",
        fields: &[SESSION],
        command: None,
        read_only: true,
        destructive: false,
        example: r#"{}"#,
    },
    // ---- helpers ----
    Tool {
        name: "maj_place_clip",
        title: "Place media on a track",
        description: "The editorial shorthand: register media if it is new, pick the track by \
                      kind and position, and append the clip. Emits one command per change, so \
                      its work is as undoable as anything else.",
        fields: &[
            SESSION,
            SEQUENCE,
            Field::free_optional(
                "track",
                FieldKind::Id { kind: "track" },
                "The track to place on. Default: the first track of the requested kind.",
            ),
            Field::required(
                "kind",
                FieldKind::Names(TRACK_KINDS),
                "Which kind of track to fall back to when `track` is omitted.",
            ),
            Field::required(
                "asset",
                FieldKind::Id { kind: "asset" },
                "The media to place.",
            ),
            Field::required("source_range", FieldKind::Range, "Half-open source range."),
            Field::required(
                "timeline_start",
                FieldKind::Integer,
                "First timeline frame.",
            ),
            Field::free_optional(
                "name",
                FieldKind::Text,
                "Name of the clip. Default: the name of the media.",
            ),
            Field::optional("speed", FieldKind::Rational, "Playback rate.", "1"),
        ],
        command: None,
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"kind":"video","asset":1,"source_range":[0,48],"timeline_start":0,"name":"shot-01"}"#,
    },
    Tool {
        name: "maj_setup_sequence",
        title: "Set up a sequence",
        description: "Create a sequence with named tracks in one call: `V1, V2, A1` and so on. \
                      The frame size, rate and sample rate come from the arguments; the tracks \
                      are created in the order given, bottom first.",
        fields: &[
            SESSION,
            Field::required("name", FieldKind::Text, "Name of the sequence."),
            Field::required("timebase", FieldKind::Rational, "Frame rate, exactly."),
            Field::required("width", FieldKind::Integer, "Frame width in pixels."),
            Field::required("height", FieldKind::Integer, "Frame height in pixels."),
            Field::optional(
                "sample_rate",
                FieldKind::Integer,
                "Audio sample rate in hertz.",
                "48000",
            ),
            Field::required(
                "tracks",
                FieldKind::NameList(TRACK_KINDS),
                "One entry per track, in order, bottom first: `[\"video\",\"video\",\"audio\"]`.",
            ),
        ],
        command: None,
        read_only: false,
        destructive: false,
        example: r#"{"name":"Reel A","timebase":"24000/1001","width":1920,"height":1080,"tracks":["video","audio"]}"#,
    },
    // ---- render ----
    Tool {
        name: "maj_render_frame",
        title: "Render a frame",
        description: "Render one timeline frame on the certified path and return the picture \
                      itself, its digest and the manifest of how it was made. This is how an \
                      agent sees its own work instead of assuming it.",
        fields: &[
            SESSION,
            SEQUENCE_REF,
            Field::required("frame", FieldKind::Integer, "Timeline frame to render."),
            Field::free_optional(
                "width",
                FieldKind::Integer,
                "Output width. Default: the sequence's own.",
            ),
            Field::free_optional(
                "height",
                FieldKind::Integer,
                "Output height. Default: the sequence's own.",
            ),
            Field::optional("tiles", FieldKind::Text, "Tile grid, as `CxR`.", "\"1x1\""),
            Field::optional(
                "transfer",
                FieldKind::Names(TRANSFER_FUNCTIONS),
                "Transfer function of the encoded still.",
                "\"srgb\"",
            ),
            Field::optional(
                "image",
                FieldKind::Bool,
                "Whether to return the picture itself.",
                "true",
            ),
        ],
        command: None,
        read_only: true,
        destructive: false,
        example: r#"{"sequence":1,"frame":24}"#,
    },
    Tool {
        name: "maj_verify_frame",
        title: "Verify a frame",
        description: "Render a frame whole and again in tiles and compare the digests. Equal \
                      digests are the claim that the renderer is deterministic on this input; \
                      they are computed here, not asserted.",
        fields: &[
            SESSION,
            SEQUENCE_REF,
            Field::required("frame", FieldKind::Integer, "Timeline frame to render."),
            Field::optional("tiles", FieldKind::Text, "Tile grid, as `CxR`.", "\"2x2\""),
        ],
        command: None,
        read_only: true,
        destructive: false,
        example: r#"{"sequence":1,"frame":24,"tiles":"2x2"}"#,
    },
    Tool {
        name: "maj_export_frame",
        title: "Export a frame to disk",
        description: "Render a frame and write it, with its manifest beside it, to a path. The \
                      format follows the extension: `.png` at eight or sixteen bits, or `.ppm` \
                      with no alpha.",
        fields: &[
            SESSION,
            SEQUENCE_REF,
            Field::required("frame", FieldKind::Integer, "Timeline frame to render."),
            Field::required("out", FieldKind::Text, "Where to write the still."),
            Field::free_optional(
                "width",
                FieldKind::Integer,
                "Output width. Default: the sequence's own.",
            ),
            Field::free_optional(
                "height",
                FieldKind::Integer,
                "Output height. Default: the sequence's own.",
            ),
            Field::optional(
                "transfer",
                FieldKind::Names(TRANSFER_FUNCTIONS),
                "Transfer function of the encoded still.",
                "\"srgb\"",
            ),
        ],
        command: None,
        read_only: false,
        destructive: false,
        example: r#"{"sequence":1,"frame":24,"out":"out/frame.0024.png"}"#,
    },
];

/// Looks a tool up by name.
#[must_use]
pub fn tool(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|tool| tool.name == name)
}

/// Every tool that appends a command, with the command variant it appends.
#[must_use]
pub fn command_tools() -> Vec<(&'static str, &'static str)> {
    TOOLS
        .iter()
        .filter_map(|tool| tool.command.map(|command| (tool.name, command)))
        .collect()
}

/// The JSON Schema a client is shown for a tool's arguments.
///
/// Generated from the same [`Field`] list [`crate::args`] decodes, so a client can never be
/// told about an argument the server will ignore, nor left unaware of one it needs.
#[must_use]
pub fn input_schema(tool: &Tool) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for field in tool.fields {
        properties.insert(field.name.to_owned(), field_schema(field));
        if field.required {
            required.push(Value::from(field.name));
        }
    }
    let mut schema = Map::new();
    schema.insert("type".to_owned(), Value::from("object"));
    schema.insert("properties".to_owned(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert("required".to_owned(), Value::Array(required));
    }
    schema.insert("additionalProperties".to_owned(), Value::from(false));
    Value::Object(schema)
}

/// The schema of one field.
fn field_schema(field: &Field) -> Value {
    let mut description = field.doc.to_owned();
    match field.default {
        Some(default) if default != "null" => {
            description.push_str(&format!(" Default: `{default}`."));
        }
        _ => {}
    }
    if !field.required && field.default.is_none() {
        description.push_str(" Optional.");
    }
    let mut schema = match field.kind {
        FieldKind::Id { kind } => {
            description.push_str(&format!(
                " Omit it and the engine allocates the next free {kind} identifier."
            ));
            json!({"type": "integer", "minimum": 1})
        }
        FieldKind::OptionalId { .. } => json!({"type": ["integer", "null"], "minimum": 1}),
        FieldKind::Rational => {
            description.push_str(
                " Exactly: a whole number, a decimal such as `0.5`, a ratio string such as \
                 `\"24000/1001\"`, or `{\"num\":24000,\"den\":1001}`.",
            );
            json!({"type": ["integer", "number", "string", "object"]})
        }
        FieldKind::Range => json!({
            "oneOf": [
                {"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2,
                 "description": "Half-open `[start, end)`."},
                {"type": "object", "properties": {"start": {"type": "integer"},
                                                  "end": {"type": "integer"}},
                 "required": ["start", "end"], "additionalProperties": false},
            ],
        }),
        FieldKind::Bool => json!({"type": "boolean"}),
        FieldKind::Integer => json!({"type": "integer"}),
        FieldKind::Text => json!({"type": "string"}),
        FieldKind::Names(names) => json!({"type": "string", "enum": names}),
        FieldKind::NameList(names) => {
            json!({"type": "array", "items": {"type": "string", "enum": names}})
        }
        FieldKind::Object(fields) => shape_schema(fields),
        FieldKind::List(fields) => json!({"type": "array", "items": shape_schema(fields)}),
        FieldKind::Params => json!({
            "oneOf": [
                {"type": "object", "description": "Parameter key to value."},
                {"type": "array", "items": {
                    "type": "object",
                    "properties": {"key": {"type": "string"}},
                    "required": ["key"],
                }},
            ],
        }),
        FieldKind::Param => {
            description.push_str(
                " A number, a string, a switch, a colour as `[r,g,b]` in sixteens of eight \
                 bits, or the tagged form `{\"type\":\"rational\",\"value\":{\"num\":1,\"den\":2}}`.",
            );
            json!({"type": ["number", "string", "boolean", "array", "object"]})
        }
    };
    if let Value::Object(map) = &mut schema {
        map.insert("description".to_owned(), Value::from(description));
    }
    schema
}

/// The schema of a nested entity.
fn shape_schema(fields: &[Field]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for field in fields {
        properties.insert(field.name.to_owned(), field_schema(field));
        if field.required {
            required.push(Value::from(field.name));
        }
    }
    let mut schema = Map::new();
    schema.insert("type".to_owned(), Value::from("object"));
    schema.insert("properties".to_owned(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert("required".to_owned(), Value::Array(required));
    }
    schema.insert("additionalProperties".to_owned(), Value::from(false));
    Value::Object(schema)
}

/// The description a tool is advertised with, example included.
#[must_use]
pub fn advertised_description(tool: &Tool) -> String {
    format!(
        "{}\n\nArguments, as a worked call:\n{}",
        tool.description, tool.example
    )
}

/// The `annotations` object of a tool, which a client may use to gate a call.
#[must_use]
pub fn annotations(tool: &Tool) -> Value {
    json!({
        "title": tool.title,
        "readOnlyHint": tool.read_only,
        "destructiveHint": tool.destructive,
        "idempotentHint": tool.read_only,
        "openWorldHint": false,
    })
}

/// The tool as `tools/list` sends it.
#[must_use]
pub fn advertised(tool: &Tool) -> Value {
    json!({
        "name": tool.name,
        "title": tool.title,
        "description": advertised_description(tool),
        "inputSchema": input_schema(tool),
        "annotations": annotations(tool),
    })
}

/// The names of the model's command variants, read out of the model itself.
///
/// The check is indirect on purpose: a command's name only exists as a serde tag, so the
/// honest way to enumerate them from another crate is to ask serde what it accepts. The
/// error message for an unknown tag lists every tag it would have taken, which is exactly
/// the list a catalogue must cover.
#[must_use]
pub fn model_command_names() -> Vec<String> {
    let error = serde_json::from_value::<maj_core::Command>(json!({"kind": "\u{0}unknown"}))
        .expect_err("an unknown command tag must be refused");
    let text = error.to_string();
    let (_, rest) = text
        .split_once("expected one of ")
        .expect("serde lists the accepted variants");
    rest.trim_matches(|c: char| c == '`' || c == ' ' || c == '\n' || c == '}')
        .split("`, `")
        .map(str::to_owned)
        .collect()
}

/// The names the model spells each effect with, for a test to compare against the catalogue.
#[must_use]
pub fn model_effect_names() -> Vec<String> {
    names_of(
        EffectKind::ALL
            .iter()
            .map(|kind| serde_json::to_value(kind.key())),
    )
}

/// The names the model spells each blend mode with.
///
/// Round-tripped through serde rather than written out again, so this list cannot disagree
/// with what the command log stores.
#[must_use]
pub fn model_blend_names() -> Vec<String> {
    names_of(BlendMode::ALL.iter().map(serde_json::to_value))
}

/// The names the model spells each track kind with.
#[must_use]
pub fn model_track_kind_names() -> Vec<String> {
    names_of(
        [TrackKind::Video, TrackKind::Audio]
            .iter()
            .map(serde_json::to_value),
    )
}

/// The names the model spells each animation channel with.
#[must_use]
pub fn model_channel_names() -> Vec<String> {
    names_of(AnimChannel::ALL.iter().map(serde_json::to_value))
}

/// The names the model spells each mask shape with.
#[must_use]
pub fn model_mask_shape_names() -> Vec<String> {
    names_of(
        [MaskShape::Rectangle, MaskShape::Ellipse]
            .iter()
            .map(serde_json::to_value),
    )
}

/// The names the model spells each transition with.
#[must_use]
pub fn model_transition_names() -> Vec<String> {
    names_of(TransitionKind::ALL.iter().map(serde_json::to_value))
}

/// The names the model spells each interpolation with.
#[must_use]
pub fn model_interpolation_names() -> Vec<String> {
    names_of(
        [Interpolation::Hold, Interpolation::Linear]
            .iter()
            .map(serde_json::to_value),
    )
}

/// Given serialised enum values, the names they carry, sorted and deduplicated.
///
/// A name that does not serialise as a string is dropped rather than guessed at: the
/// vocabularies in this crate are all plain enums, and a silent invention would be worse
/// than a shorter list.
fn names_of(values: impl Iterator<Item = Result<Value, serde_json::Error>>) -> Vec<String> {
    let mut names: Vec<String> = values
        .filter_map(|value| match value {
            Ok(Value::String(name)) => Some(name),
            _ => None,
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_is_named_uniquely_and_prefixed() {
        let mut names: Vec<&str> = TOOLS.iter().map(|tool| tool.name).collect();
        assert!(
            names.iter().all(|name| name.starts_with("maj_")),
            "{names:?}"
        );
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "tool names must be unique");
    }

    #[test]
    fn every_tool_describes_itself_and_shows_a_worked_call() {
        for tool in TOOLS {
            assert!(!tool.title.is_empty(), "{} has no title", tool.name);
            assert!(
                tool.description.len() > 40,
                "{} has a thin description",
                tool.name
            );
            let example: Value = serde_json::from_str(tool.example).unwrap_or_else(|error| {
                panic!("{} has an example that is not JSON: {error}", tool.name)
            });
            let object = example
                .as_object()
                .unwrap_or_else(|| panic!("{} example is not an object", tool.name));
            // The example must be callable as written, which means no invented arguments.
            for key in object.keys() {
                assert!(
                    tool.fields.iter().any(|field| field.name == key),
                    "{} example uses an argument it does not declare: `{key}`",
                    tool.name
                );
            }
            for field in tool.fields {
                if field.required {
                    assert!(
                        object.contains_key(field.name),
                        "{} example omits the required argument `{}`",
                        tool.name,
                        field.name
                    );
                }
            }
        }
    }

    #[test]
    fn every_declared_default_is_json() {
        fn walk(tool: &Tool, fields: &[Field]) {
            for field in fields {
                if let Some(default) = field.default {
                    serde_json::from_str::<Value>(default).unwrap_or_else(|error| {
                        panic!(
                            "{}.{} has a default that is not JSON: {error}",
                            tool.name, field.name
                        )
                    });
                }
                match field.kind {
                    FieldKind::Object(nested) | FieldKind::List(nested) => walk(tool, nested),
                    _ => {}
                }
            }
        }
        for tool in TOOLS {
            walk(tool, tool.fields);
        }
    }

    #[test]
    fn every_name_field_offers_only_names_the_model_knows() {
        fn walk(tool: &Tool, fields: &[Field]) {
            for field in fields {
                if let FieldKind::Names(names) = field.kind {
                    assert!(
                        !names.is_empty(),
                        "{}.{} offers no names",
                        tool.name,
                        field.name
                    );
                    let mut sorted: Vec<&str> = names.to_vec();
                    let count = sorted.len();
                    sorted.sort_unstable();
                    sorted.dedup();
                    assert_eq!(
                        count,
                        sorted.len(),
                        "{}.{} repeats a name",
                        tool.name,
                        field.name
                    );
                }
                match field.kind {
                    FieldKind::Object(nested) | FieldKind::List(nested) => walk(tool, nested),
                    _ => {}
                }
            }
        }
        for tool in TOOLS {
            walk(tool, tool.fields);
        }
        // The sets the catalogue offers are the sets the model accepts, name for name.
        for (offered, model) in [
            (EFFECT_KINDS, model_effect_names()),
            (BLEND_MODES, model_blend_names()),
            (TRACK_KINDS, model_track_kind_names()),
            (CHANNELS, model_channel_names()),
            (MASK_SHAPES, model_mask_shape_names()),
            (TRANSITION_KINDS, model_transition_names()),
            (INTERPOLATIONS, model_interpolation_names()),
        ] {
            let mut sorted: Vec<String> = offered.iter().map(|name| (*name).to_owned()).collect();
            sorted.sort();

            assert_eq!(
                sorted, model,
                "the catalogue offers a name the model does not accept"
            );
        }
    }

    #[test]
    fn the_catalogue_covers_every_command_the_model_defines() {
        let mut declared: Vec<String> = command_tools()
            .iter()
            .map(|(_, command)| (*command).to_owned())
            .collect();
        let mut model = model_command_names();
        declared.sort();
        model.sort();
        assert_eq!(
            declared, model,
            "a command with no tool, or a tool for a command that does not exist"
        );
        assert_eq!(model.len(), 30, "the model's command count moved");
    }

    #[test]
    fn a_tool_that_appends_a_command_never_claims_to_be_read_only() {
        for tool in TOOLS {
            if tool.command.is_some() {
                assert!(
                    !tool.read_only,
                    "{} writes and claims to be read-only",
                    tool.name
                );
            }
        }
    }

    #[test]
    fn a_nested_entity_is_always_complete_so_the_model_never_refuses_it() {
        // A nested shape is assembled by the server, so every field of it must be either
        // required of the client or filled by a default. A nested field with neither would
        // produce an object the model rejects for a missing member the client cannot see.
        fn walk(tool: &Tool, path: &str, fields: &[Field]) {
            for field in fields {
                assert!(
                    field.required || field.default.is_some() || field.name == "id",
                    "{}.{path}{} is neither required, defaulted, nor an allocated identifier",
                    tool.name,
                    field.name
                );
                match field.kind {
                    FieldKind::Object(nested) | FieldKind::List(nested) => {
                        walk(tool, &format!("{path}{}.", field.name), nested);
                    }
                    _ => {}
                }
            }
        }
        for tool in TOOLS {
            for field in tool.fields {
                match field.kind {
                    FieldKind::Object(nested) | FieldKind::List(nested) => {
                        walk(tool, &format!("{}.", field.name), nested);
                    }
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn a_schema_is_an_object_that_forbids_invented_arguments() {
        let tool = tool("maj_append_clip").expect("the tool exists");
        let schema = input_schema(tool);
        assert_eq!(schema["type"], json!("object"));
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!(["sequence", "track", "clip"]));
        assert_eq!(schema["properties"]["sequence"]["type"], json!("integer"));
        let nested = &schema["properties"]["clip"];
        assert!(nested["properties"]["asset_id"].is_object());
        assert_eq!(
            nested["required"],
            json!(["asset_id", "name", "source_range", "timeline_start"])
        );
        // The identifier is described but not demanded: the engine allocates it.
        assert!(
            nested["properties"]["id"]["description"]
                .as_str()
                .expect("a description")
                .contains("allocates")
        );
    }

    #[test]
    fn a_lookup_by_an_unknown_name_finds_nothing() {
        assert!(tool("maj_do_the_impossible").is_none());
    }

    #[test]
    fn annotations_say_what_a_call_does_to_the_project() {
        let read = annotations(tool("maj_project_state").expect("the tool exists"));
        assert_eq!(read["readOnlyHint"], json!(true));
        assert_eq!(read["openWorldHint"], json!(false));
        let remove = annotations(tool("maj_remove_clip").expect("the tool exists"));
        assert_eq!(remove["readOnlyHint"], json!(false));
        assert_eq!(remove["destructiveHint"], json!(true));
    }
}

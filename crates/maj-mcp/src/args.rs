//! Friendly arguments in, canonical commands out.
//!
//! A model writing a tool call should not have to know that a scale factor is stored as a
//! reduced ratio, that a missing identifier must be `1` rather than `"peek"`, or that an
//! effect's parameter list has a declaration order. This module is the whole of that
//! translation, and it is deliberately one directional: arguments are decoded into the JSON
//! the command log stores, and then handed to `serde` to become a
//! [`Command`](maj_core::Command). Everything the model says must therefore survive the
//! model's own deserialisation, which is what keeps a clever decoder from quietly inventing
//! a command the engine would never have accepted.
//!
//! # The four conveniences
//!
//! * **Identifiers are optional.** A field named `id` inside an entity is allocated from
//!   [`Project::next_unused_id`](maj_core::Project::next_unused_id) when the client leaves it
//!   out, so a model never guesses a number that is already taken. The allocator is derived
//!   from the project on every call, never stored, which is the same reason the model's own
//!   [`IdAllocator`] is not persisted: a counter that is not a function
//!   of the log would diverge under replay.
//! * **Numbers are exact.** `3`, `0.5`, `"1/2"`, `"24000/1001"` and `{"num":1,"den":3}` all
//!   become a [`Rational`]. A decimal is converted by exact long
//!   arithmetic — `0.1` is `1/10`, not the nearest double — because the render is a pure
//!   function of this log.
//! * **Ranges may be written two ways.** `[12, 60]` and `{"start":12,"end":60}` mean the same
//!   half-open range, and both are checked for ordering before they reach the model.
//! * **Entity defaults are filled.** A clip with no transform gets the identity, an effect
//!   with no parameters gets its declared defaults in declaration order. A nested object
//!   assembled here is always complete, so the model never refuses one for a member the
//!   client could not see.

use std::fmt;

use maj_core::{
    ClipId, EffectId, EffectKind, IdAllocator, ParamValue, Project, SequenceId, TrackId,
};
use maj_time::{FrameRange, Rational};
use serde_json::{Map, Value, json};

use crate::catalog::{Field, FieldKind, Tool};

/// Largest channel value of a colour, from [`maj_core::composite::RGBA_MAX`].
///
/// Colours written as eight-bit code values are widened by `* 257`, which maps `255` onto
/// `65535` exactly and every other value onto its unique eight-bit expansion.
const RGBA_MAX: u64 = u16::MAX as u64;

/// Why a call's arguments could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgError {
    /// A required argument was absent.
    Missing {
        /// The argument's name.
        field: String,
    },
    /// An argument was present but could not be that argument.
    Invalid {
        /// The argument's name, or the path to it inside a nested entity.
        field: String,
        /// What was wrong, in one sentence.
        message: String,
    },
}

impl ArgError {
    fn missing(field: &str) -> Self {
        Self::Missing {
            field: field.to_owned(),
        }
    }

    fn invalid(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Invalid {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ArgError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { field } => write!(formatter, "`{field}` is required"),
            Self::Invalid { field, message } => write!(formatter, "`{field}`: {message}"),
        }
    }
}

impl std::error::Error for ArgError {}

/// How a numeric parameter is stored, which the effect's own schema decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Numeric {
    /// A ratio, the normal case for amounts and angles.
    Ratio,
    /// A whole number, such as a grain seed.
    Whole,
}

/// Decodes a tool's arguments into the canonical JSON of the command it appends.
///
/// The result is the command *as the log stores it*: internally tagged with `kind`, with
/// every identifier resolved and every parameter in its declared form. Callers deserialise
/// it into a [`Command`](maj_core::Command) before applying it, so nothing here can commit
/// an edit on its own.
///
/// # Errors
///
/// Returns [`ArgError`] for a missing required argument, an argument the tool does not
/// declare, or a value that is not the right shape. A value that is the right shape but
/// violates a model rule — a negative speed, a parameter outside its range — is deliberately
/// *not* rejected here: the model rejects it, and its message is the one worth showing.
pub fn decode(
    tool: &Tool,
    args: &Map<String, Value>,
    project: &Project,
) -> Result<Value, ArgError> {
    for key in args.keys() {
        if !tool.fields.iter().any(|field| field.name == key) {
            return Err(ArgError::invalid(
                key,
                format!("`{}` has no such argument", tool.name),
            ));
        }
    }
    let mut decoder = Decoder {
        ids: IdAllocator::for_project(project),
        project,
    };
    let mut command = Map::new();
    if let Some(kind) = tool.command {
        command.insert("kind".to_owned(), Value::from(kind));
    }
    for field in tool.fields {
        // A session names a project the server holds, not a field of a command.
        if field.name == "session" {
            continue;
        }
        let numeric = hint_for(tool, args, project);
        let value = match args.get(field.name) {
            Some(value) => decoder.value(field, value, numeric)?,
            None if field.required => return Err(ArgError::missing(field.name)),
            None => match field.default {
                Some(default) => {
                    let parsed = parse_default(field, default)?;
                    decoder.value(field, &parsed, numeric)?
                }
                None if field.name == "index" => Value::from(decoder.append_index(tool, args)?),
                // An optional argument with no default and no rule is simply absent, which
                // only happens for arguments a helper tool reads for itself.
                None => continue,
            },
        };
        command.insert(field.name.to_owned(), value);
    }
    Ok(Value::Object(command))
}

/// Parses a declared default, which is JSON by construction and checked by a test.
fn parse_default(field: &Field, default: &str) -> Result<Value, ArgError> {
    serde_json::from_str(default).map_err(|error| {
        ArgError::invalid(
            field.name,
            format!("the engine's own default is not valid JSON: {error}"),
        )
    })
}

/// The state a decode carries: the identifier allocator, and the project for the lookups
/// that make a friendly argument decodable at all.
struct Decoder<'a> {
    ids: IdAllocator,
    project: &'a Project,
}

impl Decoder<'_> {
    /// Where an omitted `index` puts something: at the end of the list it belongs in.
    ///
    /// The end is what an editor means by "add", and computing it here rather than asking the
    /// client keeps the client from having to read a list length it just changed.
    fn append_index(&mut self, tool: &Tool, args: &Map<String, Value>) -> Result<usize, ArgError> {
        let index = match tool.name {
            "maj_create_media_asset" => self.project.assets().len(),
            "maj_create_sequence" => self.project.sequences().len(),
            "maj_create_track" => {
                let sequence = required_id(args, "sequence")?;
                self.project
                    .sequence(SequenceId::new(sequence))
                    .map_or(0, |sequence| sequence.tracks().len())
            }
            "maj_add_effect" | "maj_add_mask" => {
                let clip = required_id(args, "clip")?;
                self.project.clip(ClipId::new(clip)).map_or(0, |clip| {
                    if tool.name == "maj_add_mask" {
                        clip.masks().len()
                    } else {
                        clip.effects().len()
                    }
                })
            }
            "maj_add_transition" => {
                let sequence = required_id(args, "sequence")?;
                let track = required_id(args, "track")?;
                self.project
                    .sequence(SequenceId::new(sequence))
                    .and_then(|sequence| sequence.track(TrackId::new(track)))
                    .map_or(0, |track| track.transitions().len())
            }
            other => {
                return Err(ArgError::invalid(
                    other,
                    "this tool has no list to append to",
                ));
            }
        };
        Ok(index)
    }

    /// Decodes one field.
    fn value(&mut self, field: &Field, value: &Value, numeric: Numeric) -> Result<Value, ArgError> {
        match field.kind {
            FieldKind::Id { .. } | FieldKind::OptionalId { .. } => {
                if value.is_null() {
                    return Ok(Value::Null);
                }
                identifier(field.name, value)
            }
            FieldKind::Rational => rational(field.name, value),
            FieldKind::Range => range(field.name, value),
            FieldKind::Bool => value
                .as_bool()
                .map(Value::Bool)
                .ok_or_else(|| ArgError::invalid(field.name, "expected `true` or `false`")),
            FieldKind::Integer => integer(field.name, value),
            FieldKind::Text => value
                .as_str()
                .map(|text| Value::from(text.to_owned()))
                .ok_or_else(|| ArgError::invalid(field.name, "expected a string")),
            FieldKind::Names(names) => name(field.name, value, names),
            FieldKind::NameList(names) => {
                let Value::Array(items) = value else {
                    return Err(ArgError::invalid(field.name, "expected a list of names"));
                };
                let mut out = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    out.push(name(&format!("{}[{index}]", field.name), item, names)?);
                }
                Ok(Value::Array(out))
            }
            FieldKind::Object(shape) => self.entity(shape, value, field.name),
            FieldKind::List(shape) => {
                let Value::Array(items) = value else {
                    return Err(ArgError::invalid(field.name, "expected a list"));
                };
                let mut out = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    out.push(self.entity(shape, item, &format!("{}[{index}]", field.name))?);
                }
                Ok(Value::Array(out))
            }
            // An effect's parameters need the effect's kind, which is a sibling field, so
            // they are decoded by `identity`-aware code in `entity` rather than here.
            FieldKind::Params => Err(ArgError::invalid(
                field.name,
                "parameters are decoded with the effect they belong to",
            )),
            FieldKind::Param => parameter(field.name, value, numeric),
        }
    }

    /// Decodes an entity: every field the shape declares, with defaults filled.
    fn entity(&mut self, shape: &[Field], args: &Value, path: &str) -> Result<Value, ArgError> {
        let Value::Object(map) = args else {
            return Err(ArgError::invalid(path, "expected an object"));
        };
        for key in map.keys() {
            if !shape.iter().any(|field| field.name == key) {
                return Err(ArgError::invalid(format!("{path}.{key}"), "no such field"));
            }
        }
        // An effect's parameters need the effect's own kind, which is a sibling field.
        let effect = effect_kind_of(shape, map);
        let mut out = Map::new();
        for field in shape {
            let nested = format!("{path}.{}", field.name);
            let supplied = match map.get(field.name) {
                Some(value) => Some(value.clone()),
                None => match field.default {
                    Some(default) => Some(serde_json::from_str(default).map_err(|error| {
                        ArgError::invalid(
                            nested.clone(),
                            format!("the engine's own default is not valid JSON: {error}"),
                        )
                    })?),
                    // An entity's own identifier is the only field the engine fills in for
                    // the client without being told how.
                    None if field.name == "id" => None,
                    None => return Err(ArgError::missing(&nested)),
                },
            };
            let value = match (supplied, field.kind) {
                (None, kind) => self.allocate(&kind),
                (Some(value), FieldKind::Params) => {
                    let Some(kind) = effect else {
                        return Err(ArgError::invalid(
                            nested,
                            "parameters belong to an effect, and no effect kind was given",
                        ));
                    };
                    self.effect_params(kind, &value, &nested)?
                }
                (Some(value), _) => self.value(field, &value, Numeric::Ratio)?,
            };
            out.insert(field.name.to_owned(), value);
        }
        Ok(Value::Object(out))
    }

    /// Hands out the next free identifier of a kind.
    fn allocate(&mut self, kind: &FieldKind) -> Value {
        let id = match kind {
            FieldKind::Id { kind } | FieldKind::OptionalId { kind } => match *kind {
                "asset" => self.ids.media_asset().raw(),
                "sequence" => self.ids.sequence().raw(),
                "track" => self.ids.track().raw(),
                "clip" => self.ids.clip().raw(),
                "effect" => self.ids.effect().raw(),
                "mask" => self.ids.mask().raw(),
                _ => self.ids.transition().raw(),
            },
            _ => self.ids.track().raw(),
        };
        Value::from(id)
    }

    /// Decodes an effect's parameter set, merged over the effect's declared defaults.
    ///
    /// Merging rather than replacing is what lets a client say `{"radius": 6}` and get the
    /// rest of the effect as designed. The declared order is kept, because the model
    /// validates an instance against its own schema and would refuse a list in any other
    /// order, and a key the effect does not declare is refused here rather than silently
    /// dropped: a parameter that does nothing is worse than an error.
    fn effect_params(
        &mut self,
        kind: EffectKind,
        value: &Value,
        path: &str,
    ) -> Result<Value, ArgError> {
        let supplied = supplied_params(value, path)?;
        for key in supplied.keys() {
            if kind.spec(key).is_none() {
                return Err(ArgError::invalid(
                    format!("{path}.{key}"),
                    format!("`{}` has no parameter `{key}`", kind.key()),
                ));
            }
        }
        let mut out = Vec::with_capacity(kind.params().len());
        for spec in kind.params() {
            let decoded = match supplied.get(spec.key) {
                Some(value) => parameter(
                    &format!("{path}.{}", spec.key),
                    value,
                    numeric_for(Some(kind), Some(spec.key)),
                )?,
                None => serde_json::to_value(&spec.default).map_err(|error| {
                    ArgError::invalid(
                        path,
                        format!("a default parameter does not serialise: {error}"),
                    )
                })?,
            };
            out.push(json!({"key": spec.key, "value": decoded}));
        }
        Ok(Value::Array(out))
    }
}

/// The parameters a client supplied, in either of the two forms, as a map.
fn supplied_params(value: &Value, path: &str) -> Result<Map<String, Value>, ArgError> {
    match value {
        Value::Object(map) => Ok(map.clone()),
        Value::Array(items) => {
            let mut map = Map::new();
            for (index, item) in items.iter().enumerate() {
                let Value::Object(entry) = item else {
                    return Err(ArgError::invalid(
                        format!("{path}[{index}]"),
                        "expected `{\"key\": ..., \"value\": ...}`",
                    ));
                };
                let Some(key) = entry.get("key").and_then(Value::as_str) else {
                    return Err(ArgError::invalid(
                        format!("{path}[{index}]"),
                        "expected a `key`",
                    ));
                };
                let value = entry.get("value").cloned().unwrap_or(Value::Null);
                if map.insert(key.to_owned(), value).is_some() {
                    return Err(ArgError::invalid(
                        format!("{path}[{index}]"),
                        format!("`{key}` was given twice"),
                    ));
                }
            }
            Ok(map)
        }
        _ => Err(ArgError::invalid(
            path,
            "expected an object of parameters or a list of `{key, value}`",
        )),
    }
}

/// The numeric form a parameter's key calls for, when the effect that owns it is known.
///
/// Only `set_effect_param` needs a project lookup: `add_effect` learns the effect from the
/// arguments themselves, and everywhere else a number is a ratio, because every animatable
/// channel is one.
fn hint_for(tool: &Tool, args: &Map<String, Value>, project: &Project) -> Numeric {
    if tool.name != "maj_set_effect_param" {
        return Numeric::Ratio;
    }
    let key = args.get("key").and_then(Value::as_str);
    numeric_for(effect_in_project(project, args), key)
}

/// Decodes a parameter value into the tagged form the model stores.
fn parameter(path: &str, value: &Value, numeric: Numeric) -> Result<Value, ArgError> {
    match value {
        Value::Bool(flag) => Ok(json!({"type": "boolean", "value": flag})),
        Value::Number(_) => {
            let ratio = ratio_of(path, value)?;
            Ok(match numeric {
                Numeric::Whole if ratio.is_integer() => {
                    json!({"type": "integer", "value": ratio.num()})
                }
                _ => json!({"type": "rational", "value": {"num": ratio.num(), "den": ratio.den()}}),
            })
        }
        Value::String(text) => {
            if let Ok(colour) = colour_of(text) {
                return Ok(colour);
            }
            if let Ok(ratio) = ratio_of(path, value) {
                return Ok(
                    json!({"type": "rational", "value": {"num": ratio.num(), "den": ratio.den()}}),
                );
            }
            Ok(json!({"type": "text", "value": text}))
        }
        Value::Array(items) => colour_of_channels(path, items),
        Value::Object(map) => {
            if let Some(kind) = map.get("type").and_then(Value::as_str) {
                // Already canonical: the model's own deserialisation is the check that
                // matters, and it will report a malformed tagged value by name.
                let out = map.clone();
                if !out.contains_key("value") {
                    return Err(ArgError::invalid(
                        path,
                        format!("a `{kind}` parameter needs a `value`"),
                    ));
                }
                return Ok(Value::Object(out));
            }
            if map.contains_key("num") || map.contains_key("den") {
                let ratio = ratio_of(path, value)?;
                return Ok(
                    json!({"type": "rational", "value": {"num": ratio.num(), "den": ratio.den()}}),
                );
            }
            if map.contains_key("r") && map.contains_key("g") && map.contains_key("b") {
                let mut alpha = RGBA_MAX;
                let mut channels = [0_u64; 3];
                for (slot, key) in channels.iter_mut().zip(["r", "g", "b"]) {
                    let Some(raw) = map.get(key).and_then(Value::as_u64) else {
                        return Err(ArgError::invalid(
                            path,
                            format!("`{key}` must be a whole number"),
                        ));
                    };
                    *slot = widen(raw).ok_or_else(|| {
                        ArgError::invalid(path, format!("`{key}` must be at most {RGBA_MAX}"))
                    })?;
                }
                if let Some(raw) = map.get("a").and_then(Value::as_u64) {
                    alpha = widen(raw)
                        .ok_or_else(|| ArgError::invalid(path, "`a` must be a whole number"))?;
                }
                return Ok(json!({
                    "type": "color",
                    "value": {"r": channels[0], "g": channels[1], "b": channels[2], "a": alpha},
                }));
            }
            Err(ArgError::invalid(
                path,
                "expected a number, a string, a switch, a colour, or a tagged value",
            ))
        }
        Value::Null => Err(ArgError::invalid(path, "expected a value, not null")),
    }
}

/// A colour from a `#rrggbb` or `#rrggbbaa` string.
///
/// The `#` is required and not merely tolerated, because `deadbeef` and `cafebabe` are both
/// eight hexadecimal digits and both plausible values of a *text* parameter. A colour that
/// has to announce itself cannot be confused with a word.
fn colour_of(text: &str) -> Result<Value, ArgError> {
    let Some(digits) = text.strip_prefix('#') else {
        return Err(ArgError::invalid("colour", "expected `#rrggbb`"));
    };
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) || !(digits.len() == 6 || digits.len() == 8) {
        return Err(ArgError::invalid("colour", "expected `#rrggbb`"));
    }
    let byte = |at: usize| -> Result<u64, ArgError> {
        u64::from_str_radix(&digits[at..at + 2], 16)
            .map_err(|error| ArgError::invalid("colour", error.to_string()))
    };
    let channels = [byte(0)?, byte(2)?, byte(4)?];
    let alpha = if digits.len() == 8 { byte(6)? } else { 255 };
    let mut widened = [0_u64; 4];
    for (slot, raw) in widened
        .iter_mut()
        .zip([channels[0], channels[1], channels[2], alpha])
    {
        *slot = widen(raw).ok_or_else(|| ArgError::invalid("colour", "out of range"))?;
    }
    Ok(json!({
        "type": "color",
        "value": {"r": widened[0], "g": widened[1], "b": widened[2], "a": widened[3]},
    }))
}

/// A colour from three or four eight-bit channels.
fn colour_of_channels(path: &str, items: &[Value]) -> Result<Value, ArgError> {
    if items.len() != 3 && items.len() != 4 {
        return Err(ArgError::invalid(
            path,
            "a colour is three or four eight-bit channel values",
        ));
    }
    let mut widened = [0_u64; 4];
    widened[3] = RGBA_MAX;
    for (index, item) in items.iter().enumerate() {
        let raw = item.as_u64().ok_or_else(|| {
            ArgError::invalid(path, "a colour channel must be a whole number in 0..=255")
        })?;
        widened[index] = widen(raw)
            .ok_or_else(|| ArgError::invalid(path, "a colour channel must be in 0..=255"))?;
    }
    Ok(json!({
        "type": "color",
        "value": {"r": widened[0], "g": widened[1], "b": widened[2], "a": widened[3]},
    }))
}

/// Widens an eight-bit code value to the sixteen-bit space the model stores.
///
/// `255 * 257` is `65_535` exactly, which is what makes this an expansion rather than an
/// approximation: every eight-bit value keeps its exact position in the wider range.
fn widen(raw: u64) -> Option<u64> {
    if raw > 255 {
        // A value already in the sixteen-bit space is taken as it stands.
        return (raw <= RGBA_MAX).then_some(raw);
    }
    Some(raw * 257)
}

/// An entity identifier, from a number or from the `kind#n` form a log prints.
fn identifier(field: &str, value: &Value) -> Result<Value, ArgError> {
    if let Some(number) = value.as_u64() {
        if number == 0 {
            return Err(ArgError::invalid(field, "identifiers start at one"));
        }
        return Ok(Value::from(number));
    }
    if let Some(text) = value.as_str() {
        let digits = text.rsplit_once('#').map_or(text, |(_, digits)| digits);
        if let Ok(number) = digits.parse::<u64>()
            && number > 0
        {
            return Ok(Value::from(number));
        }
    }
    Err(ArgError::invalid(
        field,
        "expected an identifier, such as 4 or \"clip#4\"",
    ))
}

/// A whole number.
fn integer(field: &str, value: &Value) -> Result<Value, ArgError> {
    if let Some(number) = value.as_i64() {
        return Ok(Value::from(number));
    }
    if let Some(number) = value.as_u64()
        && let Ok(number) = i64::try_from(number)
    {
        return Ok(Value::from(number));
    }
    Err(ArgError::invalid(field, "expected a whole number"))
}

/// One name from a fixed set.
fn name(field: &str, value: &Value, names: &[&str]) -> Result<Value, ArgError> {
    let Some(text) = value.as_str() else {
        return Err(ArgError::invalid(field, "expected a name"));
    };
    if names.contains(&text) {
        return Ok(Value::from(text.to_owned()));
    }
    Err(ArgError::invalid(
        field,
        format!("`{text}` is not one of {}", names.join(", ")),
    ))
}

/// A rational from any of the four ways one can be written.
fn ratio_of(field: &str, value: &Value) -> Result<Rational, ArgError> {
    match value {
        Value::Number(number) => {
            if let Some(whole) = number.as_i64() {
                return Ok(Rational::from_integer(whole));
            }
            if let Some(whole) = number.as_u64()
                && let Ok(whole) = i64::try_from(whole)
            {
                return Ok(Rational::from_integer(whole));
            }
            decimal(field, &number.to_string())
        }
        Value::String(text) => {
            if let Some((numerator, denominator)) = text.split_once('/') {
                let numerator: i64 = numerator.trim().parse().map_err(|_| {
                    ArgError::invalid(field, format!("`{text}` has no whole numerator"))
                })?;
                let denominator: i64 = denominator.trim().parse().map_err(|_| {
                    ArgError::invalid(field, format!("`{text}` has no whole denominator"))
                })?;
                return Rational::new(numerator, denominator)
                    .map_err(|error| ArgError::invalid(field, error.to_string()));
            }
            decimal(field, text)
        }
        Value::Object(map) => {
            let numerator = map.get("num").and_then(Value::as_i64);
            let denominator = map.get("den").and_then(Value::as_i64);
            match (numerator, denominator) {
                (Some(numerator), Some(denominator)) => Rational::new(numerator, denominator)
                    .map_err(|error| ArgError::invalid(field, error.to_string())),
                (Some(numerator), None) => Ok(Rational::from_integer(numerator)),
                _ => Err(ArgError::invalid(
                    field,
                    "expected `{\"num\": ..., \"den\": ...}`",
                )),
            }
        }
        _ => Err(ArgError::invalid(
            field,
            "expected a number, a ratio such as \"1/2\", or {\"num\": ..., \"den\": ...}",
        )),
    }
}

/// A rational as a ratio of integers.
fn rational(field: &str, value: &Value) -> Result<Value, ArgError> {
    let ratio = ratio_of(field, value)?;
    Ok(json!({"num": ratio.num(), "den": ratio.den()}))
}

/// An exact rational from any of the forms a client may write one in.
///
/// Public because the helper tools read their own arguments — `maj_place_clip` takes a speed
/// and `maj_setup_sequence` a frame rate — and they must accept exactly what a command tool
/// accepts, or a client would have to know which tool it was talking to.
///
/// # Errors
///
/// Returns [`ArgError`] for a value that is not a number in any of those forms.
pub fn ratio(field: &str, value: &Value) -> Result<Rational, ArgError> {
    ratio_of(field, value)
}

/// A half-open frame range from either of the forms a client may write one in.
///
/// # Errors
///
/// Returns [`ArgError`] for a malformed range, and for one whose end precedes its start,
/// which the model's own [`FrameRange`] refuses.
pub fn frame_range(field: &str, value: &Value) -> Result<FrameRange, ArgError> {
    let (start, end) = match value {
        Value::Array(items) if items.len() == 2 => (items[0].clone(), items[1].clone()),
        Value::Object(map) => (
            map.get("start").cloned().unwrap_or(Value::Null),
            map.get("end").cloned().unwrap_or(Value::Null),
        ),
        _ => {
            return Err(ArgError::invalid(
                field,
                "expected `[start, end]` or `{\"start\": ..., \"end\": ...}`",
            ));
        }
    };
    let start = integer(field, &start)?
        .as_i64()
        .expect("an integer decodes to an integer");
    let end = integer(field, &end)?
        .as_i64()
        .expect("an integer decodes to an integer");
    FrameRange::new(start, end).map_err(|error| ArgError::invalid(field, error.to_string()))
}

/// An exact rational from a decimal string.
///
/// The conversion is long multiplication, not a float: `0.1` is `1/10` and `23.976` is
/// `2997/125`. A value that cannot be held exactly is refused rather than rounded, because
/// rounding here would put a number in the command log that the log's reader cannot
/// reproduce.
fn decimal(field: &str, text: &str) -> Result<Rational, ArgError> {
    let text = text.trim();
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (integer_part, fraction_part) = match unsigned.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (unsigned, ""),
    };
    if integer_part.is_empty() && fraction_part.is_empty() {
        return Err(ArgError::invalid(
            field,
            format!("`{text}` is not a number"),
        ));
    }
    if !integer_part.chars().all(|c| c.is_ascii_digit())
        || !fraction_part.chars().all(|c| c.is_ascii_digit())
    {
        return Err(ArgError::invalid(
            field,
            format!("`{text}` is not a decimal number"),
        ));
    }
    let mut denominator: i128 = 1;
    for _ in fraction_part.chars() {
        denominator = denominator.checked_mul(10).ok_or_else(|| {
            ArgError::invalid(field, format!("`{text}` has too many decimal places"))
        })?;
    }
    let whole: i128 = if integer_part.is_empty() {
        0
    } else {
        integer_part
            .parse()
            .map_err(|_| ArgError::invalid(field, format!("`{text}` is out of range")))?
    };
    let fraction: i128 = if fraction_part.is_empty() {
        0
    } else {
        fraction_part
            .parse()
            .map_err(|_| ArgError::invalid(field, format!("`{text}` is out of range")))?
    };
    let mut numerator = whole
        .checked_mul(denominator)
        .and_then(|scaled| scaled.checked_add(fraction))
        .ok_or_else(|| ArgError::invalid(field, format!("`{text}` is out of range")))?;
    if negative {
        numerator = -numerator;
    }
    let numerator = i64::try_from(numerator)
        .map_err(|_| ArgError::invalid(field, format!("`{text}` is out of range")))?;
    let denominator = i64::try_from(denominator)
        .map_err(|_| ArgError::invalid(field, format!("`{text}` has too many decimal places")))?;
    Rational::new(numerator, denominator)
        .map_err(|error| ArgError::invalid(field, error.to_string()))
}

/// A half-open frame range, as a canonical object.
fn range(field: &str, value: &Value) -> Result<Value, ArgError> {
    let range = frame_range(field, value)?;
    Ok(json!({"start": range.start(), "end": range.end()}))
}

/// A required identifier out of a raw argument map, for the lookups that precede decoding.
fn required_id(args: &Map<String, Value>, field: &str) -> Result<u64, ArgError> {
    match args.get(field) {
        Some(value) => identifier(field, value)?
            .as_u64()
            .ok_or_else(|| ArgError::invalid(field, "expected an identifier")),
        None => Err(ArgError::missing(field)),
    }
}

/// The effect kind an entity being decoded is describing, when it is describing one.
///
/// Recognised by its *shape* rather than by the object's name: only an effect declares a
/// parameter set, and the name of its `kind` is resolved against the model's own list of
/// effects rather than against a copy of it that could go stale.
fn effect_kind_of(shape: &[Field], map: &Map<String, Value>) -> Option<EffectKind> {
    if !shape.iter().any(|field| field.kind == FieldKind::Params) {
        return None;
    }
    let name = map.get("kind")?.as_str()?;
    EffectKind::ALL.into_iter().find(|kind| kind.key() == name)
}

/// The effect a `set_effect_param` call is talking about, found in the project.
fn effect_in_project(project: &Project, args: &Map<String, Value>) -> Option<EffectKind> {
    let sequence = args.get("sequence").and_then(Value::as_u64)?;
    let clip = args.get("clip").and_then(Value::as_u64)?;
    let effect = args.get("effect").and_then(Value::as_u64)?;
    project
        .sequence(SequenceId::new(sequence))?
        .clip(ClipId::new(clip))?
        .effect(EffectId::new(effect))
        .map(|effect| effect.kind())
}

/// How a numeric parameter of an effect is stored.
fn numeric_for(effect: Option<EffectKind>, key: Option<&str>) -> Numeric {
    let stored_integer = effect
        .and_then(|kind| key.and_then(|key| kind.spec(key)))
        .is_some_and(|spec| matches!(spec.default, ParamValue::Integer(_)));
    if stored_integer {
        Numeric::Whole
    } else {
        Numeric::Ratio
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maj_core::{
        Command, History, MediaAsset, MediaAssetId, Project, Sequence, SequenceId, Track, TrackId,
        TrackKind,
    };
    use maj_time::Timebase;

    /// A project with one media asset, one sequence and two tracks on it.
    fn project() -> Project {
        let mut project = Project::new("decode");
        let mut history = History::new();
        history
            .apply_local(
                &mut project,
                Command::CreateMediaAsset {
                    index: 0,
                    asset: MediaAsset::new(
                        MediaAssetId::new(1),
                        "shot-01",
                        "generated:shot-01",
                        Timebase::FPS_24,
                        240,
                    )
                    .expect("a valid asset"),
                },
            )
            .expect("applied");
        history
            .apply_local(
                &mut project,
                Command::CreateSequence {
                    index: 0,
                    sequence: Sequence::new(
                        SequenceId::new(1),
                        "Reel A",
                        Timebase::FPS_24,
                        1920,
                        1080,
                        48_000,
                    )
                    .expect("a valid sequence"),
                },
            )
            .expect("applied");
        for (index, kind) in [(0, TrackKind::Video), (1, TrackKind::Audio)] {
            history
                .apply_local(
                    &mut project,
                    Command::CreateTrack {
                        sequence: SequenceId::new(1),
                        index,
                        track: Track::new(TrackId::new(index as u64 + 1), kind, "T"),
                    },
                )
                .expect("applied");
        }
        project
    }

    fn args(text: &str) -> Map<String, Value> {
        match serde_json::from_str(text).expect("the test's arguments are JSON") {
            Value::Object(map) => map,
            other => panic!("expected an object, got {other}"),
        }
    }

    fn decode_tool(name: &str, text: &str, project: &Project) -> Result<Value, ArgError> {
        let tool = crate::catalog::tool(name).expect("the tool exists");
        decode(tool, &args(text), project)
    }

    #[test]
    fn an_omitted_entity_identifier_is_allocated_and_complete() {
        let project = project();
        let command = decode_tool(
            "maj_create_media_asset",
            r#"{"asset":{"name":"shot-02","uri":"generated:shot-02","timebase":"24","duration_frames":96}}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(command["kind"], json!("create_media_asset"));
        // The asset, the sequence and its two tracks use 1..=2, so the next free is three.
        assert_eq!(command["asset"]["id"], json!(3));
        assert_eq!(command["asset"]["timebase"], json!({"num": 24, "den": 1}));
        assert_eq!(command["asset"]["has_video"], json!(true));
        assert_eq!(command["asset"]["has_audio"], json!(false));
        // An omitted index appends rather than guessing at the front of the pool.
        assert_eq!(command["index"], json!(1));
        serde_json::from_value::<Command>(command).expect("the model accepts it");
    }

    #[test]
    fn a_clip_is_filled_in_with_every_default_the_model_needs() {
        let project = project();
        let command = decode_tool(
            "maj_append_clip",
            r#"{"sequence":1,"track":1,"clip":{"asset_id":1,"name":"first","source_range":[0,48],"timeline_start":0}}"#,
            &project,
        )
        .expect("decodes");
        let clip = &command["clip"];
        for (key, expected) in [
            ("speed", json!({"num": 1, "den": 1})),
            ("enabled", json!(true)),
            ("opacity", json!({"num": 1, "den": 1})),
            ("blend", json!("normal")),
            ("effects", json!([])),
            ("animation", json!([])),
            ("masks", json!([])),
        ] {
            assert_eq!(clip[key], expected, "{key} must be filled in");
        }
        assert_eq!(
            clip["transform"],
            json!({"x":{"num":0,"den":1},"y":{"num":0,"den":1},"scale_x":{"num":1,"den":1},
                   "scale_y":{"num":1,"den":1},"rotation":{"num":0,"den":1},
                   "anchor_x":{"num":1,"den":2},"anchor_y":{"num":1,"den":2}})
        );
        let decoded = serde_json::from_value::<Command>(command).expect("the model accepts it");
        assert!(matches!(decoded, Command::AppendClip { .. }));
    }

    #[test]
    fn a_nested_sequence_arrives_with_its_tracks_and_their_clips() {
        let project = Project::new("empty");
        let command = decode_tool(
            "maj_create_sequence",
            r#"{"sequence":{"name":"Reel A","timebase":"24000/1001","width":1920,"height":1080,
                 "sample_rate":48000,"tracks":[{"kind":"video","name":"V1",
                 "clips":[{"asset_id":1,"name":"a","source_range":[0,24],"timeline_start":0}]}]}}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(
            command["sequence"]["timebase"],
            json!({"num": 24000, "den": 1001})
        );
        assert_eq!(command["sequence"]["id"], json!(1));
        assert_eq!(command["sequence"]["tracks"][0]["id"], json!(2));
        assert_eq!(command["sequence"]["tracks"][0]["clips"][0]["id"], json!(3));
        assert_eq!(
            command["sequence"]["tracks"][0]["clips"][0]["speed"],
            json!({"num":1,"den":1})
        );
        serde_json::from_value::<Command>(command).expect("the model accepts it");
    }

    #[test]
    fn every_way_of_writing_a_number_lands_on_the_same_ratio() {
        let project = project();
        for text in [
            r#"{"sequence":1,"clip":1,"opacity":0.5}"#,
            r#"{"sequence":1,"clip":1,"opacity":"0.5"}"#,
            r#"{"sequence":1,"clip":1,"opacity":"1/2"}"#,
            r#"{"sequence":1,"clip":1,"opacity":{"num":1,"den":2}}"#,
        ] {
            let command = decode_tool("maj_set_clip_opacity", text, &project).expect("decodes");
            assert_eq!(command["opacity"], json!({"num": 1, "den": 2}), "{text}");
        }
        // A third is exact, not a rounded double, and a film rate survives in one piece.
        let command = decode_tool(
            "maj_set_clip_speed",
            r#"{"sequence":1,"clip":1,"speed":"23.976"}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(command["speed"], json!({"num": 2997, "den": 125}));
    }

    #[test]
    fn a_range_may_be_an_array_or_an_object_and_is_checked_for_order() {
        let project = project();
        let array = decode_tool(
            "maj_trim_clip",
            r#"{"sequence":1,"clip":1,"source_range":[12,60],"timeline_start":24}"#,
            &project,
        )
        .expect("decodes");
        let object = decode_tool(
            "maj_trim_clip",
            r#"{"sequence":1,"clip":1,"source_range":{"start":12,"end":60},"timeline_start":24}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(array["source_range"], object["source_range"]);
        assert_eq!(array["source_range"], json!({"start": 12, "end": 60}));
        let backwards = decode_tool(
            "maj_trim_clip",
            r#"{"sequence":1,"clip":1,"source_range":[60,12],"timeline_start":24}"#,
            &project,
        );
        assert!(matches!(backwards, Err(ArgError::Invalid { .. })));
    }

    #[test]
    fn an_effect_keeps_its_declared_parameters_in_declared_order() {
        let project = project();
        let command = decode_tool(
            "maj_add_effect",
            r#"{"sequence":1,"clip":1,"effect":{"kind":"vignette","params":{"amount":"1/4"}}}"#,
            &project,
        )
        .expect("decodes");
        let params = command["effect"]["params"].as_array().expect("a list");
        assert_eq!(params.len(), 2, "a partial parameter set keeps the rest");
        assert_eq!(params[0]["key"], json!("amount"));
        assert_eq!(
            params[0]["value"],
            json!({"type":"rational","value":{"num":1,"den":4}})
        );
        assert_eq!(
            params[1]["key"],
            json!("feather"),
            "declaration order is kept"
        );
        assert_eq!(
            params[1]["value"],
            json!({"type":"rational","value":{"num":1,"den":2}})
        );
        // A key the effect does not declare is refused here, by name, rather than reaching
        // the model and coming back as a validation failure.
        let wrong = decode_tool(
            "maj_add_effect",
            r#"{"sequence":1,"clip":1,"effect":{"kind":"vignette","params":{"shade":"1/4"}}}"#,
            &project,
        )
        .expect_err("refused");
        assert!(wrong.to_string().contains("shade"), "{wrong}");
    }

    #[test]
    fn a_grain_seed_is_a_whole_number_because_the_effect_says_so() {
        let project = project();
        let command = decode_tool(
            "maj_add_effect",
            r#"{"sequence":1,"clip":1,"effect":{"kind":"grain","params":{"seed":7,"amount":"1/4"}}}"#,
            &project,
        )
        .expect("decodes");
        let params = command["effect"]["params"].as_array().expect("a list");
        assert_eq!(params[1]["value"], json!({"type": "integer", "value": 7}));
        serde_json::from_value::<Command>(command).expect("the model accepts it");
    }

    #[test]
    fn a_parameter_of_an_effect_already_in_the_project_uses_its_own_kind() {
        // The hint cannot come from the arguments here: only the project knows that effect
        // seven is a grain and that its seed is stored as a whole number.
        let mut project = project();
        let mut history = History::new();
        history
            .apply_local(
                &mut project,
                Command::AppendClip {
                    sequence: SequenceId::new(1),
                    track: TrackId::new(1),
                    clip: maj_core::Clip::new(
                        maj_core::ClipId::new(1),
                        MediaAssetId::new(1),
                        "a",
                        FrameRange::new(0, 24).expect("a range"),
                        0,
                        Rational::from_integer(1),
                    )
                    .expect("a valid clip"),
                },
            )
            .expect("applied");
        history
            .apply_local(
                &mut project,
                Command::AddEffect {
                    sequence: SequenceId::new(1),
                    clip: maj_core::ClipId::new(1),
                    index: 0,
                    effect: maj_core::EffectInstance::new(EffectId::new(7), EffectKind::Grain),
                },
            )
            .expect("applied");
        let command = decode_tool(
            "maj_set_effect_param",
            r#"{"sequence":1,"clip":1,"effect":7,"key":"seed","value":3}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(command["value"], json!({"type": "integer", "value": 3}));
        let command = decode_tool(
            "maj_set_effect_param",
            r#"{"sequence":1,"clip":1,"effect":7,"key":"amount","value":0.5}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(
            command["value"],
            json!({"type": "rational", "value": {"num": 1, "den": 2}})
        );
        serde_json::from_value::<Command>(command).expect("the model accepts it");
    }

    #[test]
    fn a_mask_and_a_transition_are_filled_in_too() {
        let project = project();
        let mask = decode_tool(
            "maj_add_mask",
            r#"{"sequence":1,"clip":1,"mask":{"shape":"ellipse","width":"1/4"}}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(mask["mask"]["center_x"], json!({"num":1,"den":2}));
        assert_eq!(mask["mask"]["opacity"], json!({"num":1,"den":1}));
        assert_eq!(mask["mask"]["feather"], json!({"num":0,"den":1}));
        assert_eq!(mask["mask"]["inverted"], json!(false));
        serde_json::from_value::<Command>(mask).expect("the model accepts it");

        let transition = decode_tool(
            "maj_add_transition",
            r#"{"sequence":1,"track":1,"transition":{"kind":"cross_dissolve","at":48,"duration":24}}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(transition["transition"]["from"], json!(null));
        assert_eq!(transition["transition"]["to"], json!(null));
        serde_json::from_value::<Command>(transition).expect("the model accepts it");
    }

    #[test]
    fn an_omitted_index_lands_at_the_end_of_the_list_it_belongs_to() {
        let project = project();
        let track = decode_tool(
            "maj_create_track",
            r#"{"sequence":1,"track":{"kind":"video","name":"V2"}}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(
            track["index"],
            json!(2),
            "two tracks are already on the sequence"
        );
        assert_eq!(track["track"]["id"], json!(3));
        // And an explicit index is respected as written.
        let explicit = decode_tool(
            "maj_create_track",
            r#"{"sequence":1,"index":0,"track":{"kind":"video","name":"V0"}}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(explicit["index"], json!(0));
    }

    #[test]
    fn an_argument_the_tool_does_not_declare_is_refused_by_name() {
        let project = project();
        let error = decode_tool(
            "maj_move_clip",
            r#"{"sequence":1,"clip":1,"timeline_start":24,"speed":2}"#,
            &project,
        )
        .expect_err("refused");
        assert_eq!(
            error,
            ArgError::Invalid {
                field: "speed".to_owned(),
                message: "`maj_move_clip` has no such argument".to_owned()
            }
        );
        assert!(error.to_string().contains("speed"));
    }

    #[test]
    fn a_missing_required_argument_is_named() {
        let project = project();
        let error = decode_tool("maj_move_clip", r#"{"sequence":1,"clip":1}"#, &project)
            .expect_err("refused");
        assert_eq!(
            error,
            ArgError::Missing {
                field: "timeline_start".to_owned()
            }
        );
    }

    #[test]
    fn a_name_outside_the_vocabulary_is_refused_with_the_list() {
        let project = project();
        let error = decode_tool(
            "maj_set_clip_blend",
            r#"{"sequence":1,"clip":1,"blend":"glow"}"#,
            &project,
        )
        .expect_err("refused");
        let message = error.to_string();
        assert!(message.contains("glow"), "{message}");
        assert!(message.contains("multiply"), "{message}");
    }

    #[test]
    fn a_nested_field_that_does_not_exist_is_refused_with_its_path() {
        let project = project();
        let error = decode_tool(
            "maj_append_clip",
            r#"{"sequence":1,"track":1,"clip":{"asset_id":1,"name":"a","source_range":[0,1],"timeline_start":0,"gain":2}}"#,
            &project,
        )
        .expect_err("refused");
        assert!(error.to_string().contains("clip.gain"), "{error}");
    }

    #[test]
    fn a_colour_may_be_eight_bit_channels_or_a_hex_string() {
        let project = project();
        for text in [
            r#"{"sequence":1,"clip":1,"effect":1,"key":"tint","value":[255,128,0]}"#,
            r##"{"sequence":1,"clip":1,"effect":1,"key":"tint","value":"#ff8000"}"##,
        ] {
            let command = decode_tool("maj_set_effect_param", text, &project).expect("decodes");
            assert_eq!(
                command["value"],
                json!({"type":"color","value":{"r":65535,"g":32896,"b":0,"a":65535}}),
                "{text}"
            );
        }
    }

    #[test]
    fn a_text_parameter_stays_text_and_a_numeric_string_becomes_a_ratio() {
        let project = project();
        let text = decode_tool(
            "maj_set_effect_param",
            r#"{"sequence":1,"clip":1,"effect":1,"key":"label","value":"warm"}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(text["value"], json!({"type": "text", "value": "warm"}));
        let ratio = decode_tool(
            "maj_set_effect_param",
            r#"{"sequence":1,"clip":1,"effect":1,"key":"stops","value":"1/3"}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(
            ratio["value"],
            json!({"type":"rational","value":{"num":1,"den":3}})
        );
    }

    #[test]
    fn an_identifier_may_be_written_as_the_log_prints_it() {
        let project = project();
        let command = decode_tool(
            "maj_remove_clip",
            r#"{"sequence":"sequence#1","clip":"clip#4"}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(command["sequence"], json!(1));
        assert_eq!(command["clip"], json!(4));
    }

    #[test]
    fn a_hexadecimal_ratio_is_not_mistaken_for_a_colour() {
        // `deadbeef` has eight hex digits but no `#`, so it is not a colour, and it is not a
        // number either: it is text, and the effect's own schema decides whether that is
        // acceptable.
        let project = project();
        let command = decode_tool(
            "maj_set_effect_param",
            r#"{"sequence":1,"clip":1,"effect":1,"key":"label","value":"deadbeef"}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(
            command["value"],
            json!({"type": "text", "value": "deadbeef"})
        );
    }

    #[test]
    fn the_session_argument_never_reaches_the_command() {
        let project = project();
        let command = decode_tool(
            "maj_remove_sequence",
            r#"{"session":"b-roll","id":1}"#,
            &project,
        )
        .expect("decodes");
        assert_eq!(command, json!({"kind": "remove_sequence", "id": 1}));
    }

    #[test]
    fn a_decimal_that_cannot_be_held_exactly_is_refused_rather_than_rounded() {
        let error = decimal("speed", "0.1").expect("a tenth is exact");
        assert_eq!((error.num(), error.den()), (1, 10));
        assert!(decimal("speed", "1e-3").is_err());
        assert!(decimal("speed", "0.0000000000000000000001").is_err());
    }
}

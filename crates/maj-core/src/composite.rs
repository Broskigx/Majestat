//! The compositing model: transforms, blend modes, effects, masks and transitions.
//!
//! Everything here is **durable document state**: it is serialised into the `.maj`
//! file, hashed by [`crate::Project::state_hash`], and reached only through inverted
//! commands. If a transform lived in the render layer, the document would no longer
//! describe what the edit *is* — only what some consumer happened to draw.
//!
//! ADR 0006 forbids floating point in the model, and
//! `the_canonical_form_contains_no_floating_point_numbers` enforces it by walking the
//! canonical JSON. That is why transforms store pixels, scales, degrees and anchors as
//! [`Rational`], effect parameters are [`ParamValue`] with no float variant, and
//! parameter **ranges** are declared as `Rational` constants rather than `f64` metadata.
//!
//! A consequence worth naming: keyframe interpolation is *exact*. Interpolating between
//! two rationals at a rational position produces a rational, so an animated transform
//! sampled at frame 10 000 renders bit-identically a year later.

use serde::{Deserialize, Serialize};

use maj_time::Rational;

use crate::error::CoreError;
use crate::ids::{ClipId, EffectId, MaskId, TransitionId};

/// How a layer combines with the layers already composited beneath it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    /// Source over destination, weighted by source alpha. The default.
    Normal,
    /// Additive: `source + destination`, clamped at the output boundary.
    Add,
    /// Multiplicative darkening.
    Multiply,
    /// Multiplicative brightening.
    Screen,
    /// Contrast-increasing combination of [`BlendMode::Multiply`] and
    /// [`BlendMode::Screen`].
    Overlay,
    /// Keeps the darker contribution per channel.
    Darken,
    /// Keeps the lighter contribution per channel.
    Lighten,
    /// Absolute difference per channel.
    Difference,
    /// [`BlendMode::Overlay`] with the operands swapped.
    HardLight,
    /// A softer contrast blend with no hard transition at the mid-point.
    SoftLight,
}

impl BlendMode {
    /// Every blend mode, in a fixed order, for exhaustive tests and pickers.
    pub const ALL: [Self; 10] = [
        Self::Normal,
        Self::Add,
        Self::Multiply,
        Self::Screen,
        Self::Overlay,
        Self::Darken,
        Self::Lighten,
        Self::Difference,
        Self::HardLight,
        Self::SoftLight,
    ];

    /// Short stable name, used in logs, manifests and the interface.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Add => "add",
            Self::Multiply => "multiply",
            Self::Screen => "screen",
            Self::Overlay => "overlay",
            Self::Darken => "darken",
            Self::Lighten => "lighten",
            Self::Difference => "difference",
            Self::HardLight => "hard_light",
            Self::SoftLight => "soft_light",
        }
    }
}

/// Placement of a layer inside the sequence frame.
///
/// The mapping is applied in this order, and the order is part of the format:
///
/// 1. the source frame is scaled by `scale_x` and `scale_y` about `anchor`;
/// 2. it is rotated clockwise by `rotation` degrees about `anchor`;
/// 3. it is translated by `x` and `y` pixels.
///
/// The anchor is **normalised inside the source frame**: `1/2` is its centre, `0` its
/// top-left corner, `1` its bottom-right. Translation is in pixels from the centre of
/// the sequence frame with **positive `y` downward**, matching frame addressing.
/// Positive rotation is clockwise, which is what a viewer sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub(crate) x: Rational,
    pub(crate) y: Rational,
    pub(crate) scale_x: Rational,
    pub(crate) scale_y: Rational,
    pub(crate) rotation: Rational,
    pub(crate) anchor_x: Rational,
    pub(crate) anchor_y: Rational,
}

impl Transform {
    /// The transform that changes nothing: unit scale, no rotation, no translation and
    /// the anchor at the centre of the source frame.
    #[must_use]
    pub const fn identity() -> Self {
        Self {
            x: Rational::from_integer(0),
            y: Rational::from_integer(0),
            scale_x: Rational::from_integer(1),
            scale_y: Rational::from_integer(1),
            rotation: Rational::from_integer(0),
            anchor_x: Rational::from_reduced(1, 2),
            anchor_y: Rational::from_reduced(1, 2),
        }
    }

    /// Builds a transform, refusing a non-positive scale.
    ///
    /// A zero or negative scale is an error rather than a clamp: it collapses the layer
    /// to a line or mirrors it, and both must be requested explicitly rather than
    /// inherited from a mistyped value.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        x: Rational,
        y: Rational,
        scale_x: Rational,
        scale_y: Rational,
        rotation: Rational,
        anchor_x: Rational,
        anchor_y: Rational,
    ) -> Result<Self, CoreError> {
        let transform = Self {
            x,
            y,
            scale_x,
            scale_y,
            rotation,
            anchor_x,
            anchor_y,
        };
        transform.validate()?;
        Ok(transform)
    }

    /// Horizontal translation in pixels, positive to the right.
    #[must_use]
    pub const fn x(self) -> Rational {
        self.x
    }

    /// Vertical translation in pixels, positive downward.
    #[must_use]
    pub const fn y(self) -> Rational {
        self.y
    }

    /// Horizontal scale multiplier.
    #[must_use]
    pub const fn scale_x(self) -> Rational {
        self.scale_x
    }

    /// Vertical scale multiplier.
    #[must_use]
    pub const fn scale_y(self) -> Rational {
        self.scale_y
    }

    /// Clockwise rotation in degrees.
    #[must_use]
    pub const fn rotation(self) -> Rational {
        self.rotation
    }

    /// Horizontal anchor, normalised inside the source frame.
    #[must_use]
    pub const fn anchor_x(self) -> Rational {
        self.anchor_x
    }

    /// Vertical anchor, normalised inside the source frame.
    #[must_use]
    pub const fn anchor_y(self) -> Rational {
        self.anchor_y
    }

    /// Reports whether this transform is exactly the identity.
    #[must_use]
    pub fn is_identity(self) -> bool {
        self == Self::identity()
    }

    /// Reports whether the transform only translates.
    ///
    /// The renderer uses this to skip the rotation machinery entirely, which is a real
    /// saving on the certified path: a pure translation still resamples, but it needs no
    /// trigonometry and no rotated mask test.
    #[must_use]
    pub fn is_translation_only(self) -> bool {
        self.scale_x == Rational::from_integer(1)
            && self.scale_y == Rational::from_integer(1)
            && self.rotation.is_zero()
    }

    /// Validates the invariants a document may not violate.
    pub fn validate(&self) -> Result<(), CoreError> {
        if !self.scale_x.is_positive() || !self.scale_y.is_positive() {
            return Err(CoreError::invalid(
                "transform scale must be greater than zero",
            ));
        }
        Ok(())
    }
}

/// A colour with 16-bit-per-channel integers, in the declared working space.
///
/// 16 bits rather than 8 so a parameter colour can carry the precision a 10- or 12-bit
/// delivery needs without becoming a float. The renderer converts to linear light on
/// use; the model stays integral.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rgba {
    pub(crate) r: u16,
    pub(crate) g: u16,
    pub(crate) b: u16,
    pub(crate) a: u16,
}

/// Largest value a [`Rgba`] channel can hold.
pub const RGBA_MAX: u16 = u16::MAX;

impl Rgba {
    /// Builds an opaque colour from its components.
    #[must_use]
    pub const fn new(r: u16, g: u16, b: u16) -> Self {
        Self {
            r,
            g,
            b,
            a: RGBA_MAX,
        }
    }

    /// Replaces the alpha component.
    #[must_use]
    pub const fn with_alpha(mut self, a: u16) -> Self {
        self.a = a;
        self
    }

    /// Red component.
    #[must_use]
    pub const fn r(self) -> u16 {
        self.r
    }

    /// Green component.
    #[must_use]
    pub const fn g(self) -> u16 {
        self.g
    }

    /// Blue component.
    #[must_use]
    pub const fn b(self) -> u16 {
        self.b
    }

    /// Alpha component.
    #[must_use]
    pub const fn a(self) -> u16 {
        self.a
    }

    /// Converts to normalised components in `0.0..=1.0`.
    ///
    /// Division of exact integers by a constant is one of the operations IEEE-754
    /// requires to be correctly rounded, so this needs no custom kernel to be
    /// deterministic.
    #[must_use]
    pub fn to_normalised(self) -> [f32; 4] {
        const SCALE: f32 = 1.0 / RGBA_MAX as f32;
        [
            f32::from(self.r) * SCALE,
            f32::from(self.g) * SCALE,
            f32::from(self.b) * SCALE,
            f32::from(self.a) * SCALE,
        ]
    }
}

/// A value an effect parameter can hold.
///
/// There is deliberately no floating-point variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ParamValue {
    /// An exact rational: the normal case for amounts and angles.
    Rational(Rational),
    /// A whole number, for counts such as a grain seed.
    Integer(i64),
    /// A switch.
    Boolean(bool),
    /// Free text, for labels and references.
    Text(String),
    /// A colour.
    Color(Rgba),
}

impl ParamValue {
    /// Reads the value as a rational, converting an integer.
    ///
    /// Returns `None` for non-numeric variants, so a caller cannot silently treat a
    /// boolean as zero.
    #[must_use]
    pub fn as_rational(&self) -> Option<Rational> {
        match self {
            Self::Rational(value) => Some(*value),
            Self::Integer(value) => Some(Rational::from_integer(*value)),
            Self::Boolean(_) | Self::Text(_) | Self::Color(_) => None,
        }
    }

    /// Reads the value as a whole number when it is exactly one.
    #[must_use]
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            Self::Rational(value) if value.is_integer() => Some(value.num()),
            Self::Rational(_) | Self::Boolean(_) | Self::Text(_) | Self::Color(_) => None,
        }
    }

    /// Reads the value as a switch.
    #[must_use]
    pub const fn as_boolean(&self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(*value),
            Self::Rational(_) | Self::Integer(_) | Self::Text(_) | Self::Color(_) => None,
        }
    }

    /// Reads the value as text.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            Self::Rational(_) | Self::Integer(_) | Self::Boolean(_) | Self::Color(_) => None,
        }
    }

    /// Reads the value as a colour.
    #[must_use]
    pub const fn as_color(&self) -> Option<Rgba> {
        match self {
            Self::Color(value) => Some(*value),
            Self::Rational(_) | Self::Integer(_) | Self::Boolean(_) | Self::Text(_) => None,
        }
    }

    /// Short name of the variant, for diagnostics and manifests.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Rational(_) => "rational",
            Self::Integer(_) => "integer",
            Self::Boolean(_) => "boolean",
            Self::Text(_) => "text",
            Self::Color(_) => "color",
        }
    }

    /// Interpolates between two values at `t`, normally in `0/1..=1/1`.
    ///
    /// Arithmetic is exact where the values are exact, so interpolating at the half-way
    /// point returns the true midpoint rather than a rounded approximation. Colours
    /// interpolate per channel on integers, rounding to nearest with ties away from
    /// zero. Booleans and text do not interpolate and return `self`, which is the honest
    /// answer rather than an invented blend.
    pub fn lerp(&self, other: &Self, t: Rational) -> Result<Self, CoreError> {
        match (self, other) {
            (Self::Rational(a), Self::Rational(b)) => Ok(Self::Rational(lerp_rational(*a, *b, t)?)),
            (Self::Integer(a), Self::Integer(b)) => Ok(Self::Rational(lerp_rational(
                Rational::from_integer(*a),
                Rational::from_integer(*b),
                t,
            )?)),
            (Self::Color(a), Self::Color(b)) => Ok(Self::Color(Rgba {
                r: lerp_channel(a.r, b.r, t)?,
                g: lerp_channel(a.g, b.g, t)?,
                b: lerp_channel(a.b, b.b, t)?,
                a: lerp_channel(a.a, b.a, t)?,
            })),
            (Self::Rational(_) | Self::Integer(_), Self::Rational(_) | Self::Integer(_)) => {
                let a = self
                    .as_rational()
                    .ok_or_else(|| CoreError::invalid("interpolation operand is not numeric"))?;
                let b = other
                    .as_rational()
                    .ok_or_else(|| CoreError::invalid("interpolation operand is not numeric"))?;
                Ok(Self::Rational(lerp_rational(a, b, t)?))
            }
            _ => Ok(self.clone()),
        }
    }
}

/// Exact linear interpolation between two rationals at a rational position.
fn lerp_rational(a: Rational, b: Rational, t: Rational) -> Result<Rational, CoreError> {
    let delta = b.checked_sub(a).map_err(CoreError::from)?;
    let scaled = delta.checked_mul(t).map_err(CoreError::from)?;
    a.checked_add(scaled).map_err(CoreError::from)
}

/// Exact linear interpolation between two channels at a rational position.
///
/// The result is rounded to nearest with ties away from zero, computed in `i128` so no
/// intermediate can wrap. Rounding explicitly rather than casting keeps the behaviour
/// identical on every platform.
fn lerp_channel(a: u16, b: u16, t: Rational) -> Result<u16, CoreError> {
    let exact = lerp_rational(
        Rational::from_integer(i64::from(a)),
        Rational::from_integer(i64::from(b)),
        t,
    )?;
    let denominator = u128::try_from(exact.den())
        .map_err(|_| CoreError::invalid("colour interpolation has a non-positive denominator"))?;
    let doubled = denominator
        .checked_mul(2)
        .ok_or_else(|| CoreError::invalid("colour interpolation overflowed"))?;
    let negative = exact.num() < 0;
    let magnitude = u128::from(exact.num().unsigned_abs());
    let numerator = magnitude
        .checked_mul(2)
        .and_then(|value| value.checked_add(denominator))
        .ok_or_else(|| CoreError::invalid("colour interpolation overflowed"))?;
    let rounded = numerator / doubled;
    let signed = if negative {
        -i128::try_from(rounded)
            .map_err(|_| CoreError::invalid("colour interpolation overflowed"))?
    } else {
        i128::try_from(rounded)
            .map_err(|_| CoreError::invalid("colour interpolation overflowed"))?
    };
    u16::try_from(signed.clamp(0, i128::from(RGBA_MAX)))
        .map_err(|_| CoreError::invalid("colour channel outside the 16-bit range"))
}

/// One declared parameter of a built-in effect.
///
/// The ranges are declared here, as rationals, so validation never consults a table of
/// magic numbers and no float exists to disagree about.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamSpec {
    /// Stable key of the parameter.
    pub key: &'static str,
    /// Value used when the effect is created.
    pub default: ParamValue,
    /// Inclusive lower bound, when the parameter is numeric.
    pub min: Option<Rational>,
    /// Inclusive upper bound, when the parameter is numeric.
    pub max: Option<Rational>,
}

const EXPOSURE_PARAMS: [ParamSpec; 1] = [ParamSpec {
    key: "stops",
    default: ParamValue::Rational(Rational::from_integer(0)),
    min: Some(Rational::from_integer(-4)),
    max: Some(Rational::from_integer(4)),
}];

const BRIGHTNESS_PARAMS: [ParamSpec; 1] = [ParamSpec {
    key: "amount",
    default: ParamValue::Rational(Rational::from_integer(0)),
    min: Some(Rational::from_integer(-1)),
    max: Some(Rational::from_integer(1)),
}];

const CONTRAST_PARAMS: [ParamSpec; 1] = [ParamSpec {
    key: "amount",
    default: ParamValue::Rational(Rational::from_integer(1)),
    min: Some(Rational::from_integer(0)),
    max: Some(Rational::from_integer(4)),
}];

const SATURATION_PARAMS: [ParamSpec; 1] = [ParamSpec {
    key: "amount",
    default: ParamValue::Rational(Rational::from_integer(1)),
    min: Some(Rational::from_integer(0)),
    max: Some(Rational::from_integer(4)),
}];

const GAMMA_PARAMS: [ParamSpec; 1] = [ParamSpec {
    key: "value",
    default: ParamValue::Rational(Rational::from_integer(1)),
    min: Some(Rational::from_reduced(1, 10)),
    max: Some(Rational::from_integer(4)),
}];

const HUE_ROTATE_PARAMS: [ParamSpec; 1] = [ParamSpec {
    key: "degrees",
    default: ParamValue::Rational(Rational::from_integer(0)),
    min: Some(Rational::from_integer(-180)),
    max: Some(Rational::from_integer(180)),
}];

const BLUR_PARAMS: [ParamSpec; 1] = [ParamSpec {
    key: "radius",
    default: ParamValue::Rational(Rational::from_integer(4)),
    min: Some(Rational::from_integer(0)),
    max: Some(Rational::from_integer(256)),
}];

const SHARPEN_PARAMS: [ParamSpec; 1] = [ParamSpec {
    key: "amount",
    default: ParamValue::Rational(Rational::from_reduced(1, 2)),
    min: Some(Rational::from_integer(0)),
    max: Some(Rational::from_integer(4)),
}];

const VIGNETTE_PARAMS: [ParamSpec; 2] = [
    ParamSpec {
        key: "amount",
        default: ParamValue::Rational(Rational::from_reduced(1, 2)),
        min: Some(Rational::from_integer(0)),
        max: Some(Rational::from_integer(2)),
    },
    ParamSpec {
        key: "feather",
        default: ParamValue::Rational(Rational::from_reduced(1, 2)),
        min: Some(Rational::from_integer(0)),
        max: Some(Rational::from_integer(1)),
    },
];

const GRAIN_PARAMS: [ParamSpec; 2] = [
    ParamSpec {
        key: "amount",
        default: ParamValue::Rational(Rational::from_reduced(1, 8)),
        min: Some(Rational::from_integer(0)),
        max: Some(Rational::from_integer(1)),
    },
    ParamSpec {
        key: "seed",
        default: ParamValue::Integer(1),
        min: None,
        max: None,
    },
];

/// An effect the certified path implements natively.
///
/// The list is deliberately short and honest: every variant here is implemented by the
/// CPU reference renderer in `maj-render`, so a project using one renders identically in
/// preview and in export. Further effects arrive through `maj-vfx` (M9) and OpenFX
/// (M24), and are labelled when they are not on the certified path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    /// Linear-light exposure change in stops.
    Exposure,
    /// Additive brightness offset.
    Brightness,
    /// Contrast about the mid-grey pivot.
    Contrast,
    /// Saturation about Rec.709 luminance.
    Saturation,
    /// Power function on each channel.
    Gamma,
    /// Rotation of the hue component.
    HueRotate,
    /// Separable box blur applied twice, for a near-Gaussian result.
    Blur,
    /// Unsharp mask.
    Sharpen,
    /// Radial darkening toward the frame corners.
    Vignette,
    /// Deterministic film grain, seeded so it is reproducible.
    Grain,
}

impl EffectKind {
    /// Every built-in effect, in a fixed order.
    pub const ALL: [Self; 10] = [
        Self::Exposure,
        Self::Brightness,
        Self::Contrast,
        Self::Saturation,
        Self::Gamma,
        Self::HueRotate,
        Self::Blur,
        Self::Sharpen,
        Self::Vignette,
        Self::Grain,
    ];

    /// Short stable name, used in logs, manifests and the interface.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Exposure => "exposure",
            Self::Brightness => "brightness",
            Self::Contrast => "contrast",
            Self::Saturation => "saturation",
            Self::Gamma => "gamma",
            Self::HueRotate => "hue_rotate",
            Self::Blur => "blur",
            Self::Sharpen => "sharpen",
            Self::Vignette => "vignette",
            Self::Grain => "grain",
        }
    }

    /// The parameter schema of this effect.
    ///
    /// Declaring the schema as data is what makes parameter editing safe: a key that is
    /// not declared cannot be set, so the inverse of a parameter change is always
    /// another parameter change and never a removal.
    #[must_use]
    pub fn params(self) -> &'static [ParamSpec] {
        match self {
            Self::Exposure => &EXPOSURE_PARAMS,
            Self::Brightness => &BRIGHTNESS_PARAMS,
            Self::Contrast => &CONTRAST_PARAMS,
            Self::Saturation => &SATURATION_PARAMS,
            Self::Gamma => &GAMMA_PARAMS,
            Self::HueRotate => &HUE_ROTATE_PARAMS,
            Self::Blur => &BLUR_PARAMS,
            Self::Sharpen => &SHARPEN_PARAMS,
            Self::Vignette => &VIGNETTE_PARAMS,
            Self::Grain => &GRAIN_PARAMS,
        }
    }

    /// The default parameter list of this effect, in declaration order.
    #[must_use]
    pub fn default_params(self) -> Vec<Param> {
        self.params()
            .iter()
            .map(|spec| Param {
                key: spec.key.to_owned(),
                value: spec.default.clone(),
            })
            .collect()
    }

    /// Looks up the schema of one parameter.
    #[must_use]
    pub fn spec(self, key: &str) -> Option<&'static ParamSpec> {
        self.params().iter().find(|spec| spec.key == key)
    }

    /// Validates one parameter value against the declared schema and range.
    pub fn validate_param(self, key: &str, value: &ParamValue) -> Result<(), CoreError> {
        let spec = self.spec(key).ok_or_else(|| {
            CoreError::invalid(format!("{} has no parameter `{key}`", self.key()))
        })?;
        if matches!(
            value,
            ParamValue::Boolean(_) | ParamValue::Text(_) | ParamValue::Color(_)
        ) {
            return Ok(());
        }
        let actual = value
            .as_rational()
            .ok_or_else(|| CoreError::invalid("a numeric parameter is not numeric"))?;
        if let Some(min) = spec.min {
            if actual < min {
                return Err(CoreError::invalid(format!(
                    "{}.{key} is {actual}, below the minimum {min}",
                    self.key()
                )));
            }
        }
        if let Some(max) = spec.max {
            if actual > max {
                return Err(CoreError::invalid(format!(
                    "{}.{key} is {actual}, above the maximum {max}",
                    self.key()
                )));
            }
        }
        Ok(())
    }
}

/// One parameter of an effect instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Param {
    pub(crate) key: String,
    pub(crate) value: ParamValue,
}

impl Param {
    /// Builds a parameter.
    #[must_use]
    pub fn new(key: impl Into<String>, value: ParamValue) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }

    /// The parameter's stable key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The parameter's value.
    #[must_use]
    pub const fn value(&self) -> &ParamValue {
        &self.value
    }
}

/// An effect applied to a clip, with its own identifier so commands can target it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectInstance {
    pub(crate) id: EffectId,
    pub(crate) kind: EffectKind,
    pub(crate) enabled: bool,
    pub(crate) params: Vec<Param>,
}

impl EffectInstance {
    /// Creates an effect with its declared default parameters.
    #[must_use]
    pub fn new(id: EffectId, kind: EffectKind) -> Self {
        Self {
            id,
            kind,
            enabled: true,
            params: kind.default_params(),
        }
    }

    /// Identifier of this instance.
    #[must_use]
    pub const fn id(&self) -> EffectId {
        self.id
    }

    /// Which built-in effect this is.
    #[must_use]
    pub const fn kind(&self) -> EffectKind {
        self.kind
    }

    /// Reports whether the effect contributes to the output.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Parameters in declaration order.
    #[must_use]
    pub fn params(&self) -> &[Param] {
        &self.params
    }

    /// Looks up one parameter.
    #[must_use]
    pub fn param(&self, key: &str) -> Option<&Param> {
        self.params.iter().find(|param| param.key == key)
    }

    /// Replaces one parameter's value, refusing an undeclared key.
    pub fn set_param(&mut self, key: &str, value: ParamValue) -> Result<(), CoreError> {
        self.kind.validate_param(key, &value)?;
        let slot = self
            .params
            .iter_mut()
            .find(|param| param.key == key)
            .ok_or_else(|| CoreError::invalid(format!("effect has no parameter `{key}`")))?;
        slot.value = value;
        Ok(())
    }

    /// Validates the instance against its own schema, including parameter order.
    pub fn validate(&self) -> Result<(), CoreError> {
        let declared = self.kind.params();
        if self.params.len() != declared.len() {
            return Err(CoreError::invalid(format!(
                "{} expects {} parameters but carries {}",
                self.kind.key(),
                declared.len(),
                self.params.len()
            )));
        }
        for (index, spec) in declared.iter().enumerate() {
            let param = self
                .params
                .get(index)
                .ok_or_else(|| CoreError::invalid("the parameter list is truncated"))?;
            if param.key != spec.key {
                return Err(CoreError::invalid(format!(
                    "{} parameter {index} should be `{}` but is `{}`",
                    self.kind.key(),
                    spec.key,
                    param.key
                )));
            }
            self.kind.validate_param(&param.key, &param.value)?;
        }
        Ok(())
    }
}

/// How a value moves from one keyframe to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    /// The value holds until the next keyframe, then jumps.
    Hold,
    /// The value moves linearly, exactly, between keyframes.
    Linear,
}

/// A clip property that can be animated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimChannel {
    /// Layer opacity, in `0..=1`.
    Opacity,
    /// Horizontal translation in pixels.
    X,
    /// Vertical translation in pixels.
    Y,
    /// Horizontal scale.
    ScaleX,
    /// Vertical scale.
    ScaleY,
    /// Clockwise rotation in degrees.
    Rotation,
}

impl AnimChannel {
    /// Every animatable channel, in a fixed order.
    pub const ALL: [Self; 6] = [
        Self::Opacity,
        Self::X,
        Self::Y,
        Self::ScaleX,
        Self::ScaleY,
        Self::Rotation,
    ];

    /// Short stable name.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Opacity => "opacity",
            Self::X => "x",
            Self::Y => "y",
            Self::ScaleX => "scale_x",
            Self::ScaleY => "scale_y",
            Self::Rotation => "rotation",
        }
    }

    /// The value the channel holds when it is not animated.
    ///
    /// This is also the value an out-of-range sample clamps to, so it must agree with
    /// [`Transform::identity`] and with an unanimated clip's opacity of one.
    #[must_use]
    pub fn base_value(self) -> ParamValue {
        match self {
            Self::Opacity | Self::ScaleX | Self::ScaleY => {
                ParamValue::Rational(Rational::from_integer(1))
            }
            Self::X | Self::Y | Self::Rotation => ParamValue::Rational(Rational::from_integer(0)),
        }
    }
}

/// One keyframe of an animated channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    pub(crate) offset: i64,
    pub(crate) value: ParamValue,
    pub(crate) interpolation: Interpolation,
}

impl Keyframe {
    /// Builds a keyframe at an offset measured in frames from the start of the clip.
    #[must_use]
    pub fn new(offset: i64, value: ParamValue, interpolation: Interpolation) -> Self {
        Self {
            offset,
            value,
            interpolation,
        }
    }

    /// Frame offset from the start of the clip.
    #[must_use]
    pub const fn offset(&self) -> i64 {
        self.offset
    }

    /// Value at this keyframe.
    #[must_use]
    pub const fn value(&self) -> &ParamValue {
        &self.value
    }

    /// How the value leaves this keyframe.
    #[must_use]
    pub const fn interpolation(&self) -> Interpolation {
        self.interpolation
    }
}

/// An animated channel: an ordered list of keyframes.
///
/// Keyframes are held sorted by offset with no duplicates, an invariant
/// [`crate::Project::validate`] enforces. Sorting on write rather than on read means
/// sampling is a binary search and the document has exactly one representation of a
/// given animation — so two equal animations compare equal and hash identically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelAnimation {
    pub(crate) channel: AnimChannel,
    pub(crate) keys: Vec<Keyframe>,
}

impl ChannelAnimation {
    /// Builds an animation, sorting its keyframes and rejecting duplicates.
    pub fn new(channel: AnimChannel, mut keys: Vec<Keyframe>) -> Result<Self, CoreError> {
        if keys.is_empty() {
            return Err(CoreError::invalid(
                "an animation needs at least one keyframe",
            ));
        }
        keys.sort_by_key(Keyframe::offset);
        for pair in keys.windows(2) {
            if pair[0].offset == pair[1].offset {
                return Err(CoreError::invalid(format!(
                    "two keyframes share the offset {}",
                    pair[0].offset
                )));
            }
        }
        Ok(Self { channel, keys })
    }

    /// Replaces the keyframe list, applying the same ordering rules.
    pub fn set_keys(&mut self, keys: Vec<Keyframe>) -> Result<(), CoreError> {
        let rebuilt = Self::new(self.channel, keys)?;
        self.keys = rebuilt.keys;
        Ok(())
    }

    /// The animated channel.
    #[must_use]
    pub const fn channel(&self) -> AnimChannel {
        self.channel
    }

    /// Keyframes in offset order.
    #[must_use]
    pub fn keys(&self) -> &[Keyframe] {
        &self.keys
    }

    /// Samples the channel at an offset in frames from the start of the clip.
    ///
    /// Before the first keyframe the first value holds; after the last, the last value
    /// holds. Between two keyframes the value follows the *earlier* keyframe's declared
    /// interpolation, which is how every professional curve editor behaves.
    pub fn sample(&self, offset: i64) -> Result<ParamValue, CoreError> {
        let first = self
            .keys
            .first()
            .ok_or_else(|| CoreError::invalid("the animation has no keyframes"))?;
        if offset <= first.offset {
            return Ok(first.value.clone());
        }
        let last = self
            .keys
            .last()
            .ok_or_else(|| CoreError::invalid("the animation has no keyframes"))?;
        if offset >= last.offset {
            return Ok(last.value.clone());
        }

        // Binary search for the interval containing `offset`.
        let mut low = 0_usize;
        let mut high = self.keys.len() - 1;
        while high - low > 1 {
            let middle = low + (high - low) / 2;
            if self.keys[middle].offset <= offset {
                low = middle;
            } else {
                high = middle;
            }
        }

        let start = &self.keys[low];
        let end = &self.keys[high];
        match start.interpolation {
            Interpolation::Hold => Ok(start.value.clone()),
            Interpolation::Linear => {
                let span = end.offset - start.offset;
                let position = offset - start.offset;
                let t = Rational::new(position, span).map_err(CoreError::from)?;
                start.value.lerp(&end.value, t)
            }
        }
    }

    /// Validates ordering, uniqueness, and that a linear pair is interpolatable.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.keys.is_empty() {
            return Err(CoreError::invalid(
                "an animation needs at least one keyframe",
            ));
        }
        for pair in self.keys.windows(2) {
            if pair[0].offset >= pair[1].offset {
                return Err(CoreError::invalid(
                    "keyframes must be strictly ordered by offset",
                ));
            }
        }
        for pair in self.keys.windows(2) {
            if pair[0].interpolation == Interpolation::Linear {
                let interpolatable = matches!(
                    (&pair[0].value, &pair[1].value),
                    (ParamValue::Rational(_), ParamValue::Rational(_))
                        | (ParamValue::Integer(_), ParamValue::Integer(_))
                        | (ParamValue::Color(_), ParamValue::Color(_))
                );
                if !interpolatable {
                    return Err(CoreError::invalid(format!(
                        "a linear keyframe at {} interpolates a value that cannot be interpolated",
                        pair[0].offset
                    )));
                }
            }
        }
        Ok(())
    }
}

/// The geometry of a mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskShape {
    /// An axis-aligned rectangle before rotation.
    Rectangle,
    /// An ellipse inscribed in the rectangle before rotation.
    Ellipse,
}

/// A shape that limits where a layer contributes.
///
/// Geometry is **normalised to the source frame**, so a mask survives a change of
/// sequence resolution without being reinterpreted: a centre of `1/2,1/2` is the middle
/// of the frame and a width of `1/2` is half of its width.
///
/// Animated masks, tracked masks and roto shapes arrive with `maj-mask` in M10. This
/// model is deliberately static and exact so that M10 *extends* it rather than replaces
/// it — which is the whole point of putting it in the document now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mask {
    pub(crate) id: MaskId,
    pub(crate) shape: MaskShape,
    pub(crate) center_x: Rational,
    pub(crate) center_y: Rational,
    pub(crate) width: Rational,
    pub(crate) height: Rational,
    pub(crate) rotation: Rational,
    pub(crate) feather: Rational,
    pub(crate) opacity: Rational,
    pub(crate) inverted: bool,
}

impl Mask {
    /// Builds a mask that covers the whole frame with hard edges.
    pub fn new(id: MaskId, shape: MaskShape) -> Result<Self, CoreError> {
        Self::with_geometry(
            id,
            shape,
            Rational::from_reduced(1, 2),
            Rational::from_reduced(1, 2),
            Rational::from_integer(1),
            Rational::from_integer(1),
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(1),
            false,
        )
    }

    /// Builds a mask from explicit geometry, validating it.
    #[allow(clippy::too_many_arguments)]
    pub fn with_geometry(
        id: MaskId,
        shape: MaskShape,
        center_x: Rational,
        center_y: Rational,
        width: Rational,
        height: Rational,
        rotation: Rational,
        feather: Rational,
        opacity: Rational,
        inverted: bool,
    ) -> Result<Self, CoreError> {
        let mask = Self {
            id,
            shape,
            center_x,
            center_y,
            width,
            height,
            rotation,
            feather,
            opacity,
            inverted,
        };
        mask.validate()?;
        Ok(mask)
    }

    /// Identifier of this mask.
    #[must_use]
    pub const fn id(&self) -> MaskId {
        self.id
    }

    /// Whether the mask is a rectangle or an ellipse.
    #[must_use]
    pub const fn shape(&self) -> MaskShape {
        self.shape
    }

    /// Centre, normalised inside the source frame.
    #[must_use]
    pub const fn center(&self) -> (Rational, Rational) {
        (self.center_x, self.center_y)
    }

    /// Size, normalised against the source frame.
    #[must_use]
    pub const fn size(&self) -> (Rational, Rational) {
        (self.width, self.height)
    }

    /// Clockwise rotation in degrees.
    #[must_use]
    pub const fn rotation(&self) -> Rational {
        self.rotation
    }

    /// Width of the soft edge, normalised against the source frame.
    #[must_use]
    pub const fn feather(&self) -> Rational {
        self.feather
    }

    /// Strength of the mask, in `0..=1`.
    #[must_use]
    pub const fn opacity(&self) -> Rational {
        self.opacity
    }

    /// Reports whether the mask selects the outside rather than the inside.
    #[must_use]
    pub const fn is_inverted(&self) -> bool {
        self.inverted
    }

    /// Validates the geometry.
    pub fn validate(&self) -> Result<(), CoreError> {
        if !self.width.is_positive() || !self.height.is_positive() {
            return Err(CoreError::invalid("mask size must be greater than zero"));
        }
        if self.feather.is_negative() {
            return Err(CoreError::invalid("mask feather cannot be negative"));
        }
        if self.opacity.is_negative() || self.opacity > Rational::from_integer(1) {
            return Err(CoreError::invalid(
                "mask opacity must lie between zero and one",
            ));
        }
        Ok(())
    }
}

/// How a transition blends the outgoing and incoming clips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    /// Straight cross-dissolve weighted by progress.
    CrossDissolve,
    /// Fade the outgoing clip to black, then the incoming one in.
    DipToBlack,
    /// Fade the outgoing clip to white, then the incoming one in.
    DipToWhite,
    /// A hard edge travelling to the left.
    WipeLeft,
    /// A hard edge travelling to the right.
    WipeRight,
    /// A hard edge travelling upward.
    WipeUp,
    /// A hard edge travelling downward.
    WipeDown,
    /// A circle growing from the centre of the frame.
    Circle,
}

impl TransitionKind {
    /// Every transition kind, in a fixed order.
    pub const ALL: [Self; 8] = [
        Self::CrossDissolve,
        Self::DipToBlack,
        Self::DipToWhite,
        Self::WipeLeft,
        Self::WipeRight,
        Self::WipeUp,
        Self::WipeDown,
        Self::Circle,
    ];

    /// Short stable name.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::CrossDissolve => "cross_dissolve",
            Self::DipToBlack => "dip_to_black",
            Self::DipToWhite => "dip_to_white",
            Self::WipeLeft => "wipe_left",
            Self::WipeRight => "wipe_right",
            Self::WipeUp => "wipe_up",
            Self::WipeDown => "wipe_down",
            Self::Circle => "circle",
        }
    }

    /// Reports whether the transition needs a per-pixel geometric mask.
    ///
    /// The renderer uses this to decide whether it can take the cheap dissolve path,
    /// which is a measurable difference on the certified path at 8K.
    #[must_use]
    pub const fn is_geometric(self) -> bool {
        matches!(
            self,
            Self::WipeLeft | Self::WipeRight | Self::WipeUp | Self::WipeDown | Self::Circle
        )
    }
}

/// A transition on a track, blending an outgoing clip into an incoming one.
///
/// A transition occupies the half-open timeline interval `[at, at + duration)` and is
/// independent of the clips' own ranges: an edit may place a dissolve over a cut, over a
/// gap, or over an overlap, and the renderer resolves whichever clips actually cover the
/// frame. `from` and `to` name the intended partners so the association survives a move,
/// and are optional because a transition over a gap has none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub(crate) id: TransitionId,
    pub(crate) kind: TransitionKind,
    pub(crate) at: i64,
    pub(crate) duration: i64,
    pub(crate) from: Option<ClipId>,
    pub(crate) to: Option<ClipId>,
}

impl Transition {
    /// Builds a transition, refusing a non-positive duration.
    pub fn new(
        id: TransitionId,
        kind: TransitionKind,
        at: i64,
        duration: i64,
        from: Option<ClipId>,
        to: Option<ClipId>,
    ) -> Result<Self, CoreError> {
        if duration <= 0 {
            return Err(CoreError::invalid(
                "transition duration must be greater than zero",
            ));
        }
        Ok(Self {
            id,
            kind,
            at,
            duration,
            from,
            to,
        })
    }

    /// Identifier of this transition.
    #[must_use]
    pub const fn id(&self) -> TransitionId {
        self.id
    }

    /// Which blend this transition performs.
    #[must_use]
    pub const fn kind(&self) -> TransitionKind {
        self.kind
    }

    /// First timeline frame the transition covers.
    #[must_use]
    pub const fn at(&self) -> i64 {
        self.at
    }

    /// Length in timeline frames.
    #[must_use]
    pub const fn duration(&self) -> i64 {
        self.duration
    }

    /// The clip the transition comes from, when there is one.
    #[must_use]
    pub const fn from(&self) -> Option<ClipId> {
        self.from
    }

    /// The clip the transition goes to, when there is one.
    #[must_use]
    pub const fn to(&self) -> Option<ClipId> {
        self.to
    }

    /// Last timeline frame the transition covers, exclusive.
    #[must_use]
    pub const fn end(&self) -> i64 {
        self.at.saturating_add(self.duration)
    }

    /// Reports whether the transition covers a timeline frame.
    #[must_use]
    pub const fn covers(&self, timeline_frame: i64) -> bool {
        timeline_frame >= self.at && timeline_frame < self.end()
    }

    /// Progress through the transition.
    ///
    /// The first frame is `0/1` and the last is `(duration - 1)/duration`, so progress
    /// never reaches `1/1` while the transition is live and there is no discontinuity at
    /// the hand-off to the incoming clip.
    pub fn progress(&self, timeline_frame: i64) -> Result<Rational, CoreError> {
        if !self.covers(timeline_frame) {
            return Err(CoreError::invalid("frame is outside the transition"));
        }
        Rational::new(timeline_frame - self.at, self.duration).map_err(CoreError::from)
    }

    /// Validates the transition.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.duration <= 0 {
            return Err(CoreError::invalid(
                "transition duration must be greater than zero",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::EffectId;

    fn rat(num: i64, den: i64) -> Rational {
        Rational::new(num, den).unwrap()
    }

    fn integer(value: i64) -> ParamValue {
        ParamValue::Rational(Rational::from_integer(value))
    }

    #[test]
    fn the_identity_transform_is_the_identity_and_only_it() {
        let identity = Transform::identity();
        assert!(identity.is_identity());
        assert!(identity.is_translation_only());
        assert_eq!(identity.scale_x(), Rational::from_integer(1));
        assert_eq!(identity.anchor_x(), rat(1, 2));

        let moved = Transform::new(
            Rational::from_integer(10),
            Rational::from_integer(0),
            Rational::from_integer(1),
            Rational::from_integer(1),
            Rational::from_integer(0),
            rat(1, 2),
            rat(1, 2),
        )
        .unwrap();
        assert!(!moved.is_identity());
        assert!(
            moved.is_translation_only(),
            "a translation does not resample"
        );

        let rotated = Transform::new(
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(1),
            Rational::from_integer(1),
            Rational::from_integer(45),
            rat(1, 2),
            rat(1, 2),
        )
        .unwrap();
        assert!(!rotated.is_translation_only());
    }

    #[test]
    fn a_degenerate_scale_is_refused_rather_than_clamped() {
        let zero = Transform::new(
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(1),
            Rational::from_integer(0),
            rat(1, 2),
            rat(1, 2),
        );
        assert!(zero.is_err());

        let mirrored = Transform::new(
            Rational::from_integer(0),
            Rational::from_integer(0),
            Rational::from_integer(1),
            Rational::from_integer(-1),
            Rational::from_integer(0),
            rat(1, 2),
            rat(1, 2),
        );
        assert!(
            mirrored.is_err(),
            "a mirror must be requested explicitly, not inherited from a typo"
        );
    }

    #[test]
    fn interpolation_is_exact_at_the_midpoint() {
        let zero = integer(0);
        assert_eq!(zero.lerp(&integer(10), rat(1, 2)).unwrap(), integer(5));
        assert_eq!(
            zero.lerp(&integer(30), rat(1, 3)).unwrap(),
            integer(10),
            "one third of the way from 0 to 30 is exactly 10"
        );
        assert_eq!(
            zero.lerp(&ParamValue::Rational(rat(1, 3)), rat(1, 2))
                .unwrap(),
            ParamValue::Rational(rat(1, 6))
        );
    }

    #[test]
    fn integers_interpolate_to_exact_rationals() {
        let a = ParamValue::Integer(0);
        let b = ParamValue::Integer(3);
        assert_eq!(
            a.lerp(&b, rat(1, 3)).unwrap(),
            ParamValue::Rational(Rational::from_integer(1))
        );
        assert_eq!(a.as_integer(), Some(0));
        assert_eq!(ParamValue::Rational(rat(4, 2)).as_integer(), Some(2));
        assert_eq!(ParamValue::Rational(rat(1, 2)).as_integer(), None);
    }

    #[test]
    fn colours_interpolate_per_channel_with_defined_rounding() {
        let black = ParamValue::Color(Rgba::new(0, 0, 0));
        let white = ParamValue::Color(Rgba::new(10, 10, 10));
        assert_eq!(
            black.lerp(&white, rat(1, 2)).unwrap(),
            ParamValue::Color(Rgba::new(5, 5, 5))
        );
        // Half of three is 1.5, which rounds away from zero to 2.
        let three = ParamValue::Color(Rgba::new(3, 3, 3));
        assert_eq!(
            black.lerp(&three, rat(1, 2)).unwrap(),
            ParamValue::Color(Rgba::new(2, 2, 2))
        );
    }

    #[test]
    fn values_that_cannot_interpolate_return_themselves() {
        let text = ParamValue::Text("a".into());
        assert_eq!(
            text.lerp(&ParamValue::Text("b".into()), rat(1, 2)).unwrap(),
            text
        );
        assert!(text.as_rational().is_none(), "text is not numeric");
        assert!(ParamValue::Boolean(true).as_rational().is_none());
        assert_eq!(ParamValue::Boolean(false).kind(), "boolean");
    }

    #[test]
    fn every_effect_declares_a_consistent_schema_and_a_valid_default() {
        for kind in EffectKind::ALL {
            let spec = kind.params();
            assert!(!spec.is_empty(), "{} declares no parameters", kind.key());
            let mut keys: Vec<&str> = spec.iter().map(|s| s.key).collect();
            keys.sort_unstable();
            let mut unique = keys.clone();
            unique.dedup();
            assert_eq!(keys, unique, "{} repeats a parameter key", kind.key());

            EffectInstance::new(EffectId::new(1), kind)
                .validate()
                .unwrap_or_else(|err| panic!("{} default is invalid: {err}", kind.key()));

            for spec in spec {
                if let (Some(min), Some(max)) = (spec.min, spec.max) {
                    assert!(
                        min <= max,
                        "{}.{} has an inverted range",
                        kind.key(),
                        spec.key
                    );
                }
                kind.validate_param(spec.key, &spec.default)
                    .unwrap_or_else(|err| {
                        panic!("{}.{} default fails: {err}", kind.key(), spec.key)
                    });
            }
        }
    }

    #[test]
    fn a_parameter_outside_its_range_or_not_declared_is_refused() {
        let mut blur = EffectInstance::new(EffectId::new(1), EffectKind::Blur);
        assert!(blur.set_param("radius", integer(4)).is_ok());
        assert!(
            blur.set_param("radius", integer(300)).is_err(),
            "above the declared maximum"
        );
        assert!(
            blur.set_param("radius", integer(-1)).is_err(),
            "below the declared minimum"
        );
        assert!(
            blur.set_param("nonsense", ParamValue::Integer(1)).is_err(),
            "an undeclared key must be refused so the inverse is always a parameter change"
        );
    }
}

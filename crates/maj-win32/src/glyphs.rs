//! The icon set: every button's picture, as data.
//!
//! There is no image asset behind these and no symbol font. A symbol font is a dependency on
//! whatever the machine happens to have installed, and a bitmap would have to be decoded, which
//! this build cannot do. So each glyph is a small set of shapes in a square from (0,0) to (1,1),
//! drawn with the platform's own polygon calls at whatever size the button is. That also makes
//! the whole set testable: a glyph that is empty, or whose points fall outside its square, is a
//! failing test rather than a blank button nobody noticed.

use crate::win32::{self, Colour, Hdc, Rect};

/// One piece of a glyph, in a square from (0,0) to (1,1).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Shape {
    /// A filled polygon.
    Fill(&'static [(f32, f32)]),
    /// A line through these points, this many pixels wide.
    Stroke(&'static [(f32, f32)], i32),
    /// A disc, centred here with this radius.
    Disc(f32, f32, f32),
    /// A filled rounded rectangle, with this corner radius.
    Box(f32, f32, f32, f32, f32),
}

/// Every picture the interface can draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Glyph {
    /// Undo.
    Undo,
    /// Redo.
    Redo,
    /// Split at the playhead.
    Split,
    /// Remove.
    Remove,
    /// Duplicate.
    Duplicate,
    /// Trim the head.
    TrimIn,
    /// Trim the tail.
    TrimOut,
    /// Zoom in.
    ZoomIn,
    /// Zoom out.
    ZoomOut,
    /// Fit the sequence to the window.
    ZoomFit,
    /// Snapping.
    Snap,
    /// Linked selection.
    Link,
    /// A marker.
    Marker,
    /// A list.
    ListView,
    /// A grid.
    GridView,
    /// Play.
    Play,
    /// Pause.
    Pause,
    /// Go to the start.
    Start,
    /// Go to the end.
    End,
    /// The previous frame.
    PrevFrame,
    /// The next frame.
    NextFrame,
    /// Loop.
    Loop,
    /// A new thing.
    Plus,
    /// Close.
    Close,
    /// Minimise.
    Minimize,
    /// Maximise.
    Maximize,
    /// Restore.
    Restore,
    /// This is visible.
    Eye,
    /// This is not visible.
    EyeOff,
    /// This can be edited.
    Unlock,
    /// This is audible.
    Speaker,
    /// This is silent.
    Muted,
    /// The agent's own tools.
    Wand,
    /// A keyframe.
    Keyframe,
    /// A sequence.
    Film,
    /// An audio waveform.
    Wave,
    /// Yes.
    Check,
    /// Several channels.
    Sliders,
    /// Move down.
    ArrowDown,
}

/// The shapes of a glyph.
#[must_use]
pub(crate) fn shapes(glyph: Glyph) -> &'static [Shape] {
    match glyph {
        Glyph::Undo => &[
            Shape::Stroke(
                &[
                    (0.30, 0.68),
                    (0.30, 0.44),
                    (0.46, 0.26),
                    (0.72, 0.30),
                    (0.88, 0.52),
                    (0.78, 0.76),
                    (0.52, 0.82),
                ],
                2,
            ),
            Shape::Fill(&[(0.16, 0.56), (0.34, 0.40), (0.36, 0.66)]),
        ],
        Glyph::Redo => &[
            Shape::Stroke(
                &[
                    (0.70, 0.68),
                    (0.70, 0.44),
                    (0.54, 0.26),
                    (0.28, 0.30),
                    (0.12, 0.52),
                    (0.22, 0.76),
                    (0.48, 0.82),
                ],
                2,
            ),
            Shape::Fill(&[(0.84, 0.56), (0.66, 0.40), (0.64, 0.66)]),
        ],
        Glyph::Split => &[
            Shape::Stroke(&[(0.50, 0.16), (0.50, 0.84)], 1),
            Shape::Fill(&[(0.24, 0.32), (0.24, 0.68), (0.44, 0.50)]),
            Shape::Fill(&[(0.76, 0.32), (0.76, 0.68), (0.56, 0.50)]),
        ],
        Glyph::Remove => &[
            Shape::Box(0.24, 0.32, 0.76, 0.88, 0.04),
            Shape::Box(0.12, 0.20, 0.88, 0.29, 0.02),
            Shape::Stroke(&[(0.42, 0.12), (0.58, 0.12)], 2),
        ],
        Glyph::Duplicate => &[
            Shape::Stroke(
                &[
                    (0.14, 0.14),
                    (0.62, 0.14),
                    (0.62, 0.62),
                    (0.14, 0.62),
                    (0.14, 0.14),
                ],
                1,
            ),
            Shape::Stroke(&[(0.36, 0.86), (0.86, 0.86), (0.86, 0.36), (0.70, 0.36)], 1),
        ],
        Glyph::TrimIn => &[
            Shape::Stroke(&[(0.52, 0.18), (0.88, 0.18), (0.88, 0.82), (0.52, 0.82)], 1),
            Shape::Box(0.16, 0.28, 0.23, 0.72, 0.02),
            Shape::Fill(&[(0.34, 0.34), (0.34, 0.66), (0.50, 0.50)]),
        ],
        Glyph::TrimOut => &[
            Shape::Stroke(&[(0.12, 0.18), (0.48, 0.18), (0.48, 0.82), (0.12, 0.82)], 1),
            Shape::Box(0.77, 0.28, 0.84, 0.72, 0.02),
            Shape::Fill(&[(0.66, 0.34), (0.66, 0.66), (0.50, 0.50)]),
        ],
        Glyph::ZoomIn => &[
            Shape::Disc(0.42, 0.42, 0.28),
            Shape::Stroke(&[(0.62, 0.62), (0.90, 0.90)], 2),
            Shape::Stroke(&[(0.30, 0.42), (0.54, 0.42)], 1),
            Shape::Stroke(&[(0.42, 0.30), (0.42, 0.54)], 1),
        ],
        Glyph::ZoomOut => &[
            Shape::Disc(0.42, 0.42, 0.28),
            Shape::Stroke(&[(0.62, 0.62), (0.90, 0.90)], 2),
            Shape::Stroke(&[(0.30, 0.42), (0.54, 0.42)], 1),
        ],
        Glyph::ZoomFit => &[
            Shape::Stroke(&[(0.16, 0.40), (0.16, 0.16), (0.40, 0.16)], 2),
            Shape::Stroke(&[(0.60, 0.16), (0.84, 0.16), (0.84, 0.40)], 2),
            Shape::Stroke(&[(0.84, 0.60), (0.84, 0.84), (0.60, 0.84)], 2),
            Shape::Stroke(&[(0.40, 0.84), (0.16, 0.84), (0.16, 0.60)], 2),
        ],
        Glyph::Snap => &[Shape::Fill(&[
            (0.18, 0.14),
            (0.18, 0.62),
            (0.50, 0.86),
            (0.82, 0.62),
            (0.82, 0.14),
            (0.66, 0.14),
            (0.66, 0.56),
            (0.50, 0.68),
            (0.34, 0.56),
            (0.34, 0.14),
        ])],
        Glyph::Link => &[
            Shape::Box(0.08, 0.38, 0.53, 0.62, 0.11),
            Shape::Box(0.47, 0.38, 0.92, 0.62, 0.11),
        ],
        Glyph::Marker => &[Shape::Fill(&[
            (0.30, 0.12),
            (0.76, 0.12),
            (0.76, 0.84),
            (0.53, 0.62),
            (0.30, 0.84),
        ])],
        Glyph::ListView => &[
            Shape::Stroke(&[(0.20, 0.28), (0.86, 0.28)], 1),
            Shape::Stroke(&[(0.20, 0.50), (0.86, 0.50)], 1),
            Shape::Stroke(&[(0.20, 0.72), (0.86, 0.72)], 1),
            Shape::Disc(0.10, 0.28, 0.05),
            Shape::Disc(0.10, 0.50, 0.05),
            Shape::Disc(0.10, 0.72, 0.05),
        ],
        Glyph::GridView => &[
            Shape::Box(0.14, 0.14, 0.46, 0.46, 0.03),
            Shape::Box(0.54, 0.14, 0.86, 0.46, 0.03),
            Shape::Box(0.14, 0.54, 0.46, 0.86, 0.03),
            Shape::Box(0.54, 0.54, 0.86, 0.86, 0.03),
        ],
        Glyph::Play => &[Shape::Fill(&[(0.26, 0.16), (0.86, 0.50), (0.26, 0.84)])],
        Glyph::Pause => &[
            Shape::Box(0.26, 0.18, 0.44, 0.82, 0.02),
            Shape::Box(0.56, 0.18, 0.74, 0.82, 0.02),
        ],
        Glyph::Start => &[
            Shape::Fill(&[(0.52, 0.18), (0.94, 0.50), (0.52, 0.82)]),
            Shape::Box(0.06, 0.18, 0.18, 0.82, 0.02),
        ],
        Glyph::End => &[
            Shape::Fill(&[(0.48, 0.18), (0.06, 0.50), (0.48, 0.82)]),
            Shape::Box(0.82, 0.18, 0.94, 0.82, 0.02),
        ],
        Glyph::PrevFrame => &[Shape::Fill(&[(0.72, 0.18), (0.24, 0.50), (0.72, 0.82)])],
        Glyph::NextFrame => &[Shape::Fill(&[(0.28, 0.18), (0.76, 0.50), (0.28, 0.82)])],
        Glyph::Loop => &[
            Shape::Stroke(
                &[
                    (0.20, 0.60),
                    (0.20, 0.38),
                    (0.36, 0.22),
                    (0.64, 0.22),
                    (0.80, 0.38),
                    (0.80, 0.52),
                ],
                2,
            ),
            Shape::Fill(&[(0.62, 0.44), (0.80, 0.74), (0.92, 0.50)]),
            Shape::Stroke(&[(0.34, 0.78), (0.66, 0.78)], 2),
        ],
        Glyph::Plus => &[
            Shape::Stroke(&[(0.50, 0.18), (0.50, 0.82)], 2),
            Shape::Stroke(&[(0.18, 0.50), (0.82, 0.50)], 2),
        ],
        Glyph::Close => &[
            Shape::Stroke(&[(0.26, 0.26), (0.74, 0.74)], 2),
            Shape::Stroke(&[(0.74, 0.26), (0.26, 0.74)], 2),
        ],
        Glyph::Minimize => &[Shape::Stroke(&[(0.24, 0.58), (0.76, 0.58)], 2)],
        Glyph::Maximize => &[Shape::Stroke(
            &[
                (0.26, 0.26),
                (0.74, 0.26),
                (0.74, 0.74),
                (0.26, 0.74),
                (0.26, 0.26),
            ],
            2,
        )],
        Glyph::Restore => &[
            Shape::Stroke(&[(0.34, 0.22), (0.78, 0.22), (0.78, 0.66)], 2),
            Shape::Stroke(
                &[
                    (0.22, 0.36),
                    (0.66, 0.36),
                    (0.66, 0.80),
                    (0.22, 0.80),
                    (0.22, 0.36),
                ],
                2,
            ),
        ],
        Glyph::Eye => &[
            Shape::Stroke(
                &[
                    (0.08, 0.50),
                    (0.32, 0.26),
                    (0.68, 0.26),
                    (0.92, 0.50),
                    (0.68, 0.74),
                    (0.32, 0.74),
                    (0.08, 0.50),
                ],
                1,
            ),
            Shape::Disc(0.50, 0.50, 0.14),
        ],
        Glyph::EyeOff => &[
            Shape::Stroke(
                &[
                    (0.08, 0.50),
                    (0.32, 0.26),
                    (0.68, 0.26),
                    (0.92, 0.50),
                    (0.68, 0.74),
                    (0.32, 0.74),
                    (0.08, 0.50),
                ],
                1,
            ),
            Shape::Disc(0.50, 0.50, 0.14),
            Shape::Stroke(&[(0.14, 0.86), (0.86, 0.14)], 2),
        ],
        Glyph::Unlock => &[
            Shape::Fill(&[(0.28, 0.46), (0.72, 0.46), (0.72, 0.86), (0.28, 0.86)]),
            Shape::Stroke(
                &[
                    (0.62, 0.46),
                    (0.62, 0.30),
                    (0.74, 0.18),
                    (0.86, 0.30),
                    (0.86, 0.42),
                ],
                2,
            ),
        ],
        Glyph::Speaker => &[
            Shape::Fill(&[
                (0.10, 0.40),
                (0.30, 0.40),
                (0.52, 0.18),
                (0.52, 0.82),
                (0.30, 0.60),
                (0.10, 0.60),
            ]),
            Shape::Stroke(&[(0.60, 0.36), (0.74, 0.50), (0.60, 0.64)], 2),
            Shape::Stroke(&[(0.72, 0.24), (0.90, 0.50), (0.72, 0.76)], 2),
        ],
        Glyph::Muted => &[
            Shape::Fill(&[
                (0.10, 0.40),
                (0.30, 0.40),
                (0.52, 0.18),
                (0.52, 0.82),
                (0.30, 0.60),
                (0.10, 0.60),
            ]),
            Shape::Stroke(&[(0.62, 0.34), (0.90, 0.66)], 2),
            Shape::Stroke(&[(0.90, 0.34), (0.62, 0.66)], 2),
        ],
        Glyph::Wand => &[
            Shape::Stroke(&[(0.16, 0.84), (0.68, 0.32)], 3),
            Shape::Stroke(&[(0.62, 0.12), (0.62, 0.26)], 2),
            Shape::Stroke(&[(0.80, 0.22), (0.88, 0.32)], 2),
            Shape::Stroke(&[(0.78, 0.50), (0.92, 0.50)], 2),
        ],
        Glyph::Keyframe => &[Shape::Fill(&[
            (0.50, 0.18),
            (0.82, 0.50),
            (0.50, 0.82),
            (0.18, 0.50),
        ])],
        Glyph::Film => &[
            Shape::Box(0.10, 0.20, 0.90, 0.80, 0.03),
            Shape::Fill(&[(0.16, 0.24), (0.28, 0.24), (0.28, 0.38), (0.16, 0.38)]),
            Shape::Fill(&[(0.72, 0.24), (0.84, 0.24), (0.84, 0.38), (0.72, 0.38)]),
            Shape::Fill(&[(0.16, 0.62), (0.28, 0.62), (0.28, 0.76), (0.16, 0.76)]),
            Shape::Fill(&[(0.72, 0.62), (0.84, 0.62), (0.84, 0.76), (0.72, 0.76)]),
        ],
        Glyph::Wave => &[Shape::Stroke(
            &[
                (0.10, 0.70),
                (0.24, 0.34),
                (0.38, 0.66),
                (0.52, 0.30),
                (0.66, 0.62),
                (0.80, 0.44),
                (0.92, 0.56),
            ],
            2,
        )],
        Glyph::Check => &[Shape::Stroke(
            &[(0.18, 0.54), (0.42, 0.76), (0.82, 0.26)],
            2,
        )],
        Glyph::Sliders => &[
            Shape::Stroke(&[(0.14, 0.30), (0.86, 0.30)], 1),
            Shape::Stroke(&[(0.14, 0.50), (0.86, 0.50)], 1),
            Shape::Stroke(&[(0.14, 0.70), (0.86, 0.70)], 1),
            Shape::Disc(0.36, 0.30, 0.10),
            Shape::Disc(0.62, 0.50, 0.10),
            Shape::Disc(0.44, 0.70, 0.10),
        ],
        Glyph::ArrowDown => &[
            Shape::Fill(&[(0.50, 0.82), (0.16, 0.42), (0.84, 0.42)]),
            Shape::Box(0.40, 0.16, 0.60, 0.46, 0.02),
        ],
    }
}

/// Every glyph, in the order the set is listed, for the test that walks it.
#[cfg(test)]
pub(crate) const ALL: &[Glyph] = &[
    Glyph::Undo,
    Glyph::Redo,
    Glyph::Split,
    Glyph::Remove,
    Glyph::Duplicate,
    Glyph::TrimIn,
    Glyph::TrimOut,
    Glyph::ZoomIn,
    Glyph::ZoomOut,
    Glyph::ZoomFit,
    Glyph::Snap,
    Glyph::Link,
    Glyph::Marker,
    Glyph::ListView,
    Glyph::GridView,
    Glyph::Play,
    Glyph::Pause,
    Glyph::Start,
    Glyph::End,
    Glyph::PrevFrame,
    Glyph::NextFrame,
    Glyph::Loop,
    Glyph::Plus,
    Glyph::Close,
    Glyph::Minimize,
    Glyph::Maximize,
    Glyph::Restore,
    Glyph::Eye,
    Glyph::EyeOff,
    Glyph::Unlock,
    Glyph::Speaker,
    Glyph::Muted,
    Glyph::Wand,
    Glyph::Keyframe,
    Glyph::Film,
    Glyph::Wave,
    Glyph::Check,
    Glyph::Sliders,
    Glyph::ArrowDown,
];

/// The name of a glyph, which is what a failure names.
///
/// It exists for the test that walks [`ALL`]; the interface draws pictures rather than naming
/// them, so it is built only when the tests are.
#[cfg(test)]
#[must_use]
pub(crate) fn name(glyph: Glyph) -> &'static str {
    match glyph {
        Glyph::Undo => "undo",
        Glyph::Redo => "redo",
        Glyph::Split => "split",
        Glyph::Remove => "remove",
        Glyph::Duplicate => "duplicate",
        Glyph::TrimIn => "trim-in",
        Glyph::TrimOut => "trim-out",
        Glyph::ZoomIn => "zoom-in",
        Glyph::ZoomOut => "zoom-out",
        Glyph::ZoomFit => "zoom-fit",
        Glyph::Snap => "snap",
        Glyph::Link => "link",
        Glyph::Marker => "marker",
        Glyph::ListView => "list",
        Glyph::GridView => "grid",
        Glyph::Play => "play",
        Glyph::Pause => "pause",
        Glyph::Start => "start",
        Glyph::End => "end",
        Glyph::PrevFrame => "prev-frame",
        Glyph::NextFrame => "next-frame",
        Glyph::Loop => "loop",
        Glyph::Plus => "plus",
        Glyph::Close => "close",
        Glyph::Minimize => "minimize",
        Glyph::Maximize => "maximize",
        Glyph::Restore => "restore",
        Glyph::Eye => "eye",
        Glyph::EyeOff => "eye-off",
        Glyph::Unlock => "unlock",
        Glyph::Speaker => "speaker",
        Glyph::Muted => "muted",
        Glyph::Wand => "wand",
        Glyph::Keyframe => "keyframe",
        Glyph::Film => "film",
        Glyph::Wave => "wave",
        Glyph::Check => "check",
        Glyph::Sliders => "sliders",
        Glyph::ArrowDown => "arrow-down",
    }
}

/// Draws a glyph into a square, in one colour.
pub(crate) fn draw(hdc: Hdc, rect: Rect, glyph: Glyph, colour: Colour) {
    if rect.is_empty() {
        return;
    }
    // Half the smaller side keeps the picture square when the button is not.
    let side = rect.width().min(rect.height());
    let frame_box = Rect::new(
        rect.left + (rect.width() - side) / 2,
        rect.top + (rect.height() - side) / 2,
        rect.left + (rect.width() - side) / 2 + side,
        rect.top + (rect.height() - side) / 2 + side,
    );
    for shape in shapes(glyph) {
        match shape {
            Shape::Fill(points) => win32::polygon(hdc, frame_box, points, colour),
            Shape::Stroke(points, width) => win32::polyline(hdc, frame_box, points, colour, *width),
            Shape::Disc(x, y, radius) => win32::ellipse(
                hdc,
                frame_box,
                (x - radius, y - radius, x + radius, y + radius),
                colour,
            ),
            Shape::Box(left, top, right, bottom, radius) => {
                let area = Rect::new(
                    frame_box.left + (left * frame_box.width() as f32) as i32,
                    frame_box.top + (top * frame_box.height() as f32) as i32,
                    frame_box.left + (right * frame_box.width() as f32) as i32,
                    frame_box.top + (bottom * frame_box.height() as f32) as i32,
                );
                let corner = (radius * frame_box.width() as f32).round() as i32;
                win32::rounded(hdc, area, corner.max(1), colour, None);
            }
        }
    }
}

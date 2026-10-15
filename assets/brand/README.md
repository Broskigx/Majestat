# Brand

Five files: four of them one geometry, and the logo, which was supplied as a file rather
than drawn here. The marks are the crown and the sword, and the icon is the two of them on
the project's own ground. Every mark here is a flat vector with explicit coordinates, no
filters, no raster effects and no text: a mark that needs a font to be correct is a mark
that is wrong on the machine that does not have it.

| File | What it is | Where it is used |
|---|---|---|
| [`crown.svg`](crown.svg) | The crown alone, gold on nothing | The application mark, a suite icon, a document badge |
| [`sword.svg`](sword.svg) | The sword alone, steel on nothing | The render engine's mark: the thing that cuts |
| [`mark.svg`](mark.svg) | The emblem: the crown resting on an upright sword | Splash, about box, signing certificate, letterhead |
| [`icon.svg`](icon.svg) | The emblem on the ink field, inside a mask-safe margin | Application icon, taskbar, installer |
| [`logo.png`](logo.png) | The logo, supplied by the owner as an image rather than drawn here | The application's logo: window icon, about box, splash |
| [`check-page.sh`](check-page.sh) | Builds `target/preview/brand.html`: every mark large, the icon at 64, 32 and 16, and the logo on three grounds | Looking at the marks and the logo before shipping one |

## The logo

`logo.png` is the application's logo and it is **the owner's image, kept exactly as it was
supplied**. It is not an export of the vectors above and it is not redrawn from them: it is
a richer crown-and-sword than the marks draw, and where the logo appears, this file is what
appears.

| | |
|---|---|
| Size | 1024 by 559 |
| Format | PNG, eight bits per channel, RGB, **no alpha**, DEFLATE-compressed |
| Bytes | 113,753 |
| SHA-256 | `5c5e09b42343f20c07c3f007063aae5d4d18640d3ff31a3fd23ddc6557048e0b` |
| Provenance | Supplied by the owner; copied in byte for byte and checked with `cmp` |

Three consequences are recorded here because they are decisions, not accidents.

1. **The field.** The image is ink on white, and the application's ground is ink `#0B0E14`,
   so as supplied the white is a plate behind the mark. `check-page.sh` shows it three ways —
   as supplied on paper, as supplied on ink, and inverted on ink — because that choice is
   the owner's. Until it is made, the app uses the file **as supplied**, and the choice is
   recorded here when it is made.
2. **The small sizes.** A landscape image cannot be a taskbar icon without either a letterbox
   or a crop to the emblem's own bounds. The window icon crops to the emblem; the plate, if
   the plate is kept, does not survive sixteen pixels.
3. **The loaders.** The shell needs pixels, and this file is DEFLATE-compressed, so it is
   decoded **once, at build time**, into the raw form the shell hands to the operating
   system and to the UI texture. That keeps an image decoder out of the shipping binary and
   keeps this single file the only source of the artwork, which is the same rule the vectors
   follow: one source, derived at build time, never a second copy to drift.

The vectors stay. They are what a place that needs geometry gets — a favicon, a document
badge, a one-colour reduction, an installer that must stay legible at sixteen pixels.

## Palette

Six values, and no others. Each one has a job, which is the only reason a palette should
have a value in it at all.

| Name | Value | Job |
|---|---|---|
| Ink | `#0B0E14` | The ground. A near-black with a blue bias, so gold reads warm against it |
| Gold | `#E8B44A` | The crown, and every emphasis that is not an error |
| Gold, deep | `#B8871F` | The crown's shadow faces and the jewelled accents |
| Steel | `#9AA4B2` | The sword's blade |
| Steel, bright | `#E4E9F0` | The blade's ridge, and the only near-white in the set |
| Paper | `#F5F1E8` | The wordmark's ground where a light field is needed |

The pairing is deliberate: a charter's gold leaf on a dark seal, and cold steel for the
tool. Nothing in the set is a gradient, so every mark survives being reduced to one ink,
embossed, or rendered at sixteen pixels.

## Geometry

Both marks are drawn on a 512 by 512 grid, with the crown occupying `x 88..424` and the
sword `y 84..490`, so they share a baseline and can be composed without either changing.
The emblem puts the sword on the centre line and the crown where the blade meets the guard,
with the tip behind the crown: one device rather than a jumble of them, because a mark has
to read at sixteen pixels.

* **The crown** has five peaks at `x = 112, 184, 256, 328, 400`, with the centre peak the
  tallest, and a band from `y = 336` to `y = 384`. The peaks are symmetric about `x = 256`
  to the pixel, because an asymmetric crown looks like a mistake rather than a flourish.
* **The sword** points up. The tip is at `(256, 84)`, the blade is 56 wide where it meets
  the guard, the guard is a rounded bar 200 wide centred at `y = 356`, and the pommel is a
  circle of radius 22 at `(256, 468)`.
* **The icon** scales the emblem by 1.05 about its own centre on the ink field, which leaves
  at least a 96-unit margin on every side: an operating system that masks the icon to a
  circle or a squircle cuts the ground and never the mark.

The crown is gold and the sword is steel in every file that carries both, because the
colours are the marks: swapping them would make the sword a decorative object and the crown
a tool.

## What is deliberately absent

* **No wordmark.** Set type is the interface's job (M7), and shipping a path-based wordmark
  would freeze a typeface choice that has not been made.
* **No raster exports of the vectors.** A PNG export of a vector would be a second source
  of truth that drifts from it. When a build needs a bitmap it rasterises the vector at
  build time, so the geometry stays in one place. `logo.png` is the one raster here, and it
  is an input rather than an export: it has no vector above it to drift from.
* **No animation.** The marks do not move. Motion is the interface's, and it will be built
  from commands rather than from a GIF nobody can diff.

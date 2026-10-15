# Media validation corpus

The M2 gate is: **one hundred real media files, zero crashes**, with correct handling of
variable frame rate, interlacing, rotation, bit depth, chroma subsampling, damaged
containers and non-standard timebases.

That corpus is referenced here, not committed. Files run to hundreds of gigabytes and
carry their own distribution terms.

## What belongs here

`media/` (git-ignored) should hold files covering at least:

| Axis | Cases that must be present |
|---|---|
| Rate | 23.976, 24, 25, 29.97, 30, 50, 59.94, 60, and at least one variable-frame-rate file |
| Bit depth | 8-bit, 10-bit, 12-bit where obtainable |
| Chroma | 4:2:0, 4:2:2, 4:4:4 |
| Codec | H.264, HEVC, AV1, ProRes 422 and 4444, DNxHR, and at least one intra-frame camera format |
| Camera RAW | BRAW, R3D, and one stills RAW format |
| Range | Full and limited (video) range, tagged correctly and tagged wrongly |
| Colour | SDR Rec.709, HLG, PQ, and log footage from at least two manufacturers |
| Interlacing | Top-field-first and bottom-field-first |
| Rotation | 90, 180 and 270 degree display matrices |
| Audio | Mono, stereo, 5.1, 48 kHz, 44.1 kHz, and a file whose audio and video rates do not divide evenly |
| Pathology | Truncated file, zero-length file, wrong extension, missing stream, extremely long duration |

## Provenance

Prefer synthetic files generated from a script over downloads: they are reproducible, they
carry no third-party terms, and their expected decode output is known exactly. Record the
generation command next to the file. For the cases synthesis cannot reach — camera RAW,
genuinely damaged media — record the source and its licence in a manifest beside the file.

## Expectations

Bugs found in the corpus become permanent regression cases. A file that once crashed the
decoder stays in the corpus even after the fix, marked with the issue it reproduces. The
corpus grows; it does not curate itself down to the convenient cases.

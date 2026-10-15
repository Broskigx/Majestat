# The FFmpeg build, pinned

The product licence is **`Apache-2.0 OR MIT`** (ADR 0014), and a permissive binary cannot
absorb copyleft. FFmpeg is therefore admissible on exactly one set of terms — an LGPL
build, dynamically linked, behind an abstraction this workspace owns (ADR 0005) — and this
document is where that build stops being an intention and becomes a configuration. It is
the artefact `docs/LICENSING.md` lists under "known obligations to resolve before any
release" and `.planning/STATE.md` lists as the second next action.

Nothing in the workspace links FFmpeg yet. This is written **before** the first line of
decoder code rather than reconstructed after it, because a build configuration discovered
at release time is a build configuration that is already wrong.

## 1. The pinned version

| Field | Value |
|---|---|
| Release | **FFmpeg 9.0.2 "Lei"** |
| Release date | 2026-09-18 |
| Git tag | `n9.0.2` |
| Upstream | `https://git.ffmpeg.org/gitweb/ffmpeg.git` / `https://github.com/FFmpeg/FFmpeg` |
| Base licence | LGPL-2.1-or-later |
| Local patches | **none** — see §6 |

The 9.0 branch is the current stable major branch, which is where upstream's security
fixes will keep landing for the life of M2 through M8; 8.1.3 "Hoare" (2026-09-21) is the
newest tag overall but on an older branch, and it is the fallback if a 9.0 regression
blocks M2 rather than a parallel option. **One version is linked at a time.** A version
bump is a reviewed change to this file, with the version, the configuration and the
hashes updated together, because they are one fact in three parts.

A version is not pinned by a number alone. Before anything links it, the exact artefacts
are recorded in `third-party/ffmpeg/MANIFEST.toml`:

```toml
# Produced by the build in §3, filled from the artefacts themselves. Not written by hand.
version        = "9.0.2"
tag            = "n9.0.2"
source_sha256  = "..."   # of the release tarball or the archived checkout
configure      = "..."   # the exact line from §3, as one string
libavcodec     = { file = "avcodec-62.dll", sha256 = "..." }
libavformat    = { file = "avformat-62.dll", sha256 = "..." }
libavutil      = { file = "avutil-60.dll",  sha256 = "..." }
```

The soname numbers above are placeholders: they are whatever 9.0.2 actually produces, read
off the artefacts rather than predicted. The hashes are produced by the build, not typed:

```bash
sha256sum ffmpeg-9.0.2.tar.xz bin/*.dll
```

`maj-media` refuses to load a library whose hash is not in the manifest. That is what makes
"pinned" a property of the running program instead of a sentence in a document.

## 2. What may not be enabled, and why

Three flags are refusals rather than preferences:

* **`--enable-gpl` — never.** It makes the whole build GPL-2.0-or-later, which cannot be
  linked into an `Apache-2.0 OR MIT` binary. There is no exception path short of
  relicensing the product. This excludes `libx264`, `libx265`, `libxvid`, `libvidstab`,
  `frei0r` and the GPL filters, and it is why M8's H.264/HEVC encoder has to come from the
  platform (Media Foundation) or from a permissive library (OpenH264, BSD-2).
* **`--enable-nonfree` — never.** It produces a build that is **not redistributable at
  all**, which is worse than GPL rather than milder: GPL at least has terms. This excludes
  `libfdk-aac`.
* **`--enable-version3` — not enabled.** It upgrades affected components to LGPL-3.0, and
  while LGPL-3.0 is compatible with Apache-2.0, it is a second set of terms travelling with
  the product for no capability M2 needs. Keeping it off means one licence to reason about.
  Turning it on is a separate licence analysis, not a configure-line edit.

`libpostproc` is GPL unconditionally and is disabled explicitly rather than left to
default, so that the fact is visible in the configure line a reviewer reads.

**The most important flag in §3 is `--disable-autodetect`.** Without it, `configure`
silently links whatever it finds on the build machine. A developer who happens to have
`libx264` installed would produce a GPL build from this same configure line and nothing
would say so. With autodetect off, every external library is an explicit `--enable-*` and
the configure line is a complete statement of what went in.

## 3. The configuration

```sh
./configure \
  --prefix=/opt/majestat-ffmpeg \
  --disable-autodetect \
  --enable-shared \
  --disable-static \
  --disable-programs \
  --disable-doc \
  --disable-postproc \
  --disable-swscale \
  --disable-swresample \
  --disable-avfilter \
  --disable-devices \
  --disable-encoders \
  --disable-muxers \
  --disable-bsfs \
  --disable-network \
  --disable-iconv \
  --disable-sdl2 \
  --disable-xlib \
  --disable-zlib \
  --disable-bzlib \
  --disable-lzma \
  --enable-protocol=file \
  --enable-pic \
  --enable-asm \
  --enable-d3d11va \
  --enable-dxva2
```

Every flag is one of three kinds, and the distinction matters because only the first kind
is non-negotiable:

**Licence-mandatory.** `--disable-autodetect`, `--enable-shared --disable-static`,
`--disable-postproc`, and the absence of `--enable-gpl`, `--enable-nonfree` and
`--enable-version3`. Shared linking is not a packaging preference: it is the mechanism by
which LGPL-2.1 §6(b) is satisfied and by which a recipient's relinking right stays
exercisable.

**Determinism-motivated** (ADR 0006). `--disable-swscale` and `--disable-swresample`.
Pixel-format conversion and audio resampling are arithmetic on the certified path, and the
certified path's arithmetic belongs in this workspace where its reduction order and
rounding are the project's own and are version-stable. swscale's output is reproducible for
a given build and *not* across builds or dispatch paths, which is precisely the property
Tier 2 cannot have. Majestat converts decoded planes itself; see `docs/MEDIA.md` §5.

**Surface-minimising.** The rest. `--disable-encoders --disable-muxers` because encoding is
M8's and will not come from here; `--disable-avfilter` because the render graph is
`maj-graph`'s; `--disable-network --enable-protocol=file` because a media library that can
open a URL on its own is an attack surface an editor has no use for;
`--disable-zlib/bzlib/lzma/iconv` because they are decompressors and text converters reached
by container metadata, and a smaller linked set is a shorter audit. These are reversible
engineering choices: adding one back is a review, not a licence question.

`--enable-d3d11va --enable-dxva2` are built into FFmpeg on Windows, need no external
library and carry no extra terms. They are what M2's hardware decode path is
capability-tested against — and under ADR 0006 hardware decode is Tier 1 only, never
certified, because the output depends on a driver (`docs/MEDIA.md` §6).

NVENC and NVDEC, when M8 wants them, come through `nv-codec-headers` (MIT) and need neither
`--enable-gpl` nor `--enable-nonfree`. The historical requirement for `--enable-nonfree`
applied to combinations with `libnpp`, which is not wanted. This is recorded here so M8
does not re-derive it.

## 4. The gate

A configuration is a claim, and this project does not declare a claim met without a
measurement. Three checks, all cheap, all mechanical:

1. **The library says it is LGPL.** `avutil_license()` must return exactly
   `"LGPL version 2.1 or later"`. A GPL build returns a string naming the GPL, so this one
   call distinguishes them at runtime, in the shipped product, on the user's machine.
2. **The configuration contains no refusal.** `avutil_configuration()` must contain neither
   `--enable-gpl` nor `--enable-nonfree` nor `--enable-version3`. The string is the actual
   configure line of the actual loaded library, not the one in this file.
3. **The artefact is the pinned one.** The SHA-256 of each loaded library matches
   `third-party/ffmpeg/MANIFEST.toml`, and the version matches §1.

`maj-media` runs 1 and 2 before it decodes anything and refuses to initialise if either
fails; CI runs all three. A build that cannot prove it is the LGPL build is treated as a
build that is not, which is the only safe direction for the error to point.

This also closes a hole a document cannot: a developer who rebuilds FFmpeg locally with
different flags gets a refusal rather than a product that quietly carries the wrong terms.

## 5. The obligations, concretely

LGPL-2.1 is satisfied by doing specific things, not by intending to. Each obligation below
has a place in the tree, so compliance is reviewable as a diff.

| Obligation | LGPL-2.1 | How it is met |
|---|---|---|
| Say the library is used and is LGPL-covered | §6 | A notice in the application's About page and in `NOTICE` at the repository root, naming FFmpeg, its version and its licence |
| Ship the licence text | §6 | `third-party/ffmpeg/COPYING.LGPLv2.1`, verbatim, alongside the binaries in the installer |
| Supply the corresponding source | §4, §6(b) | The exact `n9.0.2` source tarball whose hash is in the manifest, shipped in the installer or offered in writing with a URL valid for three years, and archived in project storage so the offer survives upstream |
| Keep relinking possible | §6(b) | Dynamic linking, with the DLLs as separate files a recipient can replace; no static linking and no bundling that defeats substitution |
| Do not forbid reverse engineering for debugging modifications | §6 | The EULA must carry an explicit carve-out. **This is the one item here that is not an engineering task** — it is a licence-text change, and it is the owner's |
| Publish our patches | §2 | None exist (§6). If any appear they live in `third-party/ffmpeg/patches/` as a numbered series and ship with the source |

The notice is required to be *prominent*, so it is in the About page rather than only in a
text file in an install directory. A build with no visible notice is a non-compliant build
however correct its configure line was.

## 6. Patching: don't, and if you must

**The pinned build carries no local patches**, and that is a deliberate position rather
than a current state. A patch is not forbidden — LGPL-2.1 §2 permits modification — but it
costs three things at once: our patches become part of the corresponding source we have to
publish, the pin stops naming an upstream artefact a recipient can verify independently,
and every version bump becomes a rebase. So the order of preference is: fix it above the
abstraction in `maj-media`, or report it upstream and pin the release that carries the fix,
and only then patch.

If a patch becomes necessary, the series is in `third-party/ffmpeg/patches/`, applied in
order, recorded in the manifest, and shipped with the source. A patch that exists and is
not published is the one way this project could be in breach while believing it is not.

## 7. What this does not settle

* **Codec patents.** H.264 and HEVC distribution and decoding are patent questions, not
  copyright ones, and a permissive product licence does nothing about them. The LGPL build
  above decodes both; whether shipping that decoder needs a pool licence depends on
  jurisdiction and volume and is the owner's to resolve before release. AV1 is the
  royalty-free path and is the default preference where quality allows.
* **The encoder.** M8's, and `x264`/`x265` are excluded by §2. See `docs/LICENSING.md`.
* **The copyright holder's legal name.** Still "Majestat", still a project name and not an
  entity, and still needed in `NOTICE` and the written offer before any release.
* **Camera RAW.** Blackmagic RAW and RED R3D come from vendor SDKs behind feature flags,
  not from here (ADR 0005).

## 8. Where the build is produced

The build host is Windows with a MinGW-w64 toolchain (`.planning/STATE.md`, P0.4), so
FFmpeg is configured under MSYS2 or a MinGW shell and produces GNU-ABI DLLs matching the
`x86_64-pc-windows-gnu` target the workspace builds for. The import libraries that
`maj-media` links against are generated by `dlltool`, which the host has.

An MSVC-ABI build is also possible and is not needed: the DLL boundary is C, and the
project's own target is the GNU one. If the workspace ever moves to `-msvc` as its default
target, this section is what has to be revisited — not the configure line, which is
ABI-independent.

Producing this build is a prerequisite for M2's decoder and is not yet done. What is done
is that its configuration is no longer an open question.

## Sources

The version and release dates above were checked against upstream:

* [Download FFmpeg](https://www.ffmpeg.org/download.html) — the release list and dates
* [FFmpeg/FFmpeg releases](https://github.com/FFmpeg/FFmpeg) — the tags

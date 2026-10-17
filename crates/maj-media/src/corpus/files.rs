//! The files themselves: what each one is, how it is written and what it must probe as.
//!
//! Three builders cover four containers — ISO base media, Matroska, RIFF and the two stills —
//! and the pathologies are made by damaging a file that was valid a moment earlier, which is
//! the only honest way to write a damaged file: it differs from a readable one in exactly the
//! way the name says and in no other.

use maj_time::{Rational, Timebase};

use crate::container::ContainerKind;
use crate::picture::{BitDepth, PlaneLayout, SampleRange};
use crate::stream::{FieldOrder, Rotation};

use super::mux::{
    BitWriter, Writer, box_of, ebml, ebml_float, ebml_string, ebml_uint, filler, full_box,
    riff_chunk, riff_list,
};
use super::{AudioExpectation, Axis, CorpusFile, Expectation, RateExpectation, VideoExpectation};

/// The command that regenerates one file by name.
fn command(name: &str) -> String {
    format!("cargo run -p maj-media --example make-corpus -- --only {name}")
}

/// A readable file's expectation, with a picture and no sound.
fn picture_only(container: ContainerKind, streams: usize, video: VideoExpectation) -> Expectation {
    Expectation::Readable {
        container,
        streams,
        video: Some(video),
        audio: None,
    }
}

/// What a corpus file declares about itself, before its bytes are written.
struct Declaration {
    name: &'static str,
    purpose: &'static str,
    axes: &'static [Axis],
    bytes: Vec<u8>,
    expect: Expectation,
}

impl Declaration {
    fn into_file(self) -> CorpusFile {
        CorpusFile {
            name: self.name,
            purpose: self.purpose,
            axes: self.axes,
            command: command(self.name),
            bytes: self.bytes,
            expect: self.expect,
        }
    }
}

// ---------------------------------------------------------------------------------------
// ISO base media
// ---------------------------------------------------------------------------------------

/// What a picture track of a synthetic ISO base media file states.
struct VideoSpec {
    code: [u8; 4],
    config: Option<([u8; 4], Vec<u8>)>,
    size: (u16, u16),
    timescale: u32,
    /// Sample count and duration, in file order, exactly as `stts` is written.
    runs: Vec<(u32, u32)>,
    rotation: Rotation,
    /// `fiel`: how many fields a sample holds, and the detail that orders them.
    fiel: Option<(u8, u8)>,
    colr: Option<ColourSpec>,
    /// `pasp`: horizontal and vertical spacing.
    pasp: Option<(u32, u32)>,
}

impl VideoSpec {
    /// Eight-bit 4:2:0 H.264, upright, progressive, square pixels.
    fn h264(size: (u16, u16), timescale: u32, runs: Vec<(u32, u32)>) -> Self {
        Self {
            code: *b"avc1",
            config: Some((*b"avcC", avcc(100, 1, 0))),
            size,
            timescale,
            runs,
            rotation: Rotation::None,
            fiel: None,
            colr: None,
            pasp: None,
        }
    }

    fn with_rotation(mut self, rotation: Rotation) -> Self {
        self.rotation = rotation;
        self
    }

    fn with_fiel(mut self, fields: u8, detail: u8) -> Self {
        self.fiel = Some((fields, detail));
        self
    }

    fn with_colour(mut self, colour: ColourSpec) -> Self {
        self.colr = Some(colour);
        self
    }

    fn with_pasp(mut self, horizontal: u32, vertical: u32) -> Self {
        self.pasp = Some((horizontal, vertical));
        self
    }

    fn frames(&self) -> u64 {
        self.runs
            .iter()
            .map(|(count, _)| u64::from(*count))
            .sum::<u64>()
    }
}

/// A `colr` box: the declared primaries, transfer and matrix, and whether the range is full.
struct ColourSpec {
    kind: [u8; 4],
    primaries: u16,
    transfer: u16,
    matrix: u16,
    full: bool,
}

impl ColourSpec {
    /// Rec.709 throughout, in the `nclx` spelling that can also state a range.
    const fn rec709(full: bool) -> Self {
        Self {
            kind: *b"nclx",
            primaries: 1,
            transfer: 1,
            matrix: 1,
            full,
        }
    }
}

/// What a sound track of a synthetic ISO base media file states.
struct AudioSpec {
    code: [u8; 4],
    channels: u16,
    sample_bits: u16,
    sample_rate: u32,
    samples: u32,
}

/// One track of a synthetic ISO base media file.
enum TrackSpec {
    Video(VideoSpec),
    Audio(AudioSpec),
    /// A timecode track: present, counted, not read.
    Timecode,
}

/// An `avcC` configuration record around one sequence parameter set.
fn avcc(profile: u8, chroma: u32, depth_minus_eight: u32) -> Vec<u8> {
    let mut bits = BitWriter::new();
    bits.u(8, u32::from(profile)).u(8, 0).u(8, 40).ue(0);
    if matches!(profile, 100 | 110 | 122 | 244) {
        bits.ue(chroma);
        if chroma == 3 {
            bits.u(1, 0);
        }
        bits.ue(depth_minus_eight)
            .ue(depth_minus_eight)
            .u(1, 0)
            .u(1, 0);
    }
    bits.ue(4);
    let mut parameter_set = vec![0x67_u8];
    parameter_set.extend_from_slice(&bits.finish());

    let mut writer = Writer::new();
    writer
        .u8(1)
        .u8(profile)
        .u8(0)
        .u8(40)
        .u8(0xFF)
        .u8(0xE1)
        .be16(parameter_set.len() as u16)
        .raw(&parameter_set)
        .u8(0);
    writer.finish()
}

/// An `hvcC` configuration record, which states chroma and depth outright.
fn hvcc(chroma: u8, depth_minus_eight: u8) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.u8(1).u8(0x01).be32(0).zeros(6).u8(120).be16(0).u8(0);
    writer
        .u8(0xFC | (chroma & 0x03))
        .u8(0xF8 | (depth_minus_eight & 0x07))
        .u8(0xF8 | (depth_minus_eight & 0x07))
        .be16(0)
        .u8(0);
    writer.finish()
}

/// An `av1C` configuration record.
fn av1c(
    high_bitdepth: bool,
    twelve_bit: bool,
    monochrome: bool,
    subsampling: (bool, bool),
) -> Vec<u8> {
    let mut flags = 0_u8;
    if high_bitdepth {
        flags |= 0x40;
    }
    if twelve_bit {
        flags |= 0x20;
    }
    if monochrome {
        flags |= 0x10;
    }
    if subsampling.0 {
        flags |= 0x08;
    }
    if subsampling.1 {
        flags |= 0x04;
    }
    let mut writer = Writer::new();
    writer.u8(0x81).u8(0x00).u8(flags).u8(0x00);
    writer.finish()
}

/// A 3×3 display matrix for a quarter-turn count, in the fixed-point form `tkhd` uses.
fn display_matrix(rotation: Rotation, size: (u16, u16)) -> Vec<u8> {
    const ONE: i32 = 1 << 16;
    const UNITY: i32 = 1 << 30;
    let (width, height) = (i32::from(size.0) << 16, i32::from(size.1) << 16);
    let (a, b, c, d, x, y) = match rotation {
        Rotation::None => (ONE, 0, 0, ONE, 0, 0),
        Rotation::Quarter => (0, ONE, -ONE, 0, height, 0),
        Rotation::Half => (-ONE, 0, 0, -ONE, width, height),
        Rotation::ThreeQuarters => (0, -ONE, ONE, 0, 0, width),
    };
    let mut writer = Writer::new();
    writer
        .be32i(a)
        .be32i(b)
        .be32(0)
        .be32i(c)
        .be32i(d)
        .be32(0)
        .be32i(x)
        .be32i(y)
        .be32i(UNITY);
    writer.finish()
}

/// A `hdlr` box for a handler type.
fn hdlr(handler: &[u8; 4], name: &str) -> Vec<u8> {
    let mut writer = Writer::new();
    writer
        .be32(0)
        .raw(handler)
        .zeros(12)
        .raw(name.as_bytes())
        .u8(0);
    full_box(b"hdlr", 0, 0, &writer.finish())
}

/// A `tkhd` box.
fn tkhd(track_id: u32, duration: u32, rotation: Rotation, size: (u16, u16)) -> Vec<u8> {
    let mut writer = Writer::new();
    writer
        .be32(0)
        .be32(0)
        .be32(track_id)
        .be32(0)
        .be32(duration)
        .zeros(8)
        .be16(0)
        .be16(0)
        .be16(0)
        .be16(0)
        .raw(&display_matrix(rotation, size))
        .be32(u32::from(size.0) << 16)
        .be32(u32::from(size.1) << 16);
    full_box(b"tkhd", 0, 3, &writer.finish())
}

/// An `mdhd` box.
fn mdhd(timescale: u32, duration: u32) -> Vec<u8> {
    let mut writer = Writer::new();
    writer
        .be32(0)
        .be32(0)
        .be32(timescale)
        .be32(duration)
        .be16(0x55C4)
        .be16(0);
    full_box(b"mdhd", 0, 0, &writer.finish())
}

/// An `stts` box from a run-length table of sample durations.
fn stts(runs: &[(u32, u32)]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.be32(runs.len() as u32);
    for (count, duration) in runs {
        writer.be32(*count).be32(*duration);
    }
    full_box(b"stts", 0, 0, &writer.finish())
}

/// The three boxes that place samples in the file, written for realism rather than for the
/// probe: nothing here reads a sample, but a file whose tables disagree is not a corpus file.
fn placement(samples: u32, sample_size: u32, offset: u32) -> Vec<u8> {
    let mut chunks = Writer::new();
    chunks.be32(1).be32(1).be32(samples.max(1)).be32(1);
    let mut sizes = Writer::new();
    sizes.be32(sample_size).be32(samples);
    let mut offsets = Writer::new();
    offsets.be32(1).be32(offset);
    let mut out = full_box(b"stsc", 0, 0, &chunks.finish());
    out.extend_from_slice(&full_box(b"stsz", 0, 0, &sizes.finish()));
    out.extend_from_slice(&full_box(b"stco", 0, 0, &offsets.finish()));
    out
}

/// A picture sample description, with the extension boxes the probe reads.
fn video_stsd(spec: &VideoSpec) -> Vec<u8> {
    let mut extensions = Writer::new();
    if let Some((kind, config)) = &spec.config {
        extensions.raw(&box_of(kind, config));
    }
    if let Some(colour) = &spec.colr {
        let mut body = Writer::new();
        body.raw(&colour.kind)
            .be16(colour.primaries)
            .be16(colour.transfer)
            .be16(colour.matrix);
        if colour.kind == *b"nclx" {
            body.u8(if colour.full { 0x80 } else { 0x00 });
        }
        extensions.raw(&box_of(b"colr", &body.finish()));
    }
    if let Some((horizontal, vertical)) = spec.pasp {
        let mut body = Writer::new();
        body.be32(horizontal).be32(vertical);
        extensions.raw(&box_of(b"pasp", &body.finish()));
    }
    if let Some((fields, detail)) = spec.fiel {
        let mut body = Writer::new();
        body.u8(fields).u8(detail);
        extensions.raw(&box_of(b"fiel", &body.finish()));
    }
    let extensions = extensions.finish();

    let mut entry = Writer::new();
    entry
        .be32((86 + extensions.len()) as u32)
        .raw(&spec.code)
        .zeros(6)
        .be16(1)
        .be16(0)
        .be16(0)
        .zeros(12)
        .be16(spec.size.0)
        .be16(spec.size.1)
        .be32(0x0048_0000)
        .be32(0x0048_0000)
        .be32(0)
        .be16(1)
        .zeros(32)
        .be16(24)
        .be16(0xFFFF)
        .raw(&extensions);

    let mut body = Writer::new();
    body.be32(1).raw(&entry.finish());
    full_box(b"stsd", 0, 0, &body.finish())
}

/// A sound sample description, in the version-zero layout.
fn audio_stsd(spec: &AudioSpec) -> Vec<u8> {
    let mut entry = Writer::new();
    entry
        .be32(36)
        .raw(&spec.code)
        .zeros(6)
        .be16(1)
        .be16(0)
        .be16(0)
        .be32(0)
        .be16(spec.channels)
        .be16(spec.sample_bits)
        .be16(0)
        .be16(0)
        .be32(spec.sample_rate << 16);
    let mut body = Writer::new();
    body.be32(1).raw(&entry.finish());
    full_box(b"stsd", 0, 0, &body.finish())
}

/// A whole ISO base media file: a brand, a movie box of tracks, and filler sample data.
fn iso_file(brand: &[u8; 4], tracks: &[TrackSpec]) -> Vec<u8> {
    let mut ftyp_body = Writer::new();
    ftyp_body.raw(brand).be32(512).raw(brand).raw(b"isom");
    let ftyp = box_of(b"ftyp", &ftyp_body.finish());

    let mut traks = Vec::new();
    for (index, track) in tracks.iter().enumerate() {
        let id = (index + 1) as u32;
        let trak = match track {
            TrackSpec::Video(spec) => {
                let duration: u32 = spec
                    .runs
                    .iter()
                    .map(|(count, delta)| count.saturating_mul(*delta))
                    .sum();
                let mut stbl = video_stsd(spec);
                stbl.extend_from_slice(&stts(&spec.runs));
                stbl.extend_from_slice(&placement(spec.frames() as u32, 1_024, 2_048));
                let mut minf = full_box(b"vmhd", 0, 1, &[0, 0, 0, 0, 0, 0, 0, 0]);
                minf.extend_from_slice(&box_of(b"stbl", &stbl));
                let mut mdia = mdhd(spec.timescale, duration);
                mdia.extend_from_slice(&hdlr(b"vide", "Majestat synthetic picture"));
                mdia.extend_from_slice(&box_of(b"minf", &minf));
                let mut trak = tkhd(id, duration, spec.rotation, spec.size);
                trak.extend_from_slice(&box_of(b"mdia", &mdia));
                box_of(b"trak", &trak)
            }
            TrackSpec::Audio(spec) => {
                let mut stbl = audio_stsd(spec);
                stbl.extend_from_slice(&stts(&[(spec.samples, 1)]));
                stbl.extend_from_slice(&placement(spec.samples, 4, 2_048));
                let mut minf = full_box(b"smhd", 0, 0, &[0, 0, 0, 0]);
                minf.extend_from_slice(&box_of(b"stbl", &stbl));
                let mut mdia = mdhd(spec.sample_rate, spec.samples);
                mdia.extend_from_slice(&hdlr(b"soun", "Majestat synthetic sound"));
                mdia.extend_from_slice(&box_of(b"minf", &minf));
                let mut trak = tkhd(id, spec.samples, Rotation::None, (0, 0));
                trak.extend_from_slice(&box_of(b"mdia", &mdia));
                box_of(b"trak", &trak)
            }
            TrackSpec::Timecode => {
                let mut entry = Writer::new();
                entry
                    .be32(34)
                    .raw(b"tmcd")
                    .zeros(6)
                    .be16(1)
                    .be32(0)
                    .be32(1)
                    .be32(25)
                    .u8(1)
                    .zeros(3)
                    .be32(0);
                let mut description = Writer::new();
                description.be32(1).raw(&entry.finish());
                let mut stbl = full_box(b"stsd", 0, 0, &description.finish());
                stbl.extend_from_slice(&stts(&[(1, 1)]));
                stbl.extend_from_slice(&placement(1, 4, 2_048));
                let mut minf = box_of(b"gmhd", &[0; 8]);
                minf.extend_from_slice(&box_of(b"stbl", &stbl));
                let mut mdia = mdhd(25, 1);
                mdia.extend_from_slice(&hdlr(b"tmcd", "Majestat synthetic timecode"));
                mdia.extend_from_slice(&box_of(b"minf", &minf));
                let mut trak = tkhd(id, 1, Rotation::None, (0, 0));
                trak.extend_from_slice(&box_of(b"mdia", &mdia));
                box_of(b"trak", &trak)
            }
        };
        traks.push(trak);
    }

    let mut mvhd_body = Writer::new();
    mvhd_body
        .be32(0)
        .be32(0)
        .be32(1_000)
        .be32(1_000)
        .be32(0x0001_0000)
        .be16(0x0100)
        .zeros(10)
        .raw(&display_matrix(Rotation::None, (0, 0)))
        .zeros(24)
        .be32((tracks.len() + 1) as u32);
    let mut moov = full_box(b"mvhd", 0, 0, &mvhd_body.finish());
    for trak in &traks {
        moov.extend_from_slice(trak);
    }

    let mut out = Writer::new();
    out.raw(&ftyp)
        .raw(&box_of(b"moov", &moov))
        .raw(&box_of(b"mdat", &filler(1_024)));
    out.finish()
}

// ---------------------------------------------------------------------------------------
// Matroska
// ---------------------------------------------------------------------------------------

/// A Matroska video track's declarations.
struct MatroskaVideo {
    codec: &'static str,
    size: (u32, u32),
    /// Nanoseconds per frame, where the file states one.
    default_duration: Option<u64>,
    interlaced: u64,
    field_order: u64,
    /// Chroma subsampling, horizontal and vertical, as Matroska counts it.
    chroma: (u64, u64),
    bits: u64,
    /// 1 is limited, 2 is full.
    range: u64,
}

/// A whole Matroska file: an EBML header, a segment, its information and its tracks.
fn matroska_file(video: &MatroskaVideo, audio: Option<(u32, u16, u64)>) -> Vec<u8> {
    let mut header = Vec::new();
    header.extend_from_slice(&ebml_uint(&[0x42, 0x86], 1));
    header.extend_from_slice(&ebml_uint(&[0x42, 0xF7], 1));
    header.extend_from_slice(&ebml_uint(&[0x42, 0xF2], 4));
    header.extend_from_slice(&ebml_uint(&[0x42, 0xF3], 8));
    header.extend_from_slice(&ebml_string(&[0x42, 0x82], "matroska"));
    header.extend_from_slice(&ebml_uint(&[0x42, 0x87], 4));
    header.extend_from_slice(&ebml_uint(&[0x42, 0x85], 2));
    let header = ebml(&[0x1A, 0x45, 0xDF, 0xA3], &header);

    let mut info = Vec::new();
    info.extend_from_slice(&ebml_uint(&[0x2A, 0xD7, 0xB1], 1_000_000));
    info.extend_from_slice(&ebml_string(&[0x4D, 0x80], "maj-media corpus"));
    let info = ebml(&[0x15, 0x49, 0xA9, 0x66], &info);

    let mut colour = Vec::new();
    colour.extend_from_slice(&ebml_uint(&[0x55, 0xB1], 1));
    colour.extend_from_slice(&ebml_uint(&[0x55, 0xB2], video.bits));
    colour.extend_from_slice(&ebml_uint(&[0x55, 0xB3], video.chroma.0));
    colour.extend_from_slice(&ebml_uint(&[0x55, 0xB4], video.chroma.1));
    colour.extend_from_slice(&ebml_uint(&[0x55, 0xB9], video.range));
    colour.extend_from_slice(&ebml_uint(&[0x55, 0xBA], 1));
    colour.extend_from_slice(&ebml_uint(&[0x55, 0xBB], 1));

    let mut settings = Vec::new();
    settings.extend_from_slice(&ebml_uint(&[0xB0], u64::from(video.size.0)));
    settings.extend_from_slice(&ebml_uint(&[0xBA], u64::from(video.size.1)));
    settings.extend_from_slice(&ebml_uint(&[0x9A], video.interlaced));
    settings.extend_from_slice(&ebml_uint(&[0x9D], video.field_order));
    settings.extend_from_slice(&ebml(&[0x55, 0xB0], &colour));

    let mut entry = Vec::new();
    entry.extend_from_slice(&ebml_uint(&[0xD7], 1));
    entry.extend_from_slice(&ebml_uint(&[0x73, 0xC5], 1));
    entry.extend_from_slice(&ebml_uint(&[0x83], 1));
    entry.extend_from_slice(&ebml_string(&[0x86], video.codec));
    if let Some(duration) = video.default_duration {
        entry.extend_from_slice(&ebml_uint(&[0x23, 0xE3, 0x83], duration));
    }
    entry.extend_from_slice(&ebml(&[0xE0], &settings));
    let mut tracks = ebml(&[0xAE], &entry);

    if let Some((rate, channels, bits)) = audio {
        let mut settings = Vec::new();
        settings.extend_from_slice(&ebml_float(&[0xB5], f64::from(rate)));
        settings.extend_from_slice(&ebml_uint(&[0x9F], u64::from(channels)));
        settings.extend_from_slice(&ebml_uint(&[0x62, 0x64], bits));
        let mut entry = Vec::new();
        entry.extend_from_slice(&ebml_uint(&[0xD7], 2));
        entry.extend_from_slice(&ebml_uint(&[0x73, 0xC5], 2));
        entry.extend_from_slice(&ebml_uint(&[0x83], 2));
        entry.extend_from_slice(&ebml_string(&[0x86], "A_PCM/INT/LIT"));
        entry.extend_from_slice(&ebml(&[0xE1], &settings));
        tracks.extend_from_slice(&ebml(&[0xAE], &entry));
    }
    let tracks = ebml(&[0x16, 0x54, 0xAE, 0x6B], &tracks);

    let mut cluster = Vec::new();
    cluster.extend_from_slice(&ebml_uint(&[0xE7], 0));
    let mut block = vec![0x81, 0x00, 0x00, 0x80];
    block.extend_from_slice(&filler(64));
    cluster.extend_from_slice(&ebml(&[0xA3], &block));
    let cluster = ebml(&[0x1F, 0x43, 0xB6, 0x75], &cluster);

    let mut segment = Vec::new();
    segment.extend_from_slice(&info);
    segment.extend_from_slice(&tracks);
    segment.extend_from_slice(&cluster);

    let mut out = header;
    out.extend_from_slice(&ebml(&[0x18, 0x53, 0x80, 0x67], &segment));
    out
}

// ---------------------------------------------------------------------------------------
// RIFF
// ---------------------------------------------------------------------------------------

/// A whole AVI file: a header list, one picture stream and filler movie data.
fn avi_file(
    size: (u32, u32),
    scale: u32,
    rate: u32,
    frames: u32,
    compression: &[u8; 4],
) -> Vec<u8> {
    let mut avih = Writer::new();
    avih.le32(1_000_000 / rate.max(1) * scale.max(1))
        .le32(0)
        .le32(0)
        .le32(0x10)
        .le32(frames)
        .le32(0)
        .le32(1)
        .le32(1_024)
        .le32(size.0)
        .le32(size.1)
        .zeros(16);

    let mut strh = Writer::new();
    strh.raw(b"vids")
        .raw(compression)
        .le32(0)
        .le16(0)
        .le16(0)
        .le32(0)
        .le32(scale)
        .le32(rate)
        .le32(0)
        .le32(frames)
        .le32(1_024)
        .le32(0)
        .le32(0)
        .zeros(8);

    let mut strf = Writer::new();
    strf.le32(40)
        .le32(size.0)
        .le32(size.1)
        .le16(1)
        .le16(24)
        .raw(compression)
        .le32(size.0 * size.1 * 3)
        .le32(0)
        .le32(0)
        .le32(0)
        .le32(0);

    let mut streams = riff_chunk(b"strh", &strh.finish());
    streams.extend_from_slice(&riff_chunk(b"strf", &strf.finish()));

    let mut header = riff_chunk(b"avih", &avih.finish());
    header.extend_from_slice(&riff_list(b"strl", &streams));

    let mut body = Writer::new();
    body.raw(b"AVI ")
        .raw(&riff_list(b"hdrl", &header))
        .raw(&riff_list(b"movi", &riff_chunk(b"00dc", &filler(1_024))));
    let body = body.finish();

    let mut out = Writer::new();
    out.raw(b"RIFF").le32(body.len() as u32).raw(&body);
    out.finish()
}

/// A whole WAVE file.
fn wav_file(channels: u16, sample_rate: u32, bits: u16, frames: u32) -> Vec<u8> {
    let block = u32::from(channels) * u32::from(bits) / 8;
    let mut fmt = Writer::new();
    fmt.le16(1)
        .le16(channels)
        .le32(sample_rate)
        .le32(sample_rate * block)
        .le16(block as u16)
        .le16(bits);

    let mut body = Writer::new();
    body.raw(b"WAVE")
        .raw(&riff_chunk(b"fmt ", &fmt.finish()))
        .raw(&riff_chunk(b"data", &filler((frames * block) as usize)));
    let body = body.finish();

    let mut out = Writer::new();
    out.raw(b"RIFF").le32(body.len() as u32).raw(&body);
    out.finish()
}

// ---------------------------------------------------------------------------------------
// Stills
// ---------------------------------------------------------------------------------------

/// CRC-32 as PNG defines it.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Adler-32, which is the checksum a zlib stream ends with.
fn adler32(bytes: &[u8]) -> u32 {
    let mut low = 1_u32;
    let mut high = 0_u32;
    for byte in bytes {
        low = (low + u32::from(*byte)) % 65_521;
        high = (high + low) % 65_521;
    }
    (high << 16) | low
}

/// A zlib stream of stored deflate blocks: no compression, and no compressor needed.
fn stored_zlib(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut remaining = raw;
    loop {
        let take = remaining.len().min(65_535);
        let (block, rest) = remaining.split_at(take);
        let last = rest.is_empty();
        out.push(u8::from(last));
        out.extend_from_slice(&(block.len() as u16).to_le_bytes());
        out.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        out.extend_from_slice(block);
        if last {
            break;
        }
        remaining = rest;
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// A PNG chunk: a length, a kind, a body and a checksum over both.
fn png_chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut covered = kind.to_vec();
    covered.extend_from_slice(body);
    let mut out = (body.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&covered);
    out.extend_from_slice(&crc32(&covered).to_be_bytes());
    out
}

/// A whole PNG still, with real filtered scanlines, so the renderer's own reader can read it.
fn png_file(width: u32, height: u32, depth: u8, colour: u8) -> Vec<u8> {
    let channels = match colour {
        0 => 1,
        2 => 3,
        4 => 2,
        _ => 4,
    };
    let bytes_per_pixel = channels * usize::from(depth) / 8;
    let row = 1 + width as usize * bytes_per_pixel;
    let mut raw = Vec::with_capacity(row * height as usize);
    for line in 0..height as usize {
        // Filter type zero: the row is stored as it is, which is what makes this writable
        // without a compressor and readable without a guess.
        raw.push(0);
        for column in 0..width as usize * bytes_per_pixel {
            raw.push(((line * 7 + column * 3) % 251) as u8);
        }
    }

    let mut header = Writer::new();
    header
        .be32(width)
        .be32(height)
        .u8(depth)
        .u8(colour)
        .u8(0)
        .u8(0)
        .u8(0);

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    out.extend_from_slice(&png_chunk(b"IHDR", &header.finish()));
    out.extend_from_slice(&png_chunk(b"IDAT", &stored_zlib(&raw)));
    out.extend_from_slice(&png_chunk(b"IEND", &[]));
    out
}

/// A whole binary Netpbm still.
fn ppm_file(width: u32, height: u32, maximum: u32) -> Vec<u8> {
    let wide = maximum > 255;
    let mut out = format!("P6\n# maj-media corpus\n{width} {height}\n{maximum}\n").into_bytes();
    let samples = width as usize * height as usize * 3 * if wide { 2 } else { 1 };
    out.extend_from_slice(&filler(samples));
    out
}

// ---------------------------------------------------------------------------------------
// The corpus
// ---------------------------------------------------------------------------------------

/// A picture expectation for a constant-rate, upright, progressive stream.
#[allow(clippy::too_many_arguments)]
fn video_expectation(
    coded_size: (u32, u32),
    display_size: (u32, u32),
    rotation: Rotation,
    field_order: FieldOrder,
    rate: RateExpectation,
    layout: PlaneLayout,
    depth: BitDepth,
    range: SampleRange,
    sample_aspect: Rational,
    frames: Option<u64>,
) -> VideoExpectation {
    VideoExpectation {
        coded_size,
        display_size,
        rotation,
        field_order,
        rate,
        layout,
        depth,
        range,
        sample_aspect,
        frames,
    }
}

/// Every file of the corpus.
pub(super) fn all() -> Vec<CorpusFile> {
    let square = Rational::from_integer(1);
    // Declared one file at a time, in groups, with each file's bytes and its expected
    // reading next to each other. One literal of forty entries would read and diff worse.
    let mut corpus: Vec<Declaration> = Vec::with_capacity(48);

    // --- Rates, depths, chroma, colour -------------------------------------------------
    corpus.push(Declaration {
        name: "cfr-23976-h264-8bit-420-709-limited.mp4",
        purpose: "the commonest delivery shape: 23.976, eight-bit 4:2:0, Rec.709, limited range",
        axes: &[
            Axis::Rate,
            Axis::BitDepth,
            Axis::Chroma,
            Axis::Colour,
            Axis::Range,
            Axis::Container,
        ],
        bytes: iso_file(
            b"isom",
            &[TrackSpec::Video(
                VideoSpec::h264((1_920, 1_080), 24_000, vec![(48, 1_001)])
                    .with_colour(ColourSpec::rec709(false)),
            )],
        ),
        expect: picture_only(
            ContainerKind::Mp4,
            1,
            video_expectation(
                (1_920, 1_080),
                (1_920, 1_080),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Constant(Timebase::FPS_23_976),
                PlaneLayout::Yuv420,
                BitDepth::Eight,
                SampleRange::Limited,
                square,
                Some(48),
            ),
        ),
    });

    corpus.push(Declaration {
        name: "cfr-24-prores422-10bit-422.mov",
        purpose: "QuickTime, and a codec that states its own chroma and depth in its four-character code",
        axes: &[Axis::Rate, Axis::BitDepth, Axis::Chroma, Axis::Container],
        bytes: iso_file(
            b"qt  ",
            &[TrackSpec::Video(VideoSpec {
                code: *b"apcn",
                config: None,
                size: (1_920, 1_080),
                timescale: 24,
                runs: vec![(24, 1)],
                rotation: Rotation::None,
                fiel: None,
                colr: None,
                pasp: None,
            })],
        ),
        expect: picture_only(
            ContainerKind::QuickTime,
            1,
            video_expectation(
                (1_920, 1_080),
                (1_920, 1_080),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Constant(Timebase::FPS_24),
                PlaneLayout::Yuv422,
                BitDepth::Ten,
                SampleRange::Unspecified,
                square,
                Some(24),
            ),
        ),
    });

    corpus.push(Declaration {
        name: "cfr-25-hevc-10bit-422-full-range.mp4",
        purpose: "ten-bit 4:2:2 HEVC tagged full range, which is the pair most often got wrong",
        axes: &[
            Axis::Rate,
            Axis::BitDepth,
            Axis::Chroma,
            Axis::Range,
            Axis::Colour,
        ],
        bytes: iso_file(
            b"isom",
            &[TrackSpec::Video(VideoSpec {
                code: *b"hvc1",
                config: Some((*b"hvcC", hvcc(2, 2))),
                size: (3_840, 2_160),
                timescale: 25,
                runs: vec![(25, 1)],
                rotation: Rotation::None,
                fiel: None,
                colr: Some(ColourSpec::rec709(true)),
                pasp: None,
            })],
        ),
        expect: picture_only(
            ContainerKind::Mp4,
            1,
            video_expectation(
                (3_840, 2_160),
                (3_840, 2_160),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Constant(Timebase::FPS_25),
                PlaneLayout::Yuv422,
                BitDepth::Ten,
                SampleRange::Full,
                square,
                Some(25),
            ),
        ),
    });

    corpus.push(Declaration {
        name: "cfr-30-av1-12bit-444.mp4",
        purpose: "twelve-bit 4:4:4, stated in an AV1 configuration record",
        axes: &[Axis::Rate, Axis::BitDepth, Axis::Chroma],
        bytes: iso_file(
            b"isom",
            &[TrackSpec::Video(VideoSpec {
                code: *b"av01",
                config: Some((*b"av1C", av1c(true, true, false, (false, false)))),
                size: (1_280, 720),
                timescale: 30,
                runs: vec![(30, 1)],
                rotation: Rotation::None,
                fiel: None,
                colr: None,
                pasp: None,
            })],
        ),
        expect: picture_only(
            ContainerKind::Mp4,
            1,
            video_expectation(
                (1_280, 720),
                (1_280, 720),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Constant(Timebase::FPS_30),
                PlaneLayout::Yuv444,
                BitDepth::Twelve,
                SampleRange::Unspecified,
                square,
                Some(30),
            ),
        ),
    });

    corpus.push(Declaration {
        name: "cfr-50-h264-pal-anamorphic-16-15.mp4",
        purpose: "a non-square sample aspect, applied exactly once and rounded in one direction",
        axes: &[Axis::Rate, Axis::SampleAspect],
        bytes: iso_file(
            b"isom",
            &[TrackSpec::Video(
                VideoSpec::h264((720, 576), 50, vec![(50, 1)]).with_pasp(16, 15),
            )],
        ),
        expect: picture_only(
            ContainerKind::Mp4,
            1,
            video_expectation(
                (720, 576),
                (768, 576),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Constant(Timebase::FPS_50),
                PlaneLayout::Yuv420,
                BitDepth::Eight,
                SampleRange::Unspecified,
                Rational::from_reduced(16, 15),
                Some(50),
            ),
        ),
    });

    corpus.push(Declaration {
        name: "cfr-5994-h264-8bit-420.mp4",
        purpose: "59.94, the other rate whose denominator is 1001",
        axes: &[Axis::Rate],
        bytes: iso_file(
            b"isom",
            &[TrackSpec::Video(VideoSpec::h264(
                (1_280, 720),
                60_000,
                vec![(60, 1_001)],
            ))],
        ),
        expect: picture_only(
            ContainerKind::Mp4,
            1,
            video_expectation(
                (1_280, 720),
                (1_280, 720),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Constant(Timebase::FPS_59_94),
                PlaneLayout::Yuv420,
                BitDepth::Eight,
                SampleRange::Unspecified,
                square,
                Some(60),
            ),
        ),
    });

    corpus.push(Declaration {
        name: "cfr-60-h264-8bit-420.mp4",
        purpose: "an integer sixty, so the 1001 cases are not the only ones tested",
        axes: &[Axis::Rate],
        bytes: iso_file(
            b"isom",
            &[TrackSpec::Video(VideoSpec::h264(
                (1_920, 1_080),
                60,
                vec![(60, 1)],
            ))],
        ),
        expect: picture_only(
            ContainerKind::Mp4,
            1,
            video_expectation(
                (1_920, 1_080),
                (1_920, 1_080),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Constant(Timebase::FPS_60),
                PlaneLayout::Yuv420,
                BitDepth::Eight,
                SampleRange::Unspecified,
                square,
                Some(60),
            ),
        ),
    });

    // --- Variable rate -----------------------------------------------------------------
    corpus.push(Declaration {
        name: "vfr-h264-three-durations.mp4",
        purpose: "a sample table with three distinct durations: a variable rate, with the exact times",
        axes: &[Axis::VariableRate],
        bytes: iso_file(
            b"isom",
            &[TrackSpec::Video(VideoSpec::h264(
                (1_920, 1_080),
                24_000,
                vec![(2, 1_001), (2, 2_002), (1, 1_001)],
            ))],
        ),
        expect: picture_only(
            ContainerKind::Mp4,
            1,
            video_expectation(
                (1_920, 1_080),
                (1_920, 1_080),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Variable(vec![0, 1_001, 2_002, 4_004, 6_006]),
                PlaneLayout::Yuv420,
                BitDepth::Eight,
                SampleRange::Unspecified,
                square,
                Some(5),
            ),
        ),
    });

    // --- Rotation ----------------------------------------------------------------------
    for (name, rotation, display) in [
        (
            "rotate-90-h264.mp4",
            Rotation::Quarter,
            (1_080_u32, 1_920_u32),
        ),
        ("rotate-180-h264.mp4", Rotation::Half, (1_920, 1_080)),
        (
            "rotate-270-h264.mp4",
            Rotation::ThreeQuarters,
            (1_080, 1_920),
        ),
    ] {
        corpus.push(Declaration {
            name,
            purpose: "a display matrix that turns the picture by a right angle",
            axes: &[Axis::Rotation],
            bytes: iso_file(
                b"isom",
                &[TrackSpec::Video(
                    VideoSpec::h264((1_920, 1_080), 25, vec![(25, 1)]).with_rotation(rotation),
                )],
            ),
            expect: picture_only(
                ContainerKind::Mp4,
                1,
                video_expectation(
                    (1_920, 1_080),
                    display,
                    rotation,
                    FieldOrder::Progressive,
                    RateExpectation::Constant(Timebase::FPS_25),
                    PlaneLayout::Yuv420,
                    BitDepth::Eight,
                    SampleRange::Unspecified,
                    square,
                    Some(25),
                ),
            ),
        });
    }

    // --- Interlacing -------------------------------------------------------------------
    for (name, purpose, detail, order, timescale, runs, rate) in [
        (
            "interlaced-tff-25.mov",
            "two interleaved fields per sample, the top one first",
            1_u8,
            FieldOrder::TopFirst,
            25_u32,
            vec![(25_u32, 1_u32)],
            RateExpectation::Constant(Timebase::FPS_25),
        ),
        (
            "interlaced-bff-2997.mov",
            "two interleaved fields per sample, the bottom one first",
            6,
            FieldOrder::BottomFirst,
            30_000,
            vec![(30, 1_001)],
            RateExpectation::Constant(Timebase::FPS_29_97),
        ),
        (
            "interlaced-single-top-field-50.mov",
            "one top field per sample, which is a field rate and not a frame rate",
            9,
            FieldOrder::SingleTop,
            50,
            vec![(50, 1)],
            RateExpectation::Constant(Timebase::FPS_50),
        ),
    ] {
        let frames = runs.iter().map(|(count, _)| u64::from(*count)).sum();
        corpus.push(Declaration {
            name,
            purpose,
            axes: &[Axis::Interlacing, Axis::Rate],
            bytes: iso_file(
                b"qt  ",
                &[TrackSpec::Video(
                    VideoSpec::h264((720, 576), timescale, runs).with_fiel(2, detail),
                )],
            ),
            expect: picture_only(
                ContainerKind::QuickTime,
                1,
                video_expectation(
                    (720, 576),
                    (720, 576),
                    Rotation::None,
                    order,
                    rate,
                    PlaneLayout::Yuv420,
                    BitDepth::Eight,
                    SampleRange::Unspecified,
                    square,
                    Some(frames),
                ),
            ),
        });
    }

    // --- Several streams, and streams that are only counted ----------------------------
    corpus.push(Declaration {
        name: "video-audio-rates-that-do-not-divide.mp4",
        purpose: "29.97 picture beside 44.1 kHz sound: two timebases with no common divisor",
        axes: &[Axis::Rate, Axis::Audio],
        bytes: iso_file(
            b"isom",
            &[
                TrackSpec::Video(VideoSpec::h264((1_920, 1_080), 30_000, vec![(30, 1_001)])),
                TrackSpec::Audio(AudioSpec {
                    code: *b"mp4a",
                    channels: 2,
                    sample_bits: 16,
                    sample_rate: 44_100,
                    samples: 44_100,
                }),
            ],
        ),
        expect: Expectation::Readable {
            container: ContainerKind::Mp4,
            streams: 2,
            video: Some(video_expectation(
                (1_920, 1_080),
                (1_920, 1_080),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Constant(Timebase::FPS_29_97),
                PlaneLayout::Yuv420,
                BitDepth::Eight,
                SampleRange::Unspecified,
                square,
                Some(30),
            )),
            audio: Some(AudioExpectation {
                channels: 2,
                sample_rate: 44_100,
                sample_bits: Some(16),
            }),
        },
    });

    corpus.push(Declaration {
        name: "video-with-timecode-track.mp4",
        purpose: "a timecode track: present, counted, and deliberately not read",
        axes: &[Axis::OtherStream],
        bytes: iso_file(
            b"isom",
            &[
                TrackSpec::Video(VideoSpec::h264((1_920, 1_080), 25, vec![(25, 1)])),
                TrackSpec::Timecode,
            ],
        ),
        expect: picture_only(
            ContainerKind::Mp4,
            2,
            video_expectation(
                (1_920, 1_080),
                (1_920, 1_080),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Constant(Timebase::FPS_25),
                PlaneLayout::Yuv420,
                BitDepth::Eight,
                SampleRange::Unspecified,
                square,
                Some(25),
            ),
        ),
    });

    corpus.push(Declaration {
        name: "long-duration-nine-million-frames.mp4",
        purpose: "a run-length table describing four days of film, in one entry and a few bytes",
        axes: &[Axis::LongDuration],
        bytes: iso_file(
            b"isom",
            &[TrackSpec::Video(VideoSpec::h264(
                (1_920, 1_080),
                24_000,
                vec![(9_000_000, 1_001)],
            ))],
        ),
        expect: picture_only(
            ContainerKind::Mp4,
            1,
            video_expectation(
                (1_920, 1_080),
                (1_920, 1_080),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Constant(Timebase::FPS_23_976),
                PlaneLayout::Yuv420,
                BitDepth::Eight,
                SampleRange::Unspecified,
                square,
                Some(9_000_000),
            ),
        ),
    });

    corpus.push(Declaration {
        name: "no-picture-stream-sound-only.mp4",
        purpose: "an ISO base media file with no picture in it, which is a missing stream and not a failure",
        axes: &[Axis::Pathology, Axis::Audio],
        bytes: iso_file(
            b"isom",
            &[TrackSpec::Audio(AudioSpec {
                code: *b"mp4a",
                channels: 2,
                sample_bits: 16,
                sample_rate: 48_000,
                samples: 48_000,
            })],
        ),
        expect: Expectation::Readable {
            container: ContainerKind::Mp4,
            streams: 1,
            video: None,
            audio: Some(AudioExpectation {
                channels: 2,
                sample_rate: 48_000,
                sample_bits: Some(16),
            }),
        },
    });

    // --- Matroska ----------------------------------------------------------------------
    corpus.push(Declaration {
        name: "mkv-cfr-25-10bit-420-limited-with-surround.mkv",
        purpose: "Matroska: a rate from a frame duration in nanoseconds, and 5.1 sound at 48 kHz",
        axes: &[
            Axis::Container,
            Axis::Rate,
            Axis::BitDepth,
            Axis::Chroma,
            Axis::Range,
            Axis::Colour,
            Axis::Audio,
        ],
        bytes: matroska_file(
            &MatroskaVideo {
                codec: "V_MPEG4/ISO/AVC",
                size: (1_920, 1_080),
                default_duration: Some(40_000_000),
                interlaced: 2,
                field_order: 0,
                chroma: (1, 1),
                bits: 10,
                range: 1,
            },
            Some((48_000, 6, 24)),
        ),
        expect: Expectation::Readable {
            container: ContainerKind::Matroska,
            streams: 2,
            video: Some(video_expectation(
                (1_920, 1_080),
                (1_920, 1_080),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Constant(Timebase::FPS_25),
                PlaneLayout::Yuv420,
                BitDepth::Ten,
                SampleRange::Limited,
                square,
                None,
            )),
            audio: Some(AudioExpectation {
                channels: 6,
                sample_rate: 48_000,
                sample_bits: Some(24),
            }),
        },
    });

    corpus.push(Declaration {
        name: "mkv-23976-rounded-to-the-nanosecond.mkv",
        purpose: "41708333 nanoseconds is 24000/1001 rounded: the exact rate has to come back out",
        axes: &[Axis::Rate, Axis::Container],
        bytes: matroska_file(
            &MatroskaVideo {
                codec: "V_MPEGH/ISO/HEVC",
                size: (3_840, 2_160),
                default_duration: Some(41_708_333),
                interlaced: 2,
                field_order: 0,
                chroma: (1, 0),
                bits: 10,
                range: 2,
            },
            None,
        ),
        expect: picture_only(
            ContainerKind::Matroska,
            1,
            video_expectation(
                (3_840, 2_160),
                (3_840, 2_160),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Constant(Timebase::FPS_23_976),
                PlaneLayout::Yuv422,
                BitDepth::Ten,
                SampleRange::Full,
                square,
                None,
            ),
        ),
    });

    corpus.push(Declaration {
        name: "mkv-vfr-no-default-duration-interlaced.mkv",
        purpose: "no frame duration at all, and interlaced with the order undetermined: both facts kept",
        axes: &[Axis::VariableRate, Axis::Interlacing],
        bytes: matroska_file(
            &MatroskaVideo {
                codec: "V_MPEG4/ISO/AVC",
                size: (720, 576),
                default_duration: None,
                interlaced: 1,
                field_order: 2,
                chroma: (0, 0),
                bits: 8,
                range: 0,
            },
            None,
        ),
        expect: picture_only(
            ContainerKind::Matroska,
            1,
            video_expectation(
                (720, 576),
                (720, 576),
                Rotation::None,
                FieldOrder::TopFirst,
                RateExpectation::VariableUnread,
                PlaneLayout::Yuv444,
                BitDepth::Eight,
                SampleRange::Unspecified,
                square,
                None,
            ),
        ),
    });

    // --- RIFF --------------------------------------------------------------------------
    corpus.push(Declaration {
        name: "avi-cfr-2997-mjpeg.avi",
        purpose: "AVI, whose rate is a ratio of two integers and whose field order it never states",
        axes: &[Axis::Container, Axis::Rate],
        bytes: avi_file((640, 480), 1_001, 30_000, 60, b"MJPG"),
        expect: picture_only(
            ContainerKind::Avi,
            1,
            video_expectation(
                (640, 480),
                (640, 480),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Constant(Timebase::FPS_29_97),
                PlaneLayout::Yuv420,
                BitDepth::Eight,
                SampleRange::Unspecified,
                square,
                Some(60),
            ),
        ),
    });

    corpus.push(Declaration {
        name: "wav-stereo-48000-16bit.wav",
        purpose: "a container that carries no picture, which is its nature and not a pathology",
        axes: &[Axis::Container, Axis::Audio],
        bytes: wav_file(2, 48_000, 16, 100),
        expect: Expectation::Readable {
            container: ContainerKind::Wav,
            streams: 1,
            video: None,
            audio: Some(AudioExpectation {
                channels: 2,
                sample_rate: 48_000,
                sample_bits: Some(16),
            }),
        },
    });

    corpus.push(Declaration {
        name: "wav-mono-44100-24bit.wav",
        purpose: "44.1 kHz at twenty-four bits: a depth the four-case model cannot hold and does not round",
        axes: &[Axis::Audio],
        bytes: wav_file(1, 44_100, 24, 100),
        expect: Expectation::Readable {
            container: ContainerKind::Wav,
            streams: 1,
            video: None,
            audio: Some(AudioExpectation {
                channels: 1,
                sample_rate: 44_100,
                sample_bits: Some(24),
            }),
        },
    });

    // --- Stills ------------------------------------------------------------------------
    corpus.push(Declaration {
        name: "still-rgb-8bit.png",
        purpose: "the still the renderer actually reads today, and the file the repaid debt is about",
        axes: &[Axis::Container, Axis::BitDepth],
        bytes: png_file(64, 32, 8, 2),
        expect: picture_only(
            ContainerKind::Png,
            1,
            video_expectation(
                (64, 32),
                (64, 32),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Variable(vec![0]),
                PlaneLayout::Rgb,
                BitDepth::Eight,
                SampleRange::Full,
                square,
                Some(1),
            ),
        ),
    });

    corpus.push(Declaration {
        name: "still-rgba-16bit.png",
        purpose: "sixteen bits with an alpha, which is the deepest still this build reads",
        axes: &[Axis::BitDepth],
        bytes: png_file(16, 16, 16, 6),
        expect: picture_only(
            ContainerKind::Png,
            1,
            video_expectation(
                (16, 16),
                (16, 16),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Variable(vec![0]),
                PlaneLayout::Rgba,
                BitDepth::Sixteen,
                SampleRange::Full,
                square,
                Some(1),
            ),
        ),
    });

    corpus.push(Declaration {
        name: "still-rgb-8bit.ppm",
        purpose: "the one container that parses without a library",
        axes: &[Axis::Container],
        bytes: ppm_file(8, 8, 255),
        expect: picture_only(
            ContainerKind::Ppm,
            1,
            video_expectation(
                (8, 8),
                (8, 8),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Variable(vec![0]),
                PlaneLayout::Rgb,
                BitDepth::Eight,
                SampleRange::Full,
                square,
                Some(1),
            ),
        ),
    });

    corpus.push(Declaration {
        name: "still-rgb-16bit.ppm",
        purpose: "a sixteen-bit Netpbm, whose body is twice as long for the same declared size",
        axes: &[Axis::BitDepth],
        bytes: ppm_file(4, 4, 65_535),
        expect: picture_only(
            ContainerKind::Ppm,
            1,
            video_expectation(
                (4, 4),
                (4, 4),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Variable(vec![0]),
                PlaneLayout::Rgb,
                BitDepth::Sixteen,
                SampleRange::Full,
                square,
                Some(1),
            ),
        ),
    });

    // --- Pathologies -------------------------------------------------------------------
    let valid = iso_file(
        b"isom",
        &[TrackSpec::Video(VideoSpec::h264(
            (1_920, 1_080),
            24_000,
            vec![(48, 1_001)],
        ))],
    );

    corpus.push(Declaration {
        name: "truncated-mid-file.mp4",
        purpose: "a file that was valid and now stops in the middle of a box it declared",
        axes: &[Axis::Pathology],
        bytes: valid[..valid.len() * 2 / 3].to_vec(),
        expect: Expectation::Refused("damaged"),
    });

    corpus.push(Declaration {
        name: "zero-length.mp4",
        purpose: "present, named, and zero bytes long: not offline, and not a container either",
        axes: &[Axis::Pathology],
        bytes: Vec::new(),
        expect: Expectation::Refused("damaged"),
    });

    corpus.push(Declaration {
        name: "wrong-extension-wav-named.mov",
        purpose: "a WAVE file called `.mov`: the bytes decide, and the extension is only a claim",
        axes: &[Axis::Pathology, Axis::Container],
        bytes: wav_file(2, 48_000, 16, 50),
        expect: Expectation::Readable {
            container: ContainerKind::Wav,
            streams: 1,
            video: None,
            audio: Some(AudioExpectation {
                channels: 2,
                sample_rate: 48_000,
                sample_bits: Some(16),
            }),
        },
    });

    corpus.push(Declaration {
        name: "wrong-extension-png-named.mp4",
        purpose: "a PNG still called `.mp4`, which probes as the still it is",
        axes: &[Axis::Pathology],
        bytes: png_file(8, 8, 8, 2),
        expect: picture_only(
            ContainerKind::Png,
            1,
            video_expectation(
                (8, 8),
                (8, 8),
                Rotation::None,
                FieldOrder::Progressive,
                RateExpectation::Variable(vec![0]),
                PlaneLayout::Rgb,
                BitDepth::Eight,
                SampleRange::Full,
                square,
                Some(1),
            ),
        ),
    });

    corpus.push(Declaration {
        name: "garbage-no-signature.mp4",
        purpose: "bytes that are not any container, under a name that claims they are",
        axes: &[Axis::Pathology],
        bytes: filler(4_096),
        expect: Expectation::Refused("undecodable"),
    });

    let mut bad_checksum = png_file(32, 32, 8, 2);
    let flip = bad_checksum.len() / 2;
    bad_checksum[flip] ^= 0xFF;
    corpus.push(Declaration {
        name: "png-failing-checksum.png",
        purpose: "present, whole, and one byte different: the case `kind` could not tell apart",
        axes: &[Axis::Pathology],
        bytes: bad_checksum,
        expect: Expectation::Refused("damaged"),
    });

    let short_still = ppm_file(32, 32, 255);
    corpus.push(Declaration {
        name: "ppm-truncated-body.ppm",
        purpose: "a header that promises a thousand pixels over a body that holds a hundred",
        axes: &[Axis::Pathology],
        bytes: short_still[..short_still.len() / 4].to_vec(),
        expect: Expectation::Refused("damaged"),
    });

    let mut without_moov = Writer::new();
    let mut brand = Writer::new();
    brand.raw(b"isom").be32(512).raw(b"isom");
    without_moov
        .raw(&box_of(b"ftyp", &brand.finish()))
        .raw(&box_of(b"mdat", &filler(512)));
    corpus.push(Declaration {
        name: "mp4-without-movie-box.mp4",
        purpose: "a well-formed box structure that declares no streams at all",
        axes: &[Axis::Pathology],
        bytes: without_moov.finish(),
        expect: Expectation::Refused("damaged"),
    });

    let mut tiny_box = Writer::new();
    let mut brand = Writer::new();
    brand.raw(b"isom").be32(512).raw(b"isom");
    tiny_box.raw(&box_of(b"ftyp", &brand.finish()));
    // A box claiming three bytes, which cannot even hold the header that says so.
    tiny_box.be32(3).raw(b"moov").zeros(32);
    corpus.push(Declaration {
        name: "mp4-box-size-below-its-header.mp4",
        purpose: "a declared size that would make the box walk go backwards",
        axes: &[Axis::Pathology],
        bytes: tiny_box.finish(),
        expect: Expectation::Refused("damaged"),
    });

    // The EBML header, and a segment holding only its information: no track declaration.
    let mut info = Vec::new();
    info.extend_from_slice(&ebml_uint(&[0x2A, 0xD7, 0xB1], 1_000_000));
    let mut header = Vec::new();
    header.extend_from_slice(&ebml_string(&[0x42, 0x82], "matroska"));
    let mut mkv_without_tracks = ebml(&[0x1A, 0x45, 0xDF, 0xA3], &header);
    mkv_without_tracks.extend_from_slice(&ebml(
        &[0x18, 0x53, 0x80, 0x67],
        &ebml(&[0x15, 0x49, 0xA9, 0x66], &info),
    ));
    corpus.push(Declaration {
        name: "mkv-without-track-declaration.mkv",
        purpose: "a segment with information and no tracks: readable structure, nothing to read",
        axes: &[Axis::Pathology],
        bytes: mkv_without_tracks,
        expect: Expectation::Refused("damaged"),
    });

    let mut mxf = Vec::new();
    mxf.extend_from_slice(&[
        0x06, 0x0E, 0x2B, 0x34, 0x02, 0x05, 0x01, 0x01, 0x0D, 0x01, 0x02, 0x01, 0x01, 0x02, 0x04,
        0x00,
    ]);
    mxf.extend_from_slice(&filler(512));
    corpus.push(Declaration {
        name: "mxf-partition-pack-only.mxf",
        purpose: "a container this build recognises by signature and refuses rather than half-reads",
        axes: &[Axis::Pathology, Axis::Container],
        bytes: mxf,
        expect: Expectation::Refused("undecodable"),
    });

    corpus.into_iter().map(Declaration::into_file).collect()
}

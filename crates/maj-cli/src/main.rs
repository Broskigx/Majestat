//! `maj-cli` — headless render, batch verification and CI for edits.
//!
//! **Layer 6.** The command line is not a convenience wrapper: it is the reason every later
//! phase can be tested from a script and reproduced from a log. A render that only the
//! interface can start is a render nobody can bisect.
//!
//! Six commands, and each one answers a question a person can ask of a project without
//! opening an editor:
//!
//! | Command | Question |
//! |---|---|
//! | `render` | What does frame *N* look like, and what made it? |
//! | `verify` | Do two ways of rendering that frame agree, bit for bit? |
//! | `probe` | What is in this project? |
//! | `bench` | How long does a frame take on *this* machine, and does it repeat? |
//! | `edit` | What does applying one command to the document do to it? |
//! | `mcp` | Serve this editor to an agent, over the Model Context Protocol |
//!
//! `mcp` is the same engine behind a different door: it hands a language model the typed
//! commands as tools, in the crate [`maj_mcp`] implements. Nothing about it is a second
//! implementation — the tools append the same commands `edit` does, to the same log, which is
//! why an agent's work can be verified with the commands above.
//!
//! Argument parsing is hand-written, deliberately: the dependency policy in ADR 0009 admits
//! nothing that is not measured to earn its place, and a five-command surface with long
//! options is a hundred lines of parsing.
//!
//! Every command that renders uses [`maj_render::RenderTask`], which is the certified path.
//! `verify` and `bench` both check the digests they print rather than trusting the renderer:
//! a command that reports a number it did not check is a command that lies.

use std::fmt;
use std::process::ExitCode;

use maj_color::TransferFunction;
use maj_core::{Command, Project, Sequence, SequenceId};
use maj_export::{ImageFormat, manifest};
use maj_render::{
    BitDepth, MissingMedia, RenderError, RenderManifest, RenderPolicy, RenderRequest, RenderTask,
    WorkspaceResolver, render_frame, render_tiled,
};

/// Layer in the Majestat architecture, from `docs/ARCHITECTURE.md`.
pub const LAYER: u8 = 6;

/// Why a command could not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CliError {
    /// The arguments do not describe a command this build has.
    Usage(String),
    /// The work was understood and could not be done.
    Operation(String),
    /// The work was done and the answer is no. This is what an exit code is for.
    Failed(String),
}

impl CliError {
    /// The process exit code that goes with this failure.
    const fn code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Operation(_) => 3,
            Self::Failed(_) => 1,
        }
    }

    fn usage(what: impl Into<String>) -> Self {
        Self::Usage(what.into())
    }

    fn operation(what: impl Into<String>) -> Self {
        Self::Operation(what.into())
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(what) | Self::Operation(what) | Self::Failed(what) => {
                formatter.write_str(what)
            }
        }
    }
}

impl From<RenderError> for CliError {
    fn from(error: RenderError) -> Self {
        Self::operation(error.to_string())
    }
}

impl From<maj_export::ExportError> for CliError {
    fn from(error: maj_export::ExportError) -> Self {
        Self::operation(error.to_string())
    }
}

impl From<maj_core::CoreError> for CliError {
    fn from(error: maj_core::CoreError) -> Self {
        Self::operation(error.to_string())
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if matches!(error, CliError::Failed(_)) {
                // A failed check is not a mistake, so it does not read like one.
                println!("{}", error);
            } else {
                eprintln!("maj: {error}");
            }
            ExitCode::from(error.code())
        }
    }
}

/// Dispatches one command line.
fn run(args: &[String]) -> Result<(), CliError> {
    let Some(command) = args.first() else {
        print_usage();
        return Ok(());
    };
    let rest = &args[1..];
    match command.as_str() {
        "render" => {
            let options = Options::parse(rest)?;
            if options.has("help") {
                print_usage();
                return Ok(());
            }
            render(&options)
        }
        "verify" => {
            let options = Options::parse(rest)?;
            if options.has("help") {
                print_usage();
                return Ok(());
            }
            verify(&options)
        }
        "probe" => {
            let options = Options::parse(rest)?;
            if options.has("help") {
                print_usage();
                return Ok(());
            }
            probe(&options)
        }
        "bench" => {
            let options = Options::parse(rest)?;
            if options.has("help") {
                print_usage();
                return Ok(());
            }
            bench(&options)
        }
        "edit" => {
            let options = Options::parse(rest)?;
            if options.has("help") {
                print_usage();
                return Ok(());
            }
            edit(&options)
        }
        "mcp" => {
            let options = Options::parse(rest)?;
            if options.has("help") {
                print_usage();
                return Ok(());
            }
            mcp(&options)
        }
        "--version" | "version" => {
            println!("maj {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "--help" | "help" => {
            print_usage();
            Ok(())
        }
        other => Err(CliError::usage(format!(
            "`{other}` is not a command; try `maj help`"
        ))),
    }
}

fn print_usage() {
    println!(
        "maj {} — headless render and verification",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("USAGE");
    println!("  maj <command> [options]");
    println!();
    println!("COMMANDS");
    println!("  render   Render one frame to a still and write its manifest");
    println!("  verify   Render a frame whole and tiled, and compare the digests");
    println!("  probe    Report the sequences, tracks, clips and media of a project");
    println!("  bench    Time a real render on this machine, and check it repeats");
    println!("  edit     Apply one command to a project and save the result");
    println!("  mcp      Serve the editor to an agent over the Model Context Protocol");
    println!("  version  Print the version");
    println!();
    println!("COMMON OPTIONS");
    println!("  --project <path>     A .maj document (required)");
    println!("  --sequence <id|name> Sequence to render (default: the first)");
    println!("  --frame <n>          Timeline frame (default: 0)");
    println!("  --tiles <CxR>        Render as a grid of tiles (default: 1x1)");
    println!("  --width / --height   Output size (default: the sequence's own)");
    println!("  --strict-media       Refuse a frame whose picture is missing, damaged or");
    println!("                       undecodable, instead of drawing a labelled placeholder");
    println!("                       and naming it in the manifest. What a delivery wants.");
    println!();
    println!("RENDER OPTIONS");
    println!("  --out <path>         Where to write the still (required; png or ppm)");
    println!("  --depth <8|16>       Bits per channel for a PNG (default: 16)");
    println!("  --transfer <curve>   linear, srgb, rec709, pq, hlg, or gamma_<num>_<den>");
    println!("                       (default: srgb)");
    println!("  --no-manifest        Do not write the manifest beside the still");
    println!();
    println!("EDIT OPTIONS");
    println!("  --command <json>     One command, as the log stores it (required)");
    println!("  --out <path>         Where to save (default: in place)");
    println!();
    println!("MCP OPTIONS");
    println!("  --project <path>     The document to open (optional; an empty project otherwise)");
    println!("  --name <name>        Name of a project that is not opened from a file");
    println!("  --session <name>     Name of the starting session (default: default)");
    println!("  --read-only          Refuse every edit, answer every question");
    println!("  --list-tools         Print the tool catalogue as JSON and exit");
}

/// One command line, as long options with their values.
#[derive(Debug, Default, Clone)]
struct Options {
    values: Vec<(String, String)>,
}

impl Options {
    /// Splits `--name value`, `--name=value` and bare `--switch` forms.
    fn parse(args: &[String]) -> Result<Self, CliError> {
        let mut values = Vec::new();
        let mut index = 0;
        while index < args.len() {
            let token = &args[index];
            let trimmed = token.strip_prefix("--").or_else(|| token.strip_prefix('-'));
            let Some(name) = trimmed else {
                return Err(CliError::usage(format!(
                    "unexpected argument `{token}`; options are written `--name value`"
                )));
            };
            if name.is_empty() {
                return Err(CliError::usage("an option needs a name"));
            }
            if let Some((name, value)) = name.split_once('=') {
                values.push((name.to_owned(), value.to_owned()));
                index += 1;
                continue;
            }
            match args.get(index + 1) {
                // A value that starts with a dash is another option, not an argument: the
                // only negative values this surface takes are numbers, and those are
                // written `--frame=-1` or `--frame -1` with the number not looking like a
                // flag. Bare switches therefore work without a separate list.
                Some(value) if !value.starts_with("--") && !is_switch(args, index + 1) => {
                    values.push((name.to_owned(), value.clone()));
                    index += 2;
                }
                _ => {
                    values.push((name.to_owned(), String::new()));
                    index += 1;
                }
            }
        }
        Ok(Self { values })
    }

    /// Whether a named option is present at all.
    fn has(&self, name: &str) -> bool {
        self.values.iter().any(|(key, _)| key == name)
    }

    /// The value of a named option, if it has one.
    fn get(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The value of a named option, or a default when it is absent.
    fn string(&self, name: &str, default: &str) -> String {
        self.get(name)
            .filter(|value| !value.is_empty())
            .unwrap_or(default)
            .to_owned()
    }

    /// A named option the command cannot work without.
    fn required(&self, name: &str) -> Result<&str, CliError> {
        self.get(name)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| CliError::usage(format!("`--{name}` is required; try `maj help`")))
    }

    /// A named option parsed as a number.
    fn number<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>, CliError> {
        match self.get(name) {
            None => Ok(None),
            Some(value) => value
                .parse::<T>()
                .map(Some)
                .map_err(|_| CliError::usage(format!("`--{name} {value}` is not a number"))),
        }
    }
}

/// The picture resolver these options ask for.
///
/// One switch, `--strict-media`, and it is the difference between the two legitimate
/// answers about media that cannot be read. Without it a frame whose picture is missing,
/// damaged or undecodable renders a labelled placeholder and says so in the manifest, which
/// is what somebody looking at a timeline wants. With it the frame is refused, which is what
/// a delivery wants: a placeholder in a master is worse than a failed render, because a
/// failed render is noticed.
///
/// It is a switch rather than a default because the command line is where a delivery is
/// scripted, and a script that wanted the strict answer and silently got the forgiving one
/// would ship the placeholder.
fn resolver_for(options: &Options) -> WorkspaceResolver {
    let policy = if options.has("strict-media") {
        MissingMedia::Refuse
    } else {
        MissingMedia::Placeholder
    };
    WorkspaceResolver::new().with_missing_media(policy)
}

/// Whether the token at `index` is an option rather than a value.
///
/// The one place this matters is an option whose value could look like a flag, which is why
/// `--name=value` exists and is checked first.
fn is_switch(args: &[String], index: usize) -> bool {
    args.get(index)
        .is_some_and(|token| token.starts_with("--") && !token.contains('='))
}

/// Loads a project document.
fn load(path: &str) -> Result<Project, CliError> {
    maj_core::format::load_from_path(path)
        .map_err(|error| CliError::operation(format!("{path}: {error}")))
}

/// The sequence a command should work on.
fn select_sequence<'a>(project: &'a Project, options: &Options) -> Result<&'a Sequence, CliError> {
    match options.get("sequence") {
        None => project
            .sequences()
            .first()
            .ok_or_else(|| CliError::operation("the project holds no sequence")),
        Some(wanted) => {
            if let Ok(raw) = wanted.parse::<u64>() {
                let id = SequenceId::new(raw);
                if let Some(sequence) = project.sequence(id) {
                    return Ok(sequence);
                }
            }
            project
                .sequences()
                .iter()
                .find(|sequence| sequence.name() == wanted)
                .ok_or_else(|| {
                    CliError::operation(format!("the project has no sequence `{wanted}`"))
                })
        }
    }
}

/// A tile grid, written `CxR`.
fn parse_grid(options: &Options) -> Result<(u32, u32), CliError> {
    match options.get("tiles") {
        None => Ok((1, 1)),
        Some(value) => {
            let (columns, rows) = value.split_once(['x', 'X', ',']).ok_or_else(|| {
                CliError::usage(format!("`--tiles {value}` is not a `columns x rows` grid"))
            })?;
            let columns = columns.trim().parse::<u32>().map_err(|_| {
                CliError::usage(format!("`--tiles {value}` has an unreadable column count"))
            })?;
            let rows = rows.trim().parse::<u32>().map_err(|_| {
                CliError::usage(format!("`--tiles {value}` has an unreadable row count"))
            })?;
            if columns == 0 || rows == 0 {
                return Err(CliError::usage("a tile grid needs at least one tile"));
            }
            Ok((columns, rows))
        }
    }
}

/// The transfer function a `--transfer` value names.
fn parse_transfer(value: &str) -> Result<TransferFunction, CliError> {
    match value {
        "linear" => Ok(TransferFunction::Linear),
        "srgb" => Ok(TransferFunction::Srgb),
        "rec709" => Ok(TransferFunction::Rec709),
        "pq" => Ok(TransferFunction::Pq),
        "hlg" => Ok(TransferFunction::Hlg),
        other => {
            if let Some(rest) = other.strip_prefix("gamma_") {
                let (num, den) = rest.split_once('_').ok_or_else(|| {
                    CliError::usage(format!(
                        "`--transfer {other}` needs a gamma_<num>_<den> exponent"
                    ))
                })?;
                let num = num
                    .parse::<i32>()
                    .map_err(|_| CliError::usage(format!("`{num}` is not a numerator")))?;
                let den = den
                    .parse::<i32>()
                    .map_err(|_| CliError::usage(format!("`{den}` is not a denominator")))?;
                let gamma = TransferFunction::Gamma { num, den };
                // The curve refuses a degenerate exponent, and the check runs here rather
                // than at the first pixel so the message names the argument.
                if gamma.decode(0.5).is_nan() {
                    return Err(CliError::usage(format!(
                        "`--transfer {other}` is not a usable exponent"
                    )));
                }
                Ok(gamma)
            } else {
                Err(CliError::usage(format!(
                    "`{other}` is not a curve; try linear, srgb, rec709, pq or hlg"
                )))
            }
        }
    }
}

/// The bit depth a `--depth` value names.
fn parse_depth(options: &Options) -> Result<BitDepth, CliError> {
    match options.number::<u32>("depth")? {
        None | Some(16) => Ok(BitDepth::Sixteen),
        Some(8) => Ok(BitDepth::Eight),
        Some(other) => Err(CliError::usage(format!(
            "`--depth {other}` is neither eight nor sixteen"
        ))),
    }
}

/// The request a command's options describe.
fn request_of(project: &Project, options: &Options) -> Result<RenderRequest, CliError> {
    let sequence = select_sequence(project, options)?;
    let frame = options.number::<i64>("frame")?.unwrap_or(0);
    let width = options
        .number::<u32>("width")?
        .unwrap_or_else(|| sequence.width());
    let height = options
        .number::<u32>("height")?
        .unwrap_or_else(|| sequence.height());
    if width == 0 || height == 0 {
        return Err(CliError::usage("a frame needs a non-zero size"));
    }
    Ok(RenderRequest::new(sequence.id(), frame, width, height))
}

/// Renders one frame and writes it, with its manifest.
fn render(options: &Options) -> Result<(), CliError> {
    let project = load(options.required("project")?)?;
    let request = request_of(&project, options)?;
    let grid = parse_grid(options)?;
    let depth = parse_depth(options)?;
    let transfer = parse_transfer(&options.string("transfer", "srgb"))?;
    let out = options.required("out")?.to_owned();
    let format = ImageFormat::from_path(&out)
        .ok_or_else(|| CliError::usage(format!("`{out}` is not a png or a ppm")))?;
    if !format.has_alpha() && depth == BitDepth::Sixteen {
        // A PPM is always sixteen bits, which is worth saying once rather than twice.
    }

    let task = RenderTask {
        request,
        policy: RenderPolicy::Certified,
        grid,
    };
    let report = task.render(&project, &resolver_for(options))?;
    let mut paths = vec![out.clone()];
    if options.has("no-manifest") {
        maj_export::write_frame(&out, &report.frame, depth, transfer)?;
    } else {
        let manifest_path = manifest::path_for(std::path::Path::new(&out));
        maj_export::write_frame_with_manifest(
            &out,
            &report.frame,
            &report.manifest,
            depth,
            transfer,
        )?;
        paths.push(manifest_path.to_string_lossy().into_owned());
    }
    println!("{}", manifest::summary(&report.manifest));
    println!(
        "wrote {} — {} bits per channel, {} curve",
        paths.join(" and "),
        depth.key(),
        transfer.key()
    );
    Ok(())
}

/// Renders a frame whole and tiled, and compares the digests.
fn verify(options: &Options) -> Result<(), CliError> {
    let project = load(options.required("project")?)?;
    let request = request_of(&project, options)?;
    let grid = parse_grid(options)?;
    if grid == (1, 1) {
        return Err(CliError::usage(
            "`--tiles` must be something other than 1x1 for a tiling check to mean anything",
        ));
    }
    let task = RenderTask {
        request,
        policy: RenderPolicy::Certified,
        grid,
    };
    let halo = task.halo(&project)?;

    // Two whole-frame passes, then the tiled pass: the first pair is the Reproducible tier,
    // and the third is the promise that tiling changes nothing. One resolver for all three,
    // so the three passes cannot differ in what they did about media they could not read.
    let resolver = resolver_for(options);
    let first = render_frame(&project, request, &resolver)?;
    let second = render_frame(&project, request, &resolver)?;
    let tiled = render_tiled(&project, request, &resolver, grid)?;
    let manifest = RenderManifest::build(
        &project,
        request,
        RenderPolicy::Certified,
        grid,
        halo,
        &tiled,
        &resolver,
    )?;

    println!("{}", manifest::summary(&manifest));
    println!("whole frame, run 1: {}", first.digest());
    println!("whole frame, run 2: {}", second.digest());
    println!("tiled {}x{}:        {}", grid.0, grid.1, tiled.digest());
    println!("halo: {halo} pixel(s)");

    if first.digest() != second.digest() {
        return Err(CliError::Failed(
            "FAILED: two identical renders produced different pixels".to_owned(),
        ));
    }
    if first.digest() != tiled.digest() {
        return Err(CliError::Failed(format!(
            "FAILED: the {}x{} tiled render differs from the whole-frame render",
            grid.0, grid.1
        )));
    }
    println!("OK: the same bytes three ways");
    Ok(())
}

/// Reports what a project holds.
fn probe(options: &Options) -> Result<(), CliError> {
    let project = load(options.required("project")?)?;
    println!("project        {}", project.name());
    println!("revision       {}", project.revision());
    println!("state hash     {}", project.state_hash()?);
    println!("assets         {}", project.assets().len());
    for asset in project.assets() {
        let picture = if asset.has_video() {
            "picture"
        } else {
            "no picture"
        };
        let sound = if asset.has_audio() {
            "sound"
        } else {
            "no sound"
        };
        println!(
            "  {} `{}` {picture}, {sound}, {} frames @ {} fps",
            asset.id(),
            asset.name(),
            asset.duration_frames(),
            asset.timebase().nominal_fps()
        );
        println!("    {}", asset.uri());
    }
    println!("sequences      {}", project.sequences().len());
    for sequence in project.sequences() {
        let duration = sequence
            .duration_frames()
            .map_or_else(|| "empty".to_owned(), |frames| format!("{frames} frames"));
        println!(
            "  {} `{}` {}x{} @ {} fps, {duration}",
            sequence.id(),
            sequence.name(),
            sequence.width(),
            sequence.height(),
            sequence.timebase().nominal_fps()
        );
        for track in sequence.tracks() {
            let kind = match track.kind() {
                maj_core::TrackKind::Video => "video",
                maj_core::TrackKind::Audio => "audio",
            };
            println!(
                "    {} `{}` ({kind}, {}{})",
                track.id(),
                track.name(),
                track.clips().len(),
                if track.is_enabled() { "" } else { ", disabled" }
            );
            for clip in track.clips() {
                let effects = clip
                    .effects()
                    .iter()
                    .filter(|effect| effect.is_enabled())
                    .map(|effect| effect.kind().key())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!(
                    "      {} `{}` {}..{} source {}..{} {}",
                    clip.id(),
                    clip.name(),
                    clip.timeline_start(),
                    clip.timeline_start() + clip.timeline_length(),
                    clip.source_range().start(),
                    clip.source_range().end(),
                    if effects.is_empty() {
                        "no effects".to_owned()
                    } else {
                        format!("effects: {effects}")
                    }
                );
            }
            for transition in track.transitions() {
                println!(
                    "      {} {} at {} for {} frames",
                    transition.id(),
                    transition.kind().key(),
                    transition.at(),
                    transition.duration()
                );
            }
        }
    }
    println!("log entries    {}", project.log().len());
    Ok(())
}

/// Times real renders on this machine.
fn bench(options: &Options) -> Result<(), CliError> {
    let project = load(options.required("project")?)?;
    let request = request_of(&project, options)?;
    let grid = parse_grid(options)?;
    let repeat = options.number::<u32>("repeat")?.unwrap_or(3).max(1);
    let halo = RenderTask {
        request,
        policy: RenderPolicy::Certified,
        grid,
    }
    .halo(&project)?;

    let megapixels = f64::from(request.width) * f64::from(request.height) / 1_000_000.0;
    println!(
        "rendering {}x{} ({} megapixels) on the certified path, halo {halo}, grid {}x{}",
        request.width, request.height, megapixels, grid.0, grid.1
    );
    let mut digests = Vec::new();
    let mut timings = Vec::new();
    let resolver = resolver_for(options);
    for run in 1..=repeat {
        let started = std::time::Instant::now();
        let frame = render_tiled(&project, request, &resolver, grid)?;
        let elapsed = started.elapsed();
        digests.push(frame.digest());
        timings.push(elapsed);
        println!(
            "  run {run}: {:>8.3} ms  {:>7.2} MP/s  {}",
            elapsed.as_secs_f64() * 1000.0,
            megapixels / elapsed.as_secs_f64().max(1.0e-9),
            &frame.digest()[..12]
        );
    }
    let total: std::time::Duration = timings.iter().sum();
    let slowest = timings.iter().max().copied().unwrap_or_default();
    let fastest = timings.iter().min().copied().unwrap_or_default();
    println!(
        "total {:.3} ms, mean {:.3} ms, fastest {:.3} ms, slowest {:.3} ms",
        total.as_secs_f64() * 1000.0,
        total.as_secs_f64() * 1000.0 / f64::from(repeat),
        fastest.as_secs_f64() * 1000.0,
        slowest.as_secs_f64() * 1000.0
    );
    println!(
        "note: the timings are this machine today; the digest below is not a timing\n      {}",
        digests[0]
    );
    if digests.iter().any(|digest| digest != &digests[0]) {
        return Err(CliError::Failed(
            "FAILED: repeated renders produced different pixels".to_owned(),
        ));
    }
    Ok(())
}

/// Serves the editor over the Model Context Protocol on standard input and output.
///
/// This is the one command that does not return: it speaks the protocol until its input ends.
/// Standard output carries nothing but protocol messages, which is why every message this
/// function prints goes to standard error instead.
fn mcp(options: &Options) -> Result<(), CliError> {
    if options.has("list-tools") {
        let tools: Vec<serde_json::Value> = maj_mcp::TOOLS
            .iter()
            .map(maj_mcp::catalog::advertised)
            .collect();
        let text = serde_json::to_string_pretty(&serde_json::json!({"tools": tools}))
            .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}"));
        println!("{text}");
        return Ok(());
    }
    let mut server = maj_mcp::Options::default();
    if let Some(path) = options.get("project").filter(|value| !value.is_empty()) {
        server.project = Some(path.into());
    }
    if let Some(name) = options.get("name").filter(|value| !value.is_empty()) {
        server.name = Some(name.to_owned());
    }
    if let Some(session) = options.get("session").filter(|value| !value.is_empty()) {
        server.session = Some(session.to_owned());
    }
    server.read_only = options.has("read-only");
    maj_mcp::serve(server).map_err(|error| CliError::operation(error.to_string()))
}

/// Applies one command to a project and saves the result.
fn edit(options: &Options) -> Result<(), CliError> {
    let path = options.required("project")?.to_owned();
    let mut project = load(&path)?;
    let text = options.required("command")?;
    let command: Command = serde_json::from_str(text)
        .map_err(|error| CliError::usage(format!("`--command` is not a command: {error}")))?;
    let before = project.revision();
    let mut history = maj_core::History::new();
    history.apply_local(&mut project, command)?;
    let out = options.string("out", &path);
    maj_core::format::save_to_path(&project, &out)?;
    println!(
        "revision {before} -> {}, state hash {}",
        project.revision(),
        project.state_hash()?
    );
    println!("wrote {out}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn options_take_values_in_both_forms_and_switches_take_none() {
        let options = Options::parse(&args(&[
            "--project",
            "cut.maj",
            "--frame=12",
            "--no-manifest",
            "--out",
            "shot.png",
        ]))
        .expect("a well-formed command line");
        assert_eq!(options.get("project"), Some("cut.maj"));
        assert_eq!(options.get("frame"), Some("12"));
        assert_eq!(options.get("out"), Some("shot.png"));
        assert!(options.has("no-manifest"));
        assert_eq!(options.get("no-manifest"), Some(""));
        assert_eq!(options.get("absent"), None);

        // A negative number is a value, not a switch.
        let negative = Options::parse(&args(&["--frame", "-1"])).expect("a well-formed line");
        assert_eq!(negative.number::<i64>("frame"), Ok(Some(-1)));

        // A bare switch followed by another option consumes nothing.
        let two = Options::parse(&args(&["--no-manifest", "--frame", "3"])).expect("a line");
        assert!(two.has("no-manifest"));
        assert_eq!(two.number::<i64>("frame"), Ok(Some(3)));

        let bad = Options::parse(&args(&["cut.maj"]));
        assert!(matches!(bad, Err(CliError::Usage(_))), "{bad:?}");
        let unreadable = Options::parse(&args(&["--frame", "many"])).expect("a line");
        assert_eq!(
            unreadable.number::<i64>("frame"),
            Err(CliError::Usage("`--frame many` is not a number".to_owned()))
        );
        let required = Options::parse(&args(&["--project", ""])).expect("a line");
        assert!(required.required("project").is_err());
        assert!(required.required("missing").is_err());
    }

    #[test]
    fn the_usage_errors_say_what_is_wrong_and_where_to_look() {
        let error = CliError::usage("`x` is not a command; try `maj help`");
        assert_eq!(error.code(), 2);
        assert_eq!(
            CliError::operation("no sequence").code(),
            3,
            "an operational failure is not a usage error"
        );
        assert_eq!(CliError::Failed("FAILED".to_owned()).code(), 1);
        assert_eq!(error.to_string(), "`x` is not a command; try `maj help`");
    }

    #[test]
    fn a_grid_is_two_positive_counts() {
        let one = Options::parse(&args(&["--tiles", "1x1"])).expect("a line");
        assert_eq!(parse_grid(&one), Ok((1, 1)));
        let grid = Options::parse(&args(&["--tiles", "3x2"])).expect("a line");
        assert_eq!(parse_grid(&grid), Ok((3, 2)));
        let comma = Options::parse(&args(&["--tiles", "2,4"])).expect("a line");
        assert_eq!(parse_grid(&comma), Ok((2, 4)));
        let none = Options::default();
        assert_eq!(parse_grid(&none), Ok((1, 1)));
        for bad in ["3", "3x", "x2", "0x2", "2x0", "a x b"] {
            let options = Options::parse(&args(&["--tiles", bad])).expect("a line");
            assert!(parse_grid(&options).is_err(), "`{bad}` must be refused");
        }
    }

    #[test]
    fn the_curves_are_named_the_way_the_interface_names_them() {
        assert_eq!(parse_transfer("linear"), Ok(TransferFunction::Linear));
        assert_eq!(parse_transfer("srgb"), Ok(TransferFunction::Srgb));
        assert_eq!(parse_transfer("rec709"), Ok(TransferFunction::Rec709));
        assert_eq!(parse_transfer("pq"), Ok(TransferFunction::Pq));
        assert_eq!(parse_transfer("hlg"), Ok(TransferFunction::Hlg));
        assert_eq!(
            parse_transfer("gamma_12_5"),
            Ok(TransferFunction::Gamma { num: 12, den: 5 })
        );
        assert!(parse_transfer("gamma_12").is_err());
        assert!(parse_transfer("gamma_x_5").is_err());
        assert!(parse_transfer("aces").is_err());

        let depth = Options::parse(&args(&["--depth", "8"])).expect("a line");
        assert_eq!(parse_depth(&depth), Ok(BitDepth::Eight));
        let default = Options::default();
        assert_eq!(parse_depth(&default), Ok(BitDepth::Sixteen));
        let odd = Options::parse(&args(&["--depth", "12"])).expect("a line");
        assert!(parse_depth(&odd).is_err());
    }

    #[test]
    fn an_unknown_command_is_a_usage_error_and_the_help_is_free() {
        assert!(matches!(run(&args(&["rend"])), Err(CliError::Usage(_))));
        assert!(run(&args(&["help"])).is_ok());
        assert!(run(&args(&["--version"])).is_ok());
        assert!(run(&args(&[])).is_ok(), "no arguments prints the usage");
        assert!(run(&args(&["render", "--help"])).is_ok());
    }
}

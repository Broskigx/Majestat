//! `majestat` — the desktop application: the shell in a window.
//!
//! ```text
//! majestat                            the example project, in a window
//! majestat --project reel-a.maj       a document from disk
//! majestat --page deliver             start on a page
//! majestat --frameless                draw the frame instead of the system's
//! majestat --log target/session.log   the same, recording every action to a file
//! majestat --snapshot target/ui.ppm   paint one frame to a file and exit
//! majestat --bench 400                paint 400 frames offscreen and report the timings
//! majestat --regions                  report the painted colours by grid cell and exit
//! majestat --self-check               create a window, post real keys, assert the result
//! ```
//!
//! The window is the whole of it: there is no dialog file browser beyond the system's own, and
//! the keyboard, the pointer and the command palette are the interface. `docs/UI.md` lists the
//! keys — including `space` and `j`/`k`/`l`, which drive the clock rather than a decoder.

use std::process::ExitCode;

use maj_win32::{App, Options};

/// The client size a snapshot, a bench or a region map is painted at.
const OFFSCREEN_SIZE: (i32, i32) = (1440, 900);
/// How many frames `--bench` paints when no count is given.
const BENCH_FRAMES: u32 = 240;

fn main() -> ExitCode {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--project" => match args.next() {
                Some(value) => options.project = Some(value.into()),
                None => return complain("--project needs a path"),
            },
            "--page" => match args.next() {
                Some(value) => options.page = Some(value),
                None => return complain("--page needs a name"),
            },
            "--log" => match args.next() {
                Some(value) => options.log = Some(value.into()),
                None => return complain("--log needs a path"),
            },
            "--snapshot" => match args.next() {
                Some(value) => options.snapshot = Some(value.into()),
                None => return complain("--snapshot needs a path"),
            },
            "--bench" => match args.next() {
                Some(value) => match value.parse::<u32>() {
                    Ok(frames) => options.bench = Some(frames),
                    Err(_) => return complain("--bench needs a frame count"),
                },
                // A bare `--bench` paints the default number rather than refusing.
                None => options.bench = Some(BENCH_FRAMES),
            },
            "--regions" => options.regions = true,
            "--frameless" => options.frameless = true,
            "--self-check" => options.self_check = true,
            "--help" | "-h" => {
                help();
                return ExitCode::SUCCESS;
            }
            "--version" | "-v" | "-V" => {
                println!("majestat {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            other => {
                return complain(&format!("`{other}` is not an option; try --help"));
            }
        }
    }

    let mut app = match App::new(&options) {
        Ok(app) => app,
        Err(error) => {
            eprintln!("majestat: {error}");
            return ExitCode::from(1);
        }
    };

    // The offscreen modes, in the order they are cheapest to check.
    let offscreen: Option<Result<String, String>> = if let Some(path) = options.snapshot.as_ref() {
        Some(app.snapshot(path, OFFSCREEN_SIZE))
    } else if let Some(frames) = options.bench {
        Some(app.bench(frames, OFFSCREEN_SIZE))
    } else if options.regions {
        Some(app.region_map(OFFSCREEN_SIZE))
    } else {
        None
    };
    if let Some(result) = offscreen {
        return match result {
            Ok(summary) => {
                println!("{summary}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("majestat: {error}");
                ExitCode::from(1)
            }
        };
    }

    if options.self_check {
        return match app.self_check() {
            Ok(report) => {
                println!("{report}");
                println!("self-check: ok");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("self-check: {error}");
                ExitCode::from(1)
            }
        };
    }
    match app.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("majestat: {error}");
            ExitCode::from(1)
        }
    }
}

/// Reports a usage error and exits.
fn complain(what: &str) -> ExitCode {
    eprintln!("majestat: {what}");
    eprintln!("try `majestat --help`");
    ExitCode::from(2)
}

/// Prints the usage.
fn help() {
    println!(
        "majestat {} — the shell in a window",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("  majestat [options]");
    println!();
    println!("  --project <path>    open a .maj document (default: the example project)");
    println!("  --page <name>       start on media, cut or deliver");
    println!("  --frameless         draw the window's own frame");
    println!("  --log <path>        append every action to a file");
    println!("  --snapshot <path>   paint one frame to a PPM file and exit");
    println!("  --bench [frames]    paint offscreen and report the timings");
    println!("  --regions           report the painted colours by grid cell");
    println!(
        "  --self-check        post keys and clicks at a real window and assert what they did"
    );
    println!("  --version, --help");
    println!();
    println!("Pages: media, cut, deliver. The rest say which phase owns them.");
    println!("Keys: arrows step; shift widens; alt moves the clip; up/down change track;");
    println!("s splits; delete removes; ctrl+d copies; ctrl+z undoes; ctrl+s saves;");
    println!("ctrl+k opens the palette; ctrl+l switches language; ctrl+1/2/3 set the preview.");
}

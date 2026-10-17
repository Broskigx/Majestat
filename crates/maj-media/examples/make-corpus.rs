//! Writes the synthetic media corpus, and probes what it wrote.
//!
//! ```sh
//! cargo run -p maj-media --example make-corpus
//! cargo run -p maj-media --example make-corpus -- --only rotate-90-h264.mp4
//! cargo run -p maj-media --example make-corpus -- --into some/other/directory
//! ```
//!
//! The default destination is `testdata/media-corpus/media`, which is the directory that
//! corpus's README nominates and `.gitignore` ignores: the generator is committed and the
//! files are not. A manifest is written beside them recording each file's generation command
//! and the reading a probe of it must produce.
//!
//! With the files on disk it probes each one and prints the result against the declaration,
//! which is the same comparison `tests/corpus.rs` makes — this binary is for looking at the
//! corpus, and that test is the gate.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use maj_media::corpus::{self, Expectation};
use maj_media::{MediaBackend, NativeBackend};

fn main() -> ExitCode {
    let mut directory = PathBuf::from(corpus::DIRECTORY);
    let mut only: Option<String> = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--into" => match arguments.next() {
                Some(path) => directory = PathBuf::from(path),
                None => {
                    eprintln!("--into needs a directory");
                    return ExitCode::FAILURE;
                }
            },
            "--only" => match arguments.next() {
                Some(name) => only = Some(name),
                None => {
                    eprintln!("--only needs a file name");
                    return ExitCode::FAILURE;
                }
            },
            "--help" | "-h" => {
                println!(
                    "make-corpus [--into <directory>] [--only <file>]\n\n\
                     Writes the synthetic media corpus and probes it. The default directory is \
                     {}.",
                    corpus::DIRECTORY
                );
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument `{other}`");
                return ExitCode::FAILURE;
            }
        }
    }

    match write(&directory, only.as_deref()) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(failures) => {
            eprintln!("\n{failures} file(s) did not probe as declared");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!(
                "could not write the corpus into {}: {error}",
                directory.display()
            );
            ExitCode::FAILURE
        }
    }
}

/// Writes the corpus and probes it, returning how many files disagreed with their declaration.
fn write(directory: &Path, only: Option<&str>) -> std::io::Result<usize> {
    let files = corpus::files();
    if let Some(name) = only {
        if !files.iter().any(|file| file.name == name) {
            eprintln!("no corpus file is called `{name}`. The corpus holds:");
            for file in &files {
                eprintln!("  {}", file.name);
            }
            return Ok(1);
        }
    }
    corpus::write_all(directory)?;
    println!(
        "wrote {} files and a manifest into {}",
        files.len(),
        directory.display()
    );

    let backend = NativeBackend::new();
    let mut failures = 0;
    println!();
    for file in &files {
        if only.is_some_and(|name| name != file.name) {
            continue;
        }
        let path = directory.join(file.name);
        let found = backend.probe(&path);
        let verdict = match file.check(&found) {
            Ok(()) => "ok".to_owned(),
            Err(difference) => {
                failures += 1;
                format!("DIFFERS: {difference}")
            }
        };
        let reading = match &found {
            Ok(probe) => {
                let mut description = format!("{} ", probe.container.key());
                match probe.primary_video() {
                    Some(stream) => description.push_str(&format!(
                        "{}x{} {} {} {}bit {} rot{} {}",
                        stream.coded_size.0,
                        stream.coded_size.1,
                        stream.rate.key(),
                        stream.picture.layout.key(),
                        stream.picture.depth.bits(),
                        stream.picture.range.key(),
                        stream.rotation.degrees(),
                        stream.field_order.key(),
                    )),
                    None => description.push_str("no picture"),
                }
                if let Some(audio) = probe.audio_streams().next() {
                    description.push_str(&format!(
                        " + {}ch at {}Hz",
                        audio.channels, audio.sample_rate
                    ));
                }
                description
            }
            Err(error) => format!("{} ({})", error.label(), error.reason()),
        };
        println!("{:<48} {reading}", file.name);
        if verdict != "ok" {
            println!("{:<48} {verdict}", "");
        }
    }

    println!("\naxes covered: {}", describe(&corpus::covered_axes()));
    println!("axes not covered by synthesis:");
    for (axis, why) in corpus::NOT_COVERED {
        println!("  {axis}: {why}");
    }
    let refused = files
        .iter()
        .filter(|file| matches!(file.expect, Expectation::Refused(_)))
        .count();
    println!(
        "\n{} files: {} readable, {} refused on purpose",
        files.len(),
        files.len() - refused,
        refused
    );
    Ok(failures)
}

/// The axes, as a comma-separated list of their short names.
fn describe(axes: &[corpus::Axis]) -> String {
    axes.iter()
        .map(|axis| axis.key())
        .collect::<Vec<_>>()
        .join(", ")
}

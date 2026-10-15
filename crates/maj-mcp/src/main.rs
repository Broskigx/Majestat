//! `maj-mcp` — the Majestat Model Context Protocol server, as a program.
//!
//! Standard input and standard output carry the protocol, and nothing else does. A client
//! launches this program, speaks JSON-RPC to it, and reads a message back for every request.
//! Anything a person might want to see — the banner, a warning, a usage error — goes to
//! standard error, because a stray line on standard output would be read as a malformed
//! message and would break the session it was meant to explain.
//!
//! Two flags are not about running a server at all: `--list-tools` prints the catalogue as
//! JSON so a person can read it or a script can diff it, and `--help` explains the rest.
//! Argument parsing is hand-written and long-option only, matching the convention `maj-cli`
//! set in ADR 0009's dependency policy: a four-option surface is forty lines, and a parser
//! crate would be a dependency nothing measured a need for.

use std::process::ExitCode;

use maj_mcp::catalog::{self, TOOLS};
use maj_mcp::{Options, serve};

/// What went wrong before the server could run.
enum Failure {
    /// The command line did not make sense.
    Usage(String),
    /// The server could not start.
    Failed(String),
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(message)) => {
            eprintln!("maj-mcp: {message}");
            eprintln!();
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Failed(message)) => {
            eprintln!("maj-mcp: {message}");
            ExitCode::from(1)
        }
    }
}

/// Runs one command line.
fn run(args: &[String]) -> Result<(), Failure> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--project" | "-p" => {
                options.project = Some(value(args, index + 1, "--project")?.into());
                index += 1;
            }
            "--name" => {
                options.name = Some(value(args, index + 1, "--name")?.to_owned());
                index += 1;
            }
            "--session" => {
                options.session = Some(value(args, index + 1, "--session")?.to_owned());
                index += 1;
            }
            "--read-only" => options.read_only = true,
            "--list-tools" => {
                let tools: Vec<serde_json::Value> = TOOLS.iter().map(catalog::advertised).collect();
                let text = serde_json::to_string_pretty(&serde_json::json!({"tools": tools}))
                    .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}"));
                println!("{text}");
                return Ok(());
            }
            "--version" | "-V" => {
                println!("maj-mcp {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => {
                return Err(Failure::Usage(format!("`{other}` is not an option")));
            }
        }
        index += 1;
    }
    eprintln!(
        "maj-mcp {} — {} tools, {} resources, {} prompts on stdio",
        env!("CARGO_PKG_VERSION"),
        TOOLS.len(),
        maj_mcp::resources::list(maj_mcp::session::DEFAULT_SESSION)["resources"]
            .as_array()
            .map_or(0, Vec::len),
        maj_mcp::prompts::PROMPTS.len(),
    );
    serve(options).map_err(|error| Failure::Failed(error.to_string()))
}

/// The value of an option that takes one.
fn value<'a>(args: &'a [String], index: usize, option: &str) -> Result<&'a str, Failure> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| Failure::Usage(format!("`{option}` needs a value")))
}

/// What the program does.
const USAGE: &str = "\
maj-mcp — the Majestat editor, over the Model Context Protocol

USAGE
  maj-mcp [options]

The server speaks JSON-RPC 2.0 on standard input and standard output, one message per
line. Point an MCP client at this program; nothing else is needed.

OPTIONS
  --project <path>   Open a .maj document into the starting session
  --name <name>      Name of the project to start when no document is given
  --session <name>   Name of the starting session (default: default)
  --read-only        Refuse every edit, answer every question
  --list-tools       Print the tool catalogue as JSON and exit
  --version, -V      Print the version
  --help, -h         Print this

EXAMPLES
  maj-mcp --project reel-a.maj
  maj-mcp --name scratch --session b-roll
  maj-mcp --project locked.maj --read-only";

# 0011. The engine is exposed to an agent over MCP, not given an agent API

* Status: Accepted
* Date: 2026-10-14

## Context

Majestat's central claim is that everything is a command: an edit from a person, from a
script and from an agent is the same immutable record, applied to the same model, verifiable
by the same hashes. M11 is the phase that puts a language model in the loop. The question
this ADR answers is what the model is given.

The obvious answer — an "AI API" layered over the engine, with its own richer operations —
is the one to reject. It would mean two editing paths, two sets of invariants, two kinds of
log entry, and an agent whose work the deterministic verifier could not check. The engine
already exposes exactly one way to change a project, and M0.2's layering exists so that
nothing can quietly add another.

The Model Context Protocol (MCP) is a JSON-RPC 2.0 protocol by which a client hands a model
a catalogue of tools, a set of readable resources and a few procedures. It is deliberately
small, and two of its properties match this project unusually well:

* Tools are described by JSON Schema, so an argument shape can be *published* rather than
  guessed at, and a client can validate before it calls.
* Results are data — text, structure and pictures — so a tool can answer with a rendered
  frame and the manifest behind it rather than with a sentence claiming it worked.

## Decision

A new crate, `maj-mcp`, at **layer 5**, next to `maj-agent`. Layer 5 rather than 6 because
this is not only an application: it is the surface the Pilot (M11), a bridge, and any
external agent all use, and a layer-6 placement would make `maj-cli` and `maj-agent`
dependants of an application. It depends only downwards: `maj-core` for the model and the
command log, `maj-render` and `maj-export` for frames, `maj-color` for transfer functions.

**One tool per command variant, generated from the model.** The catalogue in
`catalog.rs` is data — a table of names, descriptions, argument schemas and worked examples —
and a test asserts that the thirty tags it declares are exactly the tags `serde` accepts
for `Command`. A command added to the model without a tool, or a tool for a command that does
not exist, fails the build. Thirty of the tools map to one command variant each; the
other fifteen read the project, change a session, render a frame, undo, or — the two
shorthands, `maj_setup_sequence` and `maj_place_clip` — append a *variable* number of
commands, which is why they declare no single variant. None of the forty-five can change the
model except by appending a command.

**Arguments are decoded, and then the model validates them.** A tool call is decoded into the
*canonical JSON the log stores* and handed to `serde` as a `Command`. The server performs no
validation of its own beyond shape, so a call cannot be accepted here and refused there, and
the error a client sees is the engine's own sentence about what it would not do.

**Four conveniences for the caller, all resolved before the model sees anything:**

1. **Identifiers are optional.** An entity's own identifier is allocated from
   `Project::next_unused_id` when the client omits it, and never stored, because a stored
   counter would diverge under replay. A model that has to choose numbers will eventually
   choose a taken one.
2. **Numbers are exact.** A whole number, a decimal, a ratio string or `{num, den}` all
   resolve to a `Rational`; decimals convert by long arithmetic (`0.1` is `1/10`), and a value
   that cannot be held exactly is refused rather than rounded.
3. **Entity defaults are filled.** A clip with no transform gets the identity; an effect with
   no parameters gets its declared defaults *in declaration order*, because the model
   validates an instance against its own schema and would refuse any other order. A key an
   effect does not declare is refused by name rather than silently doing nothing.
4. **A missing `index` means "at the end"**, computed from the project, which is what an
   editor means by *add*.

**Two kinds of failure, kept apart.** A tool that does not exist is a JSON-RPC error
(`-32602`), because that is a mistake about the catalogue. Everything a model can fix —
arguments that do not fit, a session that is not open, a command the engine refuses, a render
that fails — is a normal result with `isError` set and a sentence explaining the rule that
was broken. A model should be able to read a refusal and act on it, not watch a transport
event.

**Standard input and standard output, and nothing else.** One JSON-RPC message per line, no
embedded newlines. Banners and warnings go to standard error, where they cannot corrupt the
stream. There is **no HTTP transport**: a remote MCP endpoint needs authentication, TLS and a
policy about who may edit which project, and that belongs to M13 with the rest of the
networked story. A half-built one would look finished and not be.

**No new dependencies.** JSON-RPC 2.0 framing, base64 and the argument decoder are written
here. Each is small, has one behaviour and is tested; ADR 0009's policy asks a dependency to
earn its place, and none of these could.

**A read-only server is a policy, not a suggestion.** `--read-only` refuses every tool whose
catalogue entry changes the project, whatever the client asks for. Reviewing a cut is a real
job, and a reviewer's client should be unable to change what it is reviewing.

**A frame is a resource as well as a tool result.** `maj://frame/{sequence}/{frame}` renders
the same pixels `maj_render_frame` returns. The duplication is deliberate: a resource can be
linked, cached and shown by a client that never calls a tool, and both paths run the same
certified renderer, so they cannot disagree about what the frame looks like.

## Consequences

* **The catalogue is the interface, and it is checked against the model.** A test enumerates
  `Command`'s variants from `serde`'s own error message and compares the sets, so the tool
  surface cannot drift from the engine. The same test decodes each tool's published example
  and deserialises the result, which is how a documentation example stays executable.
* **An agent's edits are attributable and verifiable.** Commands are recorded under the
  client's name (`mcp:claude-desktop`), and a project an agent built over the wire is
  identical to one built any other way: the same document, the same hashes, the same bytes.
  `maj render` and `maj verify` check an agent's work without knowing an agent made it.
* **The engine's errors are the client's errors.** A refusal reads like
  `blur.radius is 300, above the maximum 256`, which is the message the model layer already
  produced for a human. That is a benefit, and it also means the wording of model errors is
  now part of a public surface.
* **No sampling, elicitation or roots.** This server offers tools, resources and prompts and
  declares exactly those capabilities. A model that wants to ask the user a question does it
  in its own client, not through the editor.
* **The schema of an argument is generated from the same declaration that decodes it**, so a
  client cannot be told about an argument the server ignores, nor left unaware of one it
  needs. The cost is that a field's description and its kind are written together, in one
  table, and adding a command means editing that table as well as the model.
* **HTTP, authentication and multi-user sessions remain open.** They are M13's, with the
  cloud story, and nothing here forecloses them: the transport is one module and the sessions
  are a map.

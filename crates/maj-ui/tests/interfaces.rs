//! The claim the interface exists to make: there is one door.
//!
//! These tests drive the same edit twice — once through the interface's intentions, once
//! through the agent's tools — and require the two documents to be identical, hash for hash.
//! They also require that every command the interface can name is a command the agent has a
//! tool for, so the two surfaces cannot drift into different editors sharing a file format.

use std::collections::{BTreeMap, BTreeSet};

use maj_mcp::{Session, Sessions};
use maj_ui::{AUTHOR, Action, KINDS, Shell, kind_of};
use serde_json::{Map, Value, json};

/// The edit, as the arguments of the agent's tools, in the order a model would make it.
///
/// The identifiers are the ones the engine hands out in this order: the asset is one, the
/// sequence two, its track three, the clip four.
fn by_the_agent() -> Session {
    let session = Session::new("cross", "cross-interface", "mcp:test");
    let mut registry = Sessions::new("cross", session);
    call(
        &mut registry,
        "maj_create_media_asset",
        json!({
            "asset": {
                "name": "shot-01",
                "uri": "generated:shot-01",
                "timebase": "24",
                "duration_frames": 240,
                "has_video": true,
                "has_audio": true,
            }
        }),
    );
    call(
        &mut registry,
        "maj_setup_sequence",
        json!({
            "name": "Reel A",
            "timebase": "24",
            "width": 320,
            "height": 180,
            "tracks": ["video"],
        }),
    );
    call(
        &mut registry,
        "maj_place_clip",
        json!({
            "sequence": 2,
            "kind": "video",
            "asset": 1,
            "source_range": [0, 240],
            "timeline_start": 0,
            "name": "shot-01",
        }),
    );
    registry.remove("cross").expect("the session that went in")
}

/// The same edit, as a person's intentions.
fn by_a_person() -> Shell {
    let mut shell = Shell::new("cross-interface");
    for action in [
        Action::CreateMediaAsset {
            name: "shot-01".to_owned(),
            uri: "generated:shot-01".to_owned(),
            timebase: maj_time::Timebase::FPS_24,
            duration_frames: 240,
            has_video: true,
            has_audio: true,
        },
        Action::CreateSequence {
            name: "Reel A".to_owned(),
            timebase: maj_time::Timebase::FPS_24,
            width: 320,
            height: 180,
            sample_rate: 48_000,
        },
        Action::AddTrack(maj_core::TrackKind::Video),
    ] {
        shell
            .dispatch(action.clone())
            .unwrap_or_else(|error| panic!("{action:?}: {error}"));
    }
    let asset = shell
        .project()
        .assets()
        .first()
        .expect("the media that was registered")
        .id();
    shell
        .dispatch(Action::AppendMedia(asset))
        .expect("the clip is placed");
    shell
}

/// Calls a tool and insists it succeeded.
fn call(registry: &mut Sessions, name: &str, args: Value) -> String {
    let object: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    let outcome = maj_mcp::tools::call(registry, name, &object)
        .unwrap_or_else(|error| panic!("{name} was refused: {error}"));
    assert!(
        !outcome.is_error,
        "{name} answered with an error: {}",
        outcome.text
    );
    outcome.text
}

#[test]
fn the_same_edit_made_by_a_person_and_by_a_model_is_the_same_document() {
    let person = by_a_person();
    let agent = by_the_agent();

    let person_state = person.project().state_hash().expect("a hash");
    let agent_state = agent.project().state_hash().expect("a hash");
    if person_state != agent_state {
        panic!(
            "the two documents differ.\n\nperson:\n{}\n\nagent:\n{}",
            maj_core::format::to_pretty_json(person.project()).expect("json"),
            maj_core::format::to_pretty_json(agent.project()).expect("json"),
        );
    }

    let person_kinds: Vec<String> = person
        .project()
        .log()
        .iter()
        .map(|entry| kind_of(entry.command()).to_owned())
        .collect();
    // The agent's side is read out of the log's own serialisation rather than through the
    // interface's naming, so this compares the document with the interface and not the
    // interface with itself.
    let agent_kinds: Vec<String> = agent
        .project()
        .log()
        .iter()
        .map(|entry| {
            serde_json::to_value(entry.command()).expect("a command serialises")["kind"]
                .as_str()
                .expect("the log tags every command")
                .to_owned()
        })
        .collect();
    assert_eq!(
        person_kinds, agent_kinds,
        "the two logs must be the same commands in the same order"
    );

    // The *document* hash is deliberately not equal, and the reason is a feature rather than a
    // gap: it covers the log, and the log records who made each edit. Two people who make the
    // same edit have the same document and a different attribution, which is exactly what an
    // interface that shares one door with an agent has to be able to show.
    assert_ne!(
        person.project().document_hash().expect("a hash"),
        agent.project().document_hash().expect("a hash"),
        "attribution is part of the trace, so the document hash must differ"
    );
    assert!(
        person
            .project()
            .log()
            .iter()
            .all(|entry| entry.author() == AUTHOR)
    );
    assert!(
        agent
            .project()
            .log()
            .iter()
            .all(|entry| entry.author() == "mcp:test")
    );
}

#[test]
fn the_interface_records_itself_as_the_author_of_what_it_applies() {
    let shell = by_a_person();
    assert!(!shell.project().log().is_empty());
    assert!(
        shell
            .project()
            .log()
            .iter()
            .all(|entry| entry.author() == AUTHOR),
        "every command a person makes is attributed to the interface, not to the agent"
    );
}

#[test]
fn every_command_the_interface_can_name_is_a_tool_the_agent_has() {
    let tools: BTreeSet<&str> = maj_mcp::catalog::command_tools()
        .into_iter()
        .map(|(_, command)| command)
        .collect();
    for kind in KINDS {
        assert!(
            tools.contains(kind),
            "the interface can emit {kind} and the agent has no tool for it"
        );
    }
    assert_eq!(
        tools.len(),
        KINDS.len(),
        "the two surfaces cover the same commands"
    );
}

#[test]
fn the_palette_drives_the_agents_tools_against_the_persons_session() {
    let mut shell = Shell::example("cross-interface").expect("the example builds");
    let revision = shell.project().revision();
    let commands = shell.project().log().len();

    let read = shell
        .run_tool("maj_timeline", &BTreeMap::new())
        .expect("the timeline tool answers");
    assert!(read.note.is_some(), "the palette shows what the tool said");
    assert_eq!(
        shell.project().revision(),
        revision,
        "a read is a read: nothing was appended"
    );
    assert_eq!(shell.project().log().len(), commands);

    let mut args = BTreeMap::new();
    args.insert("frame".to_owned(), "0".to_owned());
    let rendered = shell
        .run_tool("maj_render_frame", &args)
        .expect("the render tool answers");
    assert!(
        rendered.note.is_some_and(|note| !note.is_empty()),
        "a render says what it drew"
    );
    assert_eq!(
        shell.project().revision(),
        revision,
        "rendering changes no document"
    );

    // And the same session is still the one the palette edits, so a tool that does edit
    // appends to the log the person is working in.
    let mut place = BTreeMap::new();
    place.insert("kind".to_owned(), "video".to_owned());
    place.insert("asset".to_owned(), "1".to_owned());
    place.insert("source_range".to_owned(), "[0, 24]".to_owned());
    place.insert("timeline_start".to_owned(), "720".to_owned());
    place.insert("name".to_owned(), "from the palette".to_owned());
    shell
        .run_tool("maj_place_clip", &place)
        .expect("the place tool answers");
    assert!(
        shell.project().revision() > revision,
        "a tool that edits edits the session the person is holding"
    );
    assert!(
        shell
            .project()
            .log()
            .iter()
            .all(|entry| entry.author() == AUTHOR),
        "the author is still the interface, even for a tool call"
    );
}

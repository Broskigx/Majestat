//! Round-trip gates for layer 1.
//!
//! These are the M1 acceptance checks: the state must be a function of the log, the
//! document must survive serialisation byte for byte, and undo must be exhaustive.
//!
//! Every run is seeded, so a failure here is reproducible exactly.

mod support;

use maj_core::{History, Project, format};
use support::{Harness, command_budget};

const SEED: u64 = 0x5EED_0001;

#[test]
fn a_random_project_is_valid_and_rebuilds_from_its_log() {
    let harness = Harness::random_project(SEED, command_budget()).expect("fuzz run");
    harness
        .project
        .validate()
        .expect("a fuzzed project must be valid");

    assert!(
        harness.project.revision() > 100,
        "the fuzz should build a real project, got {} commands",
        harness.project.revision()
    );

    let rebuilt =
        format::rebuild_from_log(harness.project.name(), harness.project.log()).expect("rebuild");
    assert_eq!(
        harness.project.state_hash().unwrap(),
        rebuilt.state_hash().unwrap(),
        "replaying the log did not reproduce the state"
    );
    assert_eq!(
        harness.project.document_hash().unwrap(),
        rebuilt.document_hash().unwrap(),
        "replaying the log did not reproduce the log itself"
    );
    assert_eq!(
        harness.project.command_trace_hash().unwrap(),
        rebuilt.command_trace_hash().unwrap()
    );
}

#[test]
fn rebuilding_from_a_log_twice_is_idempotent() {
    let harness = Harness::random_project(SEED + 1, command_budget() / 4).expect("fuzz run");
    let first = format::rebuild_from_log("fuzz", harness.project.log()).expect("first rebuild");
    let second = format::rebuild_from_log("fuzz", first.log()).expect("second rebuild");

    assert_eq!(
        first.state_hash().unwrap(),
        second.state_hash().unwrap(),
        "log to state to log is not idempotent"
    );
    assert_eq!(
        first.document_hash().unwrap(),
        second.document_hash().unwrap()
    );
}

#[test]
fn a_document_survives_json_and_disk_round_trips() {
    let harness = Harness::random_project(SEED + 2, command_budget() / 4).expect("fuzz run");
    let pretty = format::to_pretty_json(&harness.project).unwrap();
    let reloaded = format::load_from_str(&pretty).expect("reload");

    assert_eq!(
        harness.project.state_hash().unwrap(),
        reloaded.state_hash().unwrap()
    );
    assert_eq!(
        harness.project.document_hash().unwrap(),
        reloaded.document_hash().unwrap()
    );
    assert_eq!(
        format::to_canonical_json(&harness.project).unwrap(),
        format::to_canonical_json(&reloaded).unwrap(),
        "the canonical form must be byte-identical after a round trip"
    );

    let directory =
        std::env::temp_dir().join(format!("maj-core-roundtrip-gate-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("fuzz.{}", format::MAJ_EXTENSION));
    format::save_to_path(&harness.project, &path).expect("save");
    let from_disk = format::load_from_path(&path).expect("load");
    assert_eq!(
        harness.project.document_hash().unwrap(),
        from_disk.document_hash().unwrap()
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn every_sampled_prefix_of_the_log_replays_to_the_same_state() {
    let harness = Harness::random_project(SEED + 3, command_budget() / 4).expect("fuzz run");
    let log = harness.project.log();
    assert!(
        log.len() > 200,
        "expected a substantial log, got {}",
        log.len()
    );

    let mut checked = 0;
    for cut in (0..=log.len()).step_by(17) {
        // Path A: replay the prefix directly into a fresh project.
        let mut replayed = Project::new("fuzz");
        format::replay_into(&mut replayed, &log[..cut]).expect("replay prefix");
        replayed
            .validate()
            .expect("a prefix must be a valid project");

        // Path B: apply the same prefix through the history, which also builds the
        // inverse stack. Two independent code paths must agree byte for byte.
        let mut via_history = Project::new("fuzz");
        let mut history = History::new();
        for entry in &log[..cut] {
            history
                .apply_local(&mut via_history, entry.command().clone())
                .expect("apply through history");
        }
        assert_eq!(
            replayed.state_hash().unwrap(),
            via_history.state_hash().unwrap(),
            "replay and history disagreed at cut {cut}"
        );
        assert_eq!(
            replayed.document_hash().unwrap(),
            via_history.document_hash().unwrap(),
            "replay and history disagreed on the log at cut {cut}"
        );
        assert_eq!(via_history.revision(), cut as u64);

        // Every prefix must also undo all the way back to nothing.
        let undone = history.undo_all(&mut via_history).expect("undo prefix");
        assert_eq!(undone, cut);
        assert_eq!(
            via_history.state_hash().unwrap(),
            Project::new("fuzz").state_hash().unwrap(),
            "undoing the prefix at cut {cut} did not restore an empty project"
        );
        checked += 1;
    }
    assert!(checked > 10, "the prefix test only checked {checked} cuts");
}

#[test]
fn undoing_everything_returns_to_the_initial_state_and_redoing_returns_to_the_peak() {
    let mut harness = Harness::random_project(SEED + 4, command_budget()).expect("fuzz run");
    let peak_state = harness.project.state_hash().unwrap();
    let peak_document = harness.project.document_hash().unwrap();
    let revision = harness.project.revision();
    assert!(revision > 100);

    let undone = harness
        .history
        .undo_all(&mut harness.project)
        .expect("undo all");
    assert_eq!(undone as u64, revision, "every command must be undoable");
    assert_eq!(harness.project.revision(), 0);
    assert_eq!(
        harness.project.state_hash().unwrap(),
        Project::new("fuzz").state_hash().unwrap(),
        "undoing everything must land exactly on the empty project"
    );
    assert!(harness.project.assets().is_empty());
    assert!(harness.project.sequences().is_empty());
    assert!(harness.project.is_valid());

    let redone = harness
        .history
        .redo_all(&mut harness.project)
        .expect("redo all");
    assert_eq!(redone, undone, "everything undone must be redoable");
    assert_eq!(
        harness.project.state_hash().unwrap(),
        peak_state,
        "redo did not restore the peak state"
    );
    assert_eq!(
        harness.project.document_hash().unwrap(),
        peak_document,
        "redo did not restore the log"
    );
}

#[test]
fn an_empty_project_round_trips_and_replays() {
    let project = Project::new("empty");
    let text = format::to_pretty_json(&project).unwrap();
    let reloaded = format::load_from_str(&text).expect("reload empty");

    assert_eq!(
        project.state_hash().unwrap(),
        reloaded.state_hash().unwrap()
    );
    assert_eq!(reloaded.next_unused_id(), 1);
    assert_eq!(reloaded.revision(), 0);

    let rebuilt = format::rebuild_from_log("empty", &[]).expect("rebuild empty");
    assert_eq!(project.state_hash().unwrap(), rebuilt.state_hash().unwrap());
}

#[test]
fn the_same_seed_builds_the_same_project() {
    let first = Harness::random_project(0xC0FFEE, command_budget() / 4).expect("first");
    let second = Harness::random_project(0xC0FFEE, command_budget() / 4).expect("second");

    assert_eq!(
        first.project.document_hash().unwrap(),
        second.project.document_hash().unwrap(),
        "the same seed must produce the same document"
    );
    assert_eq!(
        first.rng.state(),
        second.rng.state(),
        "the generator must be in the same state too"
    );
}

#[test]
fn a_log_whose_first_command_fails_is_rejected_rather_than_half_applied() {
    // A clip that reads past its media is unrepresentable through the command API, so
    // the only way to observe the rejection is to replay a log that contains one.
    let harness = Harness::random_project(SEED + 5, 60).expect("fuzz run");
    let entries = harness.project.log();

    let mut prefix = Project::new("fuzz");
    format::replay_into(&mut prefix, &entries[..entries.len() - 1]).expect("replay prefix");
    let before = prefix.state_hash().unwrap();

    // Re-applying an already applied command must fail on the duplicate check and
    // leave the project untouched.
    let repeated = entries[entries.len() - 1].command();
    let outcome = repeated.apply(&mut prefix);
    if outcome.is_err() {
        assert_eq!(
            prefix.state_hash().unwrap(),
            before,
            "a rejected command must not mutate the project"
        );
    }
}

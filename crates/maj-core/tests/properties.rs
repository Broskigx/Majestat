//! Seeded property tests for layer 1.
//!
//! Each test fixes its seed, so a failure reproduces exactly. The command budget scales
//! with the build profile: the M1 gate is a ten-thousand-command run, exercised under
//! `--release`, with a smaller budget in debug so the suite stays fast to iterate on.

mod support;

use maj_core::{Project, format};
use maj_time::testing::DeterministicRng;
use support::{Harness, command_budget, trace_budget};

#[test]
fn a_random_project_stays_valid_after_every_single_command() {
    let mut harness = Harness::new(0x1A2B_3C4D);
    harness.bootstrap().expect("bootstrap");
    harness.project.validate().expect("bootstrap must be valid");

    let budget = command_budget();
    for step in 0..budget {
        let Some(command) = harness.random_command() else {
            continue;
        };
        let kind = command.kind();
        harness
            .history
            .apply_local(&mut harness.project, command)
            .unwrap_or_else(|err| panic!("step {step}: {kind} was rejected: {err}"));
        harness
            .project
            .validate()
            .unwrap_or_else(|err| panic!("step {step} ({kind}): {err}"));
    }
    assert!(
        harness.project.revision() > 0,
        "the fuzz applied no commands at all"
    );
}

#[test]
fn random_undo_and_redo_sequences_land_on_the_traced_state() {
    let mut harness = Harness::random_project(0xDEAD_BEEF, trace_budget()).expect("fuzz run");

    // Ground truth: the state after applying the first n commands, for every n.
    let mut trace = vec![Project::new("fuzz").state_hash().unwrap()];
    {
        let mut replay = Project::new("fuzz");
        for entry in harness.project.log() {
            entry.command().apply(&mut replay).expect("replay");
            trace.push(replay.state_hash().unwrap());
        }
    }
    assert_eq!(
        trace.len() as u64,
        harness.project.revision() + 1,
        "the trace must cover every revision"
    );

    // Walk that trace with random undo and redo moves. Whatever route is taken, the
    // memoised inverses must land on exactly the state the log says is there.
    let mut rng = DeterministicRng::new(0x0FF1_CE00);
    let mut cursor = trace.len() - 1;
    let (mut undos, mut redos) = (0usize, 0usize);
    for _ in 0..(trace_budget() * 4) {
        let can_undo = cursor > 0;
        let can_redo = cursor + 1 < trace.len();
        let go_back = match (can_undo, can_redo) {
            (true, true) => rng.next_bool(),
            (true, false) => true,
            (false, true) => false,
            (false, false) => break,
        };
        if go_back {
            let revision = harness.history.undo(&mut harness.project).expect("undo");
            assert!(
                revision.is_some(),
                "undo reported no work at cursor {cursor}"
            );
            cursor -= 1;
            undos += 1;
        } else {
            let revision = harness.history.redo(&mut harness.project).expect("redo");
            assert!(
                revision.is_some(),
                "redo reported no work at cursor {cursor}"
            );
            cursor += 1;
            redos += 1;
        }
        assert_eq!(
            harness.project.revision(),
            cursor as u64,
            "the log length is not the cursor"
        );
        assert_eq!(
            harness.project.state_hash().unwrap(),
            trace[cursor],
            "state diverged at cursor {cursor} after {undos} undos and {redos} redos"
        );
    }
    assert!(
        undos > 0 && redos > 0,
        "the walk should exercise both directions, got {undos} undos and {redos} redos"
    );
    harness
        .project
        .validate()
        .expect("the walk left an invalid project");
}

#[test]
fn the_document_json_round_trips_at_every_sampled_step() {
    let mut harness = Harness::new(0x2468_ACE0);
    harness.bootstrap().expect("bootstrap");

    for step in 0..trace_budget() {
        if let Some(command) = harness.random_command() {
            harness
                .history
                .apply_local(&mut harness.project, command)
                .expect("apply");
        }
        if step % 7 == 0 {
            let pretty = format::to_pretty_json(&harness.project).unwrap();
            let reloaded = format::load_from_str(&pretty).unwrap_or_else(|err| {
                panic!("step {step}: the document could not be reloaded: {err}")
            });
            assert_eq!(
                harness.project.document_hash().unwrap(),
                reloaded.document_hash().unwrap(),
                "the JSON round trip diverged at step {step}"
            );
            assert_eq!(
                harness.project.state_hash().unwrap(),
                reloaded.state_hash().unwrap(),
                "the JSON round trip changed the state at step {step}"
            );
        }
    }
}

#[test]
fn the_state_is_a_function_of_the_log_for_many_seeds() {
    for seed in 0..8u64 {
        let harness = Harness::random_project(seed, trace_budget()).expect("fuzz run");
        let rebuilt =
            format::rebuild_from_log("fuzz", harness.project.log()).expect("rebuild from log");

        assert_eq!(
            harness.project.state_hash().unwrap(),
            rebuilt.state_hash().unwrap(),
            "seed {seed} did not reproduce its state from its log"
        );
        assert_eq!(
            harness.project.document_hash().unwrap(),
            rebuilt.document_hash().unwrap(),
            "seed {seed} did not reproduce its log byte for byte"
        );
    }
}

#[test]
fn undo_all_and_redo_all_are_exact_for_many_seeds() {
    let empty = Project::new("fuzz").state_hash().unwrap();
    for seed in 0..4u64 {
        let mut harness = Harness::random_project(0x2000 + seed, trace_budget()).expect("fuzz run");
        let peak = harness.project.state_hash().unwrap();

        harness
            .history
            .undo_all(&mut harness.project)
            .expect("undo all");
        assert_eq!(
            harness.project.state_hash().unwrap(),
            empty,
            "seed {seed} did not empty out"
        );

        harness
            .history
            .redo_all(&mut harness.project)
            .expect("redo all");
        assert_eq!(
            harness.project.state_hash().unwrap(),
            peak,
            "seed {seed} did not return to its peak"
        );
    }
}

#[test]
fn different_seeds_explore_genuinely_different_projects() {
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..8u64 {
        let harness = Harness::random_project(0x1000 + seed, 200).expect("fuzz run");
        assert!(
            seen.insert(harness.project.document_hash().unwrap()),
            "seed {seed} produced a project identical to an earlier seed"
        );
    }
}

#[test]
fn the_fuzz_actually_exercises_every_command_kind_it_generates() {
    // A generator that never produces, say, a removal would make the whole suite
    // weaker without failing. This test pins the coverage.
    let mut harness = Harness::new(0xABCD_EF01);
    harness.bootstrap().expect("bootstrap");
    let mut kinds = std::collections::BTreeSet::new();
    for _ in 0..4_000 {
        let Some(command) = harness.random_command() else {
            continue;
        };
        kinds.insert(command.kind());
        harness
            .history
            .apply_local(&mut harness.project, command)
            .expect("apply");
    }

    for expected in [
        "append_clip",
        "trim_clip",
        "move_clip",
        "set_clip_speed",
        "set_clip_enabled",
        "create_track",
        "remove_clip",
        "create_media_asset",
    ] {
        assert!(
            kinds.contains(expected),
            "the generator never produced `{expected}`; kinds seen: {kinds:?}"
        );
    }
}

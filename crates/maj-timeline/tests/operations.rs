//! What the four trims promise, checked against a real timeline.
//!
//! Each operation is asserted on three things, because an editorial operation that gets any
//! one of them wrong is worse than one that does not exist:
//!
//! * **the shape** — what moved and, just as much, what did not;
//! * **one step** — the whole operation undoes with one press, because
//!   [`History::apply_group`] is what applies it;
//! * **all or nothing** — a refused operation leaves the document byte-identical, which is
//!   checked against the document hash rather than by looking at the clips.

use maj_core::{
    Clip, ClipId, Command, FrameRange, History, MediaAsset, MediaAssetId, Project, Rational,
    Sequence, SequenceId, Timebase, Track, TrackId, TrackKind,
};
use maj_timeline::{Edge, EditError, close_gap, insert_blank, ripple, roll, slide, slip};

const SEQUENCE: SequenceId = SequenceId::new(1);
const TRACK: TrackId = TrackId::new(2);
const ASSET: MediaAssetId = MediaAssetId::new(3);

/// Three clips of a hundred frames each, laid end to end from zero, reading a source long
/// enough that every trim in these tests has somewhere to go.
fn fixture() -> (Project, History) {
    let mut project = Project::new("cut");
    let mut history = History::new();
    history
        .apply_local(
            &mut project,
            Command::CreateMediaAsset {
                index: 0,
                asset: MediaAsset::new(
                    ASSET,
                    "shot.mov",
                    "file:///shot.mov",
                    Timebase::FPS_25,
                    10_000,
                )
                .unwrap(),
            },
        )
        .unwrap();
    history
        .apply_local(
            &mut project,
            Command::CreateSequence {
                index: 0,
                sequence: Sequence::new(SEQUENCE, "Reel A", Timebase::FPS_25, 1920, 1080, 48_000)
                    .unwrap(),
            },
        )
        .unwrap();
    history
        .apply_local(
            &mut project,
            Command::CreateTrack {
                sequence: SEQUENCE,
                index: 0,
                track: Track::new(TRACK, TrackKind::Video, "V1"),
            },
        )
        .unwrap();
    for (id, start) in [(10_u64, 0_i64), (11, 100), (12, 200)] {
        let source = 1_000 * (i64::try_from(id).unwrap() - 9);
        history
            .apply_local(
                &mut project,
                Command::AppendClip {
                    sequence: SEQUENCE,
                    track: TRACK,
                    clip: Clip::new(
                        ClipId::new(id),
                        ASSET,
                        format!("c{id}"),
                        // Each clip reads a different, well-separated part of the source, so a
                        // slip has room in both directions.
                        FrameRange::new(source, source + 100).unwrap(),
                        start,
                        Rational::from_integer(1),
                    )
                    .unwrap(),
                },
            )
            .unwrap();
    }
    (project, history)
}

/// Where every clip sits and how long it is, in timeline frames.
fn layout(project: &Project) -> Vec<(u64, i64, i64)> {
    let track = &project.sequence(SEQUENCE).unwrap().tracks()[0];
    track
        .clips_in_timeline_order()
        .into_iter()
        .map(|clip| {
            (
                clip.id().raw(),
                clip.timeline_start(),
                clip.timeline_length(),
            )
        })
        .collect()
}

/// The source window each clip reads.
fn windows(project: &Project) -> Vec<(u64, i64, i64)> {
    let track = &project.sequence(SEQUENCE).unwrap().tracks()[0];
    track
        .clips_in_timeline_order()
        .into_iter()
        .map(|clip| {
            (
                clip.id().raw(),
                clip.source_range().start(),
                clip.source_range().end(),
            )
        })
        .collect()
}

/// Applies a composed operation as one step, and asserts it undoes as one.
fn apply_as_one_step(project: &mut Project, history: &mut History, commands: Vec<Command>) {
    let before = project.state_hash().unwrap();
    let steps = history.undo_depth();
    history.apply_group_local(project, commands).unwrap();
    assert_eq!(
        history.undo_depth(),
        steps + 1,
        "however many commands it took, it is one thing the person did"
    );
    assert!(project.is_valid());
    let after = project.state_hash().unwrap();
    assert_ne!(after, before, "the operation did nothing");

    history.undo(project).unwrap();
    assert_eq!(
        project.state_hash().unwrap(),
        before,
        "one undo must put the whole operation back"
    );
    history.redo(project).unwrap();
    assert_eq!(project.state_hash().unwrap(), after);
}

#[test]
fn a_ripple_moves_an_edge_and_everything_after_it() {
    let (mut project, mut history) = fixture();
    let commands = ripple(&project, SEQUENCE, ClipId::new(10), Edge::Tail, 10).unwrap();
    assert_eq!(
        commands.len(),
        3,
        "one trim and two clips pushed along: {commands:?}"
    );
    apply_as_one_step(&mut project, &mut history, commands);

    assert_eq!(
        layout(&project),
        vec![(10, 0, 110), (11, 110, 100), (12, 210, 100)],
        "the first clip grew by ten and the rest moved by ten, with no gap"
    );
    // The whole point of a ripple rather than a roll: the sequence is longer now.
    assert_eq!(
        layout(&project).last().map(|(_, start, len)| start + len),
        Some(310)
    );
}

#[test]
fn a_head_ripple_takes_frames_off_the_front_without_sliding_the_clip() {
    let (mut project, mut history) = fixture();
    // Trimming ten frames off the head of the middle clip: it starts later on the timeline
    // *and* later in its source, so the frames are removed rather than the clip sliding.
    let commands = ripple(&project, SEQUENCE, ClipId::new(11), Edge::Head, 10).unwrap();
    apply_as_one_step(&mut project, &mut history, commands);

    assert_eq!(
        layout(&project),
        vec![(10, 0, 100), (11, 110, 90), (12, 190, 100)],
        "the clip lost ten from its front and the one after it came back by ten"
    );
    assert_eq!(
        windows(&project)[1],
        (11, 2_010, 2_100),
        "the in point moved with the edge"
    );
}

#[test]
fn a_roll_moves_the_cut_and_nothing_else() {
    let (mut project, mut history) = fixture();
    let before_end = layout(&project).last().map(|(_, s, l)| s + l).unwrap();
    let commands = roll(&project, SEQUENCE, ClipId::new(10), Edge::Tail, 10).unwrap();
    assert_eq!(commands.len(), 2, "a roll is two trims and no moves");
    apply_as_one_step(&mut project, &mut history, commands);

    assert_eq!(
        layout(&project),
        vec![(10, 0, 110), (11, 110, 90), (12, 200, 100)],
        "one clip gained what the other lost; the third did not move"
    );
    assert_eq!(
        layout(&project).last().map(|(_, s, l)| s + l),
        Some(before_end),
        "a roll does not change the duration of the sequence"
    );
}

#[test]
fn a_slip_changes_what_is_seen_and_not_where_it_sits() {
    let (mut project, mut history) = fixture();
    let before = layout(&project);
    let commands = slip(&project, SEQUENCE, ClipId::new(11), 5).unwrap();
    assert_eq!(commands.len(), 1, "a slip touches one clip");
    apply_as_one_step(&mut project, &mut history, commands);

    assert_eq!(
        layout(&project),
        before,
        "nothing on the timeline moved, which is the whole purpose"
    );
    assert_eq!(
        windows(&project)[1],
        (11, 2_005, 2_105),
        "the window slid through the media, keeping its size"
    );
}

#[test]
fn a_slide_moves_a_clip_and_its_neighbours_absorb_it() {
    let (mut project, mut history) = fixture();
    let before_end = layout(&project).last().map(|(_, s, l)| s + l).unwrap();
    let before_window = windows(&project)[1];
    let commands = slide(&project, SEQUENCE, ClipId::new(11), 10).unwrap();
    assert_eq!(commands.len(), 3, "the clip and both neighbours");
    apply_as_one_step(&mut project, &mut history, commands);

    assert_eq!(
        layout(&project),
        vec![(10, 0, 110), (11, 110, 100), (12, 210, 90)],
        "the clip moved later; the one before grew and the one after shrank"
    );
    assert_eq!(
        windows(&project)[1],
        before_window,
        "a slide does not change which frames the clip shows"
    );
    assert_eq!(
        layout(&project).last().map(|(_, s, l)| s + l),
        Some(before_end),
        "and the sequence is the same length"
    );
}

#[test]
fn a_gap_is_closed_by_pulling_everything_after_it_back() {
    let (mut project, mut history) = fixture();
    // Make a gap by moving the last clip later, then close it.
    history
        .apply_local(
            &mut project,
            Command::MoveClip {
                sequence: SEQUENCE,
                clip: ClipId::new(12),
                timeline_start: 260,
            },
        )
        .unwrap();

    let commands = close_gap(&project, SEQUENCE, TRACK, 230).unwrap();
    apply_as_one_step(&mut project, &mut history, commands);
    assert_eq!(
        layout(&project),
        vec![(10, 0, 100), (11, 100, 100), (12, 200, 100)],
        "the gap is gone and nothing else moved"
    );

    // A frame that a clip covers is not a gap, and saying so is better than guessing which
    // nearby gap was meant.
    assert_eq!(
        close_gap(&project, SEQUENCE, TRACK, 150),
        Err(EditError::NoGap { at: 150 })
    );
}

#[test]
fn blank_space_pushes_everything_from_a_frame_onwards() {
    let (mut project, mut history) = fixture();
    let commands = insert_blank(&project, SEQUENCE, TRACK, 100, 50).unwrap();
    apply_as_one_step(&mut project, &mut history, commands);
    assert_eq!(
        layout(&project),
        vec![(10, 0, 100), (11, 150, 100), (12, 250, 100)],
        "the first clip stayed; the rest moved by fifty"
    );
    assert_eq!(
        insert_blank(&project, SEQUENCE, TRACK, 0, 0).unwrap(),
        Vec::new(),
        "inserting nothing is nothing, not an empty edit"
    );
}

#[test]
fn a_refused_operation_leaves_the_document_untouched() {
    let (project, _) = fixture();
    let before = project.document_hash().unwrap();

    // Every refusal, and each one names what is wrong rather than returning a bare error.
    assert_eq!(
        ripple(&project, SEQUENCE, ClipId::new(99), Edge::Tail, 1),
        Err(EditError::NoClip(ClipId::new(99)))
    );
    assert_eq!(
        roll(&project, SEQUENCE, ClipId::new(10), Edge::Head, 5),
        Err(EditError::NoNeighbour { edge: Edge::Head }),
        "the first clip has nothing before it to roll against"
    );
    assert_eq!(
        slide(&project, SEQUENCE, ClipId::new(10), 5),
        Err(EditError::NoNeighbour { edge: Edge::Head })
    );
    assert!(matches!(
        ripple(&project, SEQUENCE, ClipId::new(10), Edge::Tail, -100),
        Err(EditError::WouldEmpty)
    ));
    assert!(matches!(
        slip(&project, SEQUENCE, ClipId::new(10), -2_000),
        Err(EditError::PastSource { .. })
    ));

    assert_eq!(
        project.document_hash().unwrap(),
        before,
        "composing is a pure read: a refusal cannot have changed anything"
    );
}

#[test]
fn an_amount_that_is_not_a_whole_source_frame_is_refused_rather_than_rounded() {
    // The project's discipline applied where it is least convenient. A clip at half speed
    // shows two timeline frames for every source frame, so an odd number of timeline frames
    // lands between two source frames — and there is no honest answer to which one.
    let (mut project, mut history) = fixture();
    history
        .apply_local(
            &mut project,
            Command::SetClipSpeed {
                sequence: SEQUENCE,
                clip: ClipId::new(11),
                speed: Rational::new(1, 2).unwrap(),
            },
        )
        .unwrap();

    let refusal = ripple(&project, SEQUENCE, ClipId::new(11), Edge::Tail, 3);
    assert!(
        matches!(refusal, Err(EditError::NotOnAFrame { by: 3, .. })),
        "{refusal:?}"
    );
    // The refusal says what would work: an even number of timeline frames does.
    assert!(ripple(&project, SEQUENCE, ClipId::new(11), Edge::Tail, 4).is_ok());

    // And the message names the speed, so a person can act on it.
    let said = refusal.unwrap_err().to_string();
    assert!(said.contains("1/2"), "{said}");
    assert!(said.contains("drift"), "{said}");
}

#[test]
fn moving_an_edge_by_nothing_is_nothing() {
    // Not an empty edit that moves the revision: no commands at all, so a drag that ends
    // where it started appends nothing to the log.
    let (project, _) = fixture();
    for commands in [
        ripple(&project, SEQUENCE, ClipId::new(11), Edge::Tail, 0).unwrap(),
        roll(&project, SEQUENCE, ClipId::new(11), Edge::Tail, 0).unwrap(),
        slip(&project, SEQUENCE, ClipId::new(11), 0).unwrap(),
        slide(&project, SEQUENCE, ClipId::new(11), 0).unwrap(),
    ] {
        assert!(commands.is_empty());
    }
}

#[test]
fn every_operation_leaves_the_track_without_a_gap_or_an_overlap() {
    // The invariant that makes these operations worth having rather than a sequence of
    // trims a person composes by hand. Run over a spread of amounts in both directions, so
    // it is a property of the operation and not of one number.
    for by in [-20, -7, -1, 1, 7, 20] {
        for (name, composed) in [
            ("ripple", {
                let (project, _) = fixture();
                ripple(&project, SEQUENCE, ClipId::new(11), Edge::Tail, by)
            }),
            ("roll", {
                let (project, _) = fixture();
                roll(&project, SEQUENCE, ClipId::new(11), Edge::Tail, by)
            }),
            ("slide", {
                let (project, _) = fixture();
                slide(&project, SEQUENCE, ClipId::new(11), by)
            }),
        ] {
            let (mut project, mut history) = fixture();
            let commands = composed.unwrap_or_else(|error| panic!("{name} by {by}: {error}"));
            history.apply_group_local(&mut project, commands).unwrap();
            assert!(project.is_valid(), "{name} by {by} left an invalid project");

            let laid = layout(&project);
            for pair in laid.windows(2) {
                let (_, start, length) = pair[0];
                let (id, next_start, _) = pair[1];
                assert_eq!(
                    start + length,
                    next_start,
                    "{name} by {by} left a gap or an overlap before clip#{id}: {laid:?}"
                );
            }
        }
    }
}

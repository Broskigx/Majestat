//! The model's promises, tested through the public API.
//!
//! Three of them, and each one is the kind that fails silently if it is not checked:
//!
//! * every kind of command the interface can name is a kind the model actually has;
//! * every editing intention becomes the command it says it becomes;
//! * the state is still a function of the log after the interface has driven it.

use maj_core::{
    Clip, ClipId, Command, EffectId, EffectInstance, EffectKind, Interpolation, Keyframe, Mask,
    MaskId, MaskShape, ParamValue, TrackKind, Transition, TransitionId, TransitionKind, format,
};
use maj_render::{TitleAlign, TitleFace};
use maj_time::{Rational, Timebase};
use maj_ui::{
    Action, AnimChannel, Binding, Context, Draft, KINDS, NOTHING_ANIMATED, Shell, UiError, kind_of,
};

/// A shell holding the example project, with its first clip selected at frame 0.
fn demo() -> Shell {
    let mut shell = Shell::example("model tests").expect("the example builds");
    let clip = first_clip(&shell);
    shell
        .dispatch(Action::SelectClip(clip))
        .expect("a clip is selectable");
    shell
}

/// The first clip of the first video track.
fn first_clip(shell: &Shell) -> ClipId {
    shell
        .sequence_entity()
        .expect("an active sequence")
        .tracks()
        .iter()
        .filter(|track| track.kind() == TrackKind::Video)
        .flat_map(|track| track.clips())
        .next()
        .expect("a clip on the first video track")
        .id()
}

/// How many clips the active sequence holds.
fn clip_count(shell: &Shell) -> usize {
    shell
        .sequence_entity()
        .expect("an active sequence")
        .tracks()
        .iter()
        .map(|track| track.clips().len())
        .sum()
}

/// The first video track of the active sequence.
fn first_video_track(shell: &Shell) -> maj_core::TrackId {
    shell
        .sequence_entity()
        .expect("an active sequence")
        .tracks()
        .iter()
        .find(|track| track.kind() == TrackKind::Video)
        .expect("a video track")
        .id()
}

/// The context the demo's state implies.
fn context(shell: &Shell) -> Context<'_> {
    Context::new(
        shell.project(),
        shell.active_sequence(),
        shell.selection(),
        shell.playhead(),
    )
}

/// The kinds of command an intention becomes.
fn kinds(action: &Action, shell: &Shell) -> Vec<&'static str> {
    action
        .commands(&context(shell))
        .unwrap_or_else(|error| panic!("{action:?} should apply here: {error}"))
        .iter()
        .map(kind_of)
        .collect()
}

#[test]
fn every_editing_intention_becomes_the_command_it_names() {
    let mut shell = demo();
    // A clip cannot move onto an audio track, and the example ships exactly one video
    // track, so the reparent case needs a second video track to have somewhere to go.
    shell
        .dispatch(Action::AddTrack(TrackKind::Video))
        .expect("a second video track");
    let clip = first_clip(&shell);
    let asset = shell.project().assets().first().expect("an asset").id();
    let sequence = shell.active_sequence().expect("a sequence");

    let cases: Vec<(Action, &str)> = vec![
        (Action::RemoveSelected, "remove_clip"),
        (Action::ToggleSelectedEnabled, "set_clip_enabled"),
        (
            Action::SetSelectedOpacity(Rational::from_reduced(1, 2)),
            "set_clip_opacity",
        ),
        (
            Action::SetSelectedTransform(maj_ui::TransformField::ScaleX, Rational::from_integer(2)),
            "set_clip_transform",
        ),
        (
            Action::SetSelectedSpeed(Rational::from_integer(2)),
            "set_clip_speed",
        ),
        (Action::AddEffect(EffectKind::Vignette), "add_effect"),
        (Action::AddMask(MaskShape::Ellipse), "add_mask"),
        (Action::MoveSelected(3), "move_clip"),
        (Action::MoveSelectedToTrack(2), "reparent_clip"),
        (Action::TrimSelectedEnd(10), "trim_clip"),
        (Action::TrimSelectedStart(0), "trim_clip"),
        (Action::AddTrack(TrackKind::Video), "create_track"),
        (
            Action::CreateMediaAsset {
                name: "b-roll".to_owned(),
                uri: "generated:b-roll".to_owned(),
                timebase: Timebase::FPS_24,
                duration_frames: 24,
                has_video: true,
                has_audio: false,
            },
            "create_media_asset",
        ),
        (Action::RemoveMediaAsset(asset), "remove_media_asset"),
        (Action::InsertMediaAtPlayhead(asset), "insert_clip"),
        (Action::AppendMedia(asset), "append_clip"),
        (
            Action::CreateSequence {
                name: "Second".to_owned(),
                timebase: Timebase::FPS_25,
                width: 1920,
                height: 1080,
                sample_rate: 48_000,
            },
            "create_sequence",
        ),
        (Action::RemoveSequence(sequence), "remove_sequence"),
        (
            Action::RemoveTrack(
                shell
                    .sequence_entity()
                    .expect("a sequence")
                    .tracks()
                    .first()
                    .expect("a track")
                    .id(),
            ),
            "remove_track",
        ),
        (
            Action::AddTransition(TransitionKind::CrossDissolve, 12),
            "add_transition",
        ),
        (
            Action::ToggleKeyframe(AnimChannel::Opacity),
            "set_channel_keys",
        ),
    ];

    for (action, expected) in cases {
        let produced = kinds(&action, &shell);
        assert_eq!(
            produced,
            vec![expected],
            "{action:?} should be one {expected}"
        );
    }

    // The intention that is more than one command, because an insert is not a copy: the clip
    // has to exist before its look can be assigned to it. The clip needs a look first, or a
    // bare insert would be the whole truth.
    shell
        .dispatch(Action::AddEffect(EffectKind::Vignette))
        .expect("an effect on the selected clip");
    shell
        .dispatch(Action::SetSelectedOpacity(Rational::from_reduced(1, 2)))
        .expect("a half-opaque clip");
    let duplicate = kinds(&Action::DuplicateSelected, &shell);
    assert_eq!(duplicate.first(), Some(&"insert_clip"));
    assert!(
        duplicate.contains(&"add_effect") && duplicate.contains(&"set_clip_opacity"),
        "a copy carries its look across: {duplicate:?}"
    );

    let clip_at_cut = shell.project().clip(clip).expect("the clip");
    assert_eq!(clip_at_cut.speed(), Rational::from_integer(1));

    // Clearing a channel is the one editing intention with nothing to say until a curve exists,
    // so it is checked here rather than in the table above: an intention that has nothing to
    // clear refuses instead of writing an empty list.
    assert!(
        Action::ClearChannel(AnimChannel::Opacity)
            .commands(&context(&shell))
            .is_err(),
        "a channel with no curve has nothing to clear"
    );
    shell
        .dispatch(Action::ToggleKeyframe(AnimChannel::Opacity))
        .expect("a key to clear");
    assert_eq!(
        kinds(&Action::ClearChannel(AnimChannel::Opacity), &shell),
        vec!["set_channel_keys"]
    );
}

#[test]
fn a_keyframe_records_the_value_the_picture_already_has() {
    let mut shell = demo();
    let clip = first_clip(&shell);
    shell
        .dispatch(Action::SetSelectedOpacity(Rational::from_reduced(1, 2)))
        .expect("a static opacity");

    // Nothing is animated, so the diamond records what the clip already shows. It has to: a key
    // that recorded anything else would move the picture the moment it was placed.
    let report = shell
        .dispatch(Action::ToggleKeyframe(AnimChannel::Opacity))
        .expect("a keyframe");
    assert_eq!(report.commands, vec!["set_channel_keys"]);
    let animated = shell.project().clip(clip).expect("the clip");
    let curve = animated
        .animation()
        .iter()
        .find(|animation| animation.channel() == AnimChannel::Opacity)
        .expect("a curve on the opacity");
    assert_eq!(curve.keys().len(), 1);
    assert_eq!(curve.keys()[0].offset(), 0);
    assert_eq!(
        curve.keys()[0].value().as_rational(),
        Some(Rational::from_reduced(1, 2))
    );
    assert_eq!(animated.opacity(), Rational::from_reduced(1, 2));

    // The same diamond again takes the key away, and the last key takes the curve with it: the
    // channel is a static value again, which is what the clip's own field still holds.
    shell
        .dispatch(Action::ToggleKeyframe(AnimChannel::Opacity))
        .expect("the key goes away");
    let static_again = shell.project().clip(clip).expect("the clip");
    assert!(static_again.animation().is_empty());
    assert_eq!(static_again.opacity(), Rational::from_reduced(1, 2));
}

#[test]
fn a_value_written_on_an_animated_channel_becomes_a_key_and_moves_the_picture() {
    let mut shell = demo();
    let clip = first_clip(&shell);
    shell
        .dispatch(Action::SetDraft(Draft::Quarter))
        .expect("fast");

    // One key at the head, then half speed at the picture a second later. The second write is a
    // keyframe rather than the clip's own field, because the field is what the compositor only
    // reads when there is no curve — writing it here would be a control that did nothing.
    shell
        .dispatch(Action::ToggleKeyframe(AnimChannel::Opacity))
        .expect("a key at the head");
    shell.dispatch(Action::SeekTo(30)).expect("moved");
    let report = shell
        .dispatch(Action::SetSelectedOpacity(Rational::from_reduced(1, 2)))
        .expect("a second value");
    assert_eq!(report.commands, vec!["set_channel_keys"]);
    let animated = shell.project().clip(clip).expect("the clip");
    let curve = animated
        .animation()
        .iter()
        .find(|animation| animation.channel() == AnimChannel::Opacity)
        .expect("a curve");
    assert_eq!(curve.keys().len(), 2);
    assert_eq!(curve.keys()[1].offset(), 30);
    assert_eq!(
        curve.keys()[1].value().as_rational(),
        Some(Rational::from_reduced(1, 2))
    );
    assert_eq!(
        animated.opacity(),
        Rational::from_integer(1),
        "the static value is not what moved"
    );

    // And the picture really moves: two frames of one clip, one curve apart, are different
    // renders. This is the whole point of the feature, so it is asserted on pixels rather than
    // on the shape of the log.
    let head = shell.render_frame_at(0, 96, 54).expect("the head");
    let later = shell
        .render_frame_at(30, 96, 54)
        .expect("the same frame as the key");
    assert_ne!(
        head.digest(),
        later.digest(),
        "a keyframed opacity has to change what is drawn"
    );
}

#[test]
fn stepping_walks_from_key_to_key_and_stops_at_the_ends() {
    let mut shell = demo();
    shell
        .dispatch(Action::ToggleKeyframe(AnimChannel::Opacity))
        .expect("a key at the head");
    shell.dispatch(Action::SeekTo(30)).expect("moved");
    shell
        .dispatch(Action::SetSelectedOpacity(Rational::from_reduced(1, 4)))
        .expect("a key at 30");
    shell.dispatch(Action::SeekTo(10)).expect("moved");

    // Looking along the keys is view state, like looking along the cuts: the revision holds.
    let revision = shell.project().revision();
    shell
        .perform(Binding::StepKeyframe(1))
        .expect("a key ahead");
    assert_eq!(shell.playhead(), 30);
    shell
        .perform(Binding::StepKeyframe(-1))
        .expect("a key behind");
    assert_eq!(shell.playhead(), 0);
    assert_eq!(shell.project().revision(), revision);
    assert!(
        shell.perform(Binding::StepKeyframe(-1)).is_err(),
        "before the first key there is nothing to step to"
    );
    shell.dispatch(Action::SeekTo(30)).expect("moved");
    assert!(
        shell.perform(Binding::StepKeyframe(1)).is_err(),
        "past the last key there is nothing to step to"
    );
}

#[test]
fn the_keyframe_strokes_key_and_clear_every_animated_channel() {
    let mut shell = demo();
    // Nothing is animated, so the gesture refuses rather than inventing a curve to put a key on.
    let error = shell
        .perform(Binding::ToggleKeyframes)
        .expect_err("nothing to key");
    assert_eq!(error, UiError::NotApplicable(NOTHING_ANIMATED));

    shell
        .dispatch(Action::ToggleKeyframe(AnimChannel::Opacity))
        .expect("an animated channel");
    shell.dispatch(Action::SeekTo(10)).expect("moved");
    shell
        .perform(Binding::ToggleKeyframes)
        .expect("a key on every animated channel");
    let clip = first_clip(&shell);
    let animated = shell.project().clip(clip).expect("the clip");
    let curve = animated
        .animation()
        .iter()
        .find(|animation| animation.channel() == AnimChannel::Opacity)
        .expect("a curve");
    assert_eq!(curve.keys().len(), 2, "the key landed at frame 10");

    shell
        .perform(Binding::ClearKeyframes)
        .expect("every curve goes");
    let cleared = shell.project().clip(clip).expect("the clip");
    assert!(cleared.animation().is_empty());
    assert_eq!(cleared.opacity(), Rational::from_integer(1));
}

#[test]
fn the_name_of_a_command_is_the_name_the_document_stores() {
    // One of every variant, so the interface's vocabulary is compared with the model's own
    // serialisation rather than with a list someone typed twice.
    let shell = demo();
    let sequence = shell.active_sequence().expect("a sequence");
    let entity = shell.sequence_entity().expect("a sequence");
    let track = entity.tracks().first().expect("a track").id();
    let clip = first_clip(&shell);
    let asset = shell.project().assets().first().expect("an asset").clone();

    let mask = Mask::new(MaskId::new(900), MaskShape::Rectangle).expect("a mask");
    let transition = Transition::new(
        TransitionId::new(901),
        TransitionKind::DipToBlack,
        0,
        10,
        None,
        None,
    )
    .expect("a transition");
    let effect = EffectInstance::new(EffectId::new(902), EffectKind::Blur);
    let transition_id = transition.id();

    let every = vec![
        Command::SetChannelKeys {
            sequence,
            clip,
            channel: maj_core::AnimChannel::Opacity,
            keys: vec![Keyframe::new(
                0,
                ParamValue::Rational(Rational::from_integer(1)),
                Interpolation::Linear,
            )],
        },
        Command::AddMask {
            sequence,
            clip,
            index: 0,
            mask,
        },
        Command::RemoveMask {
            sequence,
            clip,
            mask: mask.id(),
        },
        Command::SetMask {
            sequence,
            clip,
            mask: mask.id(),
            value: mask,
        },
        Command::AddTransition {
            sequence,
            track,
            index: 0,
            transition,
        },
        Command::RemoveTransition {
            sequence,
            track,
            transition: transition_id,
        },
        Command::SetClipTransform {
            sequence,
            clip,
            transform: maj_core::Transform::identity(),
        },
        Command::SetClipOpacity {
            sequence,
            clip,
            opacity: Rational::from_integer(1),
        },
        Command::SetClipBlend {
            sequence,
            clip,
            blend: maj_core::BlendMode::Normal,
        },
        Command::AddEffect {
            sequence,
            clip,
            index: 0,
            effect: effect.clone(),
        },
        Command::RemoveEffect {
            sequence,
            clip,
            effect: effect.id(),
        },
        Command::SetEffectEnabled {
            sequence,
            clip,
            effect: effect.id(),
            enabled: false,
        },
        Command::SetEffectParam {
            sequence,
            clip,
            effect: effect.id(),
            key: "radius".to_owned(),
            value: ParamValue::Rational(Rational::from_integer(2)),
        },
        Command::CreateMediaAsset {
            index: 0,
            asset: asset.clone(),
        },
        Command::RemoveMediaAsset { id: asset.id() },
        Command::InsertMediaAsset { index: 0, asset },
        Command::CreateSequence {
            index: 0,
            sequence: entity.clone(),
        },
        Command::RemoveSequence { id: sequence },
        Command::InsertSequence {
            index: 0,
            sequence: entity.clone(),
        },
        Command::CreateTrack {
            sequence,
            index: 0,
            track: entity.tracks().first().expect("a track").clone(),
        },
        Command::RemoveTrack { sequence, track },
        Command::AppendClip {
            sequence,
            track,
            clip: entity
                .tracks()
                .first()
                .and_then(|track| track.clips().first())
                .expect("a clip")
                .clone(),
        },
        Command::InsertClip {
            sequence,
            track,
            index: 0,
            clip: entity
                .tracks()
                .first()
                .and_then(|track| track.clips().first())
                .expect("a clip")
                .clone(),
        },
        Command::RemoveClip { sequence, clip },
        Command::TrimClip {
            sequence,
            clip,
            source_range: maj_time::FrameRange::new(0, 24).expect("a range"),
            timeline_start: 0,
        },
        Command::MoveClip {
            sequence,
            clip,
            timeline_start: 24,
        },
        Command::ReparentClip {
            sequence,
            clip,
            track,
            index: 0,
        },
        Command::SetClipSpeed {
            sequence,
            clip,
            speed: Rational::from_integer(1),
        },
        Command::SetClipEnabled {
            sequence,
            clip,
            enabled: true,
        },
        Command::SetTrackEnabled {
            sequence,
            track,
            enabled: true,
        },
    ];

    assert_eq!(every.len(), 30, "the model has thirty commands");
    let mut seen = std::collections::BTreeSet::new();
    for command in &every {
        let json = serde_json::to_value(command).expect("a command serialises");
        let stored = json["kind"].as_str().expect("the log tags every command");
        assert_eq!(
            kind_of(command),
            stored,
            "the interface calls {command:?} something the document does not"
        );
        assert!(
            seen.insert(stored.to_owned()),
            "{stored} was constructed twice"
        );
    }
    // And the interface's own list of names is that same set, name for name, so a variant
    // added to the model cannot quietly go missing from the interface's vocabulary.
    assert_eq!(
        seen,
        KINDS.iter().map(|kind| (*kind).to_owned()).collect(),
        "the interface's list of command names is not the model's"
    );
}

#[test]
fn splitting_a_clip_leaves_two_halves_that_cover_what_the_one_did() {
    let mut shell = demo();
    let clip = first_clip(&shell);
    let before = shell.project().clip(clip).expect("the clip").clone();

    shell.dispatch(Action::SeekTo(100)).expect("moved");
    let report = shell.dispatch(Action::SplitAtPlayhead).expect("split");
    assert_eq!(report.first(), Some("trim_clip"));
    assert_eq!(report.commands.len(), 2, "a split is a trim and an insert");
    assert!(shell.project().is_valid(), "the result is a valid document");

    let track = shell
        .sequence_entity()
        .expect("a sequence")
        .tracks()
        .iter()
        .find(|track| track.kind() == TrackKind::Video)
        .expect("a video track");
    assert_eq!(track.clips().len(), 4, "one clip became two");
    let head = track.clip(clip).expect("the head");
    let tail = track
        .clips()
        .iter()
        .find(|candidate| candidate.id() != clip && candidate.timeline_start() == 100)
        .expect("the tail");
    assert_eq!(head.timeline_range().expect("a range").end(), 100);
    assert_eq!(tail.timeline_start(), 100);
    assert_eq!(
        head.source_range().len() + tail.source_range().len(),
        before.source_range().len(),
        "the two halves use exactly the source the one clip used"
    );
    assert_eq!(
        tail.source_range().start(),
        before.source_range().start() + 100
    );
}

#[test]
fn a_copy_carries_the_look_and_the_effects_of_what_it_copied() {
    let mut shell = demo();
    let clip = first_clip(&shell);
    shell
        .dispatch(Action::SetSelectedOpacity(Rational::from_reduced(1, 2)))
        .expect("dimmed");
    shell
        .dispatch(Action::SetSelectedTransform(
            maj_ui::TransformField::X,
            Rational::from_integer(12),
        ))
        .expect("moved");
    shell
        .dispatch(Action::AddEffect(EffectKind::Exposure))
        .expect("an effect");
    shell
        .dispatch(Action::AddMask(MaskShape::Ellipse))
        .expect("a mask");

    let original = shell.project().clip(clip).expect("the clip").clone();
    shell.dispatch(Action::DuplicateSelected).expect("copied");

    let track = shell
        .sequence_entity()
        .expect("a sequence")
        .tracks()
        .iter()
        .find(|track| track.kind() == TrackKind::Video)
        .expect("a video track");
    let copy: Vec<&Clip> = track
        .clips()
        .iter()
        .filter(|candidate| candidate.id() != clip)
        .filter(|candidate| candidate.timeline_start() == original.timeline_length())
        .collect();
    let copy = copy.first().expect("the copy");
    assert_eq!(copy.source_range(), original.source_range());
    assert_eq!(copy.opacity(), original.opacity());
    assert_eq!(copy.transform(), original.transform());
    assert_eq!(copy.effects().len(), original.effects().len());
    assert_eq!(copy.masks().len(), original.masks().len());
    assert_ne!(
        copy.effects().first().expect("an effect").id(),
        original.effects().first().expect("an effect").id(),
        "a copy's effects are its own, with their own identifiers"
    );
    assert!(shell.project().is_valid());
}

#[test]
fn the_engine_refuses_a_trim_beyond_the_media_and_the_interface_says_so() {
    let mut shell = demo();
    // The asset is 240 frames long, so a tail at 400 asks for source that does not exist.
    let refused = shell.dispatch(Action::TrimSelectedEnd(400));
    assert!(
        matches!(refused, Err(maj_ui::UiError::Refused(_))),
        "the model's own refusal is what comes back, not a friendly paraphrase: {refused:?}"
    );
    assert_eq!(
        shell
            .project()
            .clip(first_clip(&shell))
            .expect("the clip")
            .source_range(),
        maj_time::FrameRange::new(0, 240).expect("a range")
    );
}

#[test]
fn the_state_is_still_a_function_of_the_log_after_the_interface_has_driven_it() {
    let mut shell = demo();
    let clip = first_clip(&shell);
    for action in [
        Action::SeekTo(48),
        Action::SplitAtPlayhead,
        Action::MoveSelected(24),
        Action::SetSelectedOpacity(Rational::from_reduced(3, 4)),
        Action::AddEffect(EffectKind::Contrast),
        Action::AddMask(MaskShape::Rectangle),
        Action::ToggleSelectedEnabled,
        Action::AddTrack(TrackKind::Audio),
        Action::Undo,
        Action::Redo,
        Action::DuplicateSelected,
    ] {
        shell
            .dispatch(action.clone())
            .unwrap_or_else(|error| panic!("{action:?} should apply: {error}"));
    }
    assert!(shell.selection().is_some() || shell.project().clip(clip).is_some());

    // Replaying the log produces the state byte for byte, which is the claim the whole
    // architecture rests on and the reason the interface is only ever allowed to append.
    let log = shell.project().log().to_vec();
    let mut replayed = maj_core::Project::new(shell.project().name());
    format::replay_into(&mut replayed, &log).expect("the log replays");
    assert_eq!(
        replayed.state_hash().expect("a hash"),
        shell.project().state_hash().expect("a hash")
    );
    assert_eq!(replayed.log().len(), log.len());
}

/// The clip that begins at a frame, on any video track.
fn clip_starting_at(shell: &Shell, frame: i64) -> ClipId {
    shell
        .sequence_entity()
        .expect("a sequence")
        .tracks()
        .iter()
        .filter(|track| track.kind() == TrackKind::Video)
        .flat_map(|track| track.clips())
        .find(|clip| clip.timeline_start() == frame)
        .unwrap_or_else(|| panic!("no clip begins at {frame}"))
        .id()
}

/// Where a clip begins, or a panic naming it.
fn start_of(shell: &Shell, clip: ClipId) -> i64 {
    shell
        .project()
        .clip(clip)
        .unwrap_or_else(|| panic!("clip {clip:?} is gone"))
        .timeline_start()
}

#[test]
fn the_edit_points_are_the_cuts_and_stepping_lands_on_them() {
    let mut shell = demo();
    // The example is three 240-frame shots end to end at 24 fps, so the cuts are where the
    // shots meet, and the playhead starts on the first one.
    assert_eq!(shell.playhead(), 0);

    // Stepping forward finds the head of the second shot; stepping forward again finds the
    // third; the end of the last shot is the last cut, and past it there is nothing.
    for expected in [240, 480, 720] {
        shell
            .perform(Binding::StepEditPoint(1))
            .expect("there is another cut");
        assert_eq!(shell.playhead(), expected);
    }
    assert!(
        shell.perform(Binding::StepEditPoint(1)).is_err(),
        "past the last cut there is nothing to step to"
    );

    // Backwards walks the same cuts in reverse, and looking along them is view state: the
    // revision does not move.
    let revision = shell.project().revision();
    shell
        .perform(Binding::StepEditPoint(-1))
        .expect("there is a cut behind");
    assert_eq!(shell.playhead(), 480);
    assert_eq!(shell.project().revision(), revision);
    assert!(
        shell.selection().is_some(),
        "stepping along the cuts keeps what was selected"
    );
}

#[test]
fn selecting_the_next_clip_takes_it_and_moves_the_playhead_to_its_head() {
    let mut shell = demo();
    let second = clip_starting_at(&shell, 240);
    let third = clip_starting_at(&shell, 480);

    shell
        .perform(Binding::SelectAdjacent(1))
        .expect("there is a clip after this one");
    assert_eq!(shell.selection(), Some(second));
    assert_eq!(shell.playhead(), 240);

    shell
        .perform(Binding::SelectAdjacent(1))
        .expect("and another");
    assert_eq!(shell.selection(), Some(third));
    assert_eq!(shell.playhead(), 480);

    // Backwards from the third shot lands on the second, and the revision never moves.
    let revision = shell.project().revision();
    shell
        .perform(Binding::SelectAdjacent(-1))
        .expect("a clip behind");
    assert_eq!(shell.selection(), Some(second));
    assert_eq!(shell.playhead(), 240);
    assert_eq!(shell.project().revision(), revision);
}

#[test]
fn a_ripple_delete_removes_the_clip_and_pulls_the_rest_after_it() {
    let mut shell = demo();
    let second = clip_starting_at(&shell, 240);
    let third = clip_starting_at(&shell, 480);
    shell
        .dispatch(Action::SelectClip(second))
        .expect("selected");

    // The intention is a composition, and the composition is checkable: the clip goes, and the
    // clip that followed it moves onto the frames it vacated — the example's shots are
    // contiguous, so the shot after the hole lands exactly where the removed one began.
    assert_eq!(
        kinds(&Action::RippleDelete, &shell),
        vec!["remove_clip", "move_clip"]
    );
    shell
        .dispatch(Action::RippleDelete)
        .expect("the clip is removed");
    assert!(shell.project().clip(second).is_none());
    assert_eq!(start_of(&shell, third), 240);
    assert_eq!(start_of(&shell, clip_starting_at(&shell, 0)), 0);
}

#[test]
fn closing_a_gap_moves_the_clip_and_everything_after_it_left() {
    let mut shell = demo();
    let second = clip_starting_at(&shell, 240);
    let third = clip_starting_at(&shell, 480);

    // Open a hundred-frame hole in front of the second shot, which is what a person would do
    // before there was anything to close it with.
    shell
        .dispatch(Action::SelectClip(second))
        .expect("selected");
    shell.dispatch(Action::MoveSelected(100)).expect("moved");
    assert_eq!(start_of(&shell, second), 340);

    // Closing it pulls the shot back onto its neighbour and carries the rest of the track
    // along, in order from the head of the track.
    assert_eq!(
        kinds(&Action::CloseGap, &shell),
        vec!["move_clip", "move_clip"]
    );
    shell.dispatch(Action::CloseGap).expect("the gap closes");
    assert_eq!(start_of(&shell, second), 240);
    assert_eq!(start_of(&shell, third), 380);

    // And a clip that already touches what is before it has no gap to close, which is a
    // refusal rather than a silent nothing.
    assert!(Action::CloseGap.commands(&context(&shell)).is_err());
}

#[test]
fn inserting_a_blank_opens_a_second_where_the_playhead_is() {
    let mut shell = demo();
    let second = clip_starting_at(&shell, 240);
    let third = clip_starting_at(&shell, 480);
    shell
        .dispatch(Action::SelectClip(second))
        .expect("selected");

    // The playhead is inside the second shot, so the hole opens at that shot's head rather
    // than through the middle of it, and every clip that follows moves a second along.
    shell.dispatch(Action::SeekTo(300)).expect("moved");
    assert_eq!(
        kinds(&Action::InsertBlank, &shell),
        vec!["move_clip", "move_clip"]
    );
    shell.dispatch(Action::InsertBlank).expect("the hole opens");
    assert_eq!(start_of(&shell, second), 264);
    assert_eq!(start_of(&shell, third), 504);

    // Past the end of the sequence there is nothing to push, and saying so is the honest
    // answer rather than a command that moves nothing.
    shell.dispatch(Action::SeekTo(2_000)).expect("moved");
    assert!(Action::InsertBlank.commands(&context(&shell)).is_err());
}

#[test]
fn adding_a_title_registers_it_and_places_it_in_one_intention() {
    let mut shell = demo();
    let before = shell.project().assets().len();
    shell.dispatch(Action::SeekTo(120)).expect("moved");

    // A title is two commands, in this order: the pool learns about the asset, and the timeline
    // gets a clip of it. Both, or neither — a title that is registered and not placed is a
    // title a person has to go and find.
    let action = Action::AddTitle {
        text: "MAJESTAT".to_owned(),
        face: TitleFace::Serif,
        size: 36.0,
        align: TitleAlign::Left,
    };
    assert_eq!(
        kinds(&action, &shell),
        vec!["create_media_asset", "insert_clip"]
    );
    let report = shell.dispatch(action).expect("the title is set");
    assert_eq!(report.commands.len(), 2);

    assert_eq!(shell.project().assets().len(), before + 1);
    let asset = shell
        .project()
        .assets()
        .last()
        .expect("the title is in the pool");
    // The whole of a title is in its URI, which is what keeps a document a function of its
    // commands: the face, the cap height, the colour, the alignment and the words.
    assert_eq!(asset.uri(), "title:serif:36:ffffff:left:MAJESTAT");
    assert!(asset.has_video() && !asset.has_audio());
    assert!(asset.name().contains("MAJESTAT"));

    // And the clip is on the timeline at the playhead, as long as a still is held.
    let clip = shell
        .sequence_entity()
        .expect("a sequence")
        .tracks()
        .iter()
        .filter(|track| track.kind() == TrackKind::Video)
        .flat_map(|track| track.clips())
        .find(|clip| clip.asset_id() == asset.id())
        .expect("the title is on the first video track");
    assert_eq!(clip.timeline_start(), 120);
    assert_eq!(clip.timeline_length(), 5 * 24);

    // A title with no words is refused rather than registered empty, because the box the
    // alphabet draws for a letter it does not have is not a title either.
    for empty in ["", "   ", "\n"] {
        let refusal = Action::AddTitle {
            text: empty.to_owned(),
            face: TitleFace::Sans,
            size: 0.0,
            align: TitleAlign::Centre,
        }
        .commands(&context(&shell));
        assert!(refusal.is_err(), "{empty:?} is not a title");
    }
}

#[test]
fn one_intention_is_one_undo_step_however_many_commands_it_is() {
    // The model counts commands and a person counts actions, and undo belongs to the
    // person. Adding a title is two commands in the document — an asset, then a clip that
    // uses it — and one thing somebody did, so it has to come back with one press.
    let mut shell = demo();
    let (undo_before, _) = shell.depths();
    let revision = shell.project().revision();
    let assets = shell.project().assets().len();
    let clips = clip_count(&shell);

    let report = shell
        .dispatch(Action::AddTitle {
            text: "ONE STEP".to_owned(),
            face: TitleFace::Sans,
            size: 24.0,
            align: TitleAlign::Centre,
        })
        .expect("the title is set");

    assert_eq!(report.commands.len(), 2, "two commands reached the log");
    assert_eq!(
        shell.project().revision(),
        revision + 2,
        "the log grew by two: the document never learns that they were grouped"
    );
    assert_eq!(
        shell.depths().0,
        undo_before + 1,
        "and undo sees one step, not two"
    );

    shell.dispatch(Action::Undo).expect("one press undoes it");
    assert_eq!(
        shell.project().revision(),
        revision,
        "one press took both commands back"
    );
    assert_eq!(
        shell.project().assets().len(),
        assets,
        "the asset went with the clip, so the pool holds no orphan title"
    );
    assert_eq!(clip_count(&shell), clips);

    shell.dispatch(Action::Redo).expect("one press redoes it");
    assert_eq!(shell.project().revision(), revision + 2);
    assert_eq!(shell.project().assets().len(), assets + 1);
    assert_eq!(clip_count(&shell), clips + 1);
}

#[test]
fn importing_a_still_puts_it_in_the_pool_and_places_nothing() {
    let mut shell = demo();
    let clips = clip_count(&shell);

    // Importing is finding the material. Placing it is an edit, and this is not one: nothing
    // moves on the timeline, and the revision moves because the pool is part of the document.
    let action = Action::ImportMedia {
        path: std::path::PathBuf::from("C:/pictures/card.png"),
    };
    assert_eq!(kinds(&action, &shell), vec!["create_media_asset"]);
    let revision = shell.project().revision();
    shell.dispatch(action).expect("the still is registered");
    assert!(
        shell.project().revision() > revision,
        "the pool is the document"
    );
    assert_eq!(clip_count(&shell), clips, "importing places nothing");

    let asset = shell
        .project()
        .assets()
        .last()
        .expect("the still is in the pool");
    assert_eq!(asset.name(), "card.png", "the pool reads like the folder");
    assert_eq!(asset.uri(), "C:/pictures/card.png");
    // A still is held for five seconds at the active sequence's rate, which is what an editor
    // means by dropping a picture on a 24 fps timeline.
    assert_eq!(asset.duration_frames(), 5 * 24);
    assert_eq!(asset.timebase(), Timebase::FPS_24);

    // And it can be placed like anything else in the pool, which is the point of importing it.
    shell
        .dispatch(Action::DropMedia {
            asset: asset.id(),
            track: first_video_track(&shell),
            frame: 96,
        })
        .expect("the still is dropped");
    assert_eq!(clip_count(&shell), clips + 1);
}

#[test]
fn the_title_studio_is_view_state_until_a_title_is_added() {
    let mut shell = demo();
    let revision = shell.project().revision();
    assert!(shell.studio().text.is_empty());

    // Typing words, choosing a face and choosing a size change no document at all.
    shell
        .dispatch(Action::SetTitleText("Reel A".to_owned()))
        .expect("typed");
    shell
        .dispatch(Action::SetTitleFace(TitleFace::Condensed))
        .expect("chosen");
    shell.dispatch(Action::SetTitleSize(48.0)).expect("sized");
    shell
        .dispatch(Action::SetTitleAlign(TitleAlign::Right))
        .expect("aligned");
    assert_eq!(
        shell.project().revision(),
        revision,
        "a studio is not a document"
    );
    assert_eq!(shell.studio().text, "Reel A");
    assert_eq!(shell.title_size(), 48.0);

    // The URI the studio would hand the renderer is readable, and it is what the form shows.
    assert_eq!(
        shell.title_uri(),
        "title:condensed:48:ffffff:right:Reel A",
        "the form and the renderer agree on what a title is"
    );

    // A size of zero means the sequence's own tenth, which is a title card's proportion.
    shell
        .dispatch(Action::SetTitleSize(0.0))
        .expect("defaulted");
    assert_eq!(
        shell.title_size(),
        18.0,
        "180 pixels of frame, a tenth of it"
    );

    // And adding it is the one thing here that writes.
    shell.dispatch(shell.add_title()).expect("the title is set");
    assert!(shell.project().revision() > revision);
}

#[test]
fn a_draft_says_what_it_is_and_the_viewer_asks_for_that_size() {
    let mut shell = demo();
    let sequence = shell.sequence_entity().expect("a sequence");
    let (full_width, full_height) = (sequence.width(), sequence.height());

    let full = shell.preview_request().expect("a request");
    assert_eq!((full.width, full.height), (full_width, full_height));

    shell
        .dispatch(Action::SetDraft(Draft::Quarter))
        .expect("drafted");
    let quarter = shell.preview_request().expect("a request");
    assert_eq!(
        (quarter.width, quarter.height),
        (full_width.div_ceil(4), full_height.div_ceil(4))
    );
    assert_eq!(shell.draft().key(), "quarter");
    assert!(shell.preview_key().width == quarter.width);
}

#[test]
fn the_viewer_and_the_export_agree_on_the_pixels_of_a_frame() {
    let mut shell = demo();
    shell.dispatch(Action::SeekTo(30)).expect("moved");
    shell.dispatch(Action::SetDraft(Draft::Full)).expect("full");

    let preview = shell.render_preview().expect("a preview");
    let explicit = shell
        .render_frame_at(30, preview.request.width, preview.request.height)
        .expect("the same frame");
    assert_eq!(
        preview.digest(),
        explicit.digest(),
        "the viewer and a direct render are the same function of the same request"
    );

    // And the tiled path, which the interface offers as a switch, agrees with the untiled one.
    shell.dispatch(Action::SetTiled(true)).expect("tiled");
    let tiled = shell.render_preview().expect("a tiled preview");
    assert_eq!(
        tiled.digest(),
        preview.digest(),
        "tiling is a policy, not a different picture"
    );
}

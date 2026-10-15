use crate::command::{Command, DEFAULT_AUTHOR, LogEntry};
use crate::error::CoreError;
use crate::project::Project;

/// One applied command together with the command that reverses it.
#[derive(Debug, Clone)]
struct Applied {
    entry: LogEntry,
    inverse: Command,
}

/// One undoable step: everything a single edit applied, oldest first.
///
/// A step is the unit undo and redo move in, and it is not always one command. Most edits
/// are one command and make a step of one. An edit that needs several — adding a title is
/// a media asset and then a clip that uses it — is applied with
/// [`History::apply_group`], and a group is **one** step, so a person presses undo once
/// for an edit they performed once.
///
/// The log is unaffected. It still records each command separately, in order, under its own
/// sequence number, because the log is the document and the document is a flat history of
/// commands: state stays a pure function of it, replay stays honest, and the hashes do not
/// move. Grouping is session state, exactly like the inverses beside it.
#[derive(Debug, Clone)]
struct Step {
    /// Never empty: the constructors guarantee at least one command.
    applied: Vec<Applied>,
}

/// The undo and redo stacks for one editing session.
///
/// # What lives here and what lives in the project
///
/// The applied commands are part of the project document, because a command log is
/// the durable description of the edit. The *inverses* are session state: they were
/// computed from the state each command observed, and recomputing them after a load
/// is impossible. Opening a project therefore starts with an empty undo history,
/// which is the honest answer rather than a fabricated one.
///
/// This split is why the plan describes undo as a property of the log instead of a
/// separate mechanism: the log already contains everything shared, and the inverse
/// stack is a local convenience.
///
/// # Steps, not commands
///
/// Undo and redo move one **step** at a time, and a step is one edit — which may be
/// several commands when it was applied with [`History::apply_group`]. The grouping is
/// session state and never reaches the log, so two histories that applied the same
/// commands in the same order produce the same document whether or not they grouped them.
#[derive(Debug, Clone, Default)]
pub struct History {
    steps: Vec<Step>,
    /// One entry per undone step, holding that step's commands oldest first.
    redo: Vec<Vec<LogEntry>>,
}

impl History {
    /// Creates an empty history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies a command, records it in the project log and remembers its inverse.
    ///
    /// Returns the new revision. The redo stack is discarded, because a new edit makes
    /// the redo branch unreachable.
    ///
    /// # Errors
    ///
    /// Returns the model's own error when the command is refused. A refused command
    /// changes nothing: it validates before it mutates.
    pub fn apply(
        &mut self,
        project: &mut Project,
        command: Command,
        author: &str,
    ) -> Result<u64, CoreError> {
        let applied = Self::apply_one(project, command, author)?;
        self.steps.push(Step {
            applied: vec![applied],
        });
        self.redo.clear();
        Ok(project.revision())
    }

    /// Applies a command attributed to the local user.
    ///
    /// # Errors
    ///
    /// Returns the model's own error when the command is refused.
    pub fn apply_local(
        &mut self,
        project: &mut Project,
        command: Command,
    ) -> Result<u64, CoreError> {
        self.apply(project, command, DEFAULT_AUTHOR)
    }

    /// Applies several commands as **one** edit: one undo step, and all or nothing.
    ///
    /// This is what an interface or an agent reaches for when a single thing a person asked
    /// for is more than one command in the model. Adding a title is the clearest case: a
    /// media asset, then a clip that uses it. Applied one at a time those are two edits,
    /// which means two presses of undo for one action, and — worse — a refusal on the
    /// second one leaves the first applied, so the project holds half an edit nobody asked
    /// for.
    ///
    /// Grouping fixes both halves of that:
    ///
    /// * **One step.** Undo reverts the whole group, newest command first, and redo
    ///   re-applies the whole group in the order it was applied.
    /// * **Atomic.** If any command is refused, every command this call had already applied
    ///   is reverted and its log entry removed, so the project and the log are exactly as
    ///   they were and the caller gets the engine's own refusal. The redo branch survives,
    ///   because nothing happened.
    ///
    /// The log still records each command separately: grouping is session state and does not
    /// reach the document, so the hashes, the replay and an agent's view of the log are
    /// unchanged.
    ///
    /// An empty group is a no-op. It applies nothing, pushes no step, and leaves the redo
    /// branch alone, which is what "the person asked for nothing" should mean.
    ///
    /// # Errors
    ///
    /// Returns the model's own error for the command it refused, after rolling the group
    /// back. If the rollback itself fails — which would mean the project and this history
    /// had already diverged — returns an error saying so instead, because reporting the
    /// original refusal would hide the worse problem.
    pub fn apply_group(
        &mut self,
        project: &mut Project,
        commands: Vec<Command>,
        author: &str,
    ) -> Result<u64, CoreError> {
        if commands.is_empty() {
            return Ok(project.revision());
        }
        let mut applied = Vec::with_capacity(commands.len());
        for command in commands {
            match Self::apply_one(project, command, author) {
                Ok(one) => applied.push(one),
                Err(refusal) => {
                    return Err(match Self::roll_back(project, applied) {
                        Ok(()) => refusal,
                        Err(fatal) => CoreError::invalid(format!(
                            "a grouped edit was refused ({refusal}) and could not be \
                             rolled back ({fatal}): the project and its history have diverged"
                        )),
                    });
                }
            }
        }
        self.steps.push(Step { applied });
        self.redo.clear();
        Ok(project.revision())
    }

    /// Applies several commands as one edit, attributed to the local user.
    ///
    /// # Errors
    ///
    /// As [`History::apply_group`].
    pub fn apply_group_local(
        &mut self,
        project: &mut Project,
        commands: Vec<Command>,
    ) -> Result<u64, CoreError> {
        self.apply_group(project, commands, DEFAULT_AUTHOR)
    }

    /// Applies one command and logs it, without touching either stack.
    ///
    /// The ordering here is the whole of the log's numbering rule: the command mutates the
    /// state, the entry is numbered with the revision *before* it is logged, and pushing the
    /// entry is what advances the revision.
    fn apply_one(
        project: &mut Project,
        command: Command,
        author: &str,
    ) -> Result<Applied, CoreError> {
        let inverse = command.apply(project)?;
        let entry = LogEntry::new(project.revision(), author, command);
        debug_assert_eq!(
            entry.seq(),
            project.revision(),
            "log entries must be numbered by revision"
        );
        project.log.push(entry.clone());
        Ok(Applied { entry, inverse })
    }

    /// Reverts commands a half-finished group applied, newest first, and unlogs them.
    ///
    /// Used when a group is refused part-way through, so that a refused edit changes
    /// nothing. The inverses were computed from the state each command observed and are
    /// applied in the exact reverse order, which is why this restores the project rather
    /// than approximating it.
    fn roll_back(project: &mut Project, applied: Vec<Applied>) -> Result<(), CoreError> {
        for one in applied.into_iter().rev() {
            project.log.pop();
            one.inverse.apply(project)?;
        }
        Ok(())
    }

    /// Reverts the most recent step: one command, or a whole group applied as one edit.
    ///
    /// Returns `Ok(None)` when there is nothing to undo.
    ///
    /// # Errors
    ///
    /// Returns an error when the project's revision is not the one this step left behind,
    /// which means the project and this history have diverged, or when an inverse is
    /// refused.
    pub fn undo(&mut self, project: &mut Project) -> Result<Option<u64>, CoreError> {
        let Some(step) = self.steps.pop() else {
            return Ok(None);
        };
        let last = step
            .applied
            .last()
            .ok_or_else(|| CoreError::invalid("an undo step holds no commands"))?;
        let expected_revision = last.entry.seq() + 1;
        if project.revision() != expected_revision {
            return Err(CoreError::invalid(
                "command history is out of sync with the project log",
            ));
        }
        // Newest first, so each inverse sees the state its command produced.
        let mut entries = Vec::with_capacity(step.applied.len());
        for one in step.applied.into_iter().rev() {
            project.log.pop();
            one.inverse.apply(project)?;
            entries.push(one.entry);
        }
        entries.reverse();
        self.redo.push(entries);
        Ok(Some(project.revision()))
    }

    /// Re-applies the most recently undone step, in the order it was applied.
    ///
    /// Returns `Ok(None)` when there is nothing to redo.
    ///
    /// # Errors
    ///
    /// Returns the model's error if a command cannot be re-applied. A group that fails
    /// part-way through is rolled back, so a failed redo changes nothing.
    pub fn redo(&mut self, project: &mut Project) -> Result<Option<u64>, CoreError> {
        let Some(entries) = self.redo.pop() else {
            return Ok(None);
        };
        let mut applied = Vec::with_capacity(entries.len());
        for entry in entries {
            match entry.command.apply(project) {
                Ok(inverse) => {
                    debug_assert_eq!(entry.seq(), project.revision());
                    project.log.push(entry.clone());
                    applied.push(Applied { entry, inverse });
                }
                Err(refusal) => {
                    return Err(match Self::roll_back(project, applied) {
                        Ok(()) => refusal,
                        Err(fatal) => CoreError::invalid(format!(
                            "a redone edit was refused ({refusal}) and could not be \
                             rolled back ({fatal}): the project and its history have diverged"
                        )),
                    });
                }
            }
        }
        self.steps.push(Step { applied });
        Ok(Some(project.revision()))
    }

    /// Reverts every step applied in this session, newest first.
    ///
    /// Returns the number of steps undone, which is the number of commands unless some of
    /// them were grouped.
    ///
    /// # Errors
    ///
    /// As [`History::undo`].
    pub fn undo_all(&mut self, project: &mut Project) -> Result<usize, CoreError> {
        let mut count = 0;
        while self.undo(project)?.is_some() {
            count += 1;
        }
        Ok(count)
    }

    /// Re-applies every step available on the redo stack.
    ///
    /// Returns the number of steps redone.
    ///
    /// # Errors
    ///
    /// As [`History::redo`].
    pub fn redo_all(&mut self, project: &mut Project) -> Result<usize, CoreError> {
        let mut count = 0;
        while self.redo(project)?.is_some() {
            count += 1;
        }
        Ok(count)
    }

    /// Reports whether anything can be undone.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.steps.is_empty()
    }

    /// Reports whether anything can be redone.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Number of **steps** available to undo, which is how many times undo will do
    /// something.
    ///
    /// A step is one edit. It equals the number of commands unless some were grouped; for
    /// the commands themselves, see [`History::undo_commands`].
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.steps.len()
    }

    /// Number of **steps** available to redo.
    #[must_use]
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Number of commands behind the undo steps.
    ///
    /// This is the count the log would shrink by if everything were undone, so it is the
    /// number to show beside a revision rather than beside an undo button.
    #[must_use]
    pub fn undo_commands(&self) -> usize {
        self.steps.iter().map(|step| step.applied.len()).sum()
    }

    /// Number of commands behind the redo steps.
    #[must_use]
    pub fn redo_commands(&self) -> usize {
        self.redo.iter().map(Vec::len).sum()
    }

    /// How many commands the most recent undo step holds, or `None` when there is nothing
    /// to undo.
    ///
    /// An interface uses this to say *what* undo will revert — "undo the title", not "undo"
    /// — because a step of three commands is one thing the person did.
    #[must_use]
    pub fn next_undo_size(&self) -> Option<usize> {
        self.steps.last().map(|step| step.applied.len())
    }

    /// Discards all history without touching the project.
    pub fn clear(&mut self) {
        self.steps.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{MediaAsset, Sequence, Track, TrackKind};
    use crate::ids::{ClipId, IdAllocator, MediaAssetId, SequenceId, TrackId};
    use maj_time::{FrameRange, Rational, Timebase};

    struct Fixture {
        project: Project,
        history: History,
        sequence: SequenceId,
        track: TrackId,
    }

    fn fixture() -> Fixture {
        let mut project = Project::new("history");
        let mut ids = IdAllocator::for_project(&project);
        let asset_id = ids.media_asset();
        let sequence_id = ids.sequence();
        let track_id = ids.track();
        let mut history = History::new();

        history
            .apply_local(
                &mut project,
                Command::CreateMediaAsset {
                    index: 0,
                    asset: MediaAsset::new(
                        asset_id,
                        "a.mov",
                        "file:///a.mov",
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
                    sequence: Sequence::new(sequence_id, "S", Timebase::FPS_25, 1920, 1080, 48_000)
                        .unwrap(),
                },
            )
            .unwrap();
        history
            .apply_local(
                &mut project,
                Command::CreateTrack {
                    sequence: sequence_id,
                    index: 0,
                    track: Track::new(track_id, TrackKind::Video, "V1"),
                },
            )
            .unwrap();

        Fixture {
            project,
            history,
            sequence: sequence_id,
            track: track_id,
        }
    }

    impl Fixture {
        fn append(&mut self, clips: u64) {
            for n in 0..clips {
                let clip_id = IdAllocator::for_project(&self.project).clip();
                self.history
                    .apply_local(
                        &mut self.project,
                        Command::AppendClip {
                            sequence: self.sequence,
                            track: self.track,
                            clip: crate::entity::Clip::new(
                                clip_id,
                                MediaAssetId::new(1),
                                format!("c{n}"),
                                FrameRange::new(0, 100).unwrap(),
                                i64::try_from(n).unwrap() * 100,
                                Rational::from_integer(1),
                            )
                            .unwrap(),
                        },
                    )
                    .unwrap();
            }
        }
    }

    #[test]
    fn a_history_can_undo_what_it_applied_and_has_nothing_to_redo() {
        let mut fixture = fixture();
        assert!(fixture.history.can_undo());
        assert!(!fixture.history.can_redo());
        assert_eq!(
            fixture.history.undo_depth(),
            3,
            "fixture applies three commands"
        );
        assert_eq!(fixture.history.redo(&mut fixture.project).unwrap(), None);

        fixture.history.undo_all(&mut fixture.project).unwrap();
        assert!(!fixture.history.can_undo());
        assert_eq!(fixture.history.undo(&mut fixture.project).unwrap(), None);
    }

    #[test]
    fn the_log_revision_tracks_the_number_of_applied_commands() {
        let fixture = fixture();
        assert_eq!(fixture.project.revision(), 3);
        assert_eq!(fixture.project.log().len(), 3);
        assert_eq!(fixture.project.log()[0].seq(), 0);
        assert_eq!(fixture.project.log()[2].seq(), 2);
        assert_eq!(fixture.project.log()[0].author(), DEFAULT_AUTHOR);
    }

    #[test]
    fn undo_then_redo_restores_the_same_state() {
        let mut fixture = fixture();
        fixture.append(4);
        let peak = fixture.project.state_hash().unwrap();

        assert_eq!(fixture.history.undo_depth(), 7);
        fixture.history.undo_all(&mut fixture.project).unwrap();
        assert_eq!(fixture.project.revision(), 0);
        assert_eq!(fixture.history.redo_depth(), 7);
        assert!(fixture.project.is_valid());

        fixture.history.redo_all(&mut fixture.project).unwrap();
        assert_eq!(fixture.project.revision(), 7);
        assert_eq!(fixture.project.state_hash().unwrap(), peak);
        assert!(fixture.project.is_valid());
    }

    #[test]
    fn a_partial_undo_and_redo_sequence_is_still_exact() {
        let mut fixture = fixture();
        fixture.append(6);
        let peak = fixture.project.state_hash().unwrap();

        fixture.history.undo(&mut fixture.project).unwrap();
        fixture.history.undo(&mut fixture.project).unwrap();
        let mid = fixture.project.state_hash().unwrap();
        fixture.history.redo(&mut fixture.project).unwrap();
        fixture.history.undo(&mut fixture.project).unwrap();
        assert_eq!(fixture.project.state_hash().unwrap(), mid);

        fixture.history.redo_all(&mut fixture.project).unwrap();
        assert_eq!(fixture.project.state_hash().unwrap(), peak);
    }

    #[test]
    fn a_new_edit_discards_the_redo_branch() {
        let mut fixture = fixture();
        fixture.append(3);
        fixture.history.undo(&mut fixture.project).unwrap();
        assert_eq!(fixture.history.redo_depth(), 1);

        fixture.append(1);
        assert!(
            !fixture.history.can_redo(),
            "the redo branch must be discarded"
        );
        assert_eq!(
            fixture.project.revision(),
            6,
            "three fixture commands plus three appends"
        );
    }

    #[test]
    fn undo_restores_clip_order_exactly_after_a_middle_removal() {
        let mut fixture = fixture();
        fixture.append(3);
        let peak = fixture.project.state_hash().unwrap();
        let target = ClipId::new(4);

        fixture
            .history
            .apply_local(
                &mut fixture.project,
                Command::RemoveClip {
                    sequence: fixture.sequence,
                    clip: target,
                },
            )
            .unwrap();
        assert_eq!(fixture.project.sequences[0].tracks[0].clip_count(), 2);

        fixture.history.undo(&mut fixture.project).unwrap();
        let names: Vec<&str> = fixture.project.sequences[0].tracks[0]
            .clips()
            .iter()
            .map(crate::entity::Clip::name)
            .collect();
        assert_eq!(
            names,
            vec!["c0", "c1", "c2"],
            "the clip went back in the middle"
        );
        assert_eq!(fixture.project.state_hash().unwrap(), peak);
    }

    impl Fixture {
        /// The two commands that stand in for "one thing a person asked for": an asset, and
        /// a clip that uses it. This is the shape of adding a title.
        fn one_edit_in_two_commands(&self, name: &str) -> Vec<Command> {
            let mut ids = IdAllocator::for_project(&self.project);
            let asset_id = ids.media_asset();
            let clip_id = ids.clip();
            vec![
                Command::CreateMediaAsset {
                    index: self.project.assets().len(),
                    asset: MediaAsset::new(
                        asset_id,
                        name,
                        format!("title:sans:48:ffffff:centre:{name}"),
                        Timebase::FPS_25,
                        100,
                    )
                    .unwrap(),
                },
                Command::AppendClip {
                    sequence: self.sequence,
                    track: self.track,
                    clip: crate::entity::Clip::new(
                        clip_id,
                        asset_id,
                        name,
                        FrameRange::new(0, 100).unwrap(),
                        0,
                        Rational::from_integer(1),
                    )
                    .unwrap(),
                },
            ]
        }
    }

    #[test]
    fn a_group_is_one_undo_step_over_two_logged_commands() {
        let mut fixture = fixture();
        let before = fixture.project.state_hash().unwrap();
        let revision = fixture.project.revision();
        let steps = fixture.history.undo_depth();
        let commands = fixture.one_edit_in_two_commands("MAJESTAT");

        fixture
            .history
            .apply_group_local(&mut fixture.project, commands)
            .unwrap();

        assert_eq!(
            fixture.project.revision(),
            revision + 2,
            "the log records both commands"
        );
        assert_eq!(
            fixture.project.log().len(),
            usize::try_from(revision).unwrap() + 2
        );
        assert_eq!(
            fixture.history.undo_depth(),
            steps + 1,
            "two commands, one step"
        );
        assert_eq!(fixture.history.next_undo_size(), Some(2));
        assert_eq!(
            fixture.history.undo_commands(),
            usize::try_from(revision).unwrap() + 2
        );

        let after = fixture.project.state_hash().unwrap();
        assert_ne!(after, before, "the edit did something");

        assert!(
            fixture
                .history
                .undo(&mut fixture.project)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            fixture.project.revision(),
            revision,
            "one undo took both commands back"
        );
        assert_eq!(fixture.project.state_hash().unwrap(), before);
        assert!(fixture.project.is_valid());

        assert_eq!(fixture.history.redo_depth(), 1);
        assert_eq!(fixture.history.redo_commands(), 2);
        assert!(
            fixture
                .history
                .redo(&mut fixture.project)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            fixture.project.state_hash().unwrap(),
            after,
            "one redo put both commands back, in order"
        );
    }

    #[test]
    fn a_refused_group_changes_nothing_and_keeps_the_redo_branch() {
        let mut fixture = fixture();
        fixture.append(2);
        fixture.history.undo(&mut fixture.project).unwrap();
        let before = fixture.project.state_hash().unwrap();
        let revision = fixture.project.revision();
        let log = fixture.project.log().len();
        let steps = fixture.history.undo_depth();
        assert_eq!(fixture.history.redo_depth(), 1, "there is a branch to keep");

        // The first command is good and the second cannot apply, which is the case that
        // used to leave half an edit in the project.
        let mut commands = fixture.one_edit_in_two_commands("HALF");
        commands.push(Command::RemoveClip {
            sequence: fixture.sequence,
            clip: ClipId::new(9_999),
        });

        let refusal = fixture
            .history
            .apply_group_local(&mut fixture.project, commands)
            .expect_err("removing a clip that is not there is refused");
        assert!(
            !refusal.to_string().contains("diverged"),
            "the rollback itself must succeed: {refusal}"
        );

        assert_eq!(fixture.project.revision(), revision, "nothing was applied");
        assert_eq!(fixture.project.log().len(), log, "nothing was logged");
        assert_eq!(
            fixture.project.state_hash().unwrap(),
            before,
            "the project is exactly as it was"
        );
        assert_eq!(fixture.history.undo_depth(), steps, "no step was pushed");
        assert_eq!(
            fixture.history.redo_depth(),
            1,
            "a refused group is not an edit, so the redo branch survives"
        );
        assert!(fixture.project.is_valid());
    }

    #[test]
    fn an_empty_group_does_nothing_at_all() {
        let mut fixture = fixture();
        fixture.append(1);
        fixture.history.undo(&mut fixture.project).unwrap();
        let before = fixture.project.state_hash().unwrap();
        let steps = fixture.history.undo_depth();

        let revision = fixture
            .history
            .apply_group_local(&mut fixture.project, Vec::new())
            .unwrap();

        assert_eq!(revision, fixture.project.revision());
        assert_eq!(fixture.project.state_hash().unwrap(), before);
        assert_eq!(fixture.history.undo_depth(), steps);
        assert_eq!(
            fixture.history.redo_depth(),
            1,
            "asking for nothing must not discard the redo branch"
        );
    }

    #[test]
    fn grouping_is_session_state_and_never_reaches_the_document() {
        // The claim the whole design rests on: a group changes when undo stops, and
        // nothing else. Two projects given the same commands in the same order under the
        // same author are the same document, grouped or not.
        let mut grouped = fixture();
        let commands = grouped.one_edit_in_two_commands("ONE");
        let mut separate = fixture();

        grouped
            .history
            .apply_group_local(&mut grouped.project, commands.clone())
            .unwrap();
        for command in commands {
            separate
                .history
                .apply_local(&mut separate.project, command)
                .unwrap();
        }

        assert_eq!(
            grouped.project.state_hash().unwrap(),
            separate.project.state_hash().unwrap(),
            "the same state"
        );
        assert_eq!(
            grouped.project.document_hash().unwrap(),
            separate.project.document_hash().unwrap(),
            "the same document, so the log did not learn about the group"
        );
        assert_eq!(
            (grouped.history.undo_depth(), separate.history.undo_depth()),
            (4, 5),
            "the only difference is where undo stops"
        );
    }

    #[test]
    fn clearing_the_history_keeps_the_project_intact() {
        let mut fixture = fixture();
        fixture.append(2);
        let hash = fixture.project.state_hash().unwrap();
        fixture.history.clear();
        assert!(!fixture.history.can_undo());
        assert_eq!(fixture.project.state_hash().unwrap(), hash);
    }
}

# 0002. Everything is a command

* Status: Accepted
* Date: 2026-10-13

## Context

A video editor needs four things that are usually built as four separate subsystems: undo
and redo, a durable project format, review and attribution, and some way for automation
to drive the application. Each is normally written independently, and each drifts from
the others. Undo stacks and saved files disagree after a crash; a scripting API exposes a
different set of operations from the interface; a collaborator's change is not
reviewable because it arrived as a blob.

Majestat also has to support an AI agent that edits and collaborators who edit
simultaneously. Both need the same guarantees the interface has.

## Decision

Every mutation is a **command**: an immutable, serialisable value representing one
intentional edit. `Command::apply(&mut Project)` mutates the project **and returns the
command that undoes it**. The applied commands, in order, are the durable document.

Concretely:

* Commands that create an entity carry their insertion index, so an undo restores the
  entity to the slot it came from, not to the end.
* There is no parallel undo structure. The inverse is derived from the state the command
  observed, so it cannot be stale.
* The state is a cached derivation of the log. `rebuild_from_log` recomputes it and the
  test suite checks the two agree, on every seed.
* Undo and redo stacks are session state, because an inverse cannot be recomputed after a
  load. Opening a project starts an empty history, and that is documented rather than
  faked.

## Consequences

* Undo, redo, persistence, replay, review, attribution and automation all read from one
  representation. A bug in command application is a bug in all of them at once, which
  concentrates the test surface usefully instead of scattering it.
* The agent (M11), the CLI (M8) and a remote collaborator (M12) drive exactly the
  interface the UI drives. No separate automation API.
* A command must be **invertible**. Operations that are not naturally invertible (a
  destructive "auto-conform this bin", say) have to be expressed as a sequence of
  invertible steps or be excluded from the log. That is a real constraint on future
  features and is accepted.
* Replaying a long log is O(n) commands against a growing state, so naive replay is
  quadratic. Acceptable at the current scale; a snapshot-plus-tail format is the escape
  hatch if it ever is not.
* Collaborative undo must append an inverse command rather than truncate the log, so that
  identifiers are never reclaimed. Recorded on `maj_core::IdAllocator`.

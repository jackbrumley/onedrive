# SomeDrive Sync Single-Source-of-Truth Implementation Plan

## Objective

Converge the sync engine from a hybrid authority model to a DB-authoritative architecture where:

1. Discovery state, planner state, execution state, and user-visible lifecycle state are persisted.
2. One canonical planner graph drives all sync actions.
3. Runtime memory is delivery cache only, never competing authority.
4. Restart, pause/resume, retry, and recovery are deterministic.

This plan is intentionally implementation-focused and aligned with current code paths.

## Scope and Non-Goals

### In scope

- Complete Phase 3 style convergence to DB authority for sync decisions.
- Remove flow-critical planning dependence on hybrid JSON/in-memory maps.
- Formalize and enforce one write path for lifecycle/activity/issue state.
- Split oversized sync modules by responsibility to reduce drift.
- Add transition and invariant test coverage for end-to-end determinism.

### Out of scope

- New user-facing sync features unrelated to authority convergence.
- Backward compatibility/migration shims for old local state formats (per current development policy).
- Distributed or multi-device conflict policy redesign beyond current semantics.

## Current State Summary (What exists now)

- Durable execution queue for downloads and uploads in `sync_jobs`.
- Planner index table `sync_files` with derived actions (`download`, `upload`, `conflict`, `none`).
- Hybrid flow control still depends on `PersistedSyncState` (`delta_link`, `remote_by_id`, `local_snapshot`, two-way/bootstrap flags).
- Planner recompute exists but execution is not fully materialized from planner output.
- Lifecycle and runtime status pipeline has central writer wrappers but must stay strictly enforced.

## Target Architecture

### A. Authority model

- `sync_lifecycle_state` is authoritative for user-visible lifecycle/activity/phase status.
- `sync_files` is authoritative for file-level discovery/planning state.
- `sync_jobs` is authoritative for execution state and transfer progress.
- In-memory runtime map is event-emission cache; UI truth originates from DB-backed projection.

### B. Single writer model

- All phase/activity/issue/scan-complete lifecycle writes go through one writer API layer.
- No direct writes to lifecycle-related runtime fields outside this writer.

### C. Pipeline model

1. Discover remote changes and local snapshot observations.
2. Persist observations into `sync_files`.
3. Run planner transitions to derive desired actions.
4. Materialize actions into `sync_jobs`.
5. Workers execute jobs and persist progress/results.
6. Projection updates lifecycle/runtime payload and emits status events.

## Work Plan

## Progress Tracker

Legend:

- `[x]` done
- `[~]` partially done
- `[ ]` not started

### Current Completion Snapshot

- `[x]` Planner ownership extracted to dedicated module (`planner.rs`).
- `[~]` Planner-driven upload execution adopted (selection is planner-based; delete/conflict materialization still pending).
- `[~]` Structured lifecycle activity contract improved with `cycle_id` and `updated_at` propagation.
- `[~]` Initial planner/execution invariant logging added (planner vs active job inventory now logged).
- `[x]` Legacy file fallback for sync state load removed (DB-backed state store only).
- `[~]` Module decomposition advanced (`lifecycle_writer.rs` extracted; major queue/lane splits still pending).
- `[~]` Module decomposition advanced (`lifecycle_writer.rs`, `planner_index.rs`, and `planner_transitions.rs` extracted).
- `[~]` Initial planner tests added (`planner_*` transition coverage).
- `[x]` Roadmap document created in `temp/`.

### Remaining Work (Strict Close List)

1. Finish DB authority for operational state
   - `[ ]` Move `delta_link` and `active_delta_next_link` to DB-only authority.
   - `[ ]` Move two-way/bootstrap gate authority to DB-only paths.
   - `[ ]` Remove flow-critical planning reads from `PersistedSyncState`.
   - `[ ]` Ensure restart reconstruction reads lifecycle/planner/jobs only.

2. Make planner the only action authority
   - `[ ]` Finalize explicit planner action set (`download`, `upload`, `delete_remote`, `delete_local`, `conflict`, `none`).
   - `[ ]` Centralize all transition rules in one planner transitions owner.
   - `[ ]` Implement full job materialization from planner output to `sync_jobs`.
   - `[ ]` Ensure materialization is idempotent across repeated cycles.
   - `[ ]` Remove remaining direct apply-path decision branches that bypass planner.
   - `[ ]` Route delete and conflict execution through planner/materializer flow.

3. Complete module ownership decomposition
   - `[x]` Extract lifecycle writer into `lifecycle_writer.rs`.
   - `[x]` Extract sync_files DB primitives into `planner_index.rs`.
   - `[x]` Extract transition rules into `planner_transitions.rs`.
   - `[ ]` Extract action enqueue/update to `job_materializer.rs`.
   - `[ ]` Extract remote lane mechanics to `download_lane.rs`.
   - `[ ]` Extract upload lane mechanics to `upload_lane.rs`.
   - `[ ]` Keep cycle orchestration thin in `cycle_orchestrator.rs`.
   - `[ ]` Reduce oversized files (`job_queue.rs`, `remote_changes.rs`) below target scope.

4. Enforce single writer contract
   - `[ ]` Extend guard scripts to block unauthorized lifecycle/planner side writes.
   - `[ ]` Verify all phase/activity/issue writes route through one writer API.
   - `[ ]` Add hard invariant checks for illegal lifecycle combinations.
   - `[ ]` Ensure all activity writes carry deterministic contract fields.

5. Reliability hardening
   - `[ ]` Validate bounded backpressure behavior after full materializer rollout.
   - `[ ]` Ensure watchdog uses durable counters correctly.
   - `[ ]` Verify deterministic lease recovery on both lanes.
   - `[ ]` Verify pause drain/resume leaves no orphan running jobs.
   - `[ ]` Audit retry lifecycle (`retry_wait`, terminal fail, retry-all) for both lanes.

6. Determinism invariants and diagnostics
   - `[~]` Add planner-vs-jobs reconciliation checks by action/direction.
   - `[ ]` Add lifecycle-vs-runtime payload consistency checks.
   - `[x]` Add startup DB consistency summary logs for lifecycle/planner/jobs.

7. Test matrix completion
   - `[~]` Planner transition tests (remote-only/local-only/overlap/conflict covered; shared refs pending).
   - `[ ]` Materializer tests (idempotent enqueue/update behavior).
   - `[ ]` Lifecycle writer invariant tests.
   - `[ ]` Pause/resume/restart determinism tests.
   - `[ ]` Bootstrap gate tests (blocked -> retried -> two-way ready).
   - `[ ]` Integration scenarios for large-delete guard and conflict backup paths.

8. Final cleanup and closeout
   - `[ ]` Remove dead hybrid-authority code paths.
   - `[ ]` Remove obsolete state fields/struct usage after DB parity.
   - `[ ]` Update architecture docs to match final ownership model.
   - `[ ]` Re-run full acceptance checklist and mark completion.

### Definition of Fully Done (Close Criteria)

- `[ ]` No flow-critical sync decision depends on JSON/in-memory mirrors.
- `[ ]` Planner actions materialize to jobs for all relevant action types.
- `[ ]` One lifecycle writer path is enforced by guardrails.
- `[ ]` Restart/pause/resume/retry deterministic from DB state.
- `[ ]` `cargo check`, `cargo test`, `npm run typecheck`, and sync guard scripts all pass.

## Phase 0: Guardrails and Instrumentation Baseline

### Goals

- Ensure existing guardrails are active before refactors.
- Add missing invariant diagnostics for authority drift.

### Tasks

1. Extend write-path guard script to block unauthorized lifecycle/state writes.
2. Add invariant logs for:
   - planner action count vs materialized job count by direction;
   - lifecycle phase/activity completeness;
   - stale in-progress job and lease recovery.
3. Add startup diagnostic summary for authoritative row counts:
   - `sync_lifecycle_state`, `sync_files`, `sync_jobs`.

### Deliverables

- CI/static guard checks updated.
- Diagnostic markers documented in log conventions.

---

## Phase 1: Module Ownership Cleanup (No behavior change first)

### Goals

- Reduce oversized file complexity and assign one owner per concern.

### Proposed module boundaries

Current large modules should be split into focused files under `src-tauri/src/app/sync_engine/`:

1. `lifecycle_writer.rs`
   - runtime + lifecycle DB write-through APIs.
2. `planner_index.rs`
   - sync_files upsert/update/rebuild/query primitives.
3. `planner_transitions.rs`
   - desired_action/conflict derivation rules.
4. `job_materializer.rs`
   - convert planner desired actions into `sync_jobs` rows.
5. `download_lane.rs`
   - remote download dispatcher/worker mechanics.
6. `upload_lane.rs`
   - upload execution and durable upload transitions.
7. `state_store.rs`
   - remaining persisted state accessor functions (interim).
8. `cycle_orchestrator.rs`
   - high-level cycle orchestration only.

### Tasks

1. Move code without semantic changes.
2. Keep public function signatures stable where practical.
3. Add lightweight module-level docs describing ownership boundaries.

### Deliverables

- No sync behavior changes.
- Core sync modules each under the file ceiling and responsibility-scoped.

---

## Phase 2: Planner as Execution Authority

### Goals

- Make `sync_files` decisions the sole source of action materialization.

### Tasks

1. Define explicit planner action states (example):
   - `none`, `download`, `upload`, `delete_remote`, `delete_local`, `conflict`.
2. Expand planner transition rules in one place only (`planner_transitions.rs`).
3. Add materialization pass:
   - query actionable `sync_files` rows;
   - enqueue/update `sync_jobs` idempotently;
   - tag/track source action for diagnostics.
4. Remove direct apply-path action branching that bypasses planner materialization.
5. Keep conflict/safe-backup behavior but trigger through planner/materializer contract.

### Deliverables

- Cycle execution consumes durable jobs produced from planner output.
- Planner summary counters match actionable jobs by direction.

---

## Phase 3: Remove Hybrid Operational Authority

### Goals

- Eliminate `PersistedSyncState` as flow-critical planner authority.

### Tasks

1. Migrate authority fields to DB tables:
   - `delta_link`, `active_delta_next_link`, two-way/bootstrap flags, lifecycle timestamps.
2. Replace reads/writes currently flowing through JSON state with DB-backed accessors.
3. Keep JSON only as temporary compatibility transport during cutover, then remove.
4. Ensure local and remote discovery presence is represented only via `sync_files` and lifecycle fields.
5. Delete dead paths and structs once parity validation passes.

### Deliverables

- No planner/execution decisions require JSON state maps.
- Restart path reconstructs fully from DB.

---

## Phase 4: Deterministic Lifecycle and Activity Contract Tightening

### Goals

- Make lifecycle/activity state fully deterministic and validated.

### Tasks

1. Keep one writer API for all lifecycle mutations and enforce by guard checks.
2. Ensure every stage transition records:
   - stage, progress_mode, current/total/unit/detail, updated_at, cycle_id (if available).
3. Add consistency checks:
   - paused/idle/error cannot report active progress;
   - active work must have non-hidden progress mode where applicable.
4. Verify frontend renders authoritative fields only (no inferred substitute logic).

### Deliverables

- DB, runtime payload, and UI status are aligned across transitions.

---

## Phase 5: Reliability Hardening and Backpressure Safety

### Goals

- Keep current pipeline robustness while authority is moved to DB.

### Tasks

1. Validate queue backpressure behavior remains bounded and observable.
2. Ensure stalled pipeline watchdog references durable counters/state.
3. Ensure lease recovery and pause drain rules are deterministic for both lanes.
4. Audit retry semantics:
   - retry_wait transitions;
   - terminal failures;
   - retry-all behavior.

### Deliverables

- No deadlocks or orphan in-progress states after pause/resume/restart.

---

## Phase 6: Test Matrix and Verification

### Automated tests to add (Rust)

1. Planner transitions:
   - remote-only, local-only, overlap newer/older, metadata mismatch, shared references.
2. Action materialization:
   - planner rows to durable jobs (idempotent repeated cycles).
3. Lifecycle writer invariants:
   - phase/activity persistence correctness and invalid combination rejection.
4. Pause/resume/restart determinism:
   - interrupted runs and lease recovery.
5. Bootstrap gate transitions:
   - blocked by failed downloads, unblocked after retries, two-way ready handoff.

### Scenario verification matrix

1. First-time cloud-first bootstrap with large download set.
2. Bootstrap interrupted by pause and resumed.
3. Failed downloads block two-way; retry clears block.
4. Two-way cycle with concurrent remote+local edits.
5. Large delete guard trigger, confirm, and keep-cloud paths.
6. Restart mid-cycle and resume deterministically.
7. Network throttling and transient failures.

### Commands

- `cargo check`
- `cargo test`
- `npm run typecheck`
- `npm run lint` (if impacted)

---

## Implementation Sequence (Recommended)

1. Phase 0 guardrails.
2. Phase 1 module split with no behavior change.
3. Phase 2 planner-to-job materialization as primary execution lane.
4. Phase 3 hybrid authority removal.
5. Phase 4 lifecycle contract tightening.
6. Phase 5 reliability hardening.
7. Phase 6 tests and acceptance validation.

This order minimizes risk by separating structural refactor from behavioral convergence.

## Acceptance Criteria (Definition of Done)

1. All sync decisions are DB-derived from planner + lifecycle + job state.
2. No flow-critical planner decisions depend on JSON/in-memory mirrors.
3. One centralized lifecycle writer is the only write path for phase/activity/issue fields.
4. Restart after interruption resumes from durable state with deterministic outcomes.
5. Planner/action counters and job counters reconcile within defined invariants.
6. Pause/resume/retry/startup restore remain consistent in DB, payload, and UI.
7. `cargo check`, `cargo test`, and `npm run typecheck` pass.

## Suggested File Touchpoints

- `src-tauri/src/app/sync_engine.rs`
- `src-tauri/src/app/sync_engine/cycle.rs`
- `src-tauri/src/app/sync_engine/job_queue.rs`
- `src-tauri/src/app/sync_engine/remote_changes.rs`
- `src-tauri/src/app/sync_engine/local_changes.rs`
- `src-tauri/src/app/sync_engine/graph_transfer.rs`
- `src-tauri/src/app/sync_engine/runtime_and_models.rs`
- `src-tauri/src/app/sync_runtime.rs`
- `src-tauri/src/app/commands/sync_runtime.rs`
- `scripts/check-sync-status-writes.mjs` (guardrails)

## Risk Register

1. Refactor regression risk from large module extraction.
   - Mitigation: no-behavior phase first, then planner convergence.
2. Counter drift between planner/materializer/executor.
   - Mitigation: explicit invariant checks and tests.
3. Hidden authority fallbacks surviving in edge paths.
   - Mitigation: guard scripts + grep checks + dead path removal.
4. Performance regressions from planner expansion.
   - Mitigation: bounded queries, indexes, benchmark logs per cycle.

## Notes

- This plan favors root-cause authority cleanup over UI-level symptom handling.
- Where semantics must change, do it through planner transition rules in one owner module.
- Keep user-visible behavior stable unless a deterministic correctness fix requires explicit change.

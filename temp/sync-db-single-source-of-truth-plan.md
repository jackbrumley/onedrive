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

### Now (In Progress)

1. `[~]` Single-writer lifecycle contract closure
   - Owner modules: `src-tauri/src/app/sync_engine/lifecycle_writer.rs`, `src-tauri/src/app/sync_engine/job_queue_activity_projection.rs`, `src-tauri/src/app/sync_engine/job_queue_lifecycle_store.rs`, `src-tauri/src/app/sync_engine/job_queue_issue_throttle_store.rs`.
   - Verify all phase/activity/issue writes route through canonical writer APIs only (operational lifecycle state writes now routed through `lifecycle_writer` wrapper and guard-enforced).
   - Ensure activity payload writes always include deterministic contract fields (`stage`, `progress_mode`, `updated_at`, `cycle_id`, current/total/unit/detail when applicable) and reject invalid write combinations at persist time.
   - Verification: `cargo test -- sync_engine::tests::lifecycle*` and targeted grep/guard checks.

2. `[~]` Reliability hardening for queue/lease/watchdog/retry
    - Owner modules: `src-tauri/src/app/sync_engine/job_queue.rs`, `src-tauri/src/app/sync_engine/download_lane.rs`, `src-tauri/src/app/sync_engine/upload_lane.rs`, `src-tauri/src/app/sync_engine/runtime_watchdog.rs`.
    - Validate bounded backpressure behavior and deterministic lease recovery across both lanes.
    - Complete retry lifecycle audit for `retry_wait`, terminal failure, and retry-all behavior.
    - Lease/retry determinism tests now cover upload stale-lease recovery, action stale-lease recovery, and upload `retry_wait` due/not-due claim gating.
    - Verification: targeted integration tests + `cargo test`.

3. `[~]` Determinism matrix completion
    - Owner modules: `src-tauri/src/app/sync_engine/tests/*`, `src-tauri/src/app/commands/sync_runtime.rs`.
    - Finish lifecycle-vs-runtime payload consistency checks and end-to-end multi-cycle flow coverage.
    - Add remaining large-delete guard and conflict-backup integration scenarios (guard resolution path and safe-backup behavior coverage added in `local_changes_tests`).
    - Verification: `cargo test -- sync_engine::tests::*`.

### Next (Queued)

1. `[~]` Planner/materializer final reconciliation pass
   - Owner modules: `src-tauri/src/app/sync_engine/job_materializer.rs`, `src-tauri/src/app/sync_engine/planner_transitions.rs`.
   - Close remaining idempotency `[~]` items across repeated cycles.
   - Promote planner-vs-jobs reconciliation by action/direction from diagnostics to hard invariants where safe (materializer now fails fast on desired/materialized count drift for download/upload/delete/conflict lanes).
   - Verification: `cargo test -- sync_engine::tests::materializer*`.

2. `[x]` Hybrid-authority dead-path removal and obsolete state cleanup
    - Owner modules: `src-tauri/src/app/sync_engine/path_state.rs`, `src-tauri/src/app/sync_engine/preamble.rs`, `src-tauri/src/app/sync_engine/cycle_orchestrator.rs`.
    - Remove dead hybrid authority paths and obsolete non-authoritative fields once parity checks pass (legacy JSON authority fields removed from `PersistedSyncState`; cache-only payload narrowed; remote delete candidate/id resolution now reads `sync_files` authority instead of cache maps; planned upload execution no longer re-decides via cache timestamp comparisons; large-delete guard state moved to lifecycle DB columns).
    - Keep `PersistedSyncState` as transport/cache only where unavoidable during final cutover.
    - Verification: guard script + targeted sync restart tests.

3. `[x]` Module/structure closeout
   - Owner modules: `src-tauri/src/app/sync_engine/remote_changes.rs`, `src-tauri/src/app/sync_engine/remote_pipeline_loop.rs`.
   - Remaining decomposition trims reviewed complete: remote pipeline files stay below file-size limits and retain focused owner boundaries (orchestration vs loop processing).
   - Verification: file-size/ownership review + `cargo check`.

4. `[x]` Final acceptance and documentation closeout
   - Run full validation: `cargo check`, `cargo test`, `npm run typecheck`, sync guard scripts.
   - Update architecture docs and this tracker with final ownership model and completion markers (tracker now reflects lifecycle writer ownership, DB guard-state authority, and durable retry scheduling authority).

### Done (Completed)

- `[x]` Planner action ownership centralized with explicit action set (`download`, `upload`, `delete_remote`, `delete_local`, `conflict`, `none`).
- `[x]` Planner transition rules isolated under dedicated transition owner module.
- `[x]` Durable job materialization for download/upload/delete/conflict actions wired through planner output.
- `[x]` Flow-critical planner reads removed from `PersistedSyncState` authority paths.
- `[x]` DB-only authority migration for `delta_link`, `active_delta_next_link`, and bootstrap/two-way gate state.
- `[x]` Startup reconstruction rehydrated from lifecycle/planner/jobs authorities.
- `[x]` Legacy file fallback removed for sync-state loading.
- `[x]` Major module decomposition completed (`lifecycle_writer`, `planner_index`, `planner_transitions`, `download_lane`, `upload_lane`, `cycle_orchestrator`, split queue stores).
- `[x]` Planner transition and core materializer tests added.
- `[x]` Startup DB consistency summary diagnostics added for lifecycle/planner/job authorities.
- `[x]` Lifecycle write-path guard expanded to include issue persistence entry points and lifecycle persist-time invariants (`phase` vs `progress_mode`, determinate field completeness).
- `[x]` Lifecycle invariant coverage extended with persist-time rejection tests for paused/non-hidden activity and invalid determinate progress writes.
- `[x]` Guardrails extended to enforce ownership of operational lifecycle state writes (`persist_sync_lifecycle_operational_state`).
- `[x]` Obsolete JSON authority fields removed from `PersistedSyncState` (`delta_link`, `active_delta_next_link`, bootstrap/two-way flags, `last_cycle_at`).
- `[x]` Reliability tests expanded for lease and retry determinism across non-download lanes (`claim_upload_job_recovers_expired_lease_and_claims`, `claim_action_jobs_respects_retry_schedule_and_recovers_stale_leases`, `upload_retry_wait_is_not_claimed_until_due`).
- `[x]` Planner/materializer reconciliation promoted to hard invariants in materialization path with targeted invariant coverage (`planner_materialization_invariant_rejects_upload_count_mismatch`).
- `[x]` Large-delete guard and conflict-safe-backup determinism coverage added in dedicated local-change tests (`resolve_large_delete_guard_*`, `create_safe_backup_*`, safe-backup artifact detection).
- `[x]` Remote delete guard/execution flow now derives remote presence/item IDs from `sync_files` DB authority (`read_remote_item_id_for_path`) with dedicated authority test coverage.
- `[x]` Planned upload execution no longer depends on cache-derived remote timestamps; planner-selected upload paths execute directly after claim/cooldown checks.
- `[x]` Large-delete guard pending/approval state migrated from JSON cache to lifecycle DB authority (`large_delete_guard_approved`, `large_delete_pending_paths_json`) with commands and runtime flow using lifecycle store accessors.
- `[x]` Upload retry cooldown behavior migrated off JSON maps to durable upload job retry scheduling (`retry_wait` + `next_retry_at`) with attempt-based backoff and terminal failure thresholding.
- `[x]` Remote module structure closeout verified (`remote_changes.rs` and `remote_pipeline_loop.rs` remain responsibility-scoped and within file size constraints).
- `[x]` Operational lifecycle state writes now route through lifecycle writer wrapper (`persist_lifecycle_operational_state`) with guard script ownership tightened.
- `[x]` Upload retry delay policy now has dedicated deterministic unit coverage (`upload_lane_tests::resolve_upload_retry_delay_is_exponential_and_capped`).

### Definition of Fully Done (Close Criteria)

- `[x]` No flow-critical sync decision depends on JSON/in-memory mirrors (delete gating, upload retry gating, and large-delete approval now DB-authoritative).
- `[x]` Planner actions materialize to jobs for all relevant action types.
- `[x]` One lifecycle writer path is fully enforced by guardrails and invariants (phase/activity/scan-complete/issue + operational-state persist entry points guarded).
- `[~]` Restart/pause/resume/retry deterministic from DB state (broad coverage added; remaining matrix gaps tracked above).
- `[x]` `cargo check`, `cargo test`, `npm run typecheck`, and sync guard scripts all pass (current branch snapshot).

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

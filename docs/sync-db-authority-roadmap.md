# SomeDrive Sync DB Authority Roadmap

## Objective

Move SomeDrive sync from a hybrid model (JSON + in-memory + partial DB queue) to a DB-authoritative model where discovery, planning, execution, retries, and counters are persisted and recoverable.

## Phase Definition

### Phase 1 (Completed)

- Added durable download queue groundwork.
- Added queue/worker instrumentation for stall diagnosis.
- Added initial DB-backed download queue orchestration and counters.
- Added three-silo UI layout (Discovery, Downloads, Uploads).

### Phase 2 (This implementation pass)

Primary outcome: both transfer lanes are represented in durable job state and UI metrics are updated by durable aggregates where available.

Scope:

1. Keep download lane durable queue flow active.
2. Add upload lane durable job records (`direction = upload`) for execution tracking.
3. Publish upload counters from DB into runtime snapshot.
4. Keep bootstrap/reconcile safety semantics unchanged.
5. Preserve all diagnostic log markers.

Deliverables:

- `sync_jobs` supports download and upload directions.
- Upload jobs transition through durable states at runtime.
- UI exposes lane-specific metrics in vertical silo layout.

Non-goals in Phase 2:

- Full DB planner graph (`sync_files`) as source of truth for conflict/action derivation.
- Removal of existing `sync_state.json` discovery/baseline storage.

### Phase 3 (Next)

Primary outcome: DB becomes the single source of truth for discovery + planning + execution.

Scope:

1. Add canonical file index (`sync_files`) and planner state machine.
2. Write remote/local observations into `sync_files` first.
3. Derive actions (`download`, `upload`, deletes, conflict actions) from planner transitions.
4. Enqueue all actions into durable `sync_jobs`.
5. Remove hybrid authority from in-memory sets and JSON operational state.
6. Finalize crash-recovery and restart determinism across all action types.

## Data Model (Target)

## `sync_jobs`

- Current authoritative execution queue.
- Directions: `download`, `upload`.
- States: `queued`, `in_progress`, `retry_wait`, `done`, `failed_terminal`, `skipped`.

## `sync_files` (Phase 3)

- Canonical planner index.
- Source flags: remote/local discovery state.
- Metadata snapshots and reconciliation fields.
- Desired action and conflict resolution ownership.

## Acceptance Criteria

### Phase 2 Done

- No producer hard-stall from bounded enqueue backpressure.
- Download queue metrics sourced from DB counters.
- Upload execution emits durable job transitions and DB-based totals.
- `cargo check` and `npm run typecheck` pass.

### Phase 3 Done

- Every sync decision is DB-derived.
- Restart after crash resumes deterministically from persisted state.
- UI counters are fully DB-derived and mathematically consistent.
- Hybrid operational state paths removed.

## File Touchpoints

- `src-tauri/src/app/sync_engine/job_queue.rs`
- `src-tauri/src/app/sync_engine/remote_changes.rs`
- `src-tauri/src/app/sync_engine/graph_transfer.rs`
- `src-tauri/src/app/sync_engine/runtime_and_models.rs`
- `src-tauri/src/app/sync_runtime.rs`
- `src/components/accounts/AccountSyncActivityPanel.tsx`
- `src/types/somedrive.ts`
- `src/styles/accounts.css`

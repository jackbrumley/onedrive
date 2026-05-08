# SomeDrive Sync Reliability Rebuild Plan

## Purpose of this document

This document is a full handoff/compaction of the sync reliability investigation and decisions from the current debugging session. It is intended to let a **new chat with zero history** continue implementation without re-explaining goals, constraints, root-cause findings, or architecture direction.

The primary objective of this pass is:

1. Make sync architecture safe and reliable.
2. Remove stall/deadlock-prone behavior.
3. Keep UI **debug-first** and explicit (do not hide complexity yet).
4. Ensure metrics are truthful and understandable.

---

## High-level status

### Already fixed in prior commits

1. **Token refresh thrash fix**
   - Commit: `89ec2e6`
   - Summary: Shared token state across workers + deduped 401 refresh.
   - Result: Massive `GRAPH_DOWNLOAD_401_REFRESH` storm improved.

2. **Download temp-file collision + retry hardening**
   - Commit: `7d150c2`
   - Summary: Unique temp file names for downloads; retry stream/finalize failures.
   - Result: Reduced file-finalization race errors (for example Ratchet & Clank backup files).

3. **Progress telemetry/UI refactor (session totals + clearer labels)**
   - Commit: `5a69a51`
   - Summary: Added explicit planned/remaining/in-flight style metrics and UI wording updates.
   - Note: This improved wording/shape, but reliability issue still exists.

### New instrumentation added in this session (not yet committed)

Additional logging instrumentation has been added in:

- `src-tauri/src/app/sync_engine/remote_changes.rs`
- `src-tauri/src/app/sync_engine/preamble.rs` (atomic import extension)

This includes:

- producer lifecycle logs,
- download worker lifecycle logs,
- stall-context enrichment,
- enqueue wait logging (`DOWNLOAD_ENQUEUE_WAITING`),
- pipeline drain/complete logs.

This instrumentation produced decisive evidence of the current root issue (see below).

---

## Product/architecture intent (confirmed with user)

The user explicitly wants:

- **Correctness over shortcuts**.
- Reliability over appearance/UI polishing.
- No shotgun fixes; root-cause-first.
- Debug-visible UI during stabilization (not hiding internals yet).
- Clear stage semantics and trustworthy counters.

The user is open to larger change surface when justified, especially for core sync loop reliability.

---

## Confirmed root-cause findings from logs

### 1) Queue backpressure hard-stall is real and reproducible

Observed logs include repeated entries like:

- `DOWNLOAD_ENQUEUE_WAITING ... wait_s=5,10,15...165 ... pending_downloads=1032 ... cancel_requested=false`

Interpretation:

- Producer/scanner is blocked waiting on `download_tx.send(job).await`.
- Queue is full and no slots become available quickly enough.
- The producer cannot move forward while blocked on enqueue.
- Sync appears frozen though process remains alive.

This is not speculative; instrumentation proved the blocked enqueue path directly.

### 2) "Cycle already running" symptoms previously observed

Earlier logs showed old cycle IDs still producing retry logs after new cycle start in some runs. This suggested incomplete cycle teardown/orphaned task behavior in failure scenarios.

Even if not always reproduced now, architecture should guarantee deterministic cycle teardown and non-overlap.

### 3) Stream decode/read flakiness is frequent in real workload

Many files (especially large media) emit:

- `Failed reading download stream: error decoding response body`

Retries recover some but not all. This contributes to queue pressure and can starve throughput.

---

## Why current model stalls

Current remote pipeline couples producer and consumer with a bounded in-memory channel.

Simplified failure cycle:

1. Workers slow down or hang long enough on streams/retries.
2. Bounded queue fills.
3. Producer blocks on send awaiting free slot.
4. Discovery/progress path stops advancing.
5. User perceives stall; lock can be held for long periods.

The core issue is architectural coupling and queue backpressure behavior under adverse I/O.

---

## Decision: reliability-first architecture change

### Chosen direction

Move to a **durable job queue model** (SQLite-backed) as the source of truth for work and metrics.

This is intentionally a bigger change because this loop is core functionality and currently fragile.

### Explicitly keep

- Internal phase/state machine (for safety).
- Debug-forward UI with transparent counters.

### Explicitly avoid for now

- UI simplification that hides useful debug signals.
- one-off local hacks that do not solve producer/consumer coupling.

---

## Sync state machine (keep explicit)

Internal states should remain explicit and enforced:

1. `Bootstrap.DiscoverRemote`
2. `Bootstrap.ApplyRemote`
3. `Bootstrap.ReconcileLocalBaseline`
4. `Live.Bidirectional`

Rationale:

- Enables safe bootstrap (do not upload/delete local changes before remote truth and baseline are established).
- Keeps behavior deterministic and debuggable.

---

## Metric model (debug-first, unambiguous)

Do not conflate remote and local lanes.

For `remote -> local` lane:

- `remote_discovered_total`
- `download_planned_total`
- `download_completed_total`
- `download_failed_total`
- `download_in_progress`
- `download_remaining = planned - completed - failed_terminal - in_progress` (clamped >= 0)
- `remote_scan_complete` (bool)

For `local -> remote` lane (later phase):

- `upload_planned_total`
- `upload_completed_total`
- `upload_failed_total`
- `upload_in_progress`
- `upload_remaining`

### UI wording (for this pass)

Use explicit labels:

- `Remote discovered`
- `Need download`
- `Downloaded`
- `Downloading now`
- `Remaining`
- `Retry waiting` (if applicable)

Avoid ambiguous labels like `Queued` without direction.

Phase message should not duplicate numeric counters.

---

## Durable queue proposal

## Storage

Use SQLite-backed persistent tables. If there is already a project DB, add table(s) there; otherwise create dedicated sync DB under app data.

Current sync state is JSON-based (`sync_state.json`), not SQLite, in:

- `src-tauri/src/app/sync_engine/path_state.rs`

So introducing DB queue is a new persistence layer for jobs, not a tweak to current JSON state.

### Core table: `sync_jobs`

Suggested fields:

- `id` (PK)
- `profile_id`
- `direction` (`download` | `upload`)
- `item_id` (remote item id or deterministic local job id)
- `path`
- `state` (`queued`, `in_progress`, `retry_wait`, `done`, `failed_terminal`, `skipped`)
- `attempt_count`
- `last_error`
- `next_retry_at`
- `lease_owner`
- `lease_until`
- `created_at`
- `updated_at`
- `started_at`
- `finished_at`

Constraints/indexes:

- Unique: `(profile_id, direction, item_id)`
- Index for scheduler: `(profile_id, direction, state, next_retry_at)`
- Index for lease recovery: `(profile_id, direction, lease_until)`

### Optional table: `sync_job_events` (debug)

Can be added later for postmortem/diagnostics.

---

## Worker scheduler model (durable)

Workers should claim jobs transactionally from DB, not receive from bounded in-memory queue.

Flow:

1. Poll eligible jobs: `queued` or `retry_wait` where `next_retry_at <= now`.
2. Atomically transition selected jobs to `in_progress` with lease.
3. Execute transfer.
4. Transition to `done` / `retry_wait` / `failed_terminal`.
5. Keep retry policy in state transitions.

Lease handling:

- On startup/recovery, reclaim stale `in_progress` jobs whose lease expired.

This eliminates producer send-block deadlocks and makes pause/restart/crash recovery deterministic.

---

## Immediate implementation strategy (phased)

### Phase 0: keep new instrumentation and gather baseline

Files already instrumented:

- `src-tauri/src/app/sync_engine/remote_changes.rs`

Ensure instrumentation commit is kept before large refactor.

### Phase 1: schema + repository layer

Add module(s), likely under:

- `src-tauri/src/app/sync_engine/` (or `src-tauri/src/app/storage/` if preferred)

Responsibilities:

- create/open DB
- upsert planned jobs
- claim jobs
- update state transitions
- aggregate counters for UI/runtime

### Phase 2: redirect remote producer to durable queue

In `remote_changes.rs`:

- producer writes job records,
- no blocking `mpsc::send` in discovery path,
- page discovery can continue independently.

### Phase 3: move download workers to DB-claim model

Replace in-memory queue consumption with DB job claim loop.

### Phase 4: runtime counters sourced from durable aggregates

Update runtime status builder to derive metrics from DB job states.

Likely touch:

- `src-tauri/src/app/sync_runtime.rs`
- `src-tauri/src/app/sync_engine/runtime_and_models.rs`
- `src/components/accounts/AccountSyncActivityPanel.tsx`
- `src/types/somedrive.ts`

### Phase 5: local upload lane integration

After bootstrap barrier, generate upload jobs into same durable system.

### Phase 6: remove obsolete queue/counter paths

Cleanly retire old in-memory queue orchestration fields once durable model is primary.

---

## Guardrails and non-goals for this pass

### Do

- prioritize deterministic behavior,
- preserve/expand diagnostic logs,
- keep phase model explicit,
- ensure counters are mathematically consistent.

### Do not

- hide phases in UI prematurely,
- merge remote/local counters into one opaque metric,
- introduce cosmetic-only changes that obscure debug visibility.

---

## Current touched files map (for new chat continuity)

Recently modified in this arc:

- `src-tauri/src/app/sync_engine/cycle.rs`
- `src-tauri/src/app/sync_engine/graph_transfer.rs`
- `src-tauri/src/app/sync_engine/runtime_and_models.rs`
- `src-tauri/src/app/sync_engine/remote_changes.rs`
- `src-tauri/src/app/sync_runtime.rs`
- `src/components/accounts/AccountSyncActivityPanel.tsx`
- `src/types/somedrive.ts`
- `src-tauri/src/app/sync_engine/preamble.rs`

Recent commits to reference:

- `89ec2e6` token sharing + refresh dedupe
- `7d150c2` temp-file collision + stream/finalize retry hardening
- `5a69a51` progress telemetry/UI stat restructuring

---

## Validation and observability checklist

For each milestone:

1. `cargo fmt`
2. `cargo check`
3. `npm run typecheck`
4. Run realistic sync workload and verify:
   - no indefinite enqueue waits,
   - no persistent `cycle already running` loop due to hidden lock retention,
   - counters match state transitions,
   - restart/pause/resume recover correctly.

Log markers that must remain visible during refactor:

- `SYNC_CYCLE_START`
- `REMOTE_PIPELINE_START`
- producer/worker lifecycle markers
- retry markers (`DOWNLOAD_RETRY*`)
- pipeline completion/drain
- stall context markers

---

## Open design choices (default decisions)

If unresolved in a new chat, use these defaults:

1. **Reset session totals when?**
   - Reset on explicit worker stop/pause/app restart.
   - Do not reset on retry of same run.

2. **Retry policy for stream decode/body failures**
   - Keep existing exponential retry, transition to `retry_wait` in DB.

3. **Stall detection**
   - For durable queue model, stall should consider both producer progress and job-state churn.
   - Avoid declaring stall solely on page/result events.

4. **Close behavior**
   - Recovery-first model remains primary.
   - Graceful close improvements are optional optimization, not correctness dependency.

---

## What success looks like

The sync loop is considered fixed when:

- Producer never blocks indefinitely on in-memory queue sends.
- Workers can be slow/flaky without freezing discovery pipeline.
- Job state survives restart/crash with deterministic recovery.
- UI metrics directly reflect durable job states and are understandable.
- Initial sync completes consistently on large/slow datasets.

---

## Immediate next action for a new chat

1. Commit current instrumentation changes if not yet committed.
2. Implement durable job schema + repository layer.
3. Replace discovery->channel enqueue path with discovery->DB upsert.
4. Move download workers to DB claim model.
5. Rewire runtime/UI counters to DB aggregate queries.

This sequence minimizes rework and keeps debuggability high while moving to correct architecture.

# Sync UI Single-Authority CodePerfect Plan

## Goal

Deliver a CodePerfect sync telemetry pipeline where the UI always renders one authoritative truth that matches backend state, with no split authority and no heuristic reconstruction.

This plan replaces mixed-source runtime fields with a canonical projection path and preserves renderer stability under high-load sync workloads.

---

## Problem Statement (Current Defect)

During large sync runs, the UI can show inconsistent lanes (for example planner/discovery at zero while executor shows active/planned work).

Root cause is split authority:

1. `sync-status` live events are emitted from partially mutated in-memory runtime fields.
2. Some lane fields (planner/discovery bytes) are only set by DB hydration during snapshot fetch.
3. UI mixes these fields in one panel as if they share one authority.

Result: semantically incompatible values can be rendered together.

---

## Non-Negotiable Invariants

1. Every UI lane field must originate from one canonical builder.
2. Live `sync-status` and snapshot payloads must be shaped by the same builder.
3. UI must not infer cross-lane counts or synthesize totals.
4. If canonical projection fails, emit explicit degraded state; do not fabricate values.
5. High-frequency updates may be throttled, but values must remain exact at each emitted tick.

---

## Target Architecture

## 1) Canonical Runtime Status Builder (Backend)

Create one builder function that assembles `SyncRuntimeAccountStatus` from authoritative sources:

- lifecycle state row,
- `sync_files` planner/discovery counters,
- `sync_jobs` executor counters and activity lists,
- persisted issue state.

All lane fields used by UI must be assigned here.

## 2) Unified Event + Snapshot Shaping

- Snapshot command (`get_sync_runtime_snapshot`) already uses DB hydration; keep this.
- Change live `sync-status` event emission to pass through canonical builder before serialize/emit.
- Remove direct emit dependence on partially updated runtime-map fields for lane telemetry.

## 3) Coalesced Emit Scheduler

Keep the current 1s throttle model, but shift to canonical emit flushes:

- mark account dirty on runtime mutation,
- emit canonical projection at interval (1s default),
- immediate canonical flush for phase/issue/start/finish transitions.

This keeps UI stable and accurate under heavy transfer frequency.

---

## Implementation Phases

## Phase A - Canonical Builder Extraction

1. Extract/standardize canonical account projection logic (currently split between runtime emit and hydrate path).
2. Ensure planner fields, discovery fields, executor bytes/counts, lifecycle fields, and issue fields are all populated from DB/lifecycle authority.
3. Make canonical builder return `Result<SyncRuntimeAccountStatus, String>` with hard errors for invalid lifecycle invariants.

Deliverable:
- One reusable canonical builder callable from both snapshot command and event emitter.

## Phase B - Event Pipeline Migration

1. Update `sync-status` emit path to call canonical builder for target account.
2. Keep per-account throttling, but throttle canonical payload emission, not partial runtime state.
3. On emit failure/projection failure, log structured reason and set degraded telemetry marker in status payload.

Deliverable:
- Live events and snapshot are field-identical in source semantics.

## Phase C - Runtime Map Responsibility Narrowing

1. Restrict in-memory runtime map to transition triggers + minimal transient transfer details only.
2. Remove or de-emphasize fields that imply authority but are not canonical.
3. Ensure no lane field in UI relies on non-canonical runtime mirrors.

Deliverable:
- Runtime map is transport/support state, not competing authority.

## Phase D - UI Contract Tightening

1. Keep panel lane rendering exactly mapped to canonical fields.
2. Remove fallback expressions that choose between planner/executor mirrors.
3. Keep consistency section explicit and visible when violations exist.
4. Rename any misleading labels (for example "snapshot update") only if event cadence, not snapshot fetch, drives updates.

Deliverable:
- UI surfaces precise authoritative values without mixed semantics.

## Phase E - Verification + Hardening

1. Add backend tests for canonical builder under:
   - initial sync bootstrap,
   - heavy download in-flight,
   - retry_wait transitions,
   - pause/resume,
   - restart hydration mid-cycle.
2. Add tests confirming live event payload and snapshot payload are semantically equivalent for same DB state.
3. Keep consistency invariant tests for planner->executor materialization gaps and overcommit conditions.

Deliverable:
- Deterministic, test-backed authority guarantees.

---

## Concrete File Touch Plan

Primary backend:

- `src-tauri/src/app/sync_runtime.rs`
  - replace direct partial emit shaping with canonical projection call
  - keep throttle scheduler but emit canonical status
- `src-tauri/src/app/sync_engine/job_queue_activity_projection.rs`
  - formalize reusable canonical account builder
- `src-tauri/src/app/commands/sync_runtime.rs`
  - ensure snapshot uses same canonical builder API

Secondary backend:

- `src-tauri/src/app/sync_engine/lifecycle_writer.rs`
  - keep transition writers; no lane-authority side writes

Frontend:

- `src/components/accounts/AccountSyncActivityPanel.tsx`
  - consume canonical fields only, remove mixed fallbacks
- `src/types/somedrive.ts`
  - final authoritative shape with no legacy aliases

---

## Performance Guardrails (50k+ files / 100GB class workloads)

1. Throttle canonical emit frequency per account to 1000ms default.
2. Allow immediate flushes for major state transitions only.
3. Avoid full-account DB rescans on every tiny mutation if not needed:
   - optionally add a lightweight projection cache keyed by account revision and invalidation triggers.
4. Keep payload size bounded for activity lists (existing limits remain).

---

## Acceptance Criteria

1. No lane mismatch where planner/discovery shows zero while executor is materially active for same cycle (unless explicitly valid by phase and documented).
2. Live `sync-status` and snapshot command produce equivalent lane semantics.
3. UI never shows fabricated totals due to missing fields.
4. Heavy Linux sync runs do not white-screen from event flood.
5. `cargo check`, `cargo test`, `npm run typecheck` pass.

---

## Out of Scope

1. Reworking sync engine planner logic itself.
2. Backward-compatibility shims for old telemetry payloads.
3. Cosmetic redesign unrelated to telemetry authority.

---

## Execution Order (Recommended)

1. Canonical builder extraction.
2. Event emitter migration to canonical payloads.
3. UI fallback removal and label cleanup.
4. Equivalence + invariant tests.
5. Heavy real-world sync validation on Fedora.

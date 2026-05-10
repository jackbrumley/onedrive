# Sync Status Single-Pipeline Implementation Plan

## Purpose
This document captures the agreed architecture and implementation plan for making sync status/activity UI deterministic, authoritative, and push-driven.

It is written to support immediate continuation in a new chat/session without rediscovery.

---

## Core Philosophy (Agreed)

1. **Single source of truth** for all user-visible sync state.
2. **Single write path** in backend for phase/activity/progress/counters/issues.
3. **Single status stream** to frontend that drives all sync UI elements.
4. **No UI heuristics** for lifecycle truth when canonical fields exist.
5. **No band-aids** that mask backend drift in frontend.
6. **If UI is wrong, there must be one place to debug** (pipeline contract/producer/consumer), not many.

This explicitly includes:

- current activity stage,
- progress bars,
- transfer in-progress/queued/retrying/error lists,
- warnings and issue surfaces,
- all sync metrics/counters shown on the screen.

---

## Product/UX Intent (Agreed)

- Clicking retry should move item out of error state immediately according to authoritative queue state.
- UI must show real progression at all times, not static labels.
- If paused, UI must never show fake active progression.
- If telemetry becomes stale, UI must say so clearly and suppress active animations.
- Future UX can be simplified to a single status line/card, but still powered by the same pipeline.

---

## Target Architecture

### 1) Authority and Storage

- SQLite-backed sync lifecycle/activity projection is authoritative for user-visible sync state.
- Runtime in-memory map is a delivery cache, not competing authority.

### 2) Central Writer

- Introduce (or enforce) one central backend writer, conceptually `status_writer`.
- All mutations to phase/activity/progress/counters/issues must route through this writer.
- Writer must:
  - update runtime map,
  - persist authoritative lifecycle/activity state,
  - increment sequence,
  - emit event.

### 3) Push-First Event Stream (No Periodic Polling)

- Frontend consumes one Tauri event channel (example: `sync-status`).
- Every status mutation emits an event payload with monotonic sequence.
- Remove periodic runtime polling as normal operation.

### 4) Sequence and Ordering

- Use monotonic `statusSeq` per account (recommended) or global.
- Frontend only applies payload when `incomingSeq > currentSeq`.
- Out-of-order/stale events are dropped.

### 5) Gap/Recovery Strategy (Push-Only Operational, Snapshot Recovery)

No periodic polling, but explicit one-shot recovery when needed:

1. subscribe to stream,
2. expect full initial snapshot event or explicitly request one snapshot,
3. if sequence gap detected, fetch full authoritative snapshot once,
4. continue stream from latest seq.

This is not periodic polling; it is deterministic recovery.

---

## Canonical Payload Contract (Proposed)

All UI sync surfaces should derive from one normalized payload shape.

```json
{
  "accountId": "profile-...",
  "statusSeq": 10234,
  "updatedAt": "2026-...",
  "phase": "scanning_local",
  "phaseMessage": "Scanning local files",
  "currentActivity": {
    "stage": "scanning_local",
    "progressMode": "determinate",
    "current": 12540,
    "total": 45623,
    "unit": "files",
    "detail": "Pictures/Archive/...",
    "cycleId": "177...",
    "updatedAt": "2026-..."
  },
  "downloads": {
    "plannedTotal": 43861,
    "completedTotal": 43861,
    "failedTotal": 0,
    "inFlight": 0,
    "retryWaiting": 0,
    "remainingCount": 0,
    "plannedBytesTotal": 0,
    "completedBytesTotal": 114000000000,
    "remainingBytesTotal": 0,
    "inFlightBytesDone": 0,
    "throttleTotal": 16949,
    "throttleLastMinute": 0
  },
  "uploads": {
    "plannedTotal": 0,
    "completedTotal": 0,
    "failedTotal": 0,
    "inFlight": 0,
    "retryWaiting": 0,
    "remainingCount": 0,
    "plannedBytesTotal": 0,
    "completedBytesTotal": 0,
    "remainingBytesTotal": 0,
    "inFlightBytesDone": 0,
    "throttleTotal": 0,
    "throttleLastMinute": 0
  },
  "transfers": {
    "inProgress": [],
    "retryWaiting": [],
    "failed": [],
    "completed": []
  },
  "issue": {
    "code": null,
    "severity": "none",
    "message": null,
    "actions": [],
    "path": null,
    "secondaryPath": null
  },
  "flags": {
    "remoteScanComplete": true,
    "twoWayReady": false,
    "telemetryStale": false
  }
}
```

Notes:

- `issue.severity` should be backend-owned to avoid duplicated blocking logic across components.
- `currentActivity.progressMode` drives bar rendering (`determinate`, `indeterminate`, `hidden`).

---

## Non-Negotiable Implementation Rules

1. No direct `sync_runtime::set_*` calls outside central status writer for authoritative fields.
2. No free-form phase-message regex parsing for behavior.
3. No `lastSyncAt` fallback for readiness truth.
4. No page-specific custom sync-state computation; use one selector or backend `effectiveState`.
5. No active animation when stale/no-active-work/paused.

---

## Current Known Debt Snapshot

Already improved recently:

- phase writes centralized in key pause/worker paths,
- monotonic snapshot application guard in frontend,
- structured `currentActivity` field added and used,
- sync-state selector unification started,
- stale telemetry visual guard added.

Still needs completion to satisfy the full architecture:

- remove remaining split-authority paths entirely,
- move to true push-first stream as primary transport,
- unify all sync UI elements to one canonical store payload,
- formalize backend `issue.severity` and effective state contract,
- add hard enforcement against write-path bypasses.

---

## Execution Plan (Phased)

### Phase A - Event Stream Backbone

1. Add canonical `sync-status` event payload and emitter in central writer.
2. Ensure every authoritative state mutation increments `statusSeq` and emits.
3. Add explicit initial full-status event or startup snapshot emit.

### Phase B - Frontend Push Reducer

1. Create one reducer/store for sync status keyed by account.
2. Subscribe to `sync-status` event stream.
3. Apply only newer seq values.
4. Remove periodic runtime polling loop.

### Phase C - Gap Recovery (One-shot)

1. Detect seq gaps/disconnect.
2. Fetch full authoritative snapshot once.
3. Continue stream from recovered seq.

### Phase D - Full UI Unification

1. Ensure all sync widgets consume same store fields.
2. Remove duplicated per-page state logic.
3. Remove string parsing and inference fallbacks.

### Phase E - Guardrails and Tests

1. Add invariant logs/assertions for unauthorized writer bypass.
2. Add CI grep/lint rule to block direct phase writes outside writer.
3. Add transition tests for pause/resume/retry/startup-restore/error.

---

## Verification Matrix

Run and verify DB/payload/UI agreement for each:

1. Startup while paused
2. Resume from paused
3. Pause during scanning local
4. Pause during building index
5. Pause during planning
6. Retry failed file -> queued -> in_progress -> done
7. Retry failed file -> queued -> in_progress -> failed_terminal
8. Retry-all mixed outcomes
9. Startup restore with interrupted cycle
10. Simulated delayed/out-of-order event delivery
11. Stream disconnect/reconnect + snapshot recovery

For each case, assert:

- sequence monotonicity,
- no contradictory labels between pages,
- no fake progress when paused/stale,
- error/warning/activity lists reflect authoritative transfer states.

---

## Definition of Done

The status pipeline migration is complete only when:

1. One central writer path controls authoritative status.
2. Push stream is primary transport (no periodic poll loop).
3. UI renders one canonical payload contract for all sync surfaces.
4. Sequence ordering and gap recovery are deterministic.
5. Pause/resume/retry/startup-restore are consistent in DB, payload, and UI.
6. `cargo check` and `npm run typecheck` pass.

---

## Notes for New Chat Continuation

If implementation resumes in a new chat:

1. Start with **Phase A** (event stream backbone),
2. do not add UI-only workarounds first,
3. keep this document as the implementation contract,
4. update this file as phases complete.

---

## Locked Decisions (Implemented)

1. Sequence scope: **per-account** `statusSeq`.
2. Event channel: **`sync-status`**.
3. Gap policy: detect seq jumps and run one-shot `get_sync_runtime_snapshot` recovery.
4. Initial load policy: fetch snapshot once and subscribe to stream; request best-effort initial event snapshot.

---

## Phase Completion Record

- **Phase A - Event Stream Backbone:** Completed.
  - Added backend `sync-status` event payload with per-account monotonic sequence.
  - Wired event stream initialization at app bootstrap.
  - Added `request_sync_status_snapshot` command for initial stream hydration.

- **Phase B - Frontend Push Reducer:** Completed.
  - Added frontend `sync-status` listener and account-scoped sequence application.
  - Switched sync runtime updates to push events for normal operation.
  - Removed periodic runtime polling loop.

- **Phase C - Gap Recovery:** Completed.
  - Added sequence gap detection and one-shot snapshot recovery path.

- **Phase D - Full UI Unification:** Completed for current known drift points.
  - Centralized transfer reset path in backend runtime helpers.
  - Removed phase-message regex parsing dependency in sync activity warnings.

- **Phase E - Guardrails and Tests:** Completed.
  - Added `check:sync-status-writes` guard script to block direct `sync_runtime` write calls outside allowed writer files.
  - Verified `cargo check`, `npm run typecheck`, and guard script pass.

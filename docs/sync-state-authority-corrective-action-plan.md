# Sync State Authority Corrective Action Plan

## Purpose
This document defines the corrective actions required to eliminate sync UI state drift and enforce a single source of truth for runtime sync activity/status.

It is intentionally detailed so implementation can continue in a new chat/session without losing context.

## Problem Statement
Current sync status behavior can become stale or contradictory (for example, paused UI showing active work) because status authority is split across:

- in-memory runtime map updates,
- SQLite lifecycle/activity hydration,
- frontend heuristics and fallback inference,
- asynchronous polling/revision gating race conditions.

This violates project standards for single authority, one write path, and UI-as-renderer.

## Scope
In scope:

- Backend sync lifecycle/runtime status pipeline
- Runtime snapshot/revision semantics
- Frontend sync status rendering and state derivation
- Pause/resume/retry/status consistency

Out of scope (unless encountered as blockers):

- Graph transfer algorithm changes unrelated to state authority
- Packaging/deployment changes

---

## Findings Inventory

### F1 - Critical: Revision does not represent DB-hydrated truth
- **Files**
  - `src-tauri/src/app/sync_runtime.rs`
  - `src-tauri/src/app/commands/sync_runtime.rs`
  - `src/hooks/appRuntime/refresh.ts`
- **Issue**
  - Snapshot revision is computed before DB hydration and only tracks in-memory runtime bumps.
  - Frontend may skip applying fresh DB-derived state when revision appears unchanged.
- **Risk**
  - UI silently displays stale activity/error/phase.
- **Root-cause fix**
  - Make revision authoritative for DB + runtime changes, or remove equality-only gating and apply monotonic snapshot logic.

### F2 - High: Direct phase writes bypass centralized persistence writer
- **Files**
  - `src-tauri/src/app/sync_engine/worker_lifecycle.rs`
  - `src-tauri/src/app/sync_engine/preamble.rs`
  - writer exists in `src-tauri/src/app/sync_engine/runtime_and_models.rs`
- **Issue**
  - Multiple direct calls to `sync_runtime::set_phase(...)` bypass lifecycle DB persistence path.
- **Risk**
  - Phase reverts during hydration; pause/resume can appear inconsistent.
- **Root-cause fix**
  - Route all phase/activity writes through one centralized writer API that updates runtime + DB together.

### F3 - High: Out-of-order snapshot overwrite on frontend
- **Files**
  - `src/hooks/appRuntime/refresh.ts`
- **Issue**
  - Older responses can overwrite newer state because only `===` is checked.
- **Risk**
  - UI time-travel/backward transitions.
- **Root-cause fix**
  - Enforce monotonic guard (`reject snapshot.revision < current.revision`) and optionally request sequencing.

### F4 - High: Current activity is heuristic, not authoritative contract
- **Files**
  - `src/components/accounts/AccountSyncActivityPanel.tsx`
- **Issue**
  - Frontend derives stage/progress from mixed counters/phase text.
- **Risk**
  - Plausible but incorrect status text and bars.
- **Root-cause fix**
  - Backend must emit structured `current_activity`; frontend renders it directly.

### F5 - High: Sync state logic duplicated and inconsistent across pages
- **Files**
  - `src/components/accounts/AccountCard.tsx`
  - `src/pages/AccountDetailPage.tsx`
  - `src/components/accounts/AccountDetailUnifiedPanel.tsx`
- **Issue**
  - Different logic for "syncing" and blocking conditions.
- **Risk**
  - Conflicting labels/controls for same account.
- **Root-cause fix**
  - Centralize one selector or backend-provided effective state/severity.

### F6 - Medium: Readiness fallback inferred from `lastSyncAt`
- **Files**
  - `src/components/accounts/AccountCard.tsx`
  - `src/components/accounts/AccountDetailUnifiedPanel.tsx`
- **Issue**
  - `twoWayReady` falls back to timestamp inference.
- **Risk**
  - False readiness messaging.
- **Root-cause fix**
  - Use authoritative `twoWayReady` only; show unknown state if unavailable.

### F7 - Medium: UI parses free-form phase strings
- **Files**
  - `src/components/accounts/AccountSyncActivityPanel.tsx`
- **Issue**
  - Regex extraction from phase message drives behavior.
- **Risk**
  - Breakage on copy change; non-contract behavior.
- **Root-cause fix**
  - Add explicit structured cooldown/retry metadata fields.

### F8 - Medium: Runtime fetch failures are silent
- **Files**
  - `src/hooks/appRuntime/refresh.ts`
- **Issue**
  - No stale/disconnected indicator.
- **Risk**
  - Animated UI can appear live while data is stale.
- **Root-cause fix**
  - Add freshness tracking and stale-state rendering behavior.

---

## Target Architecture

### A. Single Authority
- SQLite lifecycle/runtime projection is authoritative for user-visible sync state.

### B. One Writer Path
- One backend writer API updates both in-memory runtime and persisted lifecycle/status.
- No direct side writes allowed for phase/activity fields.

### C. Structured Activity Contract
Backend exposes an explicit contract, for example:

```json
{
  "stage": "scanning_local",
  "progressMode": "determinate",
  "current": 12540,
  "total": 45623,
  "unit": "files",
  "detail": "Pictures/Archive/...",
  "updatedAt": "...",
  "cycleId": "..."
}
```

### D. UI Rendering Rule
- Frontend renders authoritative fields only.
- No inferred/fallback business logic for lifecycle truth.

---

## Corrective Work Plan

### Phase 1 - Authority Lockdown (must complete first)
1. Replace all direct `sync_runtime::set_phase(...)` calls in sync engine paths with centralized writer.
2. Ensure pause/resume/cancel paths persist lifecycle phase immediately.
3. Add guard logs for any legacy bypasses detected.

### Phase 2 - Revision/Freshness Integrity
1. Fix snapshot monotonicity in frontend (`<` reject logic).
2. Align snapshot revision semantics with authoritative DB-hydrated state.
3. Add stale-data indicator + suppress active animations when stale.

### Phase 3 - Structured Current Activity
1. Define and emit backend `current_activity` object.
2. Remove heuristic current-activity builder from UI.
3. Replace message regex parsing with structured fields.

### Phase 4 - Selector and Messaging Unification
1. Create one shared sync-state/issue-severity selector.
2. Remove duplicated logic in Card/Detail/Panel.
3. Remove `lastSyncAt` readiness fallback.

### Phase 5 - Cleanup and Hardening
1. Remove dead/legacy status paths.
2. Add invariants/tests for authority consistency.
3. Update docs/contracts where needed.

---

## Acceptance Criteria

1. No user-visible phase/activity field is written outside central writer.
2. Pause/resume/cancel always produce immediate and stable UI state across polling/hydration.
3. Frontend cannot regress to older snapshot state.
4. Current Activity card is fully backend-driven and never heuristic.
5. Same account shows identical effective state across Card and Detail.
6. UI never animates progress when data is stale or no active work exists.
7. `cargo check` and `npm run typecheck` pass.

---

## Verification Matrix

Run each flow and verify DB + UI agreement:

1. Startup while paused
2. Resume -> active stage progression
3. Pause during each stage (scanning local, building index, planning, downloading)
4. Retry failed item -> queued -> in_progress -> done or failed_terminal
5. Retry-all with mixed outcomes
6. Startup restore after interrupted cycle
7. Error transition and recovery
8. Initial sync blocked -> unblocked -> two-way ready

For each flow, confirm:
- lifecycle DB phase/activity,
- runtime snapshot payload,
- visible UI stage/progress,
- no contradictory labels between screens.

---

## Notes for Future Sessions

- Treat this document as the execution baseline for the authority audit remediation.
- If implementation starts in a new chat, begin with **Phase 1**, not UI polish.
- If additional drift is discovered, append it to "Findings Inventory" before patching.

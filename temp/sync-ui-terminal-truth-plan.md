# SomeDrive Sync UI "Terminal Truth" Robustness Plan

## Objective

Replace abstract or heuristic sync UI behavior with a deterministic, backend-authoritative terminal-truth experience that reflects exactly what the sync engine is doing in real time.

This pass also removes legacy shims, fallback paths, and fail-soft band-aids that can mask drift between UI and backend state.

---

## Product Direction (Non-Negotiable)

1. UI renders authoritative truth, not inferred business state.
2. Numbers shown together must share compatible semantics and scope.
3. If state is invalid or incomplete, fail loud in dev and show explicit degraded state in prod.
4. Remove legacy compatibility paths that hide correctness issues.
5. Prefer hard invariants and explicit contracts over convenience heuristics.

---

## Primary Problems To Solve

1. Mixed counter semantics in UI:
   - Discovery/session counters, planner counters, and executor/job counters are currently displayed side-by-side without strict scope boundaries.
2. Frontend-derived arithmetic:
   - Remaining counts are partially computed client-side from mixed fields.
3. Legacy compatibility fields:
   - Legacy or derived progress fields still exist and can be consumed accidentally.
4. Hidden drift risk:
   - UI can look plausible while backend truth is inconsistent (especially during bootstrap, retry_wait, pause/resume, restart).
5. Abstracted user messaging:
   - Current sync panel framing can hide concrete operational state.

---

## Target Experience

A sync screen that reads like a live operations console:

- Lifecycle lane: exact phase, phase message, activity stage/detail/progress mode, cycle id, updated timestamp.
- Discovery lane: remote scan progress/cursor state and what discovery has observed.
- Planner lane: actionable counts by action (download/upload/delete_remote/delete_local/conflict/none).
- Executor lane: durable job counts by state (queued, claimed, running, retry_wait, failed_terminal, done) and bytes.
- Issue lane: explicit blockers with canonical recovery actions.
- Event feed lane: timestamped authoritative transitions and reasons.

No hidden transforms, no mixing incompatible counters.

---

## Scope

### In Scope

- Backend runtime payload contract revision for sync UI.
- DB-projected authoritative telemetry shaping.
- Sync screen refactor to terminal-truth layout.
- Strict invariants and consistency checks.
- Removal of legacy shims and fallbacks in sync state presentation path.
- Test matrix expansion for counter consistency and transition determinism.

### Out of Scope

- New sync engine behavior semantics unless needed to expose authority cleanly.
- Cosmetic-only redesign disconnected from state contract.
- Backward-compatibility shims for pre-refactor runtime fields.

---

## Architecture Contract (Single Source by Domain)

1. Lifecycle authority
   - Source: `sync_lifecycle_state`
   - Fields: `phase`, `phase_message`, `activity_*` fields, blocker/readiness state.
2. Planner authority
   - Source: `sync_files` projection
   - Fields: actionable counts by `desired_action`.
3. Executor authority
   - Source: `sync_jobs` projection
   - Fields: counts and bytes by state/direction plus retry-wait inventory.
4. UI contract authority
   - Source: backend command/event payload shaped from DB projection plus lifecycle row.
   - UI does not derive cross-domain truth.

---

## Fail-Hard Policy (Required)

### Remove and Block Legacy Shims

- Remove or deprecate legacy alias counters in payload that are ambiguous (or gate as deprecated and unrendered).
- Remove UI fallback formulas that reconstruct state from partial fields.
- Remove best-effort assumptions that coerce missing fields into plausible values.

### Enforce Hard Invariants

Backend should fail validation (tests and dev logs) when any of these occur:

1. `remaining_count != queued + claimed + running + retry_wait` per lane contract.
2. paused/idle/error phases reporting non-hidden activity progress mode.
3. running transfer animation with no running rows.
4. incompatible counter scopes rendered in same UI block.
5. lifecycle stage/progress fields missing in authoritative projection.

Production behavior:

- Do not silently fabricate values.
- Show explicit degraded-state banner with reason code if invariant fails.

---

## Implementation Plan

## Phase 0 - Contract Design and Mapping

1. Define new sync UI payload schema in TS and Rust (authoritative by lane).
2. Create field mapping table:
   - old field -> new field
   - authority source
   - status (`keep`, `deprecated`, `remove`)
3. Add version tag to payload (`syncUiContractVersion`).

### Deliverables

- Contract doc in `temp/`.
- Type definitions updated in `src/types/somedrive.ts`.
- Rust struct alignment in `src-tauri/src/app/sync_runtime.rs` and projection layer.

---

## Phase 1 - Backend Projection Hardening

1. Build or extend authoritative projection function(s) in:
   - `src-tauri/src/app/sync_engine/job_queue_activity_projection.rs`
   - `src-tauri/src/app/commands/sync_runtime.rs`
2. Emit explicit lane snapshots:
   - lifecycle, discovery, planner, executor.
3. Add consistency marker fields:
   - `consistency.ok`
   - `consistency.violations[]`
4. Remove legacy output fields from serialization path (or mark deprecated and stop UI usage immediately).

### Deliverables

- Backend payload is lane-separated and explicit.
- All key totals are backend-computed.

---

## Phase 2 - Sync Screen Refactor (Terminal Truth)

Refactor `src/components/accounts/AccountSyncActivityPanel.tsx`:

1. Replace abstract grouping with explicit lane sections:
   - Lifecycle
   - Discovery
   - Planner
   - Executor
   - Issues
   - Event feed
2. Remove client-side inferred remaining formulas where backend now provides canonical values.
3. Show raw authoritative values with precise labels:
   - for example `executor.download.remaining_count`
4. Render mismatch/degraded diagnostics explicitly when `consistency.ok === false`.
5. Keep action buttons tied to canonical `issue_actions`.

### Deliverables

- UI reflects current engine truth in real time.
- No cross-domain inference.

---

## Phase 3 - Legacy Shim Removal Pass

1. Remove unused legacy fields and selectors from:
   - `src/components/accounts/syncStateSelectors.ts`
   - `src/components/accounts/syncModeMessaging.ts` (minimize abstraction drift)
   - stale sync props in panel tree.
2. Remove deprecated backend compatibility assignments that only served old UI.
3. Add compile-time guardrails:
   - type narrowing to prevent using removed fields
   - lint or grep guard rules for banned legacy fields.

### Deliverables

- Legacy fields not consumed.
- Build fails if old contract fields are reintroduced in UI path.

---

## Phase 4 - Determinism and Consistency Test Matrix

### Backend tests (Rust)

1. Bootstrap cloud-first with large set:
   - discovered/planned/remaining consistency over cycle progression.
2. Pause/resume mid-download:
   - running/queued/retry_wait transitions stable.
3. Retry_wait lifecycle:
   - due/not-due claims reflected immediately in executor counters.
4. Restart mid-cycle:
   - runtime hydration from DB produces consistent lane snapshots.
5. Large delete guard:
   - pending/approved state reflected exactly in issue + lifecycle + executor views.
6. Conflict backup:
   - issue payload + event feed consistency.

### Frontend tests (minimal harness)

1. Render tests for lane blocks with mock authoritative payloads.
2. Consistency violation rendering tests.
3. Ensure no old fields used by panel (type tests or grep guard).

---

## Phase 5 - Acceptance and Closeout

1. Run full verification:
   - `cargo check`
   - `cargo test`
   - `npm run typecheck`
   - `npm run lint` if UI touched significantly
   - sync writer guard script
2. Update docs:
   - add permanent architecture note for sync UI authority contract.
3. Mark plan done with final evidence links and commits.

---

## UI Design Guidelines for This Pass

1. Make the screen dense, explicit, and operationally readable.
2. Prefer what happened, what is running, what is blocked over polished abstraction.
3. Every displayed number should state source domain, update cadence, and exact meaning.
4. Include timestamps for lane updates to expose staleness.
5. If unknown, show explicit `unknown` or `unavailable` with reason; never guessed totals.

---

## Proposed Data Shape (High-Level)

```ts
sync: {
  contractVersion: number
  lifecycle: {
    phase: string
    phaseMessage: string
    activity: { stage, progressMode, current, total, unit, detail, cycleId, updatedAt }
    twoWayReady: boolean
    remoteScanComplete: boolean
    updatedAt: string
  }
  discovery: {
    remote: { discoveredTotal, pagesSeen, cursorActive, deltaLinkKnown, updatedAt }
  }
  planner: {
    actions: { download, upload, deleteRemote, deleteLocal, conflict, none }
    updatedAt: string
  }
  executor: {
    download: { queued, claimed, running, retryWait, failedTerminal, done, remainingCount, bytes }
    upload:   { queued, claimed, running, retryWait, failedTerminal, done, remainingCount, bytes }
    actions:  { deleteRemote, deleteLocal, conflict }
    updatedAt: string
  }
  issues: {
    code, message, actions, path, secondaryPath, severity
  }
  consistency: {
    ok: boolean
    violations: string[]
  }
}
```

---

## Risks and Mitigations

1. Risk: temporary UI churn while contract transitions.
   - Mitigation: short-lived adapter layer with strict removal TODO and deadline in same PR series.
2. Risk: hidden dependence on legacy fields.
   - Mitigation: grep guard plus TS compile failures for deprecated keys.
3. Risk: counter jitter during in-flight updates.
   - Mitigation: backend computes canonical snapshots atomically per emit tick.

---

## Definition of Done

1. Sync UI shows lane-based authoritative telemetry only.
2. No frontend heuristic reconstruction of cross-domain counters.
3. Legacy compatibility fields are removed from active rendering path.
4. Invariant violations are explicit, not hidden.
5. Pause/resume/restart/retry flows keep UI counters consistent with DB authority.
6. All verification commands pass.

---

## Immediate Next Execution Order

1. Contract table and payload schema.
2. Backend projection shaping plus invariants.
3. UI lane refactor.
4. Legacy field removal and guardrail enforcement.
5. Test matrix and acceptance closeout.

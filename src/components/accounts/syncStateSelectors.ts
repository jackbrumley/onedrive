import type { AccountProfile, SyncRuntimeAccountStatus } from "../../types/somedrive";

const BLOCKING_ISSUE_CODES = new Set([
  "auth_required",
  "permission_denied",
  "disk_full",
  "sync_root_unavailable",
  "large_delete_guard",
  "unknown_error",
]);

function isSyncPhaseActive(phase: string | undefined): boolean {
  return (
    phase === "syncing" ||
    phase === "scanning_remote" ||
    phase === "applying_remote" ||
    phase === "scanning_local" ||
    phase === "building_index" ||
    phase === "planning_actions" ||
    phase === "applying_local"
  );
}

export function computeEffectiveSyncState(
  account: AccountProfile,
  runtimeStatus: SyncRuntimeAccountStatus | null
): {
  runtimeIssueCode: string | null;
  hasBlockingIssue: boolean;
  syncActive: boolean;
  syncState: "stopped" | "syncing" | "paused";
} {
  const runtimeIssueCode = runtimeStatus?.issueCode ?? null;
  const runtimeIssueIsBlocking = runtimeIssueCode ? BLOCKING_ISSUE_CODES.has(runtimeIssueCode) : false;
  const phase = runtimeStatus?.phase;
  const hasBlockingIssue = !account.authConfigured || runtimeIssueIsBlocking || phase === "error";
  const syncActive = isSyncPhaseActive(phase);
  const syncState: "stopped" | "syncing" | "paused" = hasBlockingIssue
    ? "stopped"
    : syncActive
      ? "syncing"
      : "paused";
  return {
    runtimeIssueCode,
    hasBlockingIssue,
    syncActive,
    syncState,
  };
}

export function computeHasCompletedInitialSync(runtimeStatus: SyncRuntimeAccountStatus | null): boolean {
  return runtimeStatus?.twoWayReady === true;
}

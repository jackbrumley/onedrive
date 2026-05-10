import type { AccountProfile, SyncRuntimeAccountStatus } from "../../types/somedrive";

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
  _account: AccountProfile,
  runtimeStatus: SyncRuntimeAccountStatus | null
): {
  runtimeIssueCode: string | null;
  hasBlockingIssue: boolean;
  syncActive: boolean;
  syncState: "stopped" | "syncing" | "paused";
} {
  const runtimeIssueCode = runtimeStatus?.issueCode ?? null;
  const phase = runtimeStatus?.phase;
  const authReady = runtimeStatus?.authReady ?? false;
  const issueSeverity = runtimeStatus?.issueSeverity ?? "none";
  const canSync = runtimeStatus?.canSync ?? false;
  const hasBlockingIssue = !authReady || issueSeverity === "blocking" || !canSync || phase === "error";
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

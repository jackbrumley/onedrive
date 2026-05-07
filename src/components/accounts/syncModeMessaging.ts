import type { SyncRuntimeAccountStatus } from "../../types/somedrive";

type SyncModeTone = "info" | "success" | "warning" | "caution";

interface SyncModeMessage {
  title: string;
  detail: string;
  tone: SyncModeTone;
}

function isDeletionGuardIssue(runtimeStatus: SyncRuntimeAccountStatus | null): boolean {
  if (!runtimeStatus) {
    return false;
  }
  const issueCode = (runtimeStatus.issueCode ?? "").toLowerCase();
  const issueMessage = (runtimeStatus.issueMessage ?? "").toLowerCase();
  return (
    issueCode.includes("large_delete") ||
    issueCode.includes("delete_guard") ||
    issueCode.includes("mass_delete") ||
    issueCode.includes("bulk_delete") ||
    issueMessage.includes("large deletion") ||
    issueMessage.includes("mass deletion")
  );
}

export function syncModeMessage(
  runtimeStatus: SyncRuntimeAccountStatus | null,
  hasCompletedInitialSync: boolean
): SyncModeMessage {
  if (isDeletionGuardIssue(runtimeStatus)) {
    return {
      title: "Large deletion detected",
      detail: "Cloud deletion is paused until you review these changes.",
      tone: "warning",
    };
  }

  const phase = runtimeStatus?.phase ?? "idle";
  if (!hasCompletedInitialSync) {
    if (phase === "scanning_local" || phase === "applying_local") {
      return {
        title: "Preparing two-way sync",
        detail: "Cloud files are still in control. Local adds/deletes won't sync yet.",
        tone: "caution",
      };
    }
    if (phase === "paused") {
      return {
        title: "Initial sync paused",
        detail: "Initial sync runs in cloud-files-only mode.",
        tone: "caution",
      };
    }
    return {
      title: "Initial sync in progress",
      detail: "Downloading cloud files first. Local adds/deletes won't sync yet.",
      tone: "caution",
    };
  }

  if (phase === "paused") {
    return {
      title: "Two-way sync paused",
      detail: "Resume sync to keep this device and cloud files aligned.",
      tone: "info",
    };
  }

  if (phase === "error") {
    return {
      title: "Sync needs attention",
      detail: runtimeStatus?.issueMessage ?? "An issue is blocking two-way sync.",
      tone: "warning",
    };
  }

  return {
    title: "Two-way sync is active",
    detail: "Changes on this device and in cloud stay in sync.",
    tone: "success",
  };
}

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
  if (runtimeStatus.issueCode === "large_delete_guard") {
    return true;
  }
  return runtimeStatus.issueActions.includes("confirm_large_delete") || runtimeStatus.issueActions.includes("keep_cloud_files");
}

export function syncModeMessage(
  runtimeStatus: SyncRuntimeAccountStatus | null,
  hasCompletedInitialSync: boolean
): SyncModeMessage {
  if (!runtimeStatus) {
    return {
      title: "Runtime status",
      detail: "null",
      tone: "info",
    };
  }

  if (isDeletionGuardIssue(runtimeStatus)) {
    return {
      title: "Large deletion detected",
      detail: "Cloud deletion is paused until you review these changes.",
      tone: "warning",
    };
  }

  const phase = runtimeStatus?.phase ?? "idle";
  if (!hasCompletedInitialSync) {
    if (phase === "error") {
      return {
        title: "Initial sync needs attention",
        detail: runtimeStatus?.issueMessage ?? "A blocking issue interrupted initial sync.",
        tone: "warning",
      };
    }
    const failedDownloads = runtimeStatus?.remoteDownloadFailedTotal ?? 0;
    if (failedDownloads > 0) {
      return {
        title: "Two-way sync blocked",
        detail: `Cannot enable two-way sync until ${failedDownloads} failed cloud download${failedDownloads === 1 ? " is" : "s are"} resolved.`,
        tone: "warning",
      };
    }
    if (phase === "scanning_local" || phase === "applying_local" || phase === "preparing_two_way_baseline") {
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

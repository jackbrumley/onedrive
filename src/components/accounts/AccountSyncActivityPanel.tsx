import {
  IconAlertCircle,
  IconCircleCheckFilled,
  IconCode,
  IconDownload,
  IconFile,
  IconFileDescription,
  IconFileMusic,
  IconFileSpreadsheet,
  IconFileTypePdf,
  IconFileWord,
  IconFileZip,
  IconPhoto,
  IconRefresh,
  IconUpload,
  IconVideo,
} from "@tabler/icons-preact";
import type { SyncRuntimeAccountStatus } from "../../types/somedrive";
import { syncModeMessage } from "./syncModeMessaging";

interface AccountSyncActivityPanelProps {
  runtimeStatus: SyncRuntimeAccountStatus | null;
  hasCompletedInitialSync: boolean;
  issueMessage: string | null;
  issueKind: "auth_required" | "sync_error" | null;
  issueActions: string[];
  issuePath: string | null;
  issueSecondaryPath: string | null;
  onOpenItemFolder: (relativePath: string) => Promise<void>;
  onOpenSyncRootFolder: () => Promise<void>;
  onReauthenticate: () => Promise<unknown>;
  onRetrySync: () => Promise<void>;
  onConfirmLargeDelete: () => Promise<void>;
  onKeepCloudFiles: () => Promise<void>;
  largeDeletePreviewPaths: string[];
  onExportLargeDeletePreview: () => Promise<void>;
}

function formatBytes(value: number | null): string {
  if (value === null) {
    return "0 B";
  }
  if (value < 1024) {
    return `${value} B`;
  }
  if (value < 1024 * 1024) {
    return `${(value / 1024).toFixed(1)} KB`;
  }
  if (value < 1024 * 1024 * 1024) {
    return `${(value / (1024 * 1024)).toFixed(1)} MB`;
  }
  return `${(value / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function transferProgressPercent(bytesDone: number, bytesTotal: number | null): number | null {
  if (!bytesTotal || bytesTotal <= 0) {
    return null;
  }
  return Math.min(100, Math.max(0, (bytesDone / bytesTotal) * 100));
}

function shouldShowTransferBytes(bytesDone: number, bytesTotal: number | null): boolean {
  return bytesTotal !== null || bytesDone > 0;
}

function relativeUpdatedAt(updatedAt: string): string {
  const updated = new Date(updatedAt).getTime();
  if (Number.isNaN(updated)) {
    return "unknown";
  }
  const deltaSeconds = Math.max(0, Math.round((Date.now() - updated) / 1000));
  if (deltaSeconds < 5) {
    return "just now";
  }
  if (deltaSeconds < 60) {
    return `${deltaSeconds}s ago`;
  }
  const deltaMinutes = Math.round(deltaSeconds / 60);
  if (deltaMinutes < 60) {
    return `${deltaMinutes}m ago`;
  }
  const deltaHours = Math.round(deltaMinutes / 60);
  return `${deltaHours}h ago`;
}

function extractUploadCooldownHint(phaseMessage: string | null): { path: string; retryIn: string } | null {
  if (!phaseMessage) {
    return null;
  }
  const match = phaseMessage.match(/^Upload retry delayed for '(.+)' \(retry in (.+)\)$/);
  if (!match) {
    return null;
  }
  return {
    path: match[1],
    retryIn: match[2],
  };
}

function isTransientTransferError(errorText: string | null): boolean {
  if (!errorText) {
    return false;
  }
  const normalized = errorText.toLowerCase();
  return (
    normalized.includes("failed reading download stream") ||
    normalized.includes("error decoding response body") ||
    normalized.includes("timed out") ||
    normalized.includes("connection reset") ||
    normalized.includes("connection aborted") ||
    normalized.includes("temporary")
  );
}

function extensionFromPath(path: string): string {
  const filename = path.split("/").pop() ?? "";
  const dotIndex = filename.lastIndexOf(".");
  if (dotIndex <= 0 || dotIndex === filename.length - 1) {
    return "";
  }
  return filename.slice(dotIndex + 1).toLowerCase();
}

const FILE_TYPE_ICON_SIZE = 34;
const ACTIVITY_ICON_SIZE = 24;

function iconForFilePath(path: string) {
  const extension = extensionFromPath(path);

  if (["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "heic", "tif", "tiff", "ico"].includes(extension)) {
    return <IconPhoto size={FILE_TYPE_ICON_SIZE} />;
  }

  if (["mp4", "mkv", "mov", "avi", "wmv", "webm", "m4v", "flv"].includes(extension)) {
    return <IconVideo size={FILE_TYPE_ICON_SIZE} />;
  }

  if (["mp3", "wav", "flac", "m4a", "aac", "ogg", "wma"].includes(extension)) {
    return <IconFileMusic size={FILE_TYPE_ICON_SIZE} />;
  }

  if (["doc", "docx", "odt", "rtf", "txt", "md"].includes(extension)) {
    return <IconFileWord size={FILE_TYPE_ICON_SIZE} />;
  }

  if (["xls", "xlsx", "csv", "ods", "tsv"].includes(extension)) {
    return <IconFileSpreadsheet size={FILE_TYPE_ICON_SIZE} />;
  }

  if (extension === "pdf") {
    return <IconFileTypePdf size={FILE_TYPE_ICON_SIZE} />;
  }

  if (["zip", "rar", "7z", "tar", "gz", "bz2", "xz"].includes(extension)) {
    return <IconFileZip size={FILE_TYPE_ICON_SIZE} />;
  }

  if (["json", "yaml", "yml", "toml", "xml", "ini", "conf", "cfg", "env", "ts", "tsx", "js", "jsx", "rs", "py", "sh"].includes(extension)) {
    return <IconCode size={FILE_TYPE_ICON_SIZE} />;
  }

  if (["ppt", "pptx", "odp"].includes(extension)) {
    return <IconFileDescription size={FILE_TYPE_ICON_SIZE} />;
  }

  return <IconFile size={FILE_TYPE_ICON_SIZE} />;
}

export function AccountSyncActivityPanel({
  runtimeStatus,
  hasCompletedInitialSync,
  issueMessage,
  issueKind,
  issueActions,
  issuePath,
  issueSecondaryPath,
  onOpenItemFolder,
  onOpenSyncRootFolder,
  onReauthenticate,
  onRetrySync,
  onConfirmLargeDelete,
  onKeepCloudFiles,
  largeDeletePreviewPaths,
  onExportLargeDeletePreview,
}: AccountSyncActivityPanelProps) {
  const modeMessage = syncModeMessage(runtimeStatus, hasCompletedInitialSync);
  const inProgress = runtimeStatus?.inProgress ?? [];
  const recentCompleted = runtimeStatus?.recentCompleted ?? [];
  const recentFailed = runtimeStatus?.recentFailed ?? [];
  const remoteDiscoveredCount = runtimeStatus?.remoteDiscoveredTotal ?? runtimeStatus?.remoteDiscoveredCount ?? 0;
  const remoteDownloadPlannedCount = runtimeStatus?.remoteDownloadPlannedTotal ?? remoteDiscoveredCount;
  const remoteDownloadedCount = runtimeStatus?.remoteDownloadCompletedTotal ?? runtimeStatus?.remoteDownloadedCount ?? 0;
  const remoteDownloadFailedCount = runtimeStatus?.remoteDownloadFailedTotal ?? 0;
  const remoteDownloadInFlight = runtimeStatus?.remoteDownloadInFlight ?? runtimeStatus?.remoteDownloadQueueCount ?? 0;
  const remoteScanComplete = runtimeStatus?.remoteScanComplete ?? false;
  const remoteDownloadRemainingCount = Math.max(
    remoteDownloadPlannedCount - remoteDownloadedCount - remoteDownloadFailedCount,
    0
  );
  const activeUploadCount = inProgress.filter((item) => item.direction.toLowerCase() === "upload").length;
  const showTransferStats =
    remoteDiscoveredCount > 0 ||
    remoteDownloadPlannedCount > 0 ||
    remoteDownloadedCount > 0 ||
    remoteDownloadInFlight > 0 ||
    activeUploadCount > 0;
  const isRemoteScanActive = runtimeStatus?.phase === "scanning_remote";
  const uploadCooldownHint = extractUploadCooldownHint(runtimeStatus?.phaseMessage ?? null);
  const hasIssueSummary = Boolean(issueMessage);
  const transientFailureCount = recentFailed.filter((item) => isTransientTransferError(item.error)).length;
  const hasTransientRetryIssue = uploadCooldownHint !== null || transientFailureCount > 0;
  const hasBlockingIssue = hasIssueSummary || issueKind !== null;
  const hasIssueSection = hasIssueSummary || issueActions.length > 0 || recentFailed.length > 0 || uploadCooldownHint !== null;
  const issuesClassName = hasBlockingIssue
    ? "account-sync-preview-issues"
    : hasTransientRetryIssue
      ? "account-sync-preview-issues account-sync-preview-issues-warning"
      : "account-sync-preview-issues";

  const items = [
    ...inProgress.map((transfer) => ({
      id: transfer.id,
      kind: "active" as const,
      direction: transfer.direction,
      path: transfer.path,
      when: transfer.updatedAt,
      bytesDone: transfer.bytesDone,
      bytesTotal: transfer.bytesTotal,
      error: null,
    })),
    ...recentCompleted.map((item) => ({
      id: item.id,
      kind: "completed" as const,
      direction: item.direction,
      path: item.path,
      when: item.finishedAt,
      bytesDone: item.bytesTotal,
      bytesTotal: item.bytesTotal,
      error: null,
    })),
  ].sort((left, right) => {
    if (left.kind === "active" && right.kind !== "active") {
      return -1;
    }
    if (right.kind === "active" && left.kind !== "active") {
      return 1;
    }
    return new Date(right.when).getTime() - new Date(left.when).getTime();
  });

  const iconForDirection = (direction: string, className = "") => {
    const normalized = direction.toLowerCase();
    if (normalized === "upload") {
      return <IconUpload size={ACTIVITY_ICON_SIZE} class={className} />;
    }
    return <IconDownload size={ACTIVITY_ICON_SIZE} class={className} />;
  };

  const conflictTargetPath = issueSecondaryPath ?? issuePath;
  const hasConflictAction = issueActions.includes("open_conflict") && Boolean(conflictTargetPath);

  return (
    <div class="account-sync-activity-panel" role="region" aria-label="Sync activity">
      <section class={`account-sync-mode-banner account-sync-mode-banner-${modeMessage.tone}`}>
        <p class="account-sync-mode-banner-title">{modeMessage.title}</p>
        <p class="account-sync-mode-banner-detail">{modeMessage.detail}</p>
      </section>
      <p class="account-sync-preview-subtitle">
        <span class="account-sync-preview-phase-line">
          {isRemoteScanActive && <IconRefresh size={13} class="sync-preview-icon-active" />}
          <span>{runtimeStatus?.phaseMessage ?? "Waiting for runtime updates"}</span>
        </span>
        {runtimeStatus ? <span class="account-sync-preview-updated">updated {relativeUpdatedAt(runtimeStatus.updatedAt)}</span> : null}
      </p>
      {showTransferStats && (
        <p class="account-sync-preview-stats-line">
          <span>Discovered {remoteDiscoveredCount}</span>
          <span>Planned {remoteDownloadPlannedCount}</span>
          <span>
            Remaining {remoteDownloadRemainingCount}
            {!remoteScanComplete ? "+" : ""}
          </span>
          <span>Downloaded {remoteDownloadedCount}</span>
          <span>In flight {remoteDownloadInFlight}</span>
          <span>Uploading {activeUploadCount}</span>
        </p>
      )}
      {hasIssueSection && (
        <section class={issuesClassName}>
          <p class="account-sync-preview-section-label">{hasBlockingIssue ? "Issues" : "Warnings"}</p>
          {hasIssueSummary && (
            <p class="account-sync-preview-issue-summary">{issueMessage}</p>
          )}
          {!hasBlockingIssue && hasTransientRetryIssue && (
            <p class="account-sync-preview-issue-warning-note">
              Temporary transfer issue detected. Sync will retry automatically.
            </p>
          )}
          {uploadCooldownHint && (
            <p class="account-sync-preview-issue-cooldown">
              Retry queued in {uploadCooldownHint.retryIn}:{" "}
              <button
                type="button"
                class="account-sync-preview-issue-cooldown-path"
                onClick={(event) => {
                  event.stopPropagation();
                  void onOpenItemFolder(uploadCooldownHint.path);
                }}
              >
                {uploadCooldownHint.path}
              </button>
            </p>
          )}
          <div class="account-sync-preview-actions">
            {issueActions.includes("reauthenticate") && issueKind === "auth_required" && (
              <button
                type="button"
                class="account-sync-preview-action-btn"
                onClick={(event) => {
                  event.stopPropagation();
                  void onReauthenticate();
                }}
              >
                Re-authenticate
              </button>
            )}
            {issueActions.includes("open_sync_root") && (
              <button
                type="button"
                class="account-sync-preview-action-btn"
                onClick={(event) => {
                  event.stopPropagation();
                  void onOpenSyncRootFolder();
                }}
              >
                Open Sync Folder
              </button>
            )}
            {hasConflictAction && (
              <button
                type="button"
                class="account-sync-preview-action-btn account-sync-preview-action-btn-conflict"
                onClick={(event) => {
                  event.stopPropagation();
                  void onOpenItemFolder(conflictTargetPath!);
                }}
              >
                Open Conflict
              </button>
            )}
            {issueActions.includes("retry_sync") && (
              <button
                type="button"
                class="account-sync-preview-action-btn"
                onClick={(event) => {
                  event.stopPropagation();
                  void onRetrySync();
                }}
              >
                Retry Sync
              </button>
            )}
            {issueActions.includes("confirm_large_delete") && (
              <button
                type="button"
                class="account-sync-preview-action-btn account-sync-preview-action-btn-warning"
                onClick={(event) => {
                  event.stopPropagation();
                  void onConfirmLargeDelete();
                }}
              >
                Delete from Cloud
              </button>
            )}
            {issueActions.includes("keep_cloud_files") && (
              <button
                type="button"
                class="account-sync-preview-action-btn"
                onClick={(event) => {
                  event.stopPropagation();
                  void onKeepCloudFiles();
                }}
              >
                Keep Cloud Files
              </button>
            )}
          </div>
          {largeDeletePreviewPaths.length > 0 && (
            <div class="account-sync-preview-delete-review">
              <p class="account-sync-preview-delete-review-title">
                Review deletions ({largeDeletePreviewPaths.length})
              </p>
              <div class="account-sync-preview-delete-review-actions">
                <button
                  type="button"
                  class="account-sync-preview-action-btn"
                  onClick={(event) => {
                    event.stopPropagation();
                    void onExportLargeDeletePreview();
                  }}
                >
                  Export Full List
                </button>
              </div>
              <div class="account-sync-preview-list">
                {largeDeletePreviewPaths.slice(0, 40).map((path) => (
                  <article key={path} class="account-sync-preview-item">
                    <button
                      type="button"
                      class="account-sync-preview-item-button"
                      onClick={(event) => {
                        event.stopPropagation();
                        void onOpenItemFolder(path);
                      }}
                    >
                      <div class="account-sync-preview-row">
                        <span class="account-sync-preview-file-icon">{iconForFilePath(path)}</span>
                        <div class="account-sync-preview-content">
                          <p class="account-sync-preview-path">{path}</p>
                          <p class="account-sync-preview-meta">
                            <span>Pending cloud deletion</span>
                          </p>
                        </div>
                      </div>
                    </button>
                  </article>
                ))}
              </div>
              {largeDeletePreviewPaths.length > 40 && (
                <p class="account-sync-preview-delete-review-more">
                  Showing first 40 paths.
                </p>
              )}
            </div>
          )}
          {recentFailed.length > 0 && (
            <div class="account-sync-preview-list account-sync-preview-list-errors">
              {recentFailed.map((item) => (
                <article key={item.id} class="account-sync-preview-item">
                  <button
                    type="button"
                    class="account-sync-preview-item-button"
                    onClick={(event) => {
                      event.stopPropagation();
                      void onOpenItemFolder(item.path);
                    }}
                  >
                    <div class="account-sync-preview-row">
                      <span class="account-sync-preview-file-icon">
                        {iconForFilePath(item.path)}
                      </span>
                      <div class="account-sync-preview-content">
                        <p class="account-sync-preview-path">{item.path}</p>
                        <p class="account-sync-preview-meta">
                          <span>{item.error ?? "Transfer failed"}</span>
                          <span>{new Date(item.finishedAt).toLocaleTimeString()}</span>
                        </p>
                      </div>
                      <span class="account-sync-preview-right-icons">
                        <span class="account-sync-preview-direction-icon">
                          {iconForDirection(item.direction)}
                        </span>
                        <span class="account-sync-preview-status-icon">
                          <IconAlertCircle size={ACTIVITY_ICON_SIZE} class="sync-preview-icon-error" />
                        </span>
                      </span>
                    </div>
                  </button>
                </article>
              ))}
            </div>
          )}
        </section>
      )}
      <div class="account-sync-preview-activity-scroll">
        {items.length > 0 && <p class="account-sync-preview-section-label">Activity</p>}
        {items.length === 0 ? (
          <p class="account-sync-preview-empty">No sync activity yet.</p>
        ) : (
          <div class="account-sync-preview-list">
            {items.map((item) => {
              const isActive = item.kind === "active";
              const progressPercent = isActive ? transferProgressPercent(item.bytesDone ?? 0, item.bytesTotal) : null;
              return (
                <article key={item.id} class="account-sync-preview-item">
                  <button
                    type="button"
                    class="account-sync-preview-item-button"
                    onClick={(event) => {
                      event.stopPropagation();
                      void onOpenItemFolder(item.path);
                    }}
                  >
                    <div class="account-sync-preview-row">
                      <span class="account-sync-preview-file-icon">
                        {iconForFilePath(item.path)}
                      </span>
                      <div class="account-sync-preview-content">
                        <p class="account-sync-preview-path">{item.path}</p>
                        <p class="account-sync-preview-meta">
                          {item.kind === "active" ? (
                            shouldShowTransferBytes(item.bytesDone ?? 0, item.bytesTotal) ? (
                              <span>
                                {formatBytes(item.bytesDone ?? 0)}
                                {item.bytesTotal ? ` / ${formatBytes(item.bytesTotal)}` : ""}
                              </span>
                            ) : (
                              <span />
                            )
                          ) : (
                            <span>{formatBytes(item.bytesTotal)}</span>
                          )}
                          <span>{new Date(item.when).toLocaleTimeString()}</span>
                        </p>
                        {isActive && (
                          <div class="sync-runtime-progress-track-compact">
                            <div
                              class={
                                progressPercent === null
                                  ? "sync-runtime-progress-fill-compact sync-runtime-progress-fill-compact-indeterminate"
                                  : "sync-runtime-progress-fill-compact"
                              }
                              style={progressPercent === null ? { width: "34%" } : { width: `${progressPercent.toFixed(1)}%` }}
                            />
                          </div>
                        )}
                      </div>
                      <span class="account-sync-preview-right-icons">
                        <span class="account-sync-preview-direction-icon">
                          {iconForDirection(item.direction)}
                        </span>
                        <span class="account-sync-preview-status-icon">
                          {item.kind === "active" ? (
                            <IconRefresh size={ACTIVITY_ICON_SIZE} class="sync-preview-icon-active" />
                          ) : (
                            <IconCircleCheckFilled size={ACTIVITY_ICON_SIZE} class="sync-preview-icon-success" />
                          )}
                        </span>
                      </span>
                    </div>
                  </button>
                </article>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

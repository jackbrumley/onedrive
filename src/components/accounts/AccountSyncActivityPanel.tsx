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
import { useEffect, useRef, useState } from "preact/hooks";
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

function formatTransferRate(bytesPerSecond: number): string {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) {
    return "0 MB/s (0 Mbps)";
  }
  const megabytesPerSecond = bytesPerSecond / (1024 * 1024);
  const megabitsPerSecond = (bytesPerSecond * 8) / 1_000_000;
  const mbPerSecondText = megabytesPerSecond >= 10 ? megabytesPerSecond.toFixed(1) : megabytesPerSecond.toFixed(2);
  const mbpsText = megabitsPerSecond >= 100 ? megabitsPerSecond.toFixed(0) : megabitsPerSecond.toFixed(1);
  return `${mbPerSecondText} MB/s (${mbpsText} Mbps)`;
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
  const throughputSampleRef = useRef<{ timestampMs: number; downloadedBytes: number; uploadedBytes: number } | null>(null);
  const [downloadBytesPerSecond, setDownloadBytesPerSecond] = useState(0);
  const [uploadBytesPerSecond, setUploadBytesPerSecond] = useState(0);
  const modeMessage = syncModeMessage(runtimeStatus, hasCompletedInitialSync);
  const isPausedPhase = runtimeStatus?.phase === "paused";
  const inProgress = runtimeStatus?.inProgress ?? [];
  const visibleInProgress = isPausedPhase ? [] : inProgress;
  const recentCompleted = runtimeStatus?.recentCompleted ?? [];
  const recentRetryWaiting = runtimeStatus?.recentRetryWaiting ?? [];
  const recentFailed = runtimeStatus?.recentFailed ?? [];
  const remoteDiscoveredCount = runtimeStatus?.remoteDiscoveredTotal ?? 0;
  const remoteDownloadPlannedCount = runtimeStatus?.remoteDownloadPlannedTotal ?? 0;
  const remoteDownloadedCount = runtimeStatus?.remoteDownloadCompletedTotal ?? 0;
  const remoteDownloadFailedCount = runtimeStatus?.remoteDownloadFailedTotal ?? 0;
  const remoteDownloadInFlightRaw = runtimeStatus?.remoteDownloadInFlight ?? 0;
  const remoteDownloadInFlight = isPausedPhase ? 0 : remoteDownloadInFlightRaw;
  const remoteDownloadRetryWaiting = runtimeStatus?.remoteDownloadRetryWaiting ?? 0;
  const remoteDownloadPlannedBytesTotal = runtimeStatus?.remoteDownloadPlannedBytesTotal ?? 0;
  const remoteDownloadCompletedBytesTotal = runtimeStatus?.remoteDownloadCompletedBytesTotal ?? 0;
  const remoteDownloadRemainingBytesTotal = runtimeStatus?.remoteDownloadRemainingBytesTotal ?? 0;
  const remoteDownloadInFlightBytesDone = runtimeStatus?.remoteDownloadInFlightBytesDone ?? 0;
  const remoteDownloadThrottleTotal = runtimeStatus?.remoteDownloadThrottleTotal ?? 0;
  const remoteDownloadThrottleLastMinute = runtimeStatus?.remoteDownloadThrottleLastMinute ?? 0;
  const remoteScanComplete = runtimeStatus?.remoteScanComplete ?? false;
  const remoteDownloadRemainingCount = Math.max(
    remoteDownloadPlannedCount - remoteDownloadedCount - remoteDownloadFailedCount - remoteDownloadInFlight - remoteDownloadRetryWaiting,
    0
  );
  const uploadPlannedCount = runtimeStatus?.uploadPlannedTotal ?? 0;
  const activeUploadCountRaw = runtimeStatus?.uploadInFlight ?? visibleInProgress.filter((item) => item.direction.toLowerCase() === "upload").length;
  const activeUploadCount = isPausedPhase ? 0 : activeUploadCountRaw;
  const uploadedCount = runtimeStatus?.uploadCompletedTotal ?? 0;
  const uploadFailedCount = runtimeStatus?.uploadFailedTotal ?? 0;
  const uploadRetryWaitingCount = runtimeStatus?.uploadRetryWaiting ?? 0;
  const uploadPlannedBytesTotal = runtimeStatus?.uploadPlannedBytesTotal ?? 0;
  const uploadCompletedBytesTotal = runtimeStatus?.uploadCompletedBytesTotal ?? 0;
  const uploadRemainingBytesTotal = runtimeStatus?.uploadRemainingBytesTotal ?? 0;
  const uploadInFlightBytesDone = runtimeStatus?.uploadInFlightBytesDone ?? 0;
  const uploadThrottleTotal = runtimeStatus?.uploadThrottleTotal ?? 0;
  const uploadThrottleLastMinute = runtimeStatus?.uploadThrottleLastMinute ?? 0;
  const showTransferStats =
    remoteDiscoveredCount > 0 ||
    remoteDownloadPlannedCount > 0 ||
    remoteDownloadedCount > 0 ||
    remoteDownloadRetryWaiting > 0 ||
    remoteDownloadInFlight > 0 ||
    uploadPlannedCount > 0 ||
    activeUploadCount > 0 ||
    uploadedCount > 0 ||
    uploadFailedCount > 0;
  const isRemoteScanActive = runtimeStatus?.phase === "scanning_remote";
  const uploadCooldownHint = extractUploadCooldownHint(runtimeStatus?.phaseMessage ?? null);
  const hasIssueSummary = Boolean(issueMessage);
  const hasRetryWarnings =
    remoteDownloadRetryWaiting > 0 ||
    uploadRetryWaitingCount > 0 ||
    uploadCooldownHint !== null ||
    recentRetryWaiting.length > 0;
  const hasErrorItems = hasIssueSummary || issueKind !== null || recentFailed.length > 0;
  const hasBlockingIssue = hasIssueSummary || issueKind !== null;
  const hasIssueSection = hasRetryWarnings || hasErrorItems || issueActions.length > 0;
  const issuesClassName = hasBlockingIssue
    ? "account-sync-preview-issues"
    : hasRetryWarnings
      ? "account-sync-preview-issues account-sync-preview-issues-warning"
      : "account-sync-preview-issues";

  const items = [
    ...visibleInProgress.map((transfer) => ({
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

  useEffect(() => {
    if (!runtimeStatus) {
      throughputSampleRef.current = null;
      setDownloadBytesPerSecond(0);
      setUploadBytesPerSecond(0);
      return;
    }

    const timestampMs = new Date(runtimeStatus.updatedAt).getTime();
    const safeTimestampMs = Number.isFinite(timestampMs) ? timestampMs : Date.now();
    const downloadedBytes = remoteDownloadCompletedBytesTotal + remoteDownloadInFlightBytesDone;
    const uploadedBytes = uploadCompletedBytesTotal + uploadInFlightBytesDone;
    const previousSample = throughputSampleRef.current;

    if (previousSample && safeTimestampMs > previousSample.timestampMs) {
      const deltaSeconds = (safeTimestampMs - previousSample.timestampMs) / 1000;
      const nextDownloadRate = Math.max(0, (downloadedBytes - previousSample.downloadedBytes) / deltaSeconds);
      const nextUploadRate = Math.max(0, (uploadedBytes - previousSample.uploadedBytes) / deltaSeconds);
      setDownloadBytesPerSecond((currentRate) => (currentRate <= 0 ? nextDownloadRate : currentRate * 0.55 + nextDownloadRate * 0.45));
      setUploadBytesPerSecond((currentRate) => (currentRate <= 0 ? nextUploadRate : currentRate * 0.55 + nextUploadRate * 0.45));
    }

    throughputSampleRef.current = {
      timestampMs: safeTimestampMs,
      downloadedBytes,
      uploadedBytes,
    };
  }, [
    runtimeStatus,
    remoteDownloadCompletedBytesTotal,
    remoteDownloadInFlightBytesDone,
    uploadCompletedBytesTotal,
    uploadInFlightBytesDone,
  ]);

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
      </p>
      {showTransferStats && (
        <div class="account-sync-preview-stats-stack">
          <section class="account-sync-preview-stats-group">
            <p class="account-sync-preview-stats-section">Discovery</p>
            <div class="account-sync-preview-metrics-grid account-sync-preview-metrics-grid-compact">
              <p class="account-sync-preview-metric">
                <span class="account-sync-preview-metric-label">Files discovered in cloud</span>
                <span class="account-sync-preview-metric-value">{remoteDiscoveredCount}</span>
              </p>
            </div>
          </section>

          <section class="account-sync-preview-stats-group">
            <p class="account-sync-preview-stats-section">Downloads</p>
            <div class="account-sync-preview-metrics-grid">
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Download queue</span><span class="account-sync-preview-metric-value">{remoteDownloadPlannedCount}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Download queue size</span><span class="account-sync-preview-metric-value">{formatBytes(remoteDownloadPlannedBytesTotal)}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Downloading now</span><span class="account-sync-preview-metric-value">{remoteDownloadInFlight}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Download speed</span><span class="account-sync-preview-metric-value">{formatTransferRate(downloadBytesPerSecond)}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Downloaded</span><span class="account-sync-preview-metric-value">{remoteDownloadedCount}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Downloaded size</span><span class="account-sync-preview-metric-value">{formatBytes(remoteDownloadCompletedBytesTotal)}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Download retry waiting</span><span class="account-sync-preview-metric-value">{remoteDownloadRetryWaiting}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Download failed</span><span class="account-sync-preview-metric-value">{remoteDownloadFailedCount}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Rate limit hits (past 1 minute)</span><span class="account-sync-preview-metric-value">{remoteDownloadThrottleLastMinute}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Rate limit hits (total)</span><span class="account-sync-preview-metric-value">{remoteDownloadThrottleTotal}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Download remaining</span><span class="account-sync-preview-metric-value">{remoteDownloadRemainingCount}{!remoteScanComplete ? "+" : ""}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Download remaining size</span><span class="account-sync-preview-metric-value">{formatBytes(remoteDownloadRemainingBytesTotal)}{!remoteScanComplete ? "+" : ""}</span></p>
            </div>
          </section>

          <section class="account-sync-preview-stats-group">
            <p class="account-sync-preview-stats-section">Uploads</p>
            <div class="account-sync-preview-metrics-grid">
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Upload queue</span><span class="account-sync-preview-metric-value">{uploadPlannedCount}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Upload queue size</span><span class="account-sync-preview-metric-value">{formatBytes(uploadPlannedBytesTotal)}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Uploading now</span><span class="account-sync-preview-metric-value">{activeUploadCount}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Upload speed</span><span class="account-sync-preview-metric-value">{formatTransferRate(uploadBytesPerSecond)}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Uploaded</span><span class="account-sync-preview-metric-value">{uploadedCount}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Uploaded size</span><span class="account-sync-preview-metric-value">{formatBytes(uploadCompletedBytesTotal)}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Upload retry waiting</span><span class="account-sync-preview-metric-value">{uploadRetryWaitingCount}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Upload failed</span><span class="account-sync-preview-metric-value">{uploadFailedCount}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Rate limit hits (past 1 minute)</span><span class="account-sync-preview-metric-value">{uploadThrottleLastMinute}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Rate limit hits (total)</span><span class="account-sync-preview-metric-value">{uploadThrottleTotal}</span></p>
              <p class="account-sync-preview-metric"><span class="account-sync-preview-metric-label">Upload remaining size</span><span class="account-sync-preview-metric-value">{formatBytes(uploadRemainingBytesTotal)}</span></p>
            </div>
          </section>
        </div>
      )}
      {hasIssueSection && (
        <section class={issuesClassName}>
          {hasRetryWarnings && (
            <>
              <p class="account-sync-preview-section-label">Warnings</p>
              <p class="account-sync-preview-issue-warning-note">
                Retrying transfers are queued and will resume automatically.
              </p>
            </>
          )}
          {hasErrorItems && <p class="account-sync-preview-section-label">Errors</p>}
          {hasIssueSummary && <p class="account-sync-preview-issue-summary">{issueMessage}</p>}
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
          {recentRetryWaiting.length > 0 && (
            <div class="account-sync-preview-list">
              {recentRetryWaiting.map((item) => (
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
                      <span class="account-sync-preview-file-icon">{iconForFilePath(item.path)}</span>
                      <div class="account-sync-preview-content">
                        <p class="account-sync-preview-path">{item.path}</p>
                        <p class="account-sync-preview-meta">
                          <span>{item.error ?? "Retry queued"}</span>
                          <span>retry after {new Date(item.finishedAt).toLocaleTimeString()}</span>
                        </p>
                      </div>
                      <span class="account-sync-preview-right-icons">
                        <span class="account-sync-preview-direction-icon">{iconForDirection(item.direction)}</span>
                        <span class="account-sync-preview-status-icon">
                          <IconRefresh size={ACTIVITY_ICON_SIZE} class="sync-preview-icon-active" />
                        </span>
                      </span>
                    </div>
                  </button>
                </article>
              ))}
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

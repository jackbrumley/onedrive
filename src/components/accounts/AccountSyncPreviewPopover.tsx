import {
  IconAlertCircle,
  IconCircleCheckFilled,
  IconDownload,
  IconRefresh,
  IconUpload,
} from "@tabler/icons-preact";
import type { SyncRuntimeAccountStatus } from "../../types/somedrive";

interface AccountSyncPreviewPopoverProps {
  runtimeStatus: SyncRuntimeAccountStatus | null;
  errorMessage: string | null;
  issueKind: "auth_required" | "sync_error" | null;
  issueActions: string[];
  issuePath: string | null;
  issueSecondaryPath: string | null;
  onOpenItemFolder: (relativePath: string) => Promise<void>;
  onOpenSyncRootFolder: () => Promise<void>;
  onReauthenticate: () => Promise<unknown>;
  onRetrySync: () => Promise<void>;
  placement: "top" | "bottom";
  visible: boolean;
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

export function AccountSyncPreviewPopover({
  runtimeStatus,
  errorMessage,
  issueKind,
  issueActions,
  issuePath,
  issueSecondaryPath,
  onOpenItemFolder,
  onOpenSyncRootFolder,
  onReauthenticate,
  onRetrySync,
  placement,
  visible,
}: AccountSyncPreviewPopoverProps) {
  const inProgress = runtimeStatus?.inProgress.slice(0, 8) ?? [];
  const recentCompleted = runtimeStatus?.recentCompleted.slice(0, 6) ?? [];
  const recentFailed = runtimeStatus?.recentFailed.slice(0, 4) ?? [];

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
    ...recentFailed.map((item) => ({
      id: item.id,
      kind: "failed" as const,
      direction: item.direction,
      path: item.path,
      when: item.finishedAt,
      bytesDone: item.bytesTotal,
      bytesTotal: item.bytesTotal,
      error: item.error,
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
      return <IconUpload size={14} class={className} />;
    }
    return <IconDownload size={14} class={className} />;
  };

  const conflictTargetPath = issueSecondaryPath ?? issuePath;
  const hasConflictAction = issueActions.includes("open_conflict") && Boolean(conflictTargetPath);
  const popoverClassName = `account-sync-preview-popover ${
    placement === "top" ? "account-sync-preview-popover-top" : "account-sync-preview-popover-bottom"
  }${visible ? " account-sync-preview-popover-visible" : ""}${hasConflictAction ? " account-sync-preview-popover-conflict" : ""}`;

  if (errorMessage) {
    return (
      <div
        class={popoverClassName}
        role="dialog"
        aria-label="Sync activity preview"
      >
        <p class="account-sync-preview-subtitle">{errorMessage}</p>
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
        </div>
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
                    <span class="account-sync-preview-status-icon">
                      <IconAlertCircle size={14} class="sync-preview-icon-error" />
                    </span>
                    <span class="account-sync-preview-direction-icon">
                      {iconForDirection(item.direction)}
                    </span>
                    <div class="account-sync-preview-content">
                      <p class="account-sync-preview-path">{item.path}</p>
                      <p class="account-sync-preview-meta">
                        <span>{item.error ?? "Transfer failed"}</span>
                        <span>{new Date(item.finishedAt).toLocaleTimeString()}</span>
                      </p>
                    </div>
                  </div>
                </button>
              </article>
            ))}
          </div>
        )}
      </div>
    );
  }

  return (
    <div
      class={popoverClassName}
      role="dialog"
      aria-label="Sync activity preview"
    >
      <p class="account-sync-preview-subtitle">
        {runtimeStatus?.phaseMessage ?? "Waiting for runtime updates"}
        {runtimeStatus ? ` - updated ${relativeUpdatedAt(runtimeStatus.updatedAt)}` : ""}
      </p>
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
                  <span class="account-sync-preview-status-icon">
                    {item.kind === "active" ? (
                      <IconRefresh size={14} class="sync-preview-icon-active" />
                    ) : item.kind === "completed" ? (
                      <IconCircleCheckFilled size={14} class="sync-preview-icon-success" />
                    ) : (
                      <IconAlertCircle size={14} class="sync-preview-icon-error" />
                    )}
                  </span>
                  <span class="account-sync-preview-direction-icon">
                    {iconForDirection(item.direction)}
                  </span>
                  <div class="account-sync-preview-content">
                    <p class="account-sync-preview-path">{item.path}</p>
                    <p class="account-sync-preview-meta">
                      {item.kind === "active" ? (
                        <span>
                          {formatBytes(item.bytesDone ?? 0)}
                          {item.bytesTotal ? ` / ${formatBytes(item.bytesTotal)}` : ""}
                        </span>
                      ) : item.kind === "completed" ? (
                        <span>{formatBytes(item.bytesTotal)}</span>
                      ) : (
                        <span>{item.error ?? "Transfer failed"}</span>
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
                  </div>
                </button>
              </article>
            );
          })}
        </div>
      )}
    </div>
  );
}

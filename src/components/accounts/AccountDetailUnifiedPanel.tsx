import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "preact/hooks";
import type {
  AccountProfile,
  ActivityEvent,
  SyncRuntimeAccountStatus,
  SyncRuntimeRecentItem,
  SyncRuntimeTransfer,
} from "../../types/somedrive";
import { SyncStateControl } from "../sync/SyncStateControl";

interface AccountDetailUnifiedPanelProps {
  account: AccountProfile;
  runtimeStatus: SyncRuntimeAccountStatus | null;
  events: ActivityEvent[];
  onSetAgentState: (accountId: string, state: "syncing" | "paused") => Promise<void>;
  onStartAuth: (accountId: string) => Promise<unknown>;
  onRename: (id: string, name: string) => Promise<void>;
  onSetSyncRoot: (id: string, path: string) => Promise<void>;
  onClearAuth: (id: string) => Promise<void>;
  onRemoveProfile: (id: string) => Promise<void>;
  actionsDisabled?: boolean;
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

function transferProgressPercent(transfer: SyncRuntimeTransfer): number | null {
  if (!transfer.bytesTotal || transfer.bytesTotal <= 0) {
    return null;
  }
  return Math.min(100, Math.max(0, (transfer.bytesDone / transfer.bytesTotal) * 100));
}

function shouldShowTransferBytes(bytesDone: number, bytesTotal: number | null): boolean {
  return bytesTotal !== null || bytesDone > 0;
}

function recentItemSummary(item: SyncRuntimeRecentItem): string {
  if (!item.bytesTotal) {
    return item.direction;
  }
  return `${item.direction} - ${formatBytes(item.bytesTotal)}`;
}

export function AccountDetailUnifiedPanel({
  account,
  runtimeStatus,
  events,
  onSetAgentState,
  onStartAuth,
  onRename,
  onSetSyncRoot,
  onClearAuth,
  onRemoveProfile,
  actionsDisabled = false,
}: AccountDetailUnifiedPanelProps) {
  const [draftName, setDraftName] = useState(account.displayName);
  const inProgress = runtimeStatus?.inProgress.slice(0, 20) ?? [];
  const recentCompleted = runtimeStatus?.recentCompleted.slice(0, 25) ?? [];
  const recentFailed = runtimeStatus?.recentFailed.slice(0, 25) ?? [];

  useEffect(() => {
    setDraftName(account.displayName);
  }, [account.displayName]);

  const chooseSyncFolder = async () => {
    if (actionsDisabled) {
      return;
    }
    const selected = await open({
      directory: true,
      defaultPath: account.syncRoot,
      title: `Choose sync folder for ${account.displayName}`,
    });
    if (typeof selected === "string" && selected.trim()) {
      const normalizedSelected = selected.replace(/\/+$/, "");
      if (/\/OneDrive$/i.test(normalizedSelected)) {
        const confirmed = window.confirm(
          "This looks like the default folder used by other OneDrive apps. It is safer to use SomeDrive to avoid conflicts. Continue anyway?"
        );
        if (!confirmed) {
          return;
        }
      }
      await onSetSyncRoot(account.id, selected);
    }
  };

  return (
    <article class="card account-detail-unified-card">
      <div class="account-detail-unified-grid">
        <section class="account-detail-section">
          <h3>Overview</h3>
          <div class="account-meta-row">
            <span class="pill">{account.kind}</span>
            <span class="pill">{account.authConfigured ? "Connected" : "Needs Authentication"}</span>
            <span class="pill">{account.agentState}</span>
          </div>
          <p>Runtime: {runtimeStatus?.phaseMessage ?? "Waiting for runtime updates"}</p>
          <p>Last Sync: {account.lastSyncAt ? new Date(account.lastSyncAt).toLocaleString() : "Never"}</p>
          <p>Sync Root: {account.syncRoot}</p>
          <div class="button-row">
            <SyncStateControl
              state={account.agentState === "syncing" ? "syncing" : "paused"}
              disabled={actionsDisabled}
              onToggle={(next) => onSetAgentState(account.id, next)}
            />
            {!account.authConfigured && (
              <button disabled={actionsDisabled} onClick={() => onStartAuth(account.id)}>
                Authenticate
              </button>
            )}
          </div>
        </section>

        <section class="account-detail-section">
          <h3>Sync Runtime</h3>
          <h4>In Progress</h4>
          {inProgress.length === 0 ? (
            <p>No active file transfers right now.</p>
          ) : (
            <div class="sync-runtime-list">
              {inProgress.map((transfer) => {
                const progressPercent = transferProgressPercent(transfer);
                return (
                  <section key={transfer.id} class="sync-runtime-item">
                    <p class="sync-runtime-item-path">{transfer.path}</p>
                    <p class="sync-runtime-item-meta">
                      <span class="pill">{transfer.direction}</span>
                      <span>
                        {shouldShowTransferBytes(transfer.bytesDone, transfer.bytesTotal)
                          ? `${formatBytes(transfer.bytesDone)}${transfer.bytesTotal ? ` / ${formatBytes(transfer.bytesTotal)}` : ""}`
                          : ""}
                      </span>
                    </p>
                    {progressPercent !== null ? (
                      <div class="sync-runtime-progress-track" aria-label="transfer progress">
                        <div class="sync-runtime-progress-fill" style={{ width: `${progressPercent.toFixed(1)}%` }} />
                      </div>
                    ) : (
                      <div class="sync-runtime-progress-track sync-runtime-progress-indeterminate">
                        <div class="sync-runtime-progress-fill" style={{ width: "36%" }} />
                      </div>
                    )}
                  </section>
                );
              })}
            </div>
          )}

          <h4>Recently Completed</h4>
          {recentCompleted.length === 0 ? (
            <p>No completed transfers recorded yet.</p>
          ) : (
            <div class="sync-runtime-list sync-runtime-list-compact">
              {recentCompleted.map((item) => (
                <section key={item.id} class="sync-runtime-item sync-runtime-item-compact">
                  <p class="sync-runtime-item-path">{item.path}</p>
                  <p class="sync-runtime-item-meta">
                    <span class="pill">{recentItemSummary(item)}</span>
                    <span>{new Date(item.finishedAt).toLocaleTimeString()}</span>
                  </p>
                </section>
              ))}
            </div>
          )}

          <h4>Recently Failed</h4>
          {recentFailed.length === 0 ? (
            <p>No failed transfers in recent history.</p>
          ) : (
            <div class="sync-runtime-list sync-runtime-list-compact">
              {recentFailed.map((item) => (
                <section key={item.id} class="sync-runtime-item sync-runtime-item-compact">
                  <p class="sync-runtime-item-path">{item.path}</p>
                  <p class="sync-runtime-item-meta">
                    <span class="pill">{recentItemSummary(item)}</span>
                    <span>{item.error ?? "Unknown transfer error"}</span>
                  </p>
                </section>
              ))}
            </div>
          )}
        </section>

        <section class="account-detail-section">
          <h3>Activity</h3>
          {events.length === 0 ? (
            <p>No recent sync events for this account.</p>
          ) : (
            <div class="activity-list account-detail-activity-list">
              {events.map((event) => (
                <section key={event.id} class="activity-item">
                  <p>
                    <span class="pill">{event.kind}</span> {event.message}
                  </p>
                  <p class="activity-time">{new Date(event.timestamp).toLocaleString()}</p>
                </section>
              ))}
            </div>
          )}
        </section>

        <section class="account-detail-section">
          <h3>Settings</h3>
          <div class="inline-form-row">
            <input
              value={draftName}
              disabled={actionsDisabled}
              onInput={(event) => setDraftName(event.currentTarget.value)}
            />
            <button
              disabled={actionsDisabled || !draftName.trim()}
              onClick={() => onRename(account.id, draftName.trim())}
            >
              Rename
            </button>
          </div>

          <div class="button-row">
            <button disabled={actionsDisabled} onClick={chooseSyncFolder}>
              Choose Sync Folder
            </button>
          </div>

          <h4>Authentication</h4>
          <div class="button-row">
            <button disabled={actionsDisabled} onClick={() => onStartAuth(account.id)}>
              Start Microsoft Sign-In
            </button>
            <button disabled={actionsDisabled} onClick={() => onClearAuth(account.id)}>
              Clear Auth
            </button>
          </div>

          <h4>Danger Zone</h4>
          <div class="button-row">
            <button class="danger" disabled={actionsDisabled} onClick={() => onRemoveProfile(account.id)}>
              Remove Profile
            </button>
          </div>
          {actionsDisabled && <p class="page-subtitle">Preview-only mode. Actions are intentionally disabled.</p>}
        </section>
      </div>
    </article>
  );
}

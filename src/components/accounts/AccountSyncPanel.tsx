import type { ActivityEvent, AccountProfile } from "../../types/somedrive";
import type { SyncRuntimeAccountStatus, SyncRuntimeTransfer, SyncRuntimeRecentItem } from "../../types/somedrive";
import { SyncStateControl } from "../sync/SyncStateControl";

interface AccountSyncPanelProps {
  account: AccountProfile;
  runtimeStatus: SyncRuntimeAccountStatus | null;
  recentEvents: ActivityEvent[];
  onSetAgentState: (accountId: string, state: "syncing" | "paused") => Promise<void>;
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

function recentItemSummary(item: SyncRuntimeRecentItem): string {
  if (!item.bytesTotal) {
    return item.direction;
  }
  return `${item.direction} - ${formatBytes(item.bytesTotal)}`;
}

export function AccountSyncPanel({ account, runtimeStatus, recentEvents, onSetAgentState }: AccountSyncPanelProps) {
  const inProgress = runtimeStatus?.inProgress.slice(0, 20) ?? [];
  const recentCompleted = runtimeStatus?.recentCompleted.slice(0, 25) ?? [];
  const recentFailed = runtimeStatus?.recentFailed.slice(0, 25) ?? [];

  return (
    <article class="card">
      <h3>Synchronization</h3>
      <p>
        Current State: <span class="pill">{account.agentState}</span>
      </p>
      <p>
        Runtime Status: <span class="pill">{runtimeStatus?.phaseMessage ?? "Waiting for runtime updates"}</span>
      </p>
      <div class="button-row">
        <SyncStateControl
          state={account.agentState === "syncing" ? "syncing" : "paused"}
          onToggle={(next) => onSetAgentState(account.id, next)}
        />
      </div>

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
                    {formatBytes(transfer.bytesDone)}
                    {transfer.bytesTotal ? ` / ${formatBytes(transfer.bytesTotal)}` : ""}
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

      <h4>Recent Sync Activity</h4>
      {recentEvents.length === 0 ? (
        <p>No recent sync events for this account.</p>
      ) : (
        <div class="activity-list">
          {recentEvents.map((event) => (
            <section key={event.id} class="activity-item">
              <p>
                <span class="pill">{event.kind}</span> {event.message}
              </p>
              <p class="activity-time">{new Date(event.timestamp).toLocaleString()}</p>
            </section>
          ))}
        </div>
      )}
    </article>
  );
}

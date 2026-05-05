import type { ActivityEvent, AccountProfile } from "../../types/onedrive";

interface AccountSyncPanelProps {
  account: AccountProfile;
  recentEvents: ActivityEvent[];
  onSetAgentState: (accountId: string, state: "syncing" | "paused" | "idle") => Promise<void>;
}

export function AccountSyncPanel({ account, recentEvents, onSetAgentState }: AccountSyncPanelProps) {
  return (
    <article class="card">
      <h3>Synchronization</h3>
      <p>
        Current State: <span class="pill">{account.agentState}</span>
      </p>
      <div class="button-row">
        <button onClick={() => onSetAgentState(account.id, "syncing")}>Start Sync</button>
        <button onClick={() => onSetAgentState(account.id, "paused")}>Pause Sync</button>
        <button onClick={() => onSetAgentState(account.id, "idle")}>Stop Sync</button>
      </div>

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

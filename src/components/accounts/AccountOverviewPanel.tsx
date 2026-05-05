import type { AccountProfile } from "../../types/onedrive";

interface AccountOverviewPanelProps {
  account: AccountProfile;
  onSetAgentState: (accountId: string, state: "syncing" | "paused" | "idle") => Promise<void>;
  onStartAuth: (accountId: string) => Promise<unknown>;
}

export function AccountOverviewPanel({ account, onSetAgentState, onStartAuth }: AccountOverviewPanelProps) {
  return (
    <article class="card">
      <h3>Account Overview</h3>
      <p>
        Auth State: <span class="pill">{account.authConfigured ? "Connected" : "Needs Authentication"}</span>
      </p>
      <p>
        Sync State: <span class="pill">{account.agentState}</span>
      </p>
      <p>Sync Root: {account.syncRoot}</p>
      <p>Last Sync: {account.lastSyncAt ? new Date(account.lastSyncAt).toLocaleString() : "Never"}</p>
      <div class="button-row">
        <button onClick={() => onSetAgentState(account.id, "syncing")}>Sync Now</button>
        <button onClick={() => onSetAgentState(account.id, "paused")}>Pause</button>
        <button onClick={() => onSetAgentState(account.id, "idle")}>Stop</button>
        {!account.authConfigured && <button onClick={() => onStartAuth(account.id)}>Authenticate</button>}
      </div>
    </article>
  );
}

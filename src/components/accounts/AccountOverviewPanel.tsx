import type { AccountProfile } from "../../types/onedrive";
import { SyncStateControl } from "../sync/SyncStateControl";

interface AccountOverviewPanelProps {
  account: AccountProfile;
  onSetAgentState: (accountId: string, state: "syncing" | "paused") => Promise<void>;
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
        <SyncStateControl
          state={account.agentState === "syncing" ? "syncing" : "paused"}
          onToggle={(next) => onSetAgentState(account.id, next)}
        />
        {!account.authConfigured && <button onClick={() => onStartAuth(account.id)}>Authenticate</button>}
      </div>
    </article>
  );
}

import {
  IconBuildingBank,
  IconCloudCheck,
  IconCloudX,
  IconFolder,
  IconRefresh,
  IconUser,
} from "@tabler/icons-preact";
import type { AccountProfile } from "../../types/onedrive";

interface AccountCardProps {
  account: AccountProfile;
  onOpenDetails: (accountId: string) => void;
  onSetAgentState: (accountId: string, state: "syncing" | "paused" | "idle") => Promise<void>;
  onStartAuth: (accountId: string) => Promise<unknown>;
}

export function AccountCard({ account, onOpenDetails, onSetAgentState, onStartAuth }: AccountCardProps) {
  const authLabel = account.authConfigured ? "Connected" : "Needs Authentication";
  const accountIcon = account.kind === "business" ? <IconBuildingBank size={16} /> : <IconUser size={16} />;
  const authIcon = account.authConfigured ? <IconCloudCheck size={16} /> : <IconCloudX size={16} />;

  return (
    <section class="account-item account-home-card">
      <div class="account-home-header">
        <p class="account-name">{account.displayName}</p>
        <button onClick={() => onOpenDetails(account.id)}>Open</button>
      </div>

      <p>
        Type: <span class="pill icon-pill">{accountIcon} {account.kind}</span>
      </p>
      <p>
        Auth: <span class="pill icon-pill">{authIcon} {authLabel}</span>
      </p>
      <p>
        Sync: <span class="pill icon-pill"><IconRefresh size={16} /> {account.agentState}</span>
      </p>
      <p>Last Sync: {account.lastSyncAt ? new Date(account.lastSyncAt).toLocaleString() : "Never"}</p>
      <p class="account-path"><IconFolder size={15} /> {account.syncRoot}</p>

      <div class="button-row">
        <button onClick={() => onSetAgentState(account.id, "syncing")}>Sync Now</button>
        <button onClick={() => onSetAgentState(account.id, "paused")}>Pause</button>
        <button onClick={() => onSetAgentState(account.id, "idle")}>Stop</button>
        {!account.authConfigured && <button onClick={() => onStartAuth(account.id)}>Authenticate</button>}
      </div>
    </section>
  );
}

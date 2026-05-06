import {
  IconBuildingBank,
  IconChevronRight,
  IconCloudCheck,
  IconCloudX,
  IconFolder,
  IconUser,
} from "@tabler/icons-preact";
import type { AccountProfile } from "../../types/somedrive";

interface AccountCardProps {
  account: AccountProfile;
  onOpenDetails: (accountId: string) => void;
}

export function AccountCard({ account, onOpenDetails }: AccountCardProps) {
  const authLabel = account.authConfigured ? "Connected" : "Needs Authentication";
  const accountIcon = account.kind === "business" ? <IconBuildingBank size={16} /> : <IconUser size={16} />;
  const authIcon = account.authConfigured ? <IconCloudCheck size={16} /> : <IconCloudX size={16} />;

  return (
    <section class="account-item account-home-card">
      <div class="account-home-header">
        <p class="account-name">{account.displayName}</p>
        <button class="pill-icon-btn" onClick={() => onOpenDetails(account.id)} aria-label="Open account details" title="Open account details">
          <IconChevronRight size={16} />
        </button>
      </div>

      <p>
        Type: <span class="pill icon-pill">{accountIcon} {account.kind}</span>
      </p>
      <p>
        Auth: <span class="pill icon-pill">{authIcon} {authLabel}</span>
      </p>
      <p>
        Sync: <span class="pill">{account.agentState}</span>
      </p>
      <p>Last Sync: {account.lastSyncAt ? new Date(account.lastSyncAt).toLocaleString() : "Never"}</p>
      <p class="account-path"><IconFolder size={15} /> {account.syncRoot}</p>
    </section>
  );
}

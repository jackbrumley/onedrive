import {
  IconBuildingBank,
  IconCloudCheck,
  IconCloudX,
  IconFolder,
  IconRefresh,
  IconUser,
} from "@tabler/icons-preact";
import { AccountHomeCardButton } from "./AccountHomeCardButton";
import type { AccountDetailTab } from "../../routes/appRoutes";
import type { AccountProfile, SyncRuntimeAccountStatus } from "../../types/somedrive";

interface AccountCardProps {
  account: AccountProfile;
  runtimeStatus: SyncRuntimeAccountStatus | null;
  onOpenDetails: (accountId: string, tab?: AccountDetailTab) => void;
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

export function AccountCard({ account, runtimeStatus, onOpenDetails }: AccountCardProps) {
  const authLabel = account.authConfigured ? "Connected" : "Needs Authentication";
  const lastSyncLabel = account.lastSyncAt ? new Date(account.lastSyncAt).toLocaleString() : "Never";
  const accountIcon = account.kind === "business" ? <IconBuildingBank size={16} /> : <IconUser size={16} />;
  const authIcon = account.authConfigured ? <IconCloudCheck size={16} /> : <IconCloudX size={16} />;
  const currentTransfer = runtimeStatus?.inProgress[0] ?? null;
  const currentTransferText = currentTransfer
    ? currentTransfer.bytesTotal
      ? `${formatBytes(currentTransfer.bytesDone)} / ${formatBytes(currentTransfer.bytesTotal)}`
      : `${formatBytes(currentTransfer.bytesDone)} transferred`
    : null;
  const atAGlanceStatus = runtimeStatus
    ? runtimeStatus.inProgress.length > 0
      ? `Syncing ${runtimeStatus.inProgress.length} file${runtimeStatus.inProgress.length === 1 ? "" : "s"}`
      : runtimeStatus.phaseMessage
    : account.agentState === "syncing"
      ? "Syncing"
      : account.agentState === "paused"
        ? "Synchronization paused"
        : account.agentState;

  return (
    <AccountHomeCardButton
      onClick={() => onOpenDetails(account.id)}
      ariaLabel={`Open ${account.displayName} account details`}
      title="Open account details"
    >
      <p class="account-title-line">
        <span class="account-name">{account.displayName}</span>
        <span class="account-name-sep"> - </span>
        <span class="account-email">{account.email}</span>
      </p>

      <div class="account-card-actions">
        <button
          class="account-sync-nav-btn"
          type="button"
          title="Open synchronization details"
          aria-label="Open synchronization details"
          onClick={(event) => {
            event.stopPropagation();
            onOpenDetails(account.id, "sync");
          }}
        >
          <IconRefresh class={account.agentState === "syncing" ? "sync-icon-spinning" : undefined} size={14} />
        </button>
      </div>

      <div class="account-runtime-strip">
        <span class="account-runtime-primary">{atAGlanceStatus}</span>
        {currentTransfer && <span class="account-runtime-secondary">{currentTransferText}</span>}
      </div>

      <div class="account-meta-row">
        <span class="pill icon-pill">{accountIcon} {account.kind}</span>
        <span class="pill icon-pill">{authIcon} {authLabel}</span>
        <span class="pill">{account.agentState}</span>
      </div>

      <div class="account-info-row">
        <p class="account-last-sync">Last Sync: {lastSyncLabel}</p>
        <p class="account-path"><IconFolder size={15} /> {account.syncRoot}</p>
      </div>
    </AccountHomeCardButton>
  );
}

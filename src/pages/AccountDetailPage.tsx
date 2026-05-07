import {
  IconAdjustments,
  IconBuildingBank,
  IconChevronLeft,
  IconPlayerPauseFilled,
  IconPlayerPlayFilled,
  IconRefresh,
  IconUser,
} from "@tabler/icons-preact";
import { useState } from "preact/hooks";
import { AccountDetailUnifiedPanel } from "../components/accounts/AccountDetailUnifiedPanel";
import type { AccountProfile, SyncRuntimeAccountStatus } from "../types/somedrive";

interface AccountDetailPageProps {
  account: AccountProfile | null;
  runtimeStatus: SyncRuntimeAccountStatus | null;
  view: "sync" | "settings";
  onBack: () => void;
  onOpenSettings: (accountId: string) => void;
  onOpenSync: (accountId: string) => void;
  onSetAgentState: (accountId: string, state: "syncing" | "paused") => Promise<void>;
  onStartAuth: (accountId: string) => Promise<unknown>;
  onRename: (id: string, name: string) => Promise<void>;
  onSetSyncRoot: (id: string, path: string) => Promise<void>;
  onClearAuth: (id: string) => Promise<void>;
  onRemoveProfile: (id: string) => Promise<void>;
  onOpenSyncRootFolder: (accountId: string) => Promise<void>;
  onOpenItemFolder: (accountId: string, relativePath: string) => Promise<void>;
  onReauthenticate: (accountId: string) => Promise<unknown>;
  onRetrySync: (accountId: string) => Promise<void>;
}

export function AccountDetailPage({
  account,
  runtimeStatus,
  view,
  onBack,
  onOpenSettings,
  onOpenSync,
  onSetAgentState,
  onStartAuth,
  onRename,
  onSetSyncRoot,
  onClearAuth,
  onRemoveProfile,
  onOpenSyncRootFolder,
  onOpenItemFolder,
  onReauthenticate,
  onRetrySync,
}: AccountDetailPageProps) {
  const [syncButtonHovered, setSyncButtonHovered] = useState(false);

  if (!account) {
    return (
      <section class="page">
        <h2>Account Not Found</h2>
        <article class="card">
          <p>This account does not exist anymore. Return to the account list.</p>
          <button onClick={onBack}>Back to Accounts</button>
        </article>
      </section>
    );
  }

  const syncActive =
    account.agentState === "syncing" ||
    (runtimeStatus?.inProgress.length ?? 0) > 0 ||
    runtimeStatus?.phase === "syncing" ||
    runtimeStatus?.phase === "scanning_remote" ||
    runtimeStatus?.phase === "applying_remote" ||
    runtimeStatus?.phase === "scanning_local" ||
    runtimeStatus?.phase === "applying_local";
  const nextSyncState: "syncing" | "paused" = syncActive ? "paused" : "syncing";
  const accountKindIcon = account.kind === "business" ? <IconBuildingBank size={15} /> : <IconUser size={15} />;
  const isSyncView = view === "sync";
  const backHref = isSyncView ? "#/accounts" : `#/accounts/${encodeURIComponent(account.id)}`;
  const backLabel = isSyncView ? "Back to accounts" : "Back to synchronization";

  const handleBack = () => {
    if (isSyncView) {
      onBack();
      return;
    }
    onOpenSync(account.id);
  };

  return (
    <section class="page">
      <div class="page-header account-detail-page-header">
        <a
          class="page-header-back-link"
          href={backHref}
          onClick={(event) => {
            event.preventDefault();
            handleBack();
          }}
          aria-label={backLabel}
          title={backLabel}
        >
          <IconChevronLeft size={36} stroke={2.2} />
        </a>
        <h2>{account.displayName}</h2>
        {isSyncView && (
          <div class="account-detail-page-header-actions">
            <span class="pill icon-pill account-kind-pill account-detail-kind-pill">{accountKindIcon} {account.kind}</span>
            <button
              class="account-detail-settings-btn"
              type="button"
              aria-label="Open account settings"
              title="Open account settings"
              onClick={() => onOpenSettings(account.id)}
            >
              <IconAdjustments size={16} stroke={2.2} />
            </button>
            <button
              class="account-sync-nav-btn"
              type="button"
              aria-label={syncActive ? (syncButtonHovered ? "Pause synchronization" : "Synchronizing") : (syncButtonHovered ? "Resume synchronization" : "Synchronization paused")}
              title={syncActive ? (syncButtonHovered ? "Pause synchronization" : "Synchronizing") : (syncButtonHovered ? "Resume synchronization" : "Synchronization paused")}
              onClick={() => {
                void onSetAgentState(account.id, nextSyncState);
              }}
              onMouseEnter={() => setSyncButtonHovered(true)}
              onMouseLeave={() => setSyncButtonHovered(false)}
            >
              {syncActive ? (
                syncButtonHovered ? <IconPlayerPauseFilled size={24} /> : <IconRefresh class="sync-icon-spinning" size={24} />
              ) : syncButtonHovered ? (
                <IconPlayerPlayFilled size={24} />
              ) : (
                <IconPlayerPauseFilled size={24} />
              )}
            </button>
          </div>
        )}
      </div>

      <p class="page-subtitle account-detail-subtitle">
        {isSyncView ? "Synchronization status and transfer activity." : "Account configuration and profile controls."}
      </p>

      <AccountDetailUnifiedPanel
        account={account}
        runtimeStatus={runtimeStatus}
        mode={view}
        onStartAuth={onStartAuth}
        onRename={onRename}
        onSetSyncRoot={onSetSyncRoot}
        onClearAuth={onClearAuth}
        onRemoveProfile={onRemoveProfile}
        onOpenSyncRootFolder={onOpenSyncRootFolder}
        onOpenItemFolder={onOpenItemFolder}
        onReauthenticate={onReauthenticate}
        onRetrySync={onRetrySync}
      />
    </section>
  );
}

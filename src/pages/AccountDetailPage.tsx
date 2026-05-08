import {
  IconAdjustments,
  IconAlertTriangleFilled,
  IconBuildingBank,
  IconChevronLeft,
  IconPlayerPauseFilled,
  IconPlayerPlayFilled,
  IconUser,
} from "@tabler/icons-preact";
import { memo } from "preact/compat";
import { AccountDetailUnifiedPanel } from "../components/accounts/AccountDetailUnifiedPanel";
import type { AccountProfile, SyncRuntimeAccountStatus } from "../types/somedrive";

const BLOCKING_ISSUE_CODES = new Set([
  "auth_required",
  "permission_denied",
  "disk_full",
  "sync_root_unavailable",
  "large_delete_guard",
  "unknown_error",
]);

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
  onConfirmLargeDelete: (accountId: string) => Promise<void>;
  onKeepCloudFiles: (accountId: string) => Promise<void>;
  onFetchLargeDeletePreview: (accountId: string) => Promise<string[]>;
  onExportLargeDeletePreview: (accountId: string) => Promise<void>;
}

interface AccountDetailHeaderProps {
  account: AccountProfile;
  runtimeStatus: SyncRuntimeAccountStatus | null;
  view: "sync" | "settings";
  onBack: () => void;
  onOpenSettings: (accountId: string) => void;
  onOpenSync: (accountId: string) => void;
  onSetAgentState: (accountId: string, state: "syncing" | "paused") => Promise<void>;
}

const AccountDetailHeader = memo(function AccountDetailHeader({
  account,
  runtimeStatus,
  view,
  onBack,
  onOpenSettings,
  onOpenSync,
  onSetAgentState,
}: AccountDetailHeaderProps) {
  const runtimeIssueCode = runtimeStatus?.issueCode;
  const runtimeIssueIsBlocking = runtimeIssueCode ? BLOCKING_ISSUE_CODES.has(runtimeIssueCode) : false;
  const hasBlockingIssue =
    !account.authConfigured || runtimeIssueIsBlocking || account.agentState === "error" || runtimeStatus?.phase === "error";
  const syncActive = account.agentState === "syncing";
  const syncState = hasBlockingIssue ? "stopped" : syncActive ? "syncing" : "paused";
  const nextSyncState: "syncing" | "paused" = syncState === "syncing" ? "paused" : "syncing";
  const syncStateLabel = syncState === "stopped" ? "Stopped" : syncState === "syncing" ? "Syncing" : "Paused";
  const syncButtonTitle = syncState === "stopped"
    ? "Open synchronization details"
    : syncState === "syncing"
      ? "Pause synchronization"
      : "Resume synchronization";
  const accountKindLabel = account.kind.charAt(0).toUpperCase() + account.kind.slice(1);
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
    <>
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
            <span class="pill icon-pill account-kind-pill account-detail-kind-pill">{accountKindIcon} {accountKindLabel}</span>
            <button
              class="account-detail-settings-btn"
              type="button"
              aria-label="Open account settings"
              title="Open account settings"
              onClick={() => onOpenSettings(account.id)}
            >
              <IconAdjustments size={16} stroke={2.2} />
            </button>
            <div class="account-sync-nav-control">
              <button
                class={syncState === "stopped" ? "account-sync-nav-btn account-sync-nav-btn-stopped" : "account-sync-nav-btn"}
                type="button"
                aria-label={syncButtonTitle}
                title={syncButtonTitle}
                onClick={() => {
                  if (syncState === "stopped") {
                    return;
                  }
                  void onSetAgentState(account.id, nextSyncState);
                }}
              >
                {syncState === "stopped" ? (
                  <IconAlertTriangleFilled size={24} class="sync-stopped-icon" />
                ) : syncState === "syncing" ? (
                  <IconPlayerPauseFilled size={24} />
                ) : (
                  <IconPlayerPlayFilled size={24} />
                )}
              </button>
              <span class="account-sync-state-label">{syncStateLabel}</span>
            </div>
          </div>
        )}
      </div>

      <p class="page-subtitle account-detail-subtitle">
        {isSyncView ? "Synchronization status and transfer activity." : "Account configuration and profile controls."}
      </p>
    </>
  );
});

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
  onConfirmLargeDelete,
  onKeepCloudFiles,
  onFetchLargeDeletePreview,
  onExportLargeDeletePreview,
}: AccountDetailPageProps) {
  if (!account) {
    return (
      <section class="page account-detail-page">
        <div class="page-chrome">
          <div class="page-header">
            <h2>Account Not Found</h2>
          </div>
        </div>
        <div class="page-scroll">
          <article class="card">
            <p>This account does not exist anymore. Return to the account list.</p>
            <button onClick={onBack}>Back to Accounts</button>
          </article>
        </div>
      </section>
    );
  }

  return (
    <section class="page account-detail-page">
      <div class="page-chrome">
        <AccountDetailHeader
          account={account}
          runtimeStatus={runtimeStatus}
          view={view}
          onBack={onBack}
          onOpenSettings={onOpenSettings}
          onOpenSync={onOpenSync}
          onSetAgentState={onSetAgentState}
        />
      </div>
      <div class="page-scroll">
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
          onConfirmLargeDelete={onConfirmLargeDelete}
          onKeepCloudFiles={onKeepCloudFiles}
          onFetchLargeDeletePreview={onFetchLargeDeletePreview}
          onExportLargeDeletePreview={onExportLargeDeletePreview}
        />
      </div>
    </section>
  );
}

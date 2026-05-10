import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "preact/hooks";
import { AccountSyncActivityPanel } from "./AccountSyncActivityPanel";
import type { AccountProfile, SyncRuntimeAccountStatus } from "../../types/somedrive";

interface AccountDetailUnifiedPanelProps {
  account: AccountProfile;
  runtimeStatus: SyncRuntimeAccountStatus | null;
  mode: "sync" | "settings";
  onStartAuth: (accountId: string) => Promise<unknown>;
  onRename: (id: string, name: string) => Promise<void>;
  onSetSyncRoot: (id: string, path: string) => Promise<void>;
  onClearAuth: (id: string) => Promise<void>;
  onRemoveProfile: (id: string) => Promise<void>;
  onOpenSyncRootFolder: (accountId: string) => Promise<void>;
  onOpenItemFolder: (accountId: string, relativePath: string) => Promise<void>;
  onReauthenticate: (accountId: string) => Promise<unknown>;
  onRetrySync: (accountId: string) => Promise<void>;
  onRetryFailedDownload: (accountId: string, recentItemId: string, path: string) => Promise<void>;
  onRetryAllFailedDownloads: (accountId: string) => Promise<void>;
  onConfirmLargeDelete: (accountId: string) => Promise<void>;
  onKeepCloudFiles: (accountId: string) => Promise<void>;
  onFetchLargeDeletePreview: (accountId: string) => Promise<string[]>;
  onExportLargeDeletePreview: (accountId: string) => Promise<void>;
  actionsDisabled?: boolean;
}

export function AccountDetailUnifiedPanel({
  account,
  runtimeStatus,
  mode,
  onStartAuth,
  onRename,
  onSetSyncRoot,
  onClearAuth,
  onRemoveProfile,
  onOpenSyncRootFolder,
  onOpenItemFolder,
  onReauthenticate,
  onRetrySync,
  onRetryFailedDownload,
  onRetryAllFailedDownloads,
  onConfirmLargeDelete,
  onKeepCloudFiles,
  onFetchLargeDeletePreview,
  onExportLargeDeletePreview,
  actionsDisabled = false,
}: AccountDetailUnifiedPanelProps) {
  const [draftName, setDraftName] = useState(account.displayName);
  const [largeDeletePreviewPaths, setLargeDeletePreviewPaths] = useState<string[]>([]);

  const hasLargeDeleteGuardIssue = runtimeStatus?.issueCode === "large_delete_guard";

  useEffect(() => {
    setDraftName(account.displayName);
  }, [account.displayName]);

  useEffect(() => {
    if (mode !== "sync" || !hasLargeDeleteGuardIssue) {
      setLargeDeletePreviewPaths([]);
      return;
    }
    let active = true;
    void onFetchLargeDeletePreview(account.id).then((paths) => {
      if (active) {
        setLargeDeletePreviewPaths(paths);
      }
    });
    return () => {
      active = false;
    };
  }, [account.id, hasLargeDeleteGuardIssue, mode, onFetchLargeDeletePreview]);

  const chooseSyncFolder = async () => {
    if (actionsDisabled) {
      return;
    }
    const selected = await open({
      directory: true,
      defaultPath: account.syncRoot,
      title: `Choose sync folder for ${account.displayName}`,
    });
    if (typeof selected !== "string" || !selected.trim()) {
      return;
    }
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
  };

  const runtimeIssueCode = runtimeStatus?.issueCode ?? null;
  const hasBlockingIssue =
    !account.authConfigured || runtimeIssueCode !== null || account.agentState === "error" || runtimeStatus?.phase === "error";
  const syncIssueMessage =
    runtimeStatus?.issueMessage ??
    (!account.authConfigured ? "Authentication required" : runtimeStatus?.phaseMessage ?? "Synchronization blocked");
  const issueKind =
    !account.authConfigured || runtimeIssueCode === "auth_required" ? "auth_required" : hasBlockingIssue ? "sync_error" : null;
  const issueActions =
    runtimeStatus?.issueActions.length
      ? runtimeStatus.issueActions
      : issueKind === "auth_required"
        ? ["reauthenticate", "retry_sync"]
        : issueKind === "sync_error"
          ? ["retry_sync"]
          : [];

  if (mode === "sync") {
    return (
      <article class="card account-detail-unified-card account-detail-sync-preview">
        <AccountSyncActivityPanel
          runtimeStatus={runtimeStatus}
          hasCompletedInitialSync={runtimeStatus?.twoWayReady ?? account.lastSyncAt !== null}
          issueMessage={hasBlockingIssue ? syncIssueMessage : null}
          issueKind={issueKind}
          issueActions={issueActions}
          issuePath={runtimeStatus?.issuePath ?? null}
          issueSecondaryPath={runtimeStatus?.issueSecondaryPath ?? null}
          onOpenItemFolder={(relativePath) => onOpenItemFolder(account.id, relativePath)}
          onOpenSyncRootFolder={() => onOpenSyncRootFolder(account.id)}
          onReauthenticate={() => onReauthenticate(account.id)}
          onRetrySync={() => onRetrySync(account.id)}
          onRetryFailedDownload={(recentItemId, path) => onRetryFailedDownload(account.id, recentItemId, path)}
          onRetryAllFailedDownloads={() => onRetryAllFailedDownloads(account.id)}
          onConfirmLargeDelete={() => onConfirmLargeDelete(account.id)}
          onKeepCloudFiles={() => onKeepCloudFiles(account.id)}
          largeDeletePreviewPaths={largeDeletePreviewPaths}
          onExportLargeDeletePreview={() => onExportLargeDeletePreview(account.id)}
        />
      </article>
    );
  }

  return (
    <article class="card account-detail-unified-card">
      <div class="account-detail-unified-grid">
        <section class="account-detail-section">
          <h3>Account</h3>
          <p>Email: {account.email || "Not connected yet"}</p>
          <p>Type: {account.kind}</p>
          <p>Sync Root: {account.syncRoot}</p>
          <p>Profile ID: {account.id}</p>
          <p>Last Sync: {account.lastSyncAt ? new Date(account.lastSyncAt).toLocaleString() : "Never"}</p>
        </section>

        <section class="account-detail-section">
          <h3>Account Settings</h3>
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

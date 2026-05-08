import { AppPageRenderer } from "./AppPageRenderer";
import { AccountsHomePage } from "../pages/AccountsHomePage";
import { AccountDetailPage } from "../pages/AccountDetailPage";
import { SettingsPage } from "../pages/SettingsPage";
import { DebugPage } from "../pages/DebugPage";
import { UiLabPage } from "../pages/UiLabPage";
import type { useAppRuntime } from "../hooks/useAppRuntime";

type AppRuntime = ReturnType<typeof useAppRuntime>;

interface AppWorkspaceProps {
  runtime: AppRuntime;
}

export function AppWorkspace({ runtime }: AppWorkspaceProps) {
  const selectedAccount = runtime.routeState.accountId
    ? runtime.status.accounts.find((account) => account.id === runtime.routeState.accountId) ?? null
    : null;

  const syncRuntimeByAccountId = Object.fromEntries(
    runtime.syncRuntime.accounts.map((status) => [status.profileId, status])
  );

  return (
    <AppPageRenderer
      page={runtime.routeState.page}
      renderAccountsHome={() => (
        <AccountsHomePage
          accounts={runtime.status.accounts}
          appVersion={runtime.status.appVersion}
          syncRuntimeByAccountId={syncRuntimeByAccountId}
          onCreateAccount={runtime.createAccountProfile}
          onOpenAccount={runtime.openAccount}
          onSetAgentState={runtime.setAccountAgentState}
          onOpenSyncRootFolder={runtime.openAccountSyncRootFolder}
        />
      )}
      renderAccountDetail={() => (
        <AccountDetailPage
          account={selectedAccount}
          runtimeStatus={selectedAccount ? (syncRuntimeByAccountId[selectedAccount.id] ?? null) : null}
          view={runtime.routeState.accountView ?? "sync"}
          onBack={runtime.goHome}
          onOpenSettings={runtime.openAccountSettings}
          onOpenSync={runtime.openAccount}
          onSetAgentState={runtime.setAccountAgentState}
          onStartAuth={runtime.startInteractiveAuth}
          onRename={runtime.renameAccountProfile}
          onSetSyncRoot={runtime.setAccountSyncRoot}
          onClearAuth={runtime.clearAccountAuth}
          onRemoveProfile={runtime.removeAccountProfile}
          onOpenSyncRootFolder={runtime.openAccountSyncRootFolder}
          onOpenItemFolder={runtime.openAccountItemFolder}
          onReauthenticate={runtime.startInteractiveAuth}
          onRetrySync={runtime.retryAccountSync}
          onConfirmLargeDelete={runtime.confirmAccountLargeDelete}
          onKeepCloudFiles={runtime.keepCloudFilesAfterLargeDelete}
          onFetchLargeDeletePreview={runtime.fetchAccountLargeDeletePreview}
          onExportLargeDeletePreview={runtime.exportAccountLargeDeletePreview}
        />
      )}
      renderSettings={() => (
        <SettingsPage
          autostartEnabled={runtime.autostartEnabled}
          onToggleAutostart={runtime.toggleAutostart}
          rawLoggerMode={runtime.rawLoggerMode}
          onToggleRawLoggerMode={runtime.toggleRawLoggerMode}
          syncDownloadConcurrency={runtime.syncDownloadConcurrency}
          onChangeSyncDownloadConcurrency={runtime.updateSyncDownloadConcurrency}
          onGoDebug={runtime.goDebug}
          onBack={runtime.goHome}
        />
      )}
      renderDebug={() => (
        <DebugPage
          status={runtime.status}
          onBack={runtime.goSettings}
          onNavigateUiLab={runtime.goUiLab}
          onRefreshStatus={runtime.refreshStatus}
          onFetchSessionLogText={runtime.fetchSessionLogText}
          onCopySessionLog={runtime.copySessionLog}
          onOpenSessionLog={runtime.openSessionLog}
          onOpenProfileLog={runtime.openProfileLog}
        />
      )}
      renderUiLab={() => <UiLabPage onBack={runtime.goDebug} />}
    />
  );
}

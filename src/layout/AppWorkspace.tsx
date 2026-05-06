import { AppPageRenderer } from "./AppPageRenderer";
import { AccountsHomePage } from "../pages/AccountsHomePage";
import { AccountDetailPage } from "../pages/AccountDetailPage";
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

  const selectedAccountEvents = selectedAccount
    ? runtime.activityEvents.filter((event) => event.profileId === selectedAccount.id)
    : [];

  const syncRuntimeByAccountId = Object.fromEntries(
    runtime.syncRuntime.accounts.map((status) => [status.profileId, status])
  );

  return (
    <AppPageRenderer
      page={runtime.routeState.page}
      renderAccountsHome={() => (
        <AccountsHomePage
          accounts={runtime.status.accounts}
          syncRuntimeByAccountId={syncRuntimeByAccountId}
          onCreateAccount={runtime.createAccountProfile}
          onOpenAccount={runtime.openAccount}
        />
      )}
      renderAccountDetail={() => (
        <AccountDetailPage
          account={selectedAccount}
          runtimeStatus={selectedAccount ? (syncRuntimeByAccountId[selectedAccount.id] ?? null) : null}
          activeTab={runtime.routeState.accountTab}
          events={selectedAccountEvents}
          onBack={runtime.goHome}
          onChangeTab={(tab) => {
            if (selectedAccount) {
              runtime.openAccount(selectedAccount.id, tab);
            }
          }}
          onSetAgentState={runtime.setAccountAgentState}
          onStartAuth={runtime.startInteractiveAuth}
          onRename={runtime.renameAccountProfile}
          onSetSyncRoot={runtime.setAccountSyncRoot}
          onClearAuth={runtime.clearAccountAuth}
          onRemoveProfile={runtime.removeAccountProfile}
        />
      )}
      renderDebug={() => (
        <DebugPage
          status={runtime.status}
          onBack={runtime.goHome}
          onNavigateUiLab={runtime.goUiLab}
          onRefreshStatus={runtime.refreshStatus}
          onFetchSessionLogText={runtime.fetchSessionLogText}
          onCopySessionLog={runtime.copySessionLog}
          onOpenSessionLog={runtime.openSessionLog}
        />
      )}
      renderUiLab={() => <UiLabPage onBack={runtime.goDebug} />}
    />
  );
}

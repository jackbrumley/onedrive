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

  return (
    <AppPageRenderer
      page={runtime.routeState.page}
      renderAccountsHome={() => (
        <AccountsHomePage
          accounts={runtime.status.accounts}
          onCreateAccount={runtime.createAccountProfile}
          onOpenAccount={(accountId) => runtime.openAccount(accountId, "overview")}
          onSetAccountAgentState={runtime.setAccountAgentState}
          onStartAuth={runtime.startDeviceAuth}
        />
      )}
      renderAccountDetail={() => (
        <AccountDetailPage
          account={selectedAccount}
          activeTab={runtime.routeState.accountTab}
          authSession={selectedAccount ? runtime.authSessions[selectedAccount.id] ?? null : null}
          authPending={selectedAccount ? Boolean(runtime.authPending[selectedAccount.id]) : false}
          events={selectedAccountEvents}
          onBack={runtime.goHome}
          onChangeTab={(tab) => {
            if (selectedAccount) {
              runtime.openAccount(selectedAccount.id, tab);
            }
          }}
          onSetAgentState={runtime.setAccountAgentState}
          onStartAuth={runtime.startDeviceAuth}
          onRename={runtime.renameAccountProfile}
          onSetSyncRoot={runtime.setAccountSyncRoot}
          onPollAuth={runtime.pollDeviceAuth}
          onClearAuth={runtime.clearAccountAuth}
          onRemoveProfile={runtime.removeAccountProfile}
        />
      )}
      renderDebug={() => (
        <DebugPage
          status={runtime.status}
          onNavigateUiLab={runtime.goUiLab}
          onRefreshStatus={runtime.refreshStatus}
          onFetchSessionLogText={runtime.fetchSessionLogText}
        />
      )}
      renderUiLab={() => <UiLabPage onBack={runtime.goDebug} />}
    />
  );
}

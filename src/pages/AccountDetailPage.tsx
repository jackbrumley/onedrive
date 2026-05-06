import { AccountActivityPanel } from "../components/accounts/AccountActivityPanel";
import { AccountDetailTabs } from "../components/accounts/AccountDetailTabs";
import { AccountOverviewPanel } from "../components/accounts/AccountOverviewPanel";
import { AccountSettingsPanel } from "../components/accounts/AccountSettingsPanel";
import { AccountSyncPanel } from "../components/accounts/AccountSyncPanel";
import type { AccountDetailTab } from "../routes/appRoutes";
import type { AccountProfile, ActivityEvent, SyncRuntimeAccountStatus } from "../types/somedrive";

interface AccountDetailPageProps {
  account: AccountProfile | null;
  runtimeStatus: SyncRuntimeAccountStatus | null;
  activeTab: AccountDetailTab;
  events: ActivityEvent[];
  onBack: () => void;
  onChangeTab: (tab: AccountDetailTab) => void;
  onSetAgentState: (accountId: string, state: "syncing" | "paused") => Promise<void>;
  onStartAuth: (accountId: string) => Promise<unknown>;
  onRename: (id: string, name: string) => Promise<void>;
  onSetSyncRoot: (id: string, path: string) => Promise<void>;
  onClearAuth: (id: string) => Promise<void>;
  onRemoveProfile: (id: string) => Promise<void>;
}

export function AccountDetailPage({
  account,
  runtimeStatus,
  activeTab,
  events,
  onBack,
  onChangeTab,
  onSetAgentState,
  onStartAuth,
  onRename,
  onSetSyncRoot,
  onClearAuth,
  onRemoveProfile,
}: AccountDetailPageProps) {
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

  return (
    <section class="page">
      <div class="detail-header">
        <div>
          <h2>{account.displayName}</h2>
          <p class="page-subtitle">Manage this account's sync, activity, and settings.</p>
        </div>
        <button onClick={onBack}>Back to Accounts</button>
      </div>

      <AccountDetailTabs activeTab={activeTab} onSelectTab={onChangeTab} />

      {activeTab === "overview" && (
        <AccountOverviewPanel account={account} onSetAgentState={onSetAgentState} onStartAuth={onStartAuth} />
      )}

      {activeTab === "sync" && (
        <AccountSyncPanel
          account={account}
          runtimeStatus={runtimeStatus}
          recentEvents={events.slice(0, 8)}
          onSetAgentState={onSetAgentState}
        />
      )}

      {activeTab === "activity" && <AccountActivityPanel events={events} />}

      {activeTab === "settings" && (
        <AccountSettingsPanel
          account={account}
          onRename={onRename}
          onSetSyncRoot={onSetSyncRoot}
          onStartAuth={onStartAuth}
          onClearAuth={onClearAuth}
          onRemoveProfile={onRemoveProfile}
        />
      )}
    </section>
  );
}

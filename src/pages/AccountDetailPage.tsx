import { AccountActivityPanel } from "../components/accounts/AccountActivityPanel";
import { AccountDetailTabs } from "../components/accounts/AccountDetailTabs";
import { AccountOverviewPanel } from "../components/accounts/AccountOverviewPanel";
import { AccountSettingsPanel } from "../components/accounts/AccountSettingsPanel";
import { AccountSyncPanel } from "../components/accounts/AccountSyncPanel";
import type { AccountDetailTab } from "../routes/appRoutes";
import type { AccountProfile, ActivityEvent, DeviceAuthSession } from "../types/onedrive";

interface AccountDetailPageProps {
  account: AccountProfile | null;
  activeTab: AccountDetailTab;
  authSession: DeviceAuthSession | null;
  authPending: boolean;
  events: ActivityEvent[];
  onBack: () => void;
  onChangeTab: (tab: AccountDetailTab) => void;
  onSetAgentState: (accountId: string, state: "syncing" | "paused") => Promise<void>;
  onStartAuth: (accountId: string) => Promise<unknown>;
  onRename: (id: string, name: string) => Promise<void>;
  onSetSyncRoot: (id: string, path: string) => Promise<void>;
  onPollAuth: (id: string) => Promise<unknown>;
  onClearAuth: (id: string) => Promise<void>;
  onRemoveProfile: (id: string) => Promise<void>;
}

export function AccountDetailPage({
  account,
  activeTab,
  authSession,
  authPending,
  events,
  onBack,
  onChangeTab,
  onSetAgentState,
  onStartAuth,
  onRename,
  onSetSyncRoot,
  onPollAuth,
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
          recentEvents={events.slice(0, 8)}
          onSetAgentState={onSetAgentState}
        />
      )}

      {activeTab === "activity" && <AccountActivityPanel events={events} />}

      {activeTab === "settings" && (
        <AccountSettingsPanel
          account={account}
          authSession={authSession}
          authPending={authPending}
          onRename={onRename}
          onSetSyncRoot={onSetSyncRoot}
          onStartAuth={onStartAuth}
          onPollAuth={onPollAuth}
          onClearAuth={onClearAuth}
          onRemoveProfile={onRemoveProfile}
        />
      )}
    </section>
  );
}

import { AccountDetailUnifiedPanel } from "../components/accounts/AccountDetailUnifiedPanel";
import type { AccountProfile, ActivityEvent, SyncRuntimeAccountStatus } from "../types/somedrive";

interface AccountDetailPageProps {
  account: AccountProfile | null;
  runtimeStatus: SyncRuntimeAccountStatus | null;
  events: ActivityEvent[];
  onBack: () => void;
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
  events,
  onBack,
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
          <p class="page-subtitle">Manage this account in one place.</p>
        </div>
        <button onClick={onBack}>Back to Accounts</button>
      </div>

      <AccountDetailUnifiedPanel
        account={account}
        runtimeStatus={runtimeStatus}
        events={events}
        onSetAgentState={onSetAgentState}
        onStartAuth={onStartAuth}
        onRename={onRename}
        onSetSyncRoot={onSetSyncRoot}
        onClearAuth={onClearAuth}
        onRemoveProfile={onRemoveProfile}
      />
    </section>
  );
}

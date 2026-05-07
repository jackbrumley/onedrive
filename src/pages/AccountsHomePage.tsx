import { useState } from "preact/hooks";
import { AddAccountCard } from "../components/accounts/AddAccountCard";
import { AddAccountModal } from "../components/accounts/AddAccountModal";
import { AccountCard } from "../components/accounts/AccountCard";
import type { AccountProfile, AccountKind, SyncRuntimeAccountStatus } from "../types/somedrive";

interface AccountsHomePageProps {
  accounts: AccountProfile[];
  syncRuntimeByAccountId: Record<string, SyncRuntimeAccountStatus>;
  onCreateAccount: (displayName: string, kind: AccountKind) => Promise<boolean>;
  onOpenAccount: (accountId: string) => void;
  onOpenSyncRootFolder: (accountId: string) => Promise<void>;
  onOpenItemFolder: (accountId: string, relativePath: string) => Promise<void>;
  onReauthenticate: (accountId: string) => Promise<unknown>;
  onRetrySync: (accountId: string) => Promise<void>;
}

export function AccountsHomePage({
  accounts,
  syncRuntimeByAccountId,
  onCreateAccount,
  onOpenAccount,
  onOpenSyncRootFolder,
  onOpenItemFolder,
  onReauthenticate,
  onRetrySync,
}: AccountsHomePageProps) {
  const [showAddModal, setShowAddModal] = useState(false);

  return (
    <section class="page">
      <div class="page-header">
        <h2>Accounts</h2>
      </div>
      <div class="accounts-grid">
        {accounts.map((account) => (
          <AccountCard
            key={account.id}
            account={account}
            runtimeStatus={syncRuntimeByAccountId[account.id] ?? null}
            onOpenDetails={onOpenAccount}
            onOpenSyncRootFolder={onOpenSyncRootFolder}
            onOpenItemFolder={onOpenItemFolder}
            onReauthenticate={onReauthenticate}
            onRetrySync={onRetrySync}
          />
        ))}
        <AddAccountCard onClick={() => setShowAddModal(true)} />
      </div>

      {showAddModal && (
        <AddAccountModal
          onClose={() => setShowAddModal(false)}
          onCreateAccount={onCreateAccount}
        />
      )}
    </section>
  );
}

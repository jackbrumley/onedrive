import { useState } from "preact/hooks";
import { AddAccountCard } from "../components/accounts/AddAccountCard";
import { AddAccountModal } from "../components/accounts/AddAccountModal";
import { AccountCard } from "../components/accounts/AccountCard";
import type { AccountProfile, AccountKind } from "../types/somedrive";

interface AccountsHomePageProps {
  accounts: AccountProfile[];
  onCreateAccount: (displayName: string, kind: AccountKind) => Promise<boolean>;
  onOpenAccount: (accountId: string) => void;
}

export function AccountsHomePage({
  accounts,
  onCreateAccount,
  onOpenAccount,
}: AccountsHomePageProps) {
  const [showAddModal, setShowAddModal] = useState(false);

  return (
    <section class="page">
      <div class="accounts-grid">
        {accounts.map((account) => (
          <AccountCard
            key={account.id}
            account={account}
            onOpenDetails={onOpenAccount}
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

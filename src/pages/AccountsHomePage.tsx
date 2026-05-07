import { useState } from "preact/hooks";
import { IconBrandGithub, IconHeart } from "@tabler/icons-preact";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AddAccountCard } from "../components/accounts/AddAccountCard";
import { AddAccountModal } from "../components/accounts/AddAccountModal";
import { AccountCard } from "../components/accounts/AccountCard";
import type { AccountProfile, AccountKind, SyncRuntimeAccountStatus } from "../types/somedrive";

interface AccountsHomePageProps {
  accounts: AccountProfile[];
  appVersion: string;
  syncRuntimeByAccountId: Record<string, SyncRuntimeAccountStatus>;
  onCreateAccount: (displayName: string, kind: AccountKind) => Promise<boolean>;
  onOpenAccount: (accountId: string) => void;
  onSetAgentState: (accountId: string, state: "syncing" | "paused") => Promise<void>;
  onOpenSyncRootFolder: (accountId: string) => Promise<void>;
  onOpenItemFolder: (accountId: string, relativePath: string) => Promise<void>;
  onReauthenticate: (accountId: string) => Promise<unknown>;
  onRetrySync: (accountId: string) => Promise<void>;
}

export function AccountsHomePage({
  accounts,
  appVersion,
  syncRuntimeByAccountId,
  onCreateAccount,
  onOpenAccount,
  onSetAgentState,
  onOpenSyncRootFolder,
  onOpenItemFolder,
  onReauthenticate,
  onRetrySync,
}: AccountsHomePageProps) {
  const [showAddModal, setShowAddModal] = useState(false);

  return (
    <section class="page accounts-page">
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
            onSetAgentState={onSetAgentState}
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

      <footer class="accounts-page-footer" aria-label="Project links and application version">
        <div class="accounts-page-footer-links">
          <button
            type="button"
            class="accounts-page-footer-icon-btn"
            title="Open SomeDrive GitHub"
            aria-label="Open SomeDrive GitHub"
            onClick={() => {
              void openUrl("https://github.com/jackbrumley/somedrive");
            }}
          >
            <IconBrandGithub size={18} />
          </button>
          <button
            type="button"
            class="accounts-page-footer-icon-btn accounts-page-footer-heart-btn"
            title="Support SomeDrive on GitHub"
            aria-label="Support SomeDrive on GitHub"
            onClick={() => {
              void openUrl("https://github.com/jackbrumley/somedrive");
            }}
          >
            <IconHeart size={18} />
          </button>
        </div>
        <div class="accounts-page-footer-version">v{appVersion}</div>
      </footer>
    </section>
  );
}

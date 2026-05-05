import { useMemo, useState } from "preact/hooks";
import { SyncStateControl } from "../components/sync/SyncStateControl";
import type { AccountProfile } from "../types/onedrive";

interface UiLabPageProps {
  onBack: () => void;
}

const previewAccounts: AccountProfile[] = [
  {
    id: "lab-1",
    displayName: "Personal",
    slug: "personal",
    kind: "personal",
    syncRoot: "/home/user/OneDrive-OSS/personal",
    authConfigured: true,
    agentState: "syncing",
    lastSyncAt: null,
  },
  {
    id: "lab-2",
    displayName: "Work",
    slug: "work",
    kind: "business",
    syncRoot: "/home/user/OneDrive-OSS/work",
    authConfigured: true,
    agentState: "paused",
    lastSyncAt: null,
  },
  {
    id: "lab-3",
    displayName: "Personal 2",
    slug: "personal-2",
    kind: "personal",
    syncRoot: "/home/user/OneDrive-OSS/personal-2",
    authConfigured: false,
    agentState: "error",
    lastSyncAt: null,
  },
];

export function UiLabPage({ onBack }: UiLabPageProps) {
  const [scenario, setScenario] = useState<"empty" | "single" | "mixed">("mixed");
  const [showErrorBanner, setShowErrorBanner] = useState(true);
  const [demoAccountSyncState, setDemoAccountSyncState] = useState<"syncing" | "paused">("syncing");
  const [demoGlobalSyncState, setDemoGlobalSyncState] = useState<"syncing" | "paused">("paused");

  const accounts = useMemo(() => {
    if (scenario === "empty") {
      return [];
    }
    if (scenario === "single") {
      return [previewAccounts[0]];
    }
    return previewAccounts;
  }, [scenario]);

  return (
    <section class="page">
      <h2>UI Lab</h2>
      <article class="card">
        <p>Hidden visual sandbox inspired by Rusty G6 + Yambuck debug preview patterns.</p>
        <p>Route shortcut: #/ui-lab</p>
        <div class="button-row">
          <button onClick={onBack}>Back to Debug</button>
          <button onClick={() => setScenario("empty")}>Empty State</button>
          <button onClick={() => setScenario("single")}>Single Account</button>
          <button onClick={() => setScenario("mixed")}>Mixed Accounts</button>
          <button onClick={() => setShowErrorBanner((current) => !current)}>
            {showErrorBanner ? "Hide" : "Show"} Error Banner
          </button>
        </div>
      </article>

      <article class="card">
        <h3>Pause / Play Control Demo</h3>
        <p>Click these controls to simulate the sync pause/resume behavior.</p>
        <div class="button-row" style={{ alignItems: "center" }}>
          <span class="pill">Account Sync: {demoAccountSyncState}</span>
          <SyncStateControl state={demoAccountSyncState} onToggle={async (next) => setDemoAccountSyncState(next)} />
        </div>
        <div class="button-row" style={{ alignItems: "center" }}>
          <span class="pill">Global Sync: {demoGlobalSyncState}</span>
          <SyncStateControl state={demoGlobalSyncState} onToggle={async (next) => setDemoGlobalSyncState(next)} />
        </div>
      </article>

      {showErrorBanner && (
        <article class="card card-error">
          <h3>Simulated Error Banner</h3>
          <p>One account needs re-authentication. User action should stay in-app.</p>
          <button>Reconnect Account</button>
        </article>
      )}

      <article class="card">
        <h3>Preview Account Cards</h3>
        {accounts.length === 0 ? (
          <p>No accounts configured yet. Show setup call-to-action.</p>
        ) : (
          <div class="account-list">
            {accounts.map((account) => (
              <section key={account.id} class="account-item">
                <p class="account-name">{account.displayName}</p>
                <p>
                  Kind: <span class="pill">{account.kind}</span>
                </p>
                <p>
                  Agent: <span class="pill">{account.agentState}</span>
                </p>
                <p>Path: {account.syncRoot}</p>
                <div class="button-row">
                  <button>Open Folder</button>
                  <button>Sync Now</button>
                  <button>Pause</button>
                  <button>Settings</button>
                </div>
              </section>
            ))}
          </div>
        )}
      </article>
    </section>
  );
}

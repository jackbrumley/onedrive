import { useEffect, useMemo, useState } from "preact/hooks";
import { AccountCard } from "../components/accounts/AccountCard";
import { AccountDetailUnifiedPanel } from "../components/accounts/AccountDetailUnifiedPanel";
import { SyncStateControl } from "../components/sync/SyncStateControl";
import type {
  AccountProfile,
  ActivityEvent,
  SyncAgentState,
  SyncRuntimeAccountStatus,
} from "../types/somedrive";

interface UiLabPageProps {
  onBack: () => void;
}

const previewAccounts: AccountProfile[] = [
  {
    id: "lab-1",
    displayName: "Personal",
    email: "personal@example.com",
    slug: "personal",
    kind: "personal",
    syncRoot: "/home/user/SomeDrive/personal",
    authConfigured: true,
    agentState: "syncing",
    lastSyncAt: null,
  },
  {
    id: "lab-2",
    displayName: "Work",
    email: "work@example.com",
    slug: "work",
    kind: "business",
    syncRoot: "/home/user/SomeDrive/work",
    authConfigured: true,
    agentState: "paused",
    lastSyncAt: null,
  },
  {
    id: "lab-3",
    displayName: "Personal 2",
    email: "",
    slug: "personal-2",
    kind: "personal",
    syncRoot: "/home/user/SomeDrive/personal-2",
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
  const [selectedLabAccountId, setSelectedLabAccountId] = useState<string | null>(null);
  const [labAgentStateById, setLabAgentStateById] = useState<Record<string, SyncAgentState>>({});

  const accounts = useMemo(() => {
    const withOverrides = previewAccounts.map((account) => ({
      ...account,
      agentState: labAgentStateById[account.id] ?? account.agentState,
    }));
    if (scenario === "empty") {
      return [];
    }
    if (scenario === "single") {
      return [withOverrides[0]];
    }
    return withOverrides;
  }, [labAgentStateById, scenario]);

  const previewEvents: ActivityEvent[] = useMemo(
    () => [
      {
        id: "evt-lab-1",
        profileId: "lab-1",
        profileName: "Personal",
        kind: "success",
        message: "Synchronized 24 files from Documents",
        timestamp: new Date(Date.now() - 3 * 60 * 1000).toISOString(),
      },
      {
        id: "evt-lab-2",
        profileId: "lab-1",
        profileName: "Personal",
        kind: "info",
        message: "Detected 2 local file changes",
        timestamp: new Date(Date.now() - 11 * 60 * 1000).toISOString(),
      },
      {
        id: "evt-lab-3",
        profileId: "lab-2",
        profileName: "Work",
        kind: "warning",
        message: "Retrying upload for Budget-Q4.xlsx",
        timestamp: new Date(Date.now() - 7 * 60 * 1000).toISOString(),
      },
      {
        id: "evt-lab-4",
        profileId: "lab-3",
        profileName: "Personal 2",
        kind: "error",
        message: "Authentication required before next sync cycle",
        timestamp: new Date(Date.now() - 19 * 60 * 1000).toISOString(),
      },
    ],
    []
  );

  const selectedLabAccount = selectedLabAccountId
    ? accounts.find((account) => account.id === selectedLabAccountId) ?? null
    : null;

  const selectedLabEvents = selectedLabAccount
    ? previewEvents.filter((event) => event.profileId === selectedLabAccount.id)
    : [];

  const runtimeByAccountId: Record<string, SyncRuntimeAccountStatus> = useMemo(
    () => ({
      "lab-1": {
        profileId: "lab-1",
        phase: "syncing",
        phaseMessage: "Syncing 2 files",
        updatedAt: new Date().toISOString(),
        inProgress: [
          {
            id: "lab-transfer-1",
            direction: "download",
            path: "Documents/ProjectPlan.docx",
            bytesDone: 3670016,
            bytesTotal: 4928307,
            startedAt: new Date(Date.now() - 90_000).toISOString(),
            updatedAt: new Date().toISOString(),
          },
          {
            id: "lab-transfer-2",
            direction: "upload",
            path: "Pictures/team-photo.png",
            bytesDone: 1048576,
            bytesTotal: 3145728,
            startedAt: new Date(Date.now() - 45_000).toISOString(),
            updatedAt: new Date().toISOString(),
          },
        ],
        recentCompleted: [
          {
            id: "lab-complete-1",
            direction: "download",
            path: "Reports/Q4-summary.pdf",
            bytesTotal: 1873920,
            finishedAt: new Date(Date.now() - 180_000).toISOString(),
            status: "completed",
            error: null,
          },
          {
            id: "lab-complete-2",
            direction: "upload",
            path: "Invoices/paid-2026-05.csv",
            bytesTotal: 48203,
            finishedAt: new Date(Date.now() - 110_000).toISOString(),
            status: "completed",
            error: null,
          },
        ],
        recentFailed: [],
      },
      "lab-2": {
        profileId: "lab-2",
        phase: "paused",
        phaseMessage: "Synchronization paused",
        updatedAt: new Date().toISOString(),
        inProgress: [],
        recentCompleted: [],
        recentFailed: [],
      },
      "lab-3": {
        profileId: "lab-3",
        phase: "error",
        phaseMessage: "Sync error: authentication required",
        updatedAt: new Date().toISOString(),
        inProgress: [],
        recentCompleted: [],
        recentFailed: [
          {
            id: "lab-failed-1",
            direction: "upload",
            path: "Notes/todo.txt",
            bytesTotal: 4096,
            finishedAt: new Date(Date.now() - 60_000).toISOString(),
            status: "failed",
            error: "Authentication required",
          },
        ],
      },
    }),
    []
  );

  const selectedRuntimeStatus = selectedLabAccount ? runtimeByAccountId[selectedLabAccount.id] ?? null : null;

  useEffect(() => {
    if (!selectedLabAccountId) {
      return;
    }
    if (!accounts.some((account) => account.id === selectedLabAccountId)) {
      setSelectedLabAccountId(null);
    }
  }, [accounts, selectedLabAccountId]);

  return (
    <section class="page">
      <div class="page-header">
        <h2>UI Lab</h2>
        <button class="page-header-action" onClick={onBack}>
          Back
        </button>
      </div>
      {!selectedLabAccount ? (
        <>
          {accounts.length === 0 ? (
            <p>No accounts configured yet. Show setup call-to-action.</p>
          ) : (
            <div class="account-list">
              {accounts.map((account) => (
                <AccountCard
                  key={account.id}
                  account={account}
                  runtimeStatus={runtimeByAccountId[account.id] ?? null}
                  onOpenDetails={(accountId) => {
                    setSelectedLabAccountId(accountId);
                  }}
                  onOpenSyncRootFolder={async () => undefined}
                  onOpenItemFolder={async () => undefined}
                />
              ))}
            </div>
          )}

          <div class="button-row" style={{ marginTop: "10px" }}>
            <button onClick={() => setScenario("empty")}>Empty State</button>
            <button onClick={() => setScenario("single")}>Single Account</button>
            <button onClick={() => setScenario("mixed")}>Mixed Accounts</button>
            <button onClick={() => setShowErrorBanner((current) => !current)}>
              {showErrorBanner ? "Hide" : "Show"} Error Banner
            </button>
          </div>

          {showErrorBanner && (
            <article class="card card-error" style={{ marginTop: "10px" }}>
              <h3>Simulated Error Banner</h3>
              <p>One account needs re-authentication. User action should stay in-app.</p>
              <button>Reconnect Account</button>
            </article>
          )}

          <div class="button-row" style={{ alignItems: "center", marginTop: "10px" }}>
            <span class="pill">Account Sync: {demoAccountSyncState}</span>
            <SyncStateControl state={demoAccountSyncState} onToggle={async (next) => setDemoAccountSyncState(next)} />
            <span class="pill">Global Sync: {demoGlobalSyncState}</span>
            <SyncStateControl state={demoGlobalSyncState} onToggle={async (next) => setDemoGlobalSyncState(next)} />
          </div>
        </>
      ) : (
        <>
          <div class="detail-header">
            <div>
              <h3>{selectedLabAccount.displayName}</h3>
              <p class="page-subtitle">UI Lab account detail preview.</p>
            </div>
            <button
              onClick={() => {
                setSelectedLabAccountId(null);
              }}
            >
              Back to Cards
            </button>
          </div>

          <AccountDetailUnifiedPanel
            account={selectedLabAccount}
            runtimeStatus={selectedRuntimeStatus}
            events={selectedLabEvents}
            actionsDisabled
            onSetAgentState={async (accountId, nextState) => {
              setLabAgentStateById((current) => ({ ...current, [accountId]: nextState }));
            }}
            onStartAuth={async () => null}
            onRename={async () => undefined}
            onSetSyncRoot={async () => undefined}
            onClearAuth={async () => undefined}
            onRemoveProfile={async () => undefined}
          />
        </>
      )}
    </section>
  );
}

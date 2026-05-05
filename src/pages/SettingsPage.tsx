import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "preact/hooks";
import { SelectField, type SelectFieldOption } from "../components/ui/SelectField";
import type {
  AccountKind,
  AccountProfile,
  DeviceAuthSession,
  SyncAgentState,
} from "../types/onedrive";

const accountKindOptions: SelectFieldOption[] = [
  { value: "personal", label: "Personal" },
  { value: "business", label: "Business" },
];

interface SettingsPageProps {
  accounts: AccountProfile[];
  authSessions: Record<string, DeviceAuthSession>;
  authPending: Record<string, boolean>;
  onCreateAccountProfile: (displayName: string, kind: AccountKind) => Promise<boolean>;
  onRenameAccountProfile: (id: string, displayName: string) => Promise<void>;
  onRemoveAccountProfile: (id: string) => Promise<void>;
  onSetAccountAgentState: (id: string, agentState: SyncAgentState) => Promise<void>;
  onSetAccountSyncRoot: (id: string, syncRoot: string) => Promise<void>;
  onStartDeviceAuth: (profileId: string) => Promise<DeviceAuthSession | null>;
  onPollDeviceAuth: (profileId: string) => Promise<unknown>;
  onClearAccountAuth: (profileId: string) => Promise<void>;
}

export function SettingsPage({
  accounts,
  authSessions,
  authPending,
  onCreateAccountProfile,
  onRenameAccountProfile,
  onRemoveAccountProfile,
  onSetAccountAgentState,
  onSetAccountSyncRoot,
  onStartDeviceAuth,
  onPollDeviceAuth,
  onClearAccountAuth,
}: SettingsPageProps) {
  const [newAccountName, setNewAccountName] = useState("");
  const [newAccountKind, setNewAccountKind] = useState<AccountKind>("personal");
  const [renameValues, setRenameValues] = useState<Record<string, string>>({});

  const submitAddAccount = async (event: Event) => {
    event.preventDefault();
    const trimmed = newAccountName.trim();
    if (!trimmed) {
      return;
    }
    await onCreateAccountProfile(trimmed, newAccountKind);
    setNewAccountName("");
  };

  const chooseSyncFolder = async (account: AccountProfile) => {
    const selected = await open({
      directory: true,
      defaultPath: account.syncRoot,
      title: `Choose sync folder for ${account.displayName}`,
    });
    if (typeof selected === "string" && selected.trim()) {
      await onSetAccountSyncRoot(account.id, selected);
    }
  };

  return (
    <section class="page">
      <h2>Settings</h2>

      <article class="card">
        <h3>Add Account</h3>
        <p>Everything is in-app: add personal or business accounts with no terminal setup.</p>
        <form class="account-form" onSubmit={submitAddAccount}>
          <label class="field-label" for="account-name-input">
            Account Name
          </label>
          <input
            id="account-name-input"
            value={newAccountName}
            onInput={(event) => setNewAccountName(event.currentTarget.value)}
            placeholder="Personal, Work, Project Team"
          />

          <label class="field-label" for="account-kind-select">
            Account Type
          </label>
          <SelectField
            id="account-kind-select"
            name="account-kind-select"
            value={newAccountKind}
            options={accountKindOptions}
            onValueChange={(next) => setNewAccountKind(next as AccountKind)}
          />

          <button type="submit" disabled={!newAccountName.trim()}>
            Add Account Profile
          </button>
        </form>
      </article>

      <article class="card">
        <h3>Account Profiles</h3>
        {accounts.length === 0 ? (
          <p>No accounts yet. Create your first profile above.</p>
        ) : (
          <div class="account-list">
            {accounts.map((account) => {
              const renameDraft = renameValues[account.id] ?? account.displayName;
              const session = authSessions[account.id];
              return (
                <section key={account.id} class="account-item">
                  <p class="account-name">{account.displayName}</p>
                  <p>
                    Type: <span class="pill">{account.kind}</span>
                  </p>
                  <p>
                    Auth: <span class="pill">{account.authConfigured ? "connected" : "not connected"}</span>
                  </p>
                  <p>
                    Agent: <span class="pill">{account.agentState}</span>
                  </p>
                  <p>Sync Root: {account.syncRoot}</p>
                  <p>Slug: {account.slug}</p>

                  <div class="inline-form-row">
                    <input
                      value={renameDraft}
                      onInput={(event) =>
                        setRenameValues((current) => ({
                          ...current,
                          [account.id]: event.currentTarget.value,
                        }))
                      }
                      placeholder="Rename profile"
                    />
                    <button
                      onClick={() => onRenameAccountProfile(account.id, renameDraft.trim())}
                      disabled={!renameDraft.trim()}
                    >
                      Rename
                    </button>
                    <button onClick={() => chooseSyncFolder(account)}>Choose Folder</button>
                  </div>

                  <div class="button-row">
                    <button onClick={() => onSetAccountAgentState(account.id, "syncing")}>Start</button>
                    <button onClick={() => onSetAccountAgentState(account.id, "paused")}>Pause</button>
                    <button onClick={() => onSetAccountAgentState(account.id, "idle")}>Stop</button>
                    <button onClick={() => onSetAccountAgentState(account.id, "error")}>Flag Error</button>
                  </div>

                  <div class="button-row">
                    <button onClick={() => onStartDeviceAuth(account.id)}>Start Microsoft Sign-In</button>
                    <button onClick={() => onPollDeviceAuth(account.id)} disabled={Boolean(authPending[account.id])}>
                      {authPending[account.id] ? "Polling..." : "Check Sign-In"}
                    </button>
                    <button onClick={() => onClearAccountAuth(account.id)}>Clear Auth</button>
                    <button class="danger" onClick={() => onRemoveAccountProfile(account.id)}>
                      Remove Profile
                    </button>
                  </div>

                  {session && (
                    <article class="auth-card">
                      <p>
                        Device sign-in code for <strong>{account.displayName}</strong>: <strong>{session.userCode}</strong>
                      </p>
                      <p>{session.message}</p>
                      <div class="button-row">
                        <button onClick={() => openUrl(session.verificationUriComplete ?? session.verificationUri)}>
                          Open Microsoft Verification Page
                        </button>
                      </div>
                    </article>
                  )}
                </section>
              );
            })}
          </div>
        )}
      </article>
    </section>
  );
}

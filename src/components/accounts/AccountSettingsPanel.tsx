import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "preact/hooks";
import type { AccountProfile, DeviceAuthSession } from "../../types/onedrive";

interface AccountSettingsPanelProps {
  account: AccountProfile;
  authSession: DeviceAuthSession | null;
  authPending: boolean;
  onRename: (id: string, name: string) => Promise<void>;
  onSetSyncRoot: (id: string, path: string) => Promise<void>;
  onStartAuth: (id: string) => Promise<unknown>;
  onPollAuth: (id: string) => Promise<unknown>;
  onClearAuth: (id: string) => Promise<void>;
  onRemoveProfile: (id: string) => Promise<void>;
}

export function AccountSettingsPanel({
  account,
  authSession,
  authPending,
  onRename,
  onSetSyncRoot,
  onStartAuth,
  onPollAuth,
  onClearAuth,
  onRemoveProfile,
}: AccountSettingsPanelProps) {
  const [draftName, setDraftName] = useState(account.displayName);

  const chooseSyncFolder = async () => {
    const selected = await open({
      directory: true,
      defaultPath: account.syncRoot,
      title: `Choose sync folder for ${account.displayName}`,
    });
    if (typeof selected === "string" && selected.trim()) {
      const normalizedSelected = selected.replace(/\/+$/, "");
      if (/\/OneDrive$/i.test(normalizedSelected)) {
        const confirmed = window.confirm(
          "This looks like the default folder used by other OneDrive apps. It is safer to use OneDrive-OSS to avoid conflicts. Continue anyway?"
        );
        if (!confirmed) {
          return;
        }
      }
      await onSetSyncRoot(account.id, selected);
    }
  };

  return (
    <article class="card">
      <h3>Account Settings</h3>

      <div class="inline-form-row">
        <input value={draftName} onInput={(event) => setDraftName(event.currentTarget.value)} />
        <button onClick={() => onRename(account.id, draftName.trim())} disabled={!draftName.trim()}>
          Rename
        </button>
      </div>

      <p>Sync Root: {account.syncRoot}</p>
      <div class="button-row">
        <button onClick={chooseSyncFolder}>Choose Sync Folder</button>
      </div>

      <h4>Authentication</h4>
      <div class="button-row">
        <button onClick={() => onStartAuth(account.id)}>Start Microsoft Sign-In</button>
        <button onClick={() => onPollAuth(account.id)} disabled={authPending}>
          {authPending ? "Polling..." : "Check Sign-In"}
        </button>
        <button onClick={() => onClearAuth(account.id)}>Clear Auth</button>
      </div>

      {authSession && (
        <section class="auth-card">
          <p>
            Sign-in code: <strong>{authSession.userCode}</strong>
          </p>
          <p>{authSession.message}</p>
          <button onClick={() => openUrl(authSession.verificationUriComplete ?? authSession.verificationUri)}>
            Open Microsoft Verification Page
          </button>
        </section>
      )}

      <h4>Danger Zone</h4>
      <div class="button-row">
        <button class="danger" onClick={() => onRemoveProfile(account.id)}>
          Remove Profile
        </button>
      </div>
    </article>
  );
}

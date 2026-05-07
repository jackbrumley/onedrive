import { invoke } from "@tauri-apps/api/core";
import type {
  AccountKind,
  AccountProfile,
  CreateAccountProfileInput,
  RenameAccountProfileInput,
  SetAccountAgentStateInput,
  SetAccountSyncRootInput,
  SyncAgentState,
  ToastType,
} from "../../types/somedrive";

interface AccountActionsFactoryParams {
  showToast: (message: string, type?: ToastType, durationMs?: number) => void;
  refreshStatus: () => Promise<void>;
  refreshActivity: () => Promise<void>;
  refreshSyncRuntime: () => Promise<void>;
  openAccount: (accountId: string) => void;
}

export function createAccountActions({
  showToast,
  refreshStatus,
  refreshActivity,
  refreshSyncRuntime,
  openAccount,
}: AccountActionsFactoryParams) {
  const refreshAll = () => Promise.all([refreshStatus(), refreshActivity(), refreshSyncRuntime()]);

  const startInteractiveAuth = async (profileId: string) => {
    try {
      await invoke("start_interactive_auth", { profileId });
      showToast("Microsoft sign-in opened. Complete sign-in in the auth window.", "info", 3600);
      return true;
    } catch (error) {
      showToast(`Failed to start sign-in: ${error}`, "error", 4200);
      return false;
    }
  };

  const createAccountProfile = async (displayName: string, kind: AccountKind) => {
    const input: CreateAccountProfileInput = { displayName, kind };
    try {
      const profile = await invoke<AccountProfile>("create_account_profile", { input });
      showToast(`Account '${displayName}' added.`, "success", 2200);
      await refreshAll();
      const authStarted = await startInteractiveAuth(profile.id);
      openAccount(profile.id);
      return authStarted;
    } catch (error) {
      showToast(`Failed to add account: ${error}`, "error", 4200);
      return false;
    }
  };

  const renameAccountProfile = async (id: string, displayName: string) => {
    const input: RenameAccountProfileInput = { id, displayName };
    try {
      await invoke("rename_account_profile", { input });
      showToast("Account name updated.", "success", 2200);
      await refreshAll();
    } catch (error) {
      showToast(`Failed to rename account: ${error}`, "error", 4200);
    }
  };

  const removeAccountProfile = async (id: string) => {
    try {
      await invoke("remove_account_profile", { input: { id } });
      showToast("Account removed. Synced files were not deleted.", "info", 3600);
      await refreshAll();
    } catch (error) {
      showToast(`Failed to remove account: ${error}`, "error", 4200);
    }
  };

  const setAccountAgentState = async (id: string, agentState: SyncAgentState) => {
    const input: SetAccountAgentStateInput = { id, agentState };
    try {
      await invoke("set_account_agent_state", { input });
      showToast(`Agent state set to '${agentState}'.`, "success", 2200);
      await refreshAll();
    } catch (error) {
      showToast(`Failed to update agent state: ${error}`, "error", 4200);
    }
  };

  const setAccountSyncRoot = async (id: string, syncRoot: string) => {
    const input: SetAccountSyncRootInput = { id, syncRoot };
    try {
      await invoke("set_account_sync_root", { input });
      showToast("Sync folder updated.", "success", 2200);
      await refreshAll();
    } catch (error) {
      showToast(`Failed to update sync folder: ${error}`, "error", 4200);
    }
  };

  const clearAccountAuth = async (profileId: string) => {
    try {
      await invoke("clear_account_auth", { profileId });
      showToast("Account auth session cleared.", "info", 2200);
      await refreshAll();
    } catch (error) {
      showToast(`Failed to clear auth: ${error}`, "error", 4200);
    }
  };

  const pauseAllAccounts = async () => {
    try {
      const pausedCount = await invoke<number>("pause_all_accounts");
      if (pausedCount > 0) {
        showToast(`Paused synchronization for ${pausedCount} account${pausedCount === 1 ? "" : "s"}.`, "info", 2600);
      } else {
        showToast("No syncing accounts to pause.", "info", 2200);
      }
      await refreshAll();
    } catch (error) {
      showToast(`Failed to pause all accounts: ${error}`, "error", 4200);
    }
  };

  const resumeAllAccounts = async () => {
    try {
      const resumedCount = await invoke<number>("resume_all_accounts");
      if (resumedCount > 0) {
        showToast(`Resumed synchronization for ${resumedCount} account${resumedCount === 1 ? "" : "s"}.`, "success", 2600);
      } else {
        showToast("No paused accounts to resume.", "info", 2200);
      }
      await refreshAll();
    } catch (error) {
      showToast(`Failed to resume all accounts: ${error}`, "error", 4200);
    }
  };

  const retryAccountSync = async (profileId: string) => {
    const input: SetAccountAgentStateInput = { id: profileId, agentState: "syncing" };
    try {
      await invoke("set_account_agent_state", { input });
      await refreshAll();
    } catch (error) {
      showToast(`Failed to retry sync: ${error}`, "error", 3200);
    }
  };

  return {
    createAccountProfile,
    renameAccountProfile,
    removeAccountProfile,
    setAccountAgentState,
    setAccountSyncRoot,
    startInteractiveAuth,
    clearAccountAuth,
    pauseAllAccounts,
    resumeAllAccounts,
    retryAccountSync,
  };
}

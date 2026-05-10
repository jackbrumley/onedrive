import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
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

  const retryFailedDownload = async (profileId: string, recentItemId: string, path: string) => {
    try {
      const response = await invoke<{ status: "retried" | "already_retrying" | "permission_denied" }>(
        "retry_failed_download",
        { profileId, recentItemId }
      );
      if (response.status === "retried") {
        showToast(`Retry queued for '${path}'.`, "info", 2600);
      } else if (response.status === "permission_denied") {
        showToast(`Skipped '${path}' because OneDrive returned permission denied.`, "info", 3200);
      } else {
        showToast(`'${path}' is already queued or no longer failed.`, "info", 2800);
      }
      await refreshAll();
    } catch (error) {
      showToast(`Failed to retry download: ${error}`, "error", 3200);
    }
  };

  const retryAllFailedDownloads = async (profileId: string) => {
    try {
      const response = await invoke<{
        retried: number;
        skippedPermissionDenied: number;
        alreadyRetrying: number;
      }>("retry_all_failed_downloads", { profileId });
      if (response.retried > 0) {
        const skippedPart = response.skippedPermissionDenied > 0
          ? ` (${response.skippedPermissionDenied} permission-denied skipped)`
          : "";
        showToast(
          `Retry queued for ${response.retried} failed download${response.retried === 1 ? "" : "s"}${skippedPart}.`,
          "info",
          3200
        );
      } else if (response.alreadyRetrying > 0) {
        showToast(
          `${response.alreadyRetrying} file${response.alreadyRetrying === 1 ? " is" : "s are"} already queued or no longer failed.`,
          "info",
          3000
        );
      } else if (response.skippedPermissionDenied > 0) {
        showToast(
          `No retryable failed downloads. ${response.skippedPermissionDenied} permission-denied item${response.skippedPermissionDenied === 1 ? " was" : "s were"} skipped.`,
          "info",
          3400
        );
      } else {
        showToast("No retryable failed downloads found.", "info", 2600);
      }
      await refreshAll();
    } catch (error) {
      showToast(`Failed to retry downloads: ${error}`, "error", 3200);
    }
  };

  const confirmAccountLargeDelete = async (profileId: string) => {
    try {
      await invoke("confirm_account_large_delete", { profileId });
      showToast("Large deletion confirmed. Cloud changes will apply on the next sync cycle.", "info", 3400);
      await refreshAll();
    } catch (error) {
      showToast(`Failed to confirm deletion: ${error}`, "error", 4200);
    }
  };

  const keepCloudFilesAfterLargeDelete = async (profileId: string) => {
    try {
      await invoke("keep_cloud_files_after_large_delete", { profileId });
      showToast("Keeping cloud files. Initial sync will restore missing items locally.", "info", 3600);
      await refreshAll();
    } catch (error) {
      showToast(`Failed to keep cloud files: ${error}`, "error", 4200);
    }
  };

  const fetchAccountLargeDeletePreview = async (profileId: string) => {
    try {
      return await invoke<string[]>("get_account_large_delete_preview", { profileId });
    } catch (error) {
      showToast(`Failed to load deletion preview: ${error}`, "error", 4200);
      return [];
    }
  };

  const exportAccountLargeDeletePreview = async (profileId: string) => {
    try {
      const defaultFileName = `somedrive-large-delete-review-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-")}.txt`;
      const destinationPath = await save({
        title: "Export large deletion review",
        defaultPath: defaultFileName,
      });
      if (typeof destinationPath !== "string" || !destinationPath.trim()) {
        return;
      }
      const writtenPath = await invoke<string>("export_account_large_delete_preview", {
        profileId,
        destinationPath,
      });
      showToast(`Deletion review exported to ${writtenPath}.`, "success", 3200);
    } catch (error) {
      showToast(`Failed to export deletion review: ${error}`, "error", 4200);
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
    retryFailedDownload,
    retryAllFailedDownloads,
    confirmAccountLargeDelete,
    keepCloudFilesAfterLargeDelete,
    fetchAccountLargeDeletePreview,
    exportAccountLargeDeletePreview,
  };
}

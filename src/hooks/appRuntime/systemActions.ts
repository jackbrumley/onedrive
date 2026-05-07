import { invoke } from "@tauri-apps/api/core";
import type { ToastType } from "../../types/somedrive";

interface SystemActionsFactoryParams {
  showToast: (message: string, type?: ToastType, durationMs?: number) => void;
}

export function createSystemActions({ showToast }: SystemActionsFactoryParams) {
  const fetchSessionLogText = async () => {
    try {
      return await invoke<string>("get_session_log_text");
    } catch (error) {
      showToast(`Failed to read session log: ${error}`, "error", 4200);
      return "";
    }
  };

  const copySessionLog = async () => {
    try {
      await invoke("copy_session_log_to_clipboard");
      showToast("Session log copied to clipboard.", "success", 2600);
    } catch (error) {
      showToast(`Failed to copy session log: ${error}`, "error", 4200);
    }
  };

  const openSessionLog = async () => {
    try {
      await invoke("open_session_log");
    } catch (error) {
      showToast(`Failed to open session log: ${error}`, "error", 4200);
    }
  };

  const openAccountItemFolder = async (profileId: string, relativePath: string) => {
    try {
      await invoke("open_account_item_folder", {
        input: {
          profileId,
          relativePath,
        },
      });
    } catch (error) {
      showToast(`Failed to open folder: ${error}`, "error", 3200);
    }
  };

  const openAccountSyncRootFolder = async (profileId: string) => {
    try {
      await invoke("open_account_sync_root_folder", {
        profileId,
      });
    } catch (error) {
      showToast(`Failed to open folder: ${error}`, "error", 3200);
    }
  };

  return {
    fetchSessionLogText,
    copySessionLog,
    openSessionLog,
    openAccountItemFolder,
    openAccountSyncRootFolder,
  };
}

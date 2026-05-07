import { invoke } from "@tauri-apps/api/core";
import {
  disable as disableAutostart,
  enable as enableAutostart,
  isEnabled as isAutostartEnabled,
} from "@tauri-apps/plugin-autostart";
import type { ToastType } from "../../types/somedrive";

interface SystemActionsFactoryParams {
  showToast: (message: string, type?: ToastType, durationMs?: number) => void;
}

export function createSystemActions({ showToast }: SystemActionsFactoryParams) {
  const getAutostartEnabled = async () => {
    try {
      return await isAutostartEnabled();
    } catch (error) {
      showToast(`Failed to read auto-start state: ${error}`, "error", 4200);
      return false;
    }
  };

  const setAutostartEnabled = async (enabled: boolean) => {
    try {
      if (enabled) {
        await enableAutostart();
      } else {
        await disableAutostart();
      }
      showToast(`Auto-start ${enabled ? "enabled" : "disabled"}.`, "success", 2600);
      return enabled;
    } catch (error) {
      showToast(`Failed to update auto-start: ${error}`, "error", 4200);
      return null;
    }
  };

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

  const openProfileLog = async (profileId: string) => {
    try {
      await invoke("open_profile_log", { profileId });
    } catch (error) {
      showToast(`Failed to open profile log: ${error}`, "error", 4200);
    }
  };

  const fetchRawLoggerMode = async () => {
    try {
      return await invoke<boolean>("get_raw_logger_mode");
    } catch (error) {
      showToast(`Failed to read raw logger mode: ${error}`, "error", 4200);
      return false;
    }
  };

  const setRawLoggerMode = async (enabled: boolean) => {
    try {
      await invoke("set_raw_logger_mode", { enabled });
      showToast(`Raw logger mode ${enabled ? "enabled" : "disabled"}.`, "success", 2600);
      return enabled;
    } catch (error) {
      showToast(`Failed to update raw logger mode: ${error}`, "error", 4200);
      return null;
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
    getAutostartEnabled,
    setAutostartEnabled,
    fetchSessionLogText,
    copySessionLog,
    openSessionLog,
    openProfileLog,
    fetchRawLoggerMode,
    setRawLoggerMode,
    openAccountItemFolder,
    openAccountSyncRootFolder,
  };
}

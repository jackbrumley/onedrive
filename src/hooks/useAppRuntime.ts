import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "preact/hooks";
import {
  hashFromRouteState,
  routeStateFromHash,
  type AccountDetailTab,
  type AppRouteState,
} from "../routes/appRoutes";
import type {
  AccountKind,
  AccountProfile,
  ActivityEvent,
  AppStatusSnapshot,
  CreateAccountProfileInput,
  RenameAccountProfileInput,
  SetAccountSyncRootInput,
  SetAccountAgentStateInput,
  SyncAgentState,
  ToastType,
  UpdateCheckResult,
} from "../types/onedrive";

interface UseAppRuntimeProps {
  showToast: (message: string, type?: ToastType, durationMs?: number) => void;
}

const initialStatus: AppStatusSnapshot = {
  appVersion: "0.0.0",
  platform: "unknown",
  syncEngineReady: false,
  authConfigured: false,
  activeAccount: null,
  lastSyncAt: null,
  health: "degraded",
  accounts: [],
};

export function useAppRuntime({ showToast }: UseAppRuntimeProps) {
  const [routeState, setRouteState] = useState<AppRouteState>(routeStateFromHash(window.location.hash));
  const [status, setStatus] = useState<AppStatusSnapshot>(initialStatus);
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [updateResult, setUpdateResult] = useState<UpdateCheckResult | null>(null);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [lastCheckedAt, setLastCheckedAt] = useState<number | null>(null);
  const [activityEvents, setActivityEvents] = useState<ActivityEvent[]>([]);

  const navigate = (nextState: AppRouteState) => {
    const nextHash = hashFromRouteState(nextState);
    if (window.location.hash === nextHash) {
      setRouteState(nextState);
      return;
    }
    window.location.hash = nextHash;
  };

  const goHome = () => {
    navigate({
      page: "accountsHome",
      accountId: null,
      accountTab: "overview",
    });
  };

  const openAccount = (accountId: string, tab: AccountDetailTab = "overview") => {
    navigate({
      page: "accountDetail",
      accountId,
      accountTab: tab,
    });
  };

  const goDebug = () => {
    navigate({
      page: "debug",
      accountId: null,
      accountTab: "overview",
    });
  };

  const goUiLab = () => {
    navigate({
      page: "uiLab",
      accountId: null,
      accountTab: "overview",
    });
  };

  const refreshStatus = async () => {
    try {
      const snapshot = await invoke<AppStatusSnapshot>("get_status_snapshot");
      setStatus(snapshot);
    } catch (error) {
      showToast(`Failed to load status: ${error}`, "error");
    }
  };

  const refreshActivity = async () => {
    try {
      const events = await invoke<ActivityEvent[]>("list_activity_events", { limit: 200 });
      setActivityEvents(events);
    } catch (error) {
      showToast(`Failed to load activity: ${error}`, "error");
    }
  };

  const checkForUpdates = async () => {
    setCheckingUpdates(true);
    setUpdateError(null);
    try {
      const result = await invoke<UpdateCheckResult>("check_for_updates");
      setUpdateResult(result);
      setLastCheckedAt(Date.now());
      if (result.updateAvailable) {
        showToast(`Update available: v${result.latestVersion}`, "info", 3400);
      } else {
        showToast("You are on the latest version.", "success", 2200);
      }
    } catch (error) {
      const message = String(error);
      setUpdateError(message);
      setLastCheckedAt(Date.now());
      showToast(`Update check failed: ${message}`, "error", 4200);
    } finally {
      setCheckingUpdates(false);
    }
  };

  const createAccountProfile = async (displayName: string, kind: AccountKind) => {
    const input: CreateAccountProfileInput = { displayName, kind };
    try {
      const profile = await invoke<AccountProfile>("create_account_profile", { input });
      showToast(`Account '${displayName}' added.`, "success", 2200);
      await Promise.all([refreshStatus(), refreshActivity()]);
      const authStarted = await startInteractiveAuth(profile.id);
      openAccount(profile.id, "settings");
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
      await Promise.all([refreshStatus(), refreshActivity()]);
    } catch (error) {
      showToast(`Failed to rename account: ${error}`, "error", 4200);
    }
  };

  const removeAccountProfile = async (id: string) => {
    try {
      await invoke("remove_account_profile", { input: { id } });
      showToast("Account removed. Synced files were not deleted.", "info", 3600);
      await Promise.all([refreshStatus(), refreshActivity()]);
    } catch (error) {
      showToast(`Failed to remove account: ${error}`, "error", 4200);
    }
  };

  const setAccountAgentState = async (id: string, agentState: SyncAgentState) => {
    const input: SetAccountAgentStateInput = { id, agentState };
    try {
      await invoke("set_account_agent_state", { input });
      showToast(`Agent state set to '${agentState}'.`, "success", 2200);
      await Promise.all([refreshStatus(), refreshActivity()]);
    } catch (error) {
      showToast(`Failed to update agent state: ${error}`, "error", 4200);
    }
  };

  const setAccountSyncRoot = async (id: string, syncRoot: string) => {
    const input: SetAccountSyncRootInput = { id, syncRoot };
    try {
      await invoke("set_account_sync_root", { input });
      showToast("Sync folder updated.", "success", 2200);
      await Promise.all([refreshStatus(), refreshActivity()]);
    } catch (error) {
      showToast(`Failed to update sync folder: ${error}`, "error", 4200);
    }
  };

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

  const clearAccountAuth = async (profileId: string) => {
    try {
      await invoke("clear_account_auth", { profileId });
      showToast("Account auth session cleared.", "info", 2200);
      await Promise.all([refreshStatus(), refreshActivity()]);
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
      await Promise.all([refreshStatus(), refreshActivity()]);
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
      await Promise.all([refreshStatus(), refreshActivity()]);
    } catch (error) {
      showToast(`Failed to resume all accounts: ${error}`, "error", 4200);
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

  useEffect(() => {
    const syncRoute = () => setRouteState(routeStateFromHash(window.location.hash));
    window.addEventListener("hashchange", syncRoute);
    refreshStatus();
    refreshActivity();
    return () => window.removeEventListener("hashchange", syncRoute);
  }, []);

  return {
    routeState,
    status,
    checkingUpdates,
    updateResult,
    updateError,
    lastCheckedAt,
    activityEvents,
    syncingCount: status.accounts.filter((account) => account.agentState === "syncing").length,
    pausedCount: status.accounts.filter((account) => account.agentState === "paused").length,
    navigate,
    goHome,
    openAccount,
    goDebug,
    goUiLab,
    refreshStatus,
    refreshActivity,
    checkForUpdates,
    createAccountProfile,
    renameAccountProfile,
    removeAccountProfile,
    setAccountAgentState,
    setAccountSyncRoot,
    startInteractiveAuth,
    clearAccountAuth,
    pauseAllAccounts,
    resumeAllAccounts,
    fetchSessionLogText,
    copySessionLog,
    openSessionLog,
  };
}

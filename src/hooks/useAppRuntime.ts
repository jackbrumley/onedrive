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
  ActivityEvent,
  AppStatusSnapshot,
  CreateAccountProfileInput,
  DeviceAuthPollResult,
  DeviceAuthSession,
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
  const [authSessions, setAuthSessions] = useState<Record<string, DeviceAuthSession>>({});
  const [authPending, setAuthPending] = useState<Record<string, boolean>>({});

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
      await invoke("create_account_profile", { input });
      showToast(`Account '${displayName}' added.`, "success", 2500);
      await Promise.all([refreshStatus(), refreshActivity()]);
    } catch (error) {
      showToast(`Failed to add account: ${error}`, "error", 4200);
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

  const startDeviceAuth = async (profileId: string) => {
    try {
      const session = await invoke<DeviceAuthSession>("start_device_auth", { profileId });
      setAuthSessions((current) => ({ ...current, [profileId]: session }));
      showToast("Device sign-in started. Complete approval in your browser.", "info", 3400);
      return session;
    } catch (error) {
      showToast(`Failed to start sign-in: ${error}`, "error", 4200);
      return null;
    }
  };

  const pollDeviceAuth = async (profileId: string) => {
    setAuthPending((current) => ({ ...current, [profileId]: true }));
    try {
      const result = await invoke<DeviceAuthPollResult>("poll_device_auth", { profileId });
      if (result.status === "authorized") {
        showToast("Account authentication complete.", "success", 2600);
        setAuthSessions((current) => {
          const next = { ...current };
          delete next[profileId];
          return next;
        });
        await Promise.all([refreshStatus(), refreshActivity()]);
      } else if (result.status === "pending") {
        showToast("Still waiting for approval.", "info", 1800);
      } else {
        showToast(result.detail, "error", 4200);
      }
      return result;
    } catch (error) {
      showToast(`Failed to poll sign-in: ${error}`, "error", 4200);
      return null;
    } finally {
      setAuthPending((current) => ({ ...current, [profileId]: false }));
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
    const text = await fetchSessionLogText();
    if (!text) {
      showToast("Session log is empty.", "info", 2200);
      return;
    }

    try {
      await navigator.clipboard.writeText(text);
      showToast("Session log copied to clipboard.", "success", 2600);
    } catch {
      const fallback = document.createElement("textarea");
      fallback.value = text;
      fallback.setAttribute("readonly", "true");
      fallback.style.position = "fixed";
      fallback.style.opacity = "0";
      document.body.appendChild(fallback);
      fallback.select();
      try {
        document.execCommand("copy");
        showToast("Session log copied to clipboard.", "success", 2600);
      } catch {
        showToast("Failed to copy session log.", "error", 3000);
      } finally {
        document.body.removeChild(fallback);
      }
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
    authSessions,
    authPending,
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
    startDeviceAuth,
    pollDeviceAuth,
    clearAccountAuth,
    pauseAllAccounts,
    resumeAllAccounts,
    fetchSessionLogText,
    copySessionLog,
  };
}

import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "preact/hooks";
import { routeStateFromHash, type AppRouteState } from "../routes/appRoutes";
import type {
  ActivityEvent,
  AppStatusSnapshot,
  SyncRuntimeSnapshot,
  ToastType,
  UpdateCheckResult,
} from "../types/somedrive";
import { createAccountActions } from "./appRuntime/accountActions";
import { createNavigationActions } from "./appRuntime/navigation";
import { createRefreshActions } from "./appRuntime/refresh";
import { createSystemActions } from "./appRuntime/systemActions";

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

const initialSyncRuntime: SyncRuntimeSnapshot = {
  generatedAt: new Date(0).toISOString(),
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
  const [syncRuntime, setSyncRuntime] = useState<SyncRuntimeSnapshot>(initialSyncRuntime);

  const navigation = createNavigationActions({ setRouteState });
  const refreshActions = createRefreshActions({
    showToast,
    setStatus,
    setActivityEvents,
    setSyncRuntime,
    setCheckingUpdates,
    setUpdateResult,
    setUpdateError,
    setLastCheckedAt,
  });
  const accountActions = createAccountActions({
    showToast,
    refreshStatus: refreshActions.refreshStatus,
    refreshActivity: refreshActions.refreshActivity,
    refreshSyncRuntime: refreshActions.refreshSyncRuntime,
    openAccount: navigation.openAccount,
  });
  const systemActions = createSystemActions({ showToast });

  useEffect(() => {
    const syncRoute = () => setRouteState(routeStateFromHash(window.location.hash));
    let isDisposed = false;
    const unlistenAuthUpdatePromise = listen("account-auth-updated", () => {
      if (!isDisposed) {
        void Promise.all([
          refreshActions.refreshStatus(),
          refreshActions.refreshActivity(),
          refreshActions.refreshSyncRuntime(),
        ]);
      }
    });

    window.addEventListener("hashchange", syncRoute);
    void refreshActions.refreshStatus();
    void refreshActions.refreshActivity();
    void refreshActions.refreshSyncRuntime();
    const runtimeInterval = window.setInterval(() => {
      void refreshActions.refreshSyncRuntime();
    }, 1500);
    return () => {
      isDisposed = true;
      window.removeEventListener("hashchange", syncRoute);
      window.clearInterval(runtimeInterval);
      unlistenAuthUpdatePromise
        .then((unlisten) => unlisten())
        .catch(() => {
          // no-op
        });
    };
  }, []);

  return {
    routeState,
    status,
    checkingUpdates,
    updateResult,
    updateError,
    lastCheckedAt,
    activityEvents,
    syncRuntime,
    syncingCount: status.accounts.filter((account) => account.agentState === "syncing").length,
    pausedCount: status.accounts.filter((account) => account.agentState === "paused").length,
    navigate: navigation.navigate,
    goHome: navigation.goHome,
    openAccount: navigation.openAccount,
    goDebug: navigation.goDebug,
    goUiLab: navigation.goUiLab,
    refreshStatus: refreshActions.refreshStatus,
    refreshActivity: refreshActions.refreshActivity,
    refreshSyncRuntime: refreshActions.refreshSyncRuntime,
    checkForUpdates: refreshActions.checkForUpdates,
    createAccountProfile: accountActions.createAccountProfile,
    renameAccountProfile: accountActions.renameAccountProfile,
    removeAccountProfile: accountActions.removeAccountProfile,
    setAccountAgentState: accountActions.setAccountAgentState,
    setAccountSyncRoot: accountActions.setAccountSyncRoot,
    startInteractiveAuth: accountActions.startInteractiveAuth,
    clearAccountAuth: accountActions.clearAccountAuth,
    pauseAllAccounts: accountActions.pauseAllAccounts,
    resumeAllAccounts: accountActions.resumeAllAccounts,
    retryAccountSync: accountActions.retryAccountSync,
    fetchSessionLogText: systemActions.fetchSessionLogText,
    copySessionLog: systemActions.copySessionLog,
    openSessionLog: systemActions.openSessionLog,
    openAccountSyncRootFolder: systemActions.openAccountSyncRootFolder,
    openAccountItemFolder: systemActions.openAccountItemFolder,
  };
}

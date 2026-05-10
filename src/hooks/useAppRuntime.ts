import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useState } from "preact/hooks";
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
  revision: 0,
  accounts: [],
};

function shouldPollSyncRuntime(routeState: AppRouteState, isDocumentVisible: boolean): boolean {
  if (!isDocumentVisible) {
    return false;
  }
  if (routeState.page === "accountsHome") {
    return true;
  }
  return routeState.page === "accountDetail" && (routeState.accountView ?? "sync") === "sync";
}

export function useAppRuntime({ showToast }: UseAppRuntimeProps) {
  const [routeState, setRouteState] = useState<AppRouteState>(routeStateFromHash(window.location.hash));
  const [status, setStatus] = useState<AppStatusSnapshot>(initialStatus);
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [updateResult, setUpdateResult] = useState<UpdateCheckResult | null>(null);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [lastCheckedAt, setLastCheckedAt] = useState<number | null>(null);
  const [activityEvents, setActivityEvents] = useState<ActivityEvent[]>([]);
  const [syncRuntime, setSyncRuntime] = useState<SyncRuntimeSnapshot>(initialSyncRuntime);
  const [isDocumentVisible, setIsDocumentVisible] = useState(document.visibilityState === "visible");
  const [autostartEnabled, setAutostartEnabled] = useState(false);
  const [rawLoggerMode, setRawLoggerMode] = useState(false);
  const [syncDownloadConcurrency, setSyncDownloadConcurrency] = useState(12);

  const navigation = useMemo(() => createNavigationActions({ setRouteState }), [setRouteState]);
  const refreshActions = useMemo(
    () =>
      createRefreshActions({
        showToast,
        setStatus,
        setActivityEvents,
        setSyncRuntime,
        setCheckingUpdates,
        setUpdateResult,
        setUpdateError,
        setLastCheckedAt,
      }),
    [showToast]
  );
  const accountActions = useMemo(
    () =>
      createAccountActions({
        showToast,
        refreshStatus: refreshActions.refreshStatus,
        refreshActivity: refreshActions.refreshActivity,
        refreshSyncRuntime: refreshActions.refreshSyncRuntime,
        openAccount: navigation.openAccount,
      }),
    [navigation.openAccount, refreshActions.refreshActivity, refreshActions.refreshStatus, refreshActions.refreshSyncRuntime, showToast]
  );
  const systemActions = useMemo(() => createSystemActions({ showToast }), [showToast]);

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
    void systemActions.getAutostartEnabled().then(setAutostartEnabled);
    void systemActions.fetchRawLoggerMode().then(setRawLoggerMode);
    void systemActions.fetchSyncDownloadConcurrency().then(setSyncDownloadConcurrency);
    return () => {
      isDisposed = true;
      window.removeEventListener("hashchange", syncRoute);
      unlistenAuthUpdatePromise
        .then((unlisten) => unlisten())
        .catch(() => {
          // no-op
        });
    };
  }, []);

  useEffect(() => {
    const handleVisibilityChange = () => {
      setIsDocumentVisible(document.visibilityState === "visible");
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, []);

  useEffect(() => {
    const pollEnabled = shouldPollSyncRuntime(routeState, isDocumentVisible);
    if (!pollEnabled) {
      return;
    }
    void refreshActions.refreshSyncRuntime();
    const runtimeInterval = window.setInterval(() => {
      void refreshActions.refreshSyncRuntime();
    }, 1500);
    return () => {
      window.clearInterval(runtimeInterval);
    };
  }, [isDocumentVisible, routeState.accountView, routeState.page]);

  const toggleAutostart = async (enabled: boolean) => {
    const updated = await systemActions.setAutostartEnabled(enabled);
    if (typeof updated === "boolean") {
      setAutostartEnabled(updated);
    }
  };

  const toggleRawLoggerMode = async (enabled: boolean) => {
    const updated = await systemActions.setRawLoggerMode(enabled);
    if (typeof updated === "boolean") {
      setRawLoggerMode(updated);
    }
  };

  const updateSyncDownloadConcurrency = async (value: number) => {
    const updated = await systemActions.setSyncDownloadConcurrency(value);
    if (typeof updated === "number") {
      setSyncDownloadConcurrency(updated);
    }
  };

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
    openAccountSettings: navigation.openAccountSettings,
    goSettings: navigation.goSettings,
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
    retryFailedDownload: accountActions.retryFailedDownload,
    retryAllFailedDownloads: accountActions.retryAllFailedDownloads,
    confirmAccountLargeDelete: accountActions.confirmAccountLargeDelete,
    keepCloudFilesAfterLargeDelete: accountActions.keepCloudFilesAfterLargeDelete,
    fetchAccountLargeDeletePreview: accountActions.fetchAccountLargeDeletePreview,
    exportAccountLargeDeletePreview: accountActions.exportAccountLargeDeletePreview,
    autostartEnabled,
    toggleAutostart,
    rawLoggerMode,
    toggleRawLoggerMode,
    syncDownloadConcurrency,
    updateSyncDownloadConcurrency,
    fetchSessionLogText: systemActions.fetchSessionLogText,
    copySessionLog: systemActions.copySessionLog,
    openSessionLog: systemActions.openSessionLog,
    openProfileLog: systemActions.openProfileLog,
    openAccountSyncRootFolder: systemActions.openAccountSyncRootFolder,
    openAccountItemFolder: systemActions.openAccountItemFolder,
  };
}

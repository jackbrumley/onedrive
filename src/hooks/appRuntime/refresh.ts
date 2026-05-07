import { invoke } from "@tauri-apps/api/core";
import type {
  ActivityEvent,
  AppStatusSnapshot,
  SyncRuntimeSnapshot,
  ToastType,
  UpdateCheckResult,
} from "../../types/somedrive";

interface RefreshFactoryParams {
  showToast: (message: string, type?: ToastType, durationMs?: number) => void;
  setStatus: (value: AppStatusSnapshot) => void;
  setActivityEvents: (value: ActivityEvent[]) => void;
  setSyncRuntime: (value: SyncRuntimeSnapshot | ((current: SyncRuntimeSnapshot) => SyncRuntimeSnapshot)) => void;
  setCheckingUpdates: (value: boolean) => void;
  setUpdateResult: (value: UpdateCheckResult | null) => void;
  setUpdateError: (value: string | null) => void;
  setLastCheckedAt: (value: number | null) => void;
}

export function createRefreshActions({
  showToast,
  setStatus,
  setActivityEvents,
  setSyncRuntime,
  setCheckingUpdates,
  setUpdateResult,
  setUpdateError,
  setLastCheckedAt,
}: RefreshFactoryParams) {
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

  const refreshSyncRuntime = async () => {
    try {
      const snapshot = await invoke<SyncRuntimeSnapshot>("get_sync_runtime_snapshot");
      setSyncRuntime((current) => (current.revision === snapshot.revision ? current : snapshot));
    } catch {
      // runtime telemetry is best-effort for now
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

  return {
    refreshStatus,
    refreshActivity,
    refreshSyncRuntime,
    checkForUpdates,
  };
}

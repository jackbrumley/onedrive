import { invoke } from "@tauri-apps/api/core";
import { AppShell } from "./components/shell/AppShell";
import { ToastHost } from "./components/toast/ToastHost";
import { useAppRuntime } from "./hooks/useAppRuntime";
import { useToastManager } from "./hooks/useToastManager";
import { AppWorkspace } from "./layout/AppWorkspace";

function App() {
  const { toast, showToast, dismissToast, pauseToast, resumeToast } = useToastManager();
  const runtime = useAppRuntime({ showToast });

  const handleGlobalClickCapture = (event: MouseEvent) => {
    const target = event.target as HTMLElement | null;
    if (!target) {
      return;
    }

    const button = target.closest("button");
    if (!button || button.dataset.noLog === "true") {
      return;
    }

    const rawLabel = button.dataset.logAction ?? button.textContent ?? "button";
    const label = rawLabel.replace(/\s+/g, " ").trim() || "button";
    const accountContext = runtime.routeState.accountId ? ` account=${runtime.routeState.accountId}` : "";
    const message = `click page=${runtime.routeState.page}${accountContext} label="${label}"`;

    void invoke("log_ui_event", { message }).catch(() => {
      // no-op
    });
  };

  return (
    <div onClickCapture={handleGlobalClickCapture}>
      <AppShell
        page={runtime.routeState.page}
        onGoHome={runtime.goHome}
        onGoDebug={runtime.goDebug}
        syncingCount={runtime.syncingCount}
        pausedCount={runtime.pausedCount}
        onPauseAll={runtime.pauseAllAccounts}
        onResumeAll={runtime.resumeAllAccounts}
      >
        <AppWorkspace runtime={runtime} />
        <ToastHost toast={toast} onDismiss={dismissToast} onPause={pauseToast} onResume={resumeToast} />
      </AppShell>
    </div>
  );
}

export default App;

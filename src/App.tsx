import { AppShell } from "./components/shell/AppShell";
import { ToastHost } from "./components/toast/ToastHost";
import { useAppRuntime } from "./hooks/useAppRuntime";
import { useToastManager } from "./hooks/useToastManager";
import { useUiInteractionLogger } from "./hooks/useUiInteractionLogger";
import { AppWorkspace } from "./layout/AppWorkspace";

function App() {
  const { toast, showToast, dismissToast, pauseToast, resumeToast } = useToastManager();
  const runtime = useAppRuntime({ showToast });
  const handleGlobalClickCapture = useUiInteractionLogger(runtime.routeState);

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

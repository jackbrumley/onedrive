import { AppShell } from "./components/shell/AppShell";
import { ToastHost } from "./components/toast/ToastHost";
import { useAppRuntime } from "./hooks/useAppRuntime";
import { useToastManager } from "./hooks/useToastManager";
import { AppWorkspace } from "./layout/AppWorkspace";

function App() {
  const { toast, showToast, dismissToast, pauseToast, resumeToast } = useToastManager();
  const runtime = useAppRuntime({ showToast });

  return (
    <AppShell page={runtime.routeState.page} onGoHome={runtime.goHome} onGoDebug={runtime.goDebug}>
      <AppWorkspace runtime={runtime} />
      <ToastHost toast={toast} onDismiss={dismissToast} onPause={pauseToast} onResume={resumeToast} />
    </AppShell>
  );
}

export default App;

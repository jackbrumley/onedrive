import type { ComponentChildren } from "preact";
import { IconSettings } from "@tabler/icons-preact";
import type { AppPage } from "../../routes/appRoutes";
import { useWindowControls } from "../../hooks/useWindowControls";
import { SyncStateControl } from "../sync/SyncStateControl";
import { WindowControls } from "./WindowControls";

interface AppShellProps {
  page: AppPage;
  onGoHome: () => void;
  onGoSettings: () => void;
  syncingCount: number;
  pausedCount: number;
  onPauseAll: () => Promise<void>;
  onResumeAll: () => Promise<void>;
  children: ComponentChildren;
}

export function AppShell({
  page,
  onGoHome,
  onGoSettings,
  syncingCount,
  pausedCount,
  onPauseAll,
  onResumeAll,
  children,
}: AppShellProps) {
  const {
    isMaximized,
    minimize,
    toggleMaximize,
    close,
    handleTitleBarMouseDown,
    handleTitleBarDoubleClick,
  } = useWindowControls();

  const isSettingsArea = page === "settings" || page === "debug" || page === "uiLab";
  const hasSyncAccounts = syncingCount > 0 || pausedCount > 0;

  return (
    <div class="app-shell">
      <header class="title-bar" onMouseDown={handleTitleBarMouseDown} onDblClick={handleTitleBarDoubleClick}>
        <div class="title-block">
          <h1>SomeDrive</h1>
        </div>
        <div class="title-right-actions">
          <button
            class={isSettingsArea ? "window-control settings-nav-btn active" : "window-control settings-nav-btn"}
            onClick={isSettingsArea ? onGoHome : onGoSettings}
            aria-label={isSettingsArea ? "Back to accounts" : "Open settings"}
            title={isSettingsArea ? "Back to accounts" : "Open settings"}
          >
            <IconSettings size={14} stroke={2.2} />
          </button>
          <SyncStateControl
            state={hasSyncAccounts ? (syncingCount > 0 ? "syncing" : "paused") : "inactive"}
            onToggle={async (next) => {
              if (!hasSyncAccounts) {
                return;
              }
              if (next === "paused") {
                await onPauseAll();
              } else {
                await onResumeAll();
              }
            }}
            disabled={!hasSyncAccounts}
            size={15}
          />
          <WindowControls
            isMaximized={isMaximized}
            onMinimize={minimize}
            onToggleMaximize={toggleMaximize}
            onClose={close}
          />
        </div>
      </header>

      <main class="workspace">{children}</main>
    </div>
  );
}

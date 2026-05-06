import type { ComponentChildren } from "preact";
import type { AppPage } from "../../routes/appRoutes";
import { useWindowControls } from "../../hooks/useWindowControls";
import { SyncStateControl } from "../sync/SyncStateControl";
import { WindowControls } from "./WindowControls";

interface AppShellProps {
  page: AppPage;
  onGoHome: () => void;
  onGoDebug: () => void;
  syncingCount: number;
  pausedCount: number;
  onPauseAll: () => Promise<void>;
  onResumeAll: () => Promise<void>;
  children: ComponentChildren;
}

export function AppShell({
  page,
  onGoHome,
  onGoDebug,
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

  const isHome = page === "accountsHome";
  const isDebug = page === "debug" || page === "uiLab";
  const hasSyncAccounts = syncingCount > 0 || pausedCount > 0;

  return (
    <div class="app-shell">
      <header class="title-bar" onMouseDown={handleTitleBarMouseDown} onDblClick={handleTitleBarDoubleClick}>
        <div class="title-block">
          <h1>SomeDrive</h1>
        </div>
        <div class="shell-actions">
          <button class={isHome ? "top-pill active" : "top-pill"} onClick={onGoHome}>
            Accounts
          </button>
          <button class={isDebug ? "top-pill active" : "top-pill"} onClick={onGoDebug}>
            Settings
          </button>
        </div>
        <div class="title-right-actions">
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

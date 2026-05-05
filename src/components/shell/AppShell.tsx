import type { ComponentChildren } from "preact";
import type { AppPage } from "../../routes/appRoutes";
import { useWindowControls } from "../../hooks/useWindowControls";
import { WindowControls } from "./WindowControls";

interface AppShellProps {
  page: AppPage;
  onGoHome: () => void;
  onGoDebug: () => void;
  children: ComponentChildren;
}

export function AppShell({ page, onGoHome, onGoDebug, children }: AppShellProps) {
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

  return (
    <div class="app-shell">
      <header class="title-bar" onMouseDown={handleTitleBarMouseDown} onDblClick={handleTitleBarDoubleClick}>
        <div class="title-block">
          <h1>OneDrive</h1>
        </div>
        <div class="shell-actions">
          <button class={isHome ? "top-pill active" : "top-pill"} onClick={onGoHome}>
            Accounts
          </button>
          <button class={isDebug ? "top-pill active" : "top-pill"} onClick={onGoDebug}>
            Debug
          </button>
        </div>
        <WindowControls
          isMaximized={isMaximized}
          onMinimize={minimize}
          onToggleMaximize={toggleMaximize}
          onClose={close}
        />
      </header>

      <main class="workspace">{children}</main>
    </div>
  );
}

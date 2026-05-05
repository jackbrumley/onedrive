import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "preact/hooks";

export function useWindowControls() {
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    const appWindow = getCurrentWindow();

    const syncMaximizeState = async () => {
      try {
        setIsMaximized(await appWindow.isMaximized());
      } catch {
        // no-op
      }
    };

    syncMaximizeState();
    const unlistenPromise = appWindow.onResized(() => {
      syncMaximizeState();
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten()).catch(() => {
        // no-op
      });
    };
  }, []);

  const minimize = async () => {
    try {
      await getCurrentWindow().minimize();
    } catch (error) {
      console.error("Failed to minimize window:", error);
    }
  };

  const toggleMaximize = async () => {
    try {
      const appWindow = getCurrentWindow();
      if (await appWindow.isMaximized()) {
        await appWindow.unmaximize();
      } else {
        await appWindow.maximize();
      }
      setIsMaximized(await appWindow.isMaximized());
    } catch (error) {
      console.error("Failed to toggle maximize:", error);
    }
  };

  const close = async () => {
    try {
      await getCurrentWindow().close();
    } catch (error) {
      console.error("Failed to close window:", error);
    }
  };

  const handleTitleBarMouseDown = async (event: MouseEvent) => {
    if (event.detail > 1) {
      event.preventDefault();
      return;
    }

    if (event.button === 0 && !(event.target as HTMLElement).closest("button, input, select, a")) {
      try {
        await getCurrentWindow().startDragging();
      } catch (error) {
        console.error("Failed to start dragging:", error);
      }
    }
  };

  const handleTitleBarDoubleClick = async (event: MouseEvent) => {
    if (!(event.target as HTMLElement).closest("button, input, select, a")) {
      event.preventDefault();
      await toggleMaximize();
    }
  };

  return {
    isMaximized,
    minimize,
    toggleMaximize,
    close,
    handleTitleBarMouseDown,
    handleTitleBarDoubleClick,
  };
}

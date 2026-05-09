import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "preact/hooks";

type ResizeDirection = "East" | "North" | "NorthEast" | "NorthWest" | "South" | "SouthEast" | "SouthWest" | "West";

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

    if (event.button === 0 && !(event.target as HTMLElement).closest("button, input, select, a, [data-no-drag='true']")) {
      try {
        await getCurrentWindow().startDragging();
      } catch (error) {
        console.error("Failed to start dragging:", error);
      }
    }
  };

  const handleTitleBarDoubleClick = async (event: MouseEvent) => {
    if (!(event.target as HTMLElement).closest("button, input, select, a, [data-no-drag='true']")) {
      event.preventDefault();
      await toggleMaximize();
    }
  };

  const handleResizeMouseDown = (direction: ResizeDirection) => async (event: MouseEvent) => {
    if (event.buttons !== 1) {
      return;
    }

    if (isMaximized) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();

    try {
      await getCurrentWindow().startResizeDragging(direction);
    } catch {
      // no-op when resize dragging is unavailable
    }
  };

  return {
    isMaximized,
    minimize,
    toggleMaximize,
    close,
    handleTitleBarMouseDown,
    handleTitleBarDoubleClick,
    handleResizeMouseDown,
  };
}

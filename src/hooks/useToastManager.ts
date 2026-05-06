import { useEffect, useRef, useState } from "preact/hooks";
import type { ToastMessage, ToastType } from "../types/somedrive";

export function useToastManager() {
  const [toast, setToast] = useState<ToastMessage | null>(null);
  const timeoutRef = useRef<number | null>(null);
  const remainingRef = useRef(0);
  const startedAtRef = useRef(0);

  const clearTimer = () => {
    if (timeoutRef.current !== null) {
      window.clearTimeout(timeoutRef.current);
      timeoutRef.current = null;
    }
  };

  const dismissToast = () => {
    clearTimer();
    setToast(null);
  };

  const scheduleDismiss = (durationMs: number) => {
    clearTimer();
    startedAtRef.current = Date.now();
    remainingRef.current = durationMs;
    timeoutRef.current = window.setTimeout(() => {
      setToast(null);
      timeoutRef.current = null;
    }, durationMs);
  };

  const showToast = (message: string, type: ToastType = "info", durationMs = 2800) => {
    setToast({
      id: Date.now(),
      message,
      type,
      durationMs,
    });
    scheduleDismiss(durationMs);
  };

  const pauseToast = () => {
    if (!toast || timeoutRef.current === null) {
      return;
    }
    const elapsed = Date.now() - startedAtRef.current;
    remainingRef.current = Math.max(0, remainingRef.current - elapsed);
    clearTimer();
  };

  const resumeToast = () => {
    if (!toast || timeoutRef.current !== null) {
      return;
    }
    scheduleDismiss(Math.max(250, remainingRef.current));
  };

  useEffect(() => () => clearTimer(), []);

  return {
    toast,
    showToast,
    dismissToast,
    pauseToast,
    resumeToast,
  };
}

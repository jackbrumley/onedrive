import { invoke } from "@tauri-apps/api/core";
import type { AppRouteState } from "../routes/appRoutes";

export function useUiInteractionLogger(routeState: AppRouteState) {
  return (event: MouseEvent) => {
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
    const accountContext = routeState.accountId ? ` account=${routeState.accountId}` : "";
    const message = `click page=${routeState.page}${accountContext} label="${label}"`;

    void invoke("log_ui_event", { message }).catch(() => {
      // no-op
    });
  };
}

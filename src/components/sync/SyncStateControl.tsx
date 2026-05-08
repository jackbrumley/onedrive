import { useMemo } from "preact/hooks";
import { IconPlayerPauseFilled, IconPlayerPlayFilled } from "@tabler/icons-preact";

interface SyncStateControlProps {
  state: "syncing" | "paused" | "inactive";
  onToggle: (next: "syncing" | "paused") => Promise<void>;
  disabled?: boolean;
  size?: number;
}

export function SyncStateControl({ state, onToggle, disabled = false, size = 16 }: SyncStateControlProps) {
  const nextState: "syncing" | "paused" = state === "syncing" ? "paused" : "syncing";

  const title = useMemo(() => {
    if (state === "inactive") {
      return "No accounts to sync";
    }
    if (disabled) {
      return "Synchronization unavailable";
    }
    if (state === "syncing") {
      return "Pause synchronization";
    }
    return "Resume synchronization";
  }, [disabled, state]);

  return (
    <button
      class="sync-state-btn"
      disabled={disabled || state === "inactive"}
      title={title}
      aria-label={title}
      onClick={() => {
        if (state === "inactive") {
          return;
        }
        onToggle(nextState);
      }}
    >
      {state === "inactive" ? (
        <IconPlayerPauseFilled size={size} />
      ) : state === "syncing" ? (
        <IconPlayerPauseFilled size={size} />
      ) : (
        <IconPlayerPlayFilled size={size} />
      )}
    </button>
  );
}

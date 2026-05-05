import { useMemo, useState } from "preact/hooks";
import { IconPlayerPauseFilled, IconPlayerPlayFilled, IconRefresh } from "@tabler/icons-preact";

interface SyncStateControlProps {
  state: "syncing" | "paused";
  onToggle: (next: "syncing" | "paused") => Promise<void>;
  disabled?: boolean;
  size?: number;
}

export function SyncStateControl({ state, onToggle, disabled = false, size = 16 }: SyncStateControlProps) {
  const [hovered, setHovered] = useState(false);

  const nextState = state === "syncing" ? "paused" : "syncing";

  const title = useMemo(() => {
    if (disabled) {
      return "Synchronization unavailable";
    }
    if (state === "syncing") {
      return hovered ? "Pause synchronization" : "Synchronizing";
    }
    return hovered ? "Resume synchronization" : "Synchronization paused";
  }, [disabled, hovered, state]);

  return (
    <button
      class="sync-state-btn"
      disabled={disabled}
      title={title}
      aria-label={title}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onClick={() => onToggle(nextState)}
    >
      {state === "syncing" ? (
        hovered ? (
          <IconPlayerPauseFilled size={size} />
        ) : (
          <IconRefresh class="sync-icon-spinning" size={size} />
        )
      ) : hovered ? (
        <IconPlayerPlayFilled size={size} />
      ) : (
        <IconPlayerPauseFilled size={size} />
      )}
    </button>
  );
}

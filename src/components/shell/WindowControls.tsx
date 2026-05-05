import { IconCopy, IconMinus, IconSquare, IconX } from "@tabler/icons-preact";

interface WindowControlsProps {
  isMaximized: boolean;
  onMinimize: () => Promise<void>;
  onToggleMaximize: () => Promise<void>;
  onClose: () => Promise<void>;
}

export function WindowControls({
  isMaximized,
  onMinimize,
  onToggleMaximize,
  onClose,
}: WindowControlsProps) {
  return (
    <div class="window-controls" data-no-drag="true">
      <button class="window-control" onClick={onMinimize} aria-label="Minimize window">
        <IconMinus size={14} stroke={2.3} />
      </button>
      <button class="window-control" onClick={onToggleMaximize} aria-label="Maximize or restore window">
        {isMaximized ? <IconCopy size={13} stroke={2.3} /> : <IconSquare size={13} stroke={2.3} />}
      </button>
      <button class="window-control close" onClick={onClose} aria-label="Close window">
        <IconX size={14} stroke={2.3} />
      </button>
    </div>
  );
}

import type { ToastMessage } from "../../types/somedrive";

interface ToastHostProps {
  toast: ToastMessage | null;
  onDismiss: () => void;
  onPause: () => void;
  onResume: () => void;
}

export function ToastHost({ toast, onDismiss, onPause, onResume }: ToastHostProps) {
  return (
    <div class="toast-host">
      {toast && (
        <div class={`toast toast-${toast.type}`} onMouseEnter={onPause} onMouseLeave={onResume}>
          <p>{toast.message}</p>
          <button onClick={onDismiss} aria-label="Dismiss notification">
            x
          </button>
        </div>
      )}
    </div>
  );
}

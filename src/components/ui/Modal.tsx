import type { ComponentChildren } from "preact";

interface ModalProps {
  title: string;
  onClose: () => void;
  children: ComponentChildren;
}

export function Modal({ title, onClose, children }: ModalProps) {
  return (
    <div class="modal-overlay" onClick={onClose}>
      <section class="modal-shell" onClick={(event) => event.stopPropagation()}>
        <header class="modal-header">
          <h3>{title}</h3>
          <button class="modal-close" onClick={onClose} aria-label="Close modal">
            x
          </button>
        </header>
        <div class="modal-body">{children}</div>
      </section>
    </div>
  );
}

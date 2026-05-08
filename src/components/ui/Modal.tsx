import type { ComponentChildren } from "preact";
import { IconX } from "@tabler/icons-preact";
import { useEffect } from "preact/hooks";

interface ModalProps {
  title: string;
  onClose: () => void;
  children: ComponentChildren;
}

export function Modal({ title, onClose, children }: ModalProps) {
  useEffect(() => {
    const { body } = document;
    const currentCount = Number.parseInt(body.dataset.modalOpenCount ?? "0", 10);
    const nextCount = Number.isNaN(currentCount) ? 1 : currentCount + 1;

    body.dataset.modalOpenCount = String(nextCount);
    body.classList.add("app-modal-open");

    return () => {
      const previousCount = Number.parseInt(body.dataset.modalOpenCount ?? "1", 10);
      const updatedCount = Number.isNaN(previousCount) ? 0 : Math.max(0, previousCount - 1);

      if (updatedCount === 0) {
        delete body.dataset.modalOpenCount;
        body.classList.remove("app-modal-open");
        return;
      }

      body.dataset.modalOpenCount = String(updatedCount);
    };
  }, []);

  return (
    <div class="modal-overlay" onClick={onClose}>
      <section class="modal-shell" onClick={(event) => event.stopPropagation()}>
        <header class="modal-header">
          <h3>{title}</h3>
          <button class="modal-close" onClick={onClose} aria-label="Close modal">
            <IconX size={16} stroke={2.2} />
          </button>
        </header>
        <div class="modal-body">{children}</div>
      </section>
    </div>
  );
}

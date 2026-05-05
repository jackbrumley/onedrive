import { useState } from "preact/hooks";
import type { AccountKind } from "../../types/onedrive";
import { Modal } from "../ui/Modal";

interface AddAccountModalProps {
  onClose: () => void;
  onCreateAccount: (displayName: string, kind: AccountKind) => Promise<void>;
}

export function AddAccountModal({ onClose, onCreateAccount }: AddAccountModalProps) {
  const [name, setName] = useState("");
  const [kind, setKind] = useState<AccountKind>("personal");
  const [saving, setSaving] = useState(false);

  const submit = async (event: Event) => {
    event.preventDefault();
    const trimmed = name.trim();
    if (!trimmed || saving) {
      return;
    }
    setSaving(true);
    await onCreateAccount(trimmed, kind);
    setSaving(false);
    onClose();
  };

  return (
    <Modal title="Add Account" onClose={onClose}>
      <form class="account-form" onSubmit={submit}>
        <label class="field-label" for="new-account-name">
          Account Name
        </label>
        <input
          id="new-account-name"
          value={name}
          onInput={(event) => setName(event.currentTarget.value)}
          placeholder="Personal, Work, Family"
          autoFocus
        />

        <label class="field-label" for="new-account-kind">
          Account Type
        </label>
        <select id="new-account-kind" value={kind} onChange={(event) => setKind(event.currentTarget.value as AccountKind)}>
          <option value="personal">Personal</option>
          <option value="business">Business</option>
        </select>

        <div class="button-row">
          <button type="submit" disabled={!name.trim() || saving}>
            {saving ? "Adding..." : "Create Account"}
          </button>
          <button type="button" class="ghost" onClick={onClose}>
            Cancel
          </button>
        </div>
      </form>
    </Modal>
  );
}

import { IconPlus } from "@tabler/icons-preact";

interface AddAccountCardProps {
  onClick: () => void;
}

export function AddAccountCard({ onClick }: AddAccountCardProps) {
  return (
    <button class="account-item account-home-card add-account-card" onClick={onClick}>
      <div class="add-account-icon-wrap">
        <IconPlus size={40} stroke={2.6} />
      </div>
      <p class="add-account-title">Add Account</p>
      <p class="add-account-subtitle">Create a new personal or business profile</p>
    </button>
  );
}

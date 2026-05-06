import { IconPlus } from "@tabler/icons-preact";
import { AccountHomeCardButton } from "./AccountHomeCardButton";

interface AddAccountCardProps {
  onClick: () => void;
}

export function AddAccountCard({ onClick }: AddAccountCardProps) {
  return (
    <AccountHomeCardButton
      className="add-account-card"
      onClick={onClick}
      ariaLabel="Add account"
      title="Add account"
    >
      <div class="add-account-icon-wrap">
        <IconPlus size={40} stroke={2.6} />
      </div>
      <p class="add-account-title">Add Account</p>
    </AccountHomeCardButton>
  );
}

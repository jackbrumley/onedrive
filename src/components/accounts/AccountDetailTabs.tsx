import type { AccountDetailTab } from "../../routes/appRoutes";

interface AccountDetailTabsProps {
  activeTab: AccountDetailTab;
  onSelectTab: (tab: AccountDetailTab) => void;
}

export function AccountDetailTabs({ activeTab, onSelectTab }: AccountDetailTabsProps) {
  return (
    <nav class="account-detail-tabs">
      <button class={activeTab === "overview" ? "tab active" : "tab"} onClick={() => onSelectTab("overview")}>
        Overview
      </button>
      <button class={activeTab === "sync" ? "tab active" : "tab"} onClick={() => onSelectTab("sync")}>
        Sync
      </button>
      <button class={activeTab === "activity" ? "tab active" : "tab"} onClick={() => onSelectTab("activity")}>
        Activity
      </button>
      <button class={activeTab === "settings" ? "tab active" : "tab"} onClick={() => onSelectTab("settings")}>
        Settings
      </button>
    </nav>
  );
}

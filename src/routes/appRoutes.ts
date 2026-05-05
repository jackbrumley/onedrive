export type AccountDetailTab = "overview" | "sync" | "activity" | "settings";

export type AppPage = "accountsHome" | "accountDetail" | "debug" | "uiLab";

export interface AppRouteState {
  page: AppPage;
  accountId: string | null;
  accountTab: AccountDetailTab;
}

const defaultState: AppRouteState = {
  page: "accountsHome",
  accountId: null,
  accountTab: "overview",
};

export function routeStateFromHash(hash: string): AppRouteState {
  const segments = hash.replace(/^#\/?/, "").split("/").filter(Boolean);
  if (segments.length === 0 || segments[0] === "accounts") {
    if (segments[1] && segments[2]) {
      const tab = normalizeAccountTab(segments[2]);
      return {
        page: "accountDetail",
        accountId: decodeURIComponent(segments[1]),
        accountTab: tab,
      };
    }
    return defaultState;
  }

  if (segments[0] === "settings" || segments[0] === "debug") {
    return {
      page: "debug",
      accountId: null,
      accountTab: "overview",
    };
  }

  if (segments[0] === "ui-lab") {
    return {
      page: "uiLab",
      accountId: null,
      accountTab: "overview",
    };
  }

  return defaultState;
}

export function hashFromRouteState(state: AppRouteState): string {
  if (state.page === "accountsHome") {
    return "#/accounts";
  }
  if (state.page === "accountDetail" && state.accountId) {
    return `#/accounts/${encodeURIComponent(state.accountId)}/${state.accountTab}`;
  }
  if (state.page === "debug") {
    return "#/settings";
  }
  if (state.page === "uiLab") {
    return "#/ui-lab";
  }
  return "#/accounts";
}

function normalizeAccountTab(value: string): AccountDetailTab {
  const normalized = value.trim().toLowerCase();
  if (normalized === "overview" || normalized === "sync" || normalized === "activity" || normalized === "settings") {
    return normalized;
  }
  return "overview";
}

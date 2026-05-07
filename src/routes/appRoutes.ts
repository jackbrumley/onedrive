export type AppPage = "accountsHome" | "accountDetail" | "settings" | "debug" | "uiLab";
export type AccountDetailView = "sync" | "settings";

export interface AppRouteState {
  page: AppPage;
  accountId: string | null;
  accountView: AccountDetailView | null;
}

const defaultState: AppRouteState = {
  page: "accountsHome",
  accountId: null,
  accountView: null,
};

export function routeStateFromHash(hash: string): AppRouteState {
  const segments = hash.replace(/^#\/?/, "").split("/").filter(Boolean);
  if (segments.length === 0 || segments[0] === "accounts") {
    if (segments[1]) {
      const accountView: AccountDetailView = segments[2] === "settings" ? "settings" : "sync";
      return {
        page: "accountDetail",
        accountId: decodeURIComponent(segments[1]),
        accountView,
      };
    }
    return defaultState;
  }

  if (segments[0] === "settings") {
    return {
      page: "settings",
      accountId: null,
      accountView: null,
    };
  }

  if (segments[0] === "debug") {
    return {
      page: "debug",
      accountId: null,
      accountView: null,
    };
  }

  if (segments[0] === "ui-lab") {
    return {
      page: "uiLab",
      accountId: null,
      accountView: null,
    };
  }

  return defaultState;
}

export function hashFromRouteState(state: AppRouteState): string {
  if (state.page === "accountsHome") {
    return "#/accounts";
  }
  if (state.page === "accountDetail" && state.accountId) {
    if (state.accountView === "settings") {
      return `#/accounts/${encodeURIComponent(state.accountId)}/settings`;
    }
    return `#/accounts/${encodeURIComponent(state.accountId)}`;
  }
  if (state.page === "settings") {
    return "#/settings";
  }
  if (state.page === "debug") {
    return "#/debug";
  }
  if (state.page === "uiLab") {
    return "#/ui-lab";
  }
  return "#/accounts";
}

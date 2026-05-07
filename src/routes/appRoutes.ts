export type AppPage = "accountsHome" | "accountDetail" | "settings" | "debug" | "uiLab";

export interface AppRouteState {
  page: AppPage;
  accountId: string | null;
}

const defaultState: AppRouteState = {
  page: "accountsHome",
  accountId: null,
};

export function routeStateFromHash(hash: string): AppRouteState {
  const segments = hash.replace(/^#\/?/, "").split("/").filter(Boolean);
  if (segments.length === 0 || segments[0] === "accounts") {
    if (segments[1]) {
      return {
        page: "accountDetail",
        accountId: decodeURIComponent(segments[1]),
      };
    }
    return defaultState;
  }

  if (segments[0] === "settings") {
    return {
      page: "settings",
      accountId: null,
    };
  }

  if (segments[0] === "debug") {
    return {
      page: "debug",
      accountId: null,
    };
  }

  if (segments[0] === "ui-lab") {
    return {
      page: "uiLab",
      accountId: null,
    };
  }

  return defaultState;
}

export function hashFromRouteState(state: AppRouteState): string {
  if (state.page === "accountsHome") {
    return "#/accounts";
  }
  if (state.page === "accountDetail" && state.accountId) {
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

import { hashFromRouteState, type AppRouteState } from "../../routes/appRoutes";

interface NavigationFactoryParams {
  setRouteState: (route: AppRouteState) => void;
}

export function createNavigationActions({ setRouteState }: NavigationFactoryParams) {
  const navigate = (nextState: AppRouteState) => {
    const nextHash = hashFromRouteState(nextState);
    if (window.location.hash === nextHash) {
      setRouteState(nextState);
      return;
    }
    window.location.hash = nextHash;
  };

  const goHome = () => {
    navigate({
      page: "accountsHome",
      accountId: null,
      accountView: null,
    });
  };

  const openAccount = (accountId: string) => {
    navigate({
      page: "accountDetail",
      accountId,
      accountView: "sync",
    });
  };

  const openAccountSettings = (accountId: string) => {
    navigate({
      page: "accountDetail",
      accountId,
      accountView: "settings",
    });
  };

  const goSettings = () => {
    navigate({
      page: "settings",
      accountId: null,
      accountView: null,
    });
  };

  const goDebug = () => {
    navigate({
      page: "debug",
      accountId: null,
      accountView: null,
    });
  };

  const goUiLab = () => {
    navigate({
      page: "uiLab",
      accountId: null,
      accountView: null,
    });
  };

  return {
    navigate,
    goHome,
    openAccount,
    openAccountSettings,
    goSettings,
    goDebug,
    goUiLab,
  };
}

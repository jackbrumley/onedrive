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
    });
  };

  const openAccount = (accountId: string) => {
    navigate({
      page: "accountDetail",
      accountId,
    });
  };

  const goDebug = () => {
    navigate({
      page: "debug",
      accountId: null,
    });
  };

  const goUiLab = () => {
    navigate({
      page: "uiLab",
      accountId: null,
    });
  };

  return {
    navigate,
    goHome,
    openAccount,
    goDebug,
    goUiLab,
  };
}

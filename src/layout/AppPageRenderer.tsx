import type { ComponentChildren } from "preact";
import type { AppPage } from "../routes/appRoutes";

type RenderPage = () => ComponentChildren;

interface AppPageRendererProps {
  page: AppPage;
  renderAccountsHome: RenderPage;
  renderAccountDetail: RenderPage;
  renderSettings: RenderPage;
  renderDebug: RenderPage;
  renderUiLab: RenderPage;
}

export function AppPageRenderer({
  page,
  renderAccountsHome,
  renderAccountDetail,
  renderSettings,
  renderDebug,
  renderUiLab,
}: AppPageRendererProps) {
  if (page === "accountsHome") {
    return <>{renderAccountsHome()}</>;
  }
  if (page === "accountDetail") {
    return <>{renderAccountDetail()}</>;
  }
  if (page === "settings") {
    return <>{renderSettings()}</>;
  }
  if (page === "debug") {
    return <>{renderDebug()}</>;
  }
  return <>{renderUiLab()}</>;
}

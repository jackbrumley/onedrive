import { useState } from "preact/hooks";
import type { AppStatusSnapshot } from "../types/somedrive";

interface DebugPageProps {
  status: AppStatusSnapshot;
  onBack: () => void;
  onNavigateUiLab: () => void;
  onRefreshStatus: () => Promise<void>;
  onFetchSessionLogText: () => Promise<string>;
  onCopySessionLog: () => Promise<void>;
  onOpenSessionLog: () => Promise<void>;
}

export function DebugPage({
  status,
  onBack,
  onNavigateUiLab,
  onRefreshStatus,
  onFetchSessionLogText,
  onCopySessionLog,
  onOpenSessionLog,
}: DebugPageProps) {
  const [logPreview, setLogPreview] = useState("");
  const [loadingLog, setLoadingLog] = useState(false);

  const loadLog = async () => {
    setLoadingLog(true);
    const text = await onFetchSessionLogText();
    const lines = text.split("\n").slice(-14).join("\n");
    setLogPreview(lines);
    setLoadingLog(false);
  };

  return (
    <section class="page">
      <div class="page-header">
        <h2>Debug Tools</h2>
        <button class="page-header-action" onClick={onBack}>
          Settings
        </button>
      </div>

      <article class="card">
        <h3>Development Tools</h3>
        <p>Use the UI Lab to preview account states and screen variants while tuning look and feel.</p>
        <div class="button-row">
          <button onClick={onNavigateUiLab}>Open UI Lab</button>
          <button onClick={onRefreshStatus}>Refresh Runtime State</button>
          <button onClick={loadLog} disabled={loadingLog}>
            {loadingLog ? "Loading..." : "Load Session Log Preview"}
          </button>
          <button onClick={onCopySessionLog}>Copy Session Log</button>
          <button onClick={onOpenSessionLog}>Open Session Log File</button>
        </div>
      </article>

      <article class="card">
        <h3>Runtime Snapshot</h3>
        <p>Platform: {status.platform}</p>
        <p>Version: v{status.appVersion}</p>
        <p>Health: {status.health}</p>
        <p>Profile Count: {status.accounts.length}</p>
      </article>

      <article class="card">
        <h3>Recent Session Log</h3>
        {logPreview ? <pre class="log-preview">{logPreview}</pre> : <p>Log preview not loaded yet.</p>}
      </article>
    </section>
  );
}

import type { AppStatusSnapshot, UpdateCheckResult } from "../types/onedrive";

interface StatusPageProps {
  status: AppStatusSnapshot;
  checkingUpdates: boolean;
  updateResult: UpdateCheckResult | null;
  updateError: string | null;
  lastCheckedAt: number | null;
  onRefreshStatus: () => Promise<void>;
  onCheckUpdates: () => Promise<void>;
}

export function StatusPage({
  status,
  checkingUpdates,
  updateResult,
  updateError,
  lastCheckedAt,
  onRefreshStatus,
  onCheckUpdates,
}: StatusPageProps) {
  return (
    <section class="page">
      <h2>System Status</h2>
      <div class="card-grid">
        <article class="card">
          <h3>Runtime</h3>
          <p>Version: v{status.appVersion}</p>
          <p>Platform: {status.platform}</p>
          <p>Health: {status.health}</p>
        </article>
        <article class="card">
          <h3>Sync Baseline</h3>
          <p>Engine Ready: {status.syncEngineReady ? "yes" : "not yet"}</p>
          <p>Any Account Configured: {status.authConfigured ? "yes" : "not yet"}</p>
          <p>Active Account: {status.activeAccount ?? "none"}</p>
          <p>Last Sync: {status.lastSyncAt ?? "never"}</p>
          <p>Profiles: {status.accounts.length}</p>
        </article>
      </div>

      <article class="card">
        <h3>Configured Accounts</h3>
        {status.accounts.length === 0 ? (
          <p>No account profiles yet. Add accounts in Settings using the GUI flow.</p>
        ) : (
          <div>
            {status.accounts.map((account) => (
              <p key={account.id}>
                {account.displayName} ({account.kind}) | root: {account.syncRoot} | state: {account.agentState}
              </p>
            ))}
          </div>
        )}
      </article>

      <div class="button-row">
        <button onClick={onRefreshStatus}>Refresh Status</button>
        <button onClick={onCheckUpdates} disabled={checkingUpdates}>
          {checkingUpdates ? "Checking..." : "Check for Updates"}
        </button>
      </div>

      <article class="card">
        <h3>Updater</h3>
        <p>
          {updateResult
            ? `Current v${updateResult.currentVersion} | Latest v${updateResult.latestVersion} | Update available: ${
                updateResult.updateAvailable ? "yes" : "no"
              }`
            : "No update check has been run yet."}
        </p>
        {updateResult?.updateAvailable && (
          <p>
            Release: <a href={updateResult.releaseUrl}>{updateResult.releaseUrl}</a>
          </p>
        )}
        {updateError && <p class="error">Last update error: {updateError}</p>}
        {lastCheckedAt && <p>Last checked: {new Date(lastCheckedAt).toLocaleString()}</p>}
      </article>
    </section>
  );
}

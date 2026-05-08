import {
  IconAlertTriangleFilled,
  IconBuildingBank,
  IconFolder,
  IconPlayerPauseFilled,
  IconPlayerPlayFilled,
  IconUser,
} from "@tabler/icons-preact";
import { AccountHomeCardButton } from "./AccountHomeCardButton";
import type { AccountProfile, SyncRuntimeAccountStatus } from "../../types/somedrive";
import { syncModeMessage } from "./syncModeMessaging";

const ISSUE_BADGE_WINDOW_MS = 10 * 60 * 1000;
const BLOCKING_ISSUE_CODES = new Set([
  "auth_required",
  "permission_denied",
  "disk_full",
  "sync_root_unavailable",
  "large_delete_guard",
  "unknown_error",
]);

function recentIssueCount(runtimeStatus: SyncRuntimeAccountStatus | null): number {
  if (!runtimeStatus) {
    return 0;
  }
  const now = Date.now();
  return runtimeStatus.recentFailed.filter((item) => {
    const finishedAt = new Date(item.finishedAt).getTime();
    return Number.isFinite(finishedAt) && now - finishedAt <= ISSUE_BADGE_WINDOW_MS;
  }).length;
}

function isSyncPhaseActive(phase: string | undefined): boolean {
  return (
    phase === "syncing" ||
    phase === "scanning_remote" ||
    phase === "applying_remote" ||
    phase === "scanning_local" ||
    phase === "applying_local"
  );
}

interface AccountCardProps {
  account: AccountProfile;
  runtimeStatus: SyncRuntimeAccountStatus | null;
  onOpenDetails: (accountId: string) => void;
  onSetAgentState: (accountId: string, state: "syncing" | "paused") => Promise<void>;
  onOpenSyncRootFolder: (accountId: string) => Promise<void>;
}

export function AccountCard({ account, runtimeStatus, onOpenDetails, onSetAgentState, onOpenSyncRootFolder }: AccountCardProps) {
  const modeMessage = syncModeMessage(runtimeStatus, account.lastSyncAt !== null);
  const accountKindLabel = account.kind.charAt(0).toUpperCase() + account.kind.slice(1);
  const accountIcon = account.kind === "business" ? <IconBuildingBank size={16} /> : <IconUser size={16} />;
  const runtimeIssueCode = runtimeStatus?.issueCode;
  const recentIssueTotal = recentIssueCount(runtimeStatus);
  const runtimeIssueIsBlocking = runtimeIssueCode ? BLOCKING_ISSUE_CODES.has(runtimeIssueCode) : false;
  const hasBlockingIssue =
    !account.authConfigured || runtimeIssueIsBlocking || account.agentState === "error" || runtimeStatus?.phase === "error";
  const hasNonBlockingIssue = !hasBlockingIssue && (recentIssueTotal > 0 || Boolean(runtimeIssueCode));
  const nonBlockingIssueCount = recentIssueTotal + (runtimeIssueCode && !hasBlockingIssue ? 1 : 0);
  const syncActive =
    account.agentState === "syncing" ||
    isSyncPhaseActive(runtimeStatus?.phase);
  const syncState = hasBlockingIssue ? "stopped" : syncActive ? "syncing" : "paused";
  const showIssueBadge = syncState === "syncing" && hasNonBlockingIssue && nonBlockingIssueCount > 0;
  const showBlockingMarker = syncState === "stopped";
  const syncButtonClass = showBlockingMarker
    ? "account-sync-nav-btn account-sync-nav-btn-stopped"
    : "account-sync-nav-btn";
  const nextSyncState: "syncing" | "paused" = syncActive ? "paused" : "syncing";
  const syncButtonTitle = showBlockingMarker
    ? "Open synchronization details"
    : syncState === "syncing"
      ? "Pause synchronization"
      : "Resume synchronization";
  const syncStateLabel = syncState === "stopped" ? "Stopped" : syncState === "syncing" ? "Syncing" : "Paused";

  return (
    <AccountHomeCardButton
      onClick={() => onOpenDetails(account.id)}
      ariaLabel={`Open ${account.displayName} account details`}
    >
      <div class="account-card-layout">
        <div class="account-card-left">
          <p class="account-title-line">
            <span class="account-name">{account.displayName}</span>
            <span class="account-name-sep"> - </span>
            <span class="account-email">{account.email}</span>
          </p>
          <button
            type="button"
            class="account-path-link"
            onClick={(event) => {
              event.stopPropagation();
              void onOpenSyncRootFolder(account.id);
            }}
          >
            <IconFolder size={14} /> {account.syncRoot}
          </button>
          <p class={`account-sync-mode-line account-sync-mode-line-${modeMessage.tone}`}>
            <span class="account-sync-mode-title">{modeMessage.title}</span>
            <span class="account-sync-mode-sep">: </span>
            <span class="account-sync-mode-detail">{modeMessage.detail}</span>
          </p>
        </div>
        <div class="account-card-right">
          <div class="account-sync-control-box">
            <span class="pill icon-pill account-kind-pill">{accountIcon} {accountKindLabel}</span>
            <div class="account-sync-action-wrap">
              <button
                class={syncButtonClass}
                type="button"
                aria-label={syncButtonTitle}
                title={syncButtonTitle}
                onClick={(event) => {
                  event.stopPropagation();
                  if (showBlockingMarker) {
                    onOpenDetails(account.id);
                    return;
                  }
                  void onSetAgentState(account.id, nextSyncState);
                }}
              >
                {showBlockingMarker ? (
                  <IconAlertTriangleFilled size={24} class="sync-stopped-icon" />
                ) : syncState === "syncing" ? (
                  <IconPlayerPauseFilled size={24} />
                ) : (
                  <IconPlayerPlayFilled size={24} />
                )}
              </button>
              {showIssueBadge && (
                <span
                  class="account-sync-issue-badge"
                  aria-label={`${nonBlockingIssueCount} sync issue${nonBlockingIssueCount === 1 ? "" : "s"}`}
                >
                  {nonBlockingIssueCount > 9 ? "9+" : nonBlockingIssueCount}
                </span>
              )}
            </div>
            <span class="account-sync-state-label">{syncStateLabel}</span>
          </div>
        </div>
      </div>
    </AccountHomeCardButton>
  );
}

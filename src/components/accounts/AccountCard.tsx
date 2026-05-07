import {
  IconAlertCircle,
  IconBuildingBank,
  IconFolder,
  IconPlayerPauseFilled,
  IconPlayerPlayFilled,
  IconRefresh,
  IconUser,
} from "@tabler/icons-preact";
import { useState } from "preact/hooks";
import { AccountHomeCardButton } from "./AccountHomeCardButton";
import type { AccountProfile, SyncRuntimeAccountStatus } from "../../types/somedrive";

const ISSUE_BADGE_WINDOW_MS = 10 * 60 * 1000;

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
  const [syncButtonHovered, setSyncButtonHovered] = useState(false);
  const lastSyncLabel = account.lastSyncAt ? new Date(account.lastSyncAt).toLocaleString() : "Never";
  const accountIcon = account.kind === "business" ? <IconBuildingBank size={16} /> : <IconUser size={16} />;
  const runtimeIssueCode = runtimeStatus?.issueCode ?? null;
  const hasBlockingIssue =
    !account.authConfigured || runtimeIssueCode !== null || account.agentState === "error" || runtimeStatus?.phase === "error";
  const syncActive =
    account.agentState === "syncing" ||
    (runtimeStatus?.inProgress.length ?? 0) > 0 ||
    isSyncPhaseActive(runtimeStatus?.phase);
  const issueCount = recentIssueCount(runtimeStatus) + (hasBlockingIssue ? 1 : 0);
  const showIssueBadge = issueCount > 0;
  const showBlockingIssueWarning = hasBlockingIssue && !syncActive;
  const syncButtonClass = showBlockingIssueWarning
    ? "account-sync-nav-btn account-sync-nav-btn-warning"
    : "account-sync-nav-btn";
  const nextSyncState: "syncing" | "paused" = syncActive ? "paused" : "syncing";
  const syncButtonTitle = showBlockingIssueWarning
    ? "Open synchronization details"
    : syncActive
      ? syncButtonHovered
        ? "Pause synchronization"
        : "Synchronizing"
      : syncButtonHovered
        ? "Resume synchronization"
        : "Synchronization paused";

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
          <p class="account-last-sync">Last Sync: {lastSyncLabel}</p>
        </div>
        <div class="account-card-right">
          <span class="pill icon-pill account-kind-pill">{accountIcon} {account.kind}</span>
          <button
            class={syncButtonClass}
            type="button"
            aria-label={syncButtonTitle}
            title={syncButtonTitle}
            onClick={(event) => {
              event.stopPropagation();
              if (showBlockingIssueWarning) {
                onOpenDetails(account.id);
                return;
              }
              void onSetAgentState(account.id, nextSyncState);
            }}
            onMouseEnter={() => setSyncButtonHovered(true)}
            onMouseLeave={() => setSyncButtonHovered(false)}
          >
            {showBlockingIssueWarning ? (
              <IconAlertCircle class="sync-icon-warning-pulse" size={24} />
            ) : syncActive ? (
              syncButtonHovered ? (
                <IconPlayerPauseFilled size={24} />
              ) : (
                <IconRefresh class="sync-icon-spinning" size={24} />
              )
            ) : syncButtonHovered ? (
              <IconPlayerPlayFilled size={24} />
            ) : (
              <IconPlayerPauseFilled size={24} />
            )}
            {showIssueBadge && (
              <span class="account-sync-issue-badge" aria-label={`${issueCount} sync issue${issueCount === 1 ? "" : "s"}`}>
                {issueCount > 9 ? "9+" : issueCount}
              </span>
            )}
          </button>
        </div>
      </div>
    </AccountHomeCardButton>
  );
}

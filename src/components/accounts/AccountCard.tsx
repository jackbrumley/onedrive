import {
  IconAlertCircle,
  IconBuildingBank,
  IconFolder,
  IconPlayerPauseFilled,
  IconRefresh,
  IconUser,
} from "@tabler/icons-preact";
import { createPortal } from "preact/compat";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { AccountSyncPreviewPopover } from "./AccountSyncPreviewPopover";
import { AccountHomeCardButton } from "./AccountHomeCardButton";
import type { AccountProfile, SyncRuntimeAccountStatus } from "../../types/somedrive";

const PREVIEW_VIEWPORT_MARGIN = 8;
const PREVIEW_TRIGGER_GAP = 8;
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
  if (!phase) {
    return false;
  }
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
  onOpenSyncRootFolder: (accountId: string) => Promise<void>;
  onOpenItemFolder: (accountId: string, relativePath: string) => Promise<void>;
  onReauthenticate: (accountId: string) => Promise<unknown>;
  onRetrySync: (accountId: string) => Promise<void>;
}

export function AccountCard({
  account,
  runtimeStatus,
  onOpenDetails,
  onOpenSyncRootFolder,
  onOpenItemFolder,
  onReauthenticate,
  onRetrySync,
}: AccountCardProps) {
  const [previewOpen, setPreviewOpen] = useState(false);
  const [previewPosition, setPreviewPosition] = useState<{
    top: number;
    left: number;
    placement: "top" | "bottom";
  } | null>(null);
  const closePreviewTimerRef = useRef<number | null>(null);
  const previewAnchorRef = useRef<HTMLDivElement | null>(null);
  const previewFloatingRef = useRef<HTMLDivElement | null>(null);

  const clearClosePreviewTimer = () => {
    if (closePreviewTimerRef.current !== null) {
      window.clearTimeout(closePreviewTimerRef.current);
      closePreviewTimerRef.current = null;
    }
  };

  const openPreview = () => {
    clearClosePreviewTimer();
    setPreviewOpen(true);
  };

  const closePreviewWithDelay = () => {
    clearClosePreviewTimer();
    closePreviewTimerRef.current = window.setTimeout(() => {
      setPreviewOpen(false);
      closePreviewTimerRef.current = null;
    }, 120);
  };

  useEffect(() => {
    return () => {
      clearClosePreviewTimer();
    };
  }, []);

  const updatePreviewPosition = useCallback(() => {
    const anchor = previewAnchorRef.current;
    const floating = previewFloatingRef.current;
    if (!anchor || !floating) {
      return;
    }

    const anchorRect = anchor.getBoundingClientRect();
    const floatingRect = floating.getBoundingClientRect();
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;

    let left = anchorRect.right - floatingRect.width;
    const maxLeft = viewportWidth - floatingRect.width - PREVIEW_VIEWPORT_MARGIN;
    left = Math.min(Math.max(left, PREVIEW_VIEWPORT_MARGIN), Math.max(PREVIEW_VIEWPORT_MARGIN, maxLeft));

    const topCandidate = anchorRect.top - floatingRect.height - PREVIEW_TRIGGER_GAP;
    const bottomCandidate = anchorRect.bottom + PREVIEW_TRIGGER_GAP;
    const canPlaceTop = topCandidate >= PREVIEW_VIEWPORT_MARGIN;
    const canPlaceBottom = bottomCandidate + floatingRect.height <= viewportHeight - PREVIEW_VIEWPORT_MARGIN;
    const placement = canPlaceBottom || !canPlaceTop ? "bottom" : "top";
    const top =
      placement === "top"
        ? Math.max(PREVIEW_VIEWPORT_MARGIN, topCandidate)
        : Math.min(viewportHeight - floatingRect.height - PREVIEW_VIEWPORT_MARGIN, bottomCandidate);

    setPreviewPosition({ left, top, placement });
  }, []);

  useLayoutEffect(() => {
    if (!previewOpen) {
      setPreviewPosition(null);
      return;
    }

    const firstFrame = window.requestAnimationFrame(updatePreviewPosition);
    const secondFrame = window.requestAnimationFrame(updatePreviewPosition);

    const handleViewportChange = () => {
      updatePreviewPosition();
    };

    window.addEventListener("resize", handleViewportChange);
    window.addEventListener("scroll", handleViewportChange, true);

    let resizeObserver: ResizeObserver | null = null;
    if (previewFloatingRef.current && typeof ResizeObserver !== "undefined") {
      resizeObserver = new ResizeObserver(() => {
        updatePreviewPosition();
      });
      resizeObserver.observe(previewFloatingRef.current);
    }

    return () => {
      window.cancelAnimationFrame(firstFrame);
      window.cancelAnimationFrame(secondFrame);
      window.removeEventListener("resize", handleViewportChange);
      window.removeEventListener("scroll", handleViewportChange, true);
      resizeObserver?.disconnect();
    };
  }, [previewOpen, updatePreviewPosition]);

  const lastSyncLabel = account.lastSyncAt ? new Date(account.lastSyncAt).toLocaleString() : "Never";
  const accountIcon = account.kind === "business" ? <IconBuildingBank size={16} /> : <IconUser size={16} />;
  const runtimeIssueCode = runtimeStatus?.issueCode ?? null;
  const hasBlockingIssue =
    !account.authConfigured || runtimeIssueCode !== null || account.agentState === "error" || runtimeStatus?.phase === "error";
  const syncIssueMessage =
    runtimeStatus?.issueMessage ??
    (!account.authConfigured ? "Authentication required" : runtimeStatus?.phaseMessage ?? "Synchronization blocked");
  const issueKind =
    !account.authConfigured || runtimeIssueCode === "auth_required" ? "auth_required" : hasBlockingIssue ? "sync_error" : null;
  const issueActions =
    runtimeStatus?.issueActions.length
      ? runtimeStatus.issueActions
      : issueKind === "auth_required"
        ? ["reauthenticate", "retry_sync"]
        : issueKind === "sync_error"
          ? ["retry_sync"]
          : [];
  const syncActive =
    account.agentState === "syncing" ||
    (runtimeStatus?.inProgress.length ?? 0) > 0 ||
    isSyncPhaseActive(runtimeStatus?.phase);
  const issueCount = recentIssueCount(runtimeStatus) + (hasBlockingIssue ? 1 : 0);
  const showIssueBadge = issueCount > 0;
  const syncButtonClass = hasBlockingIssue && !syncActive
    ? "account-sync-nav-btn account-sync-nav-btn-warning"
    : "account-sync-nav-btn";

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
          <div
            ref={previewAnchorRef}
            class="account-sync-preview-anchor"
            onMouseEnter={openPreview}
            onMouseLeave={closePreviewWithDelay}
            onFocusIn={openPreview}
            onFocusOut={(event) => {
              const nextTarget = event.relatedTarget as Node | null;
              if (!nextTarget || !event.currentTarget.contains(nextTarget)) {
                closePreviewWithDelay();
              }
            }}
          >
            <button
              class={syncButtonClass}
              type="button"
              aria-label="Open synchronization details"
              onClick={(event) => {
                event.stopPropagation();
                const isTouchLikePointer =
                  typeof window !== "undefined" && window.matchMedia("(hover: none), (pointer: coarse)").matches;
                if (isTouchLikePointer) {
                  clearClosePreviewTimer();
                  setPreviewOpen((current) => !current);
                  return;
                }
                onOpenDetails(account.id);
              }}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  clearClosePreviewTimer();
                  setPreviewOpen(false);
                }
              }}
            >
              {syncActive ? (
                <IconRefresh class="sync-icon-spinning" size={24} />
              ) : hasBlockingIssue ? (
                <IconAlertCircle class="sync-icon-warning-pulse" size={24} />
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
      </div>
      {previewOpen &&
        typeof document !== "undefined" &&
        createPortal(
          <div
            ref={previewFloatingRef}
            class="account-sync-preview-floating"
            style={
              previewPosition
                ? {
                    left: `${previewPosition.left}px`,
                    top: `${previewPosition.top}px`,
                  }
                : undefined
            }
            onMouseEnter={openPreview}
            onMouseLeave={closePreviewWithDelay}
            onFocusIn={openPreview}
            onClick={(event) => event.stopPropagation()}
            onMouseDown={(event) => event.stopPropagation()}
            onTouchStart={(event) => event.stopPropagation()}
          >
              <AccountSyncPreviewPopover
                runtimeStatus={runtimeStatus}
                issueMessage={hasBlockingIssue ? syncIssueMessage : null}
                issueKind={issueKind}
                issueActions={issueActions}
              issuePath={runtimeStatus?.issuePath ?? null}
              issueSecondaryPath={runtimeStatus?.issueSecondaryPath ?? null}
              onOpenItemFolder={(relativePath) => onOpenItemFolder(account.id, relativePath)}
              onOpenSyncRootFolder={() => onOpenSyncRootFolder(account.id)}
              onReauthenticate={() => onReauthenticate(account.id)}
              onRetrySync={() => onRetrySync(account.id)}
              placement={previewPosition?.placement ?? "bottom"}
              visible={previewPosition !== null}
            />
          </div>,
          document.body
        )}
    </AccountHomeCardButton>
  );
}

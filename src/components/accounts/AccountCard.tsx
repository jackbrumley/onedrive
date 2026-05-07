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

interface AccountCardProps {
  account: AccountProfile;
  runtimeStatus: SyncRuntimeAccountStatus | null;
  onOpenDetails: (accountId: string) => void;
  onOpenSyncRootFolder: (accountId: string) => Promise<void>;
  onOpenItemFolder: (accountId: string, relativePath: string) => Promise<void>;
}

export function AccountCard({
  account,
  runtimeStatus,
  onOpenDetails,
  onOpenSyncRootFolder,
  onOpenItemFolder,
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
  const hasSyncIssue = !account.authConfigured || account.agentState === "error" || runtimeStatus?.phase === "error";
  const syncIssueMessage = !account.authConfigured
    ? "Authentication required"
    : runtimeStatus?.phaseMessage ?? "Synchronization blocked";
  const syncButtonClass = hasSyncIssue
    ? "account-sync-nav-btn account-sync-nav-btn-warning"
    : account.agentState === "syncing"
      ? "account-sync-nav-btn account-sync-nav-btn-syncing"
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
              {hasSyncIssue ? (
                <IconAlertCircle class="sync-icon-warning-pulse" size={24} />
              ) : account.agentState === "syncing" ? (
                <IconRefresh class="sync-icon-spinning" size={24} />
              ) : (
                <IconPlayerPauseFilled size={24} />
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
              errorMessage={hasSyncIssue ? syncIssueMessage : null}
              onOpenItemFolder={(relativePath) => onOpenItemFolder(account.id, relativePath)}
              placement={previewPosition?.placement ?? "bottom"}
              visible={previewPosition !== null}
            />
          </div>,
          document.body
        )}
    </AccountHomeCardButton>
  );
}

export type AccountKind = "personal" | "business";

export type SyncAgentState = "idle" | "syncing" | "paused" | "error";

export interface AccountProfile {
  id: string;
  displayName: string;
  email: string;
  slug: string;
  kind: AccountKind;
  syncRoot: string;
  authConfigured: boolean;
  agentState: SyncAgentState;
  lastSyncAt: string | null;
}

export interface ActivityEvent {
  id: string;
  profileId: string;
  profileName: string;
  kind: "info" | "success" | "warning" | "error";
  message: string;
  timestamp: string;
}

export interface DeviceAuthSession {
  profileId: string;
  userCode: string;
  verificationUri: string;
  verificationUriComplete: string | null;
  expiresIn: number;
  interval: number;
  message: string;
}

export interface DeviceAuthPollResult {
  status: "pending" | "authorized" | "error";
  detail: string;
}

export interface CreateAccountProfileInput {
  displayName: string;
  kind: AccountKind;
}

export interface RenameAccountProfileInput {
  id: string;
  displayName: string;
}

export interface SetAccountAgentStateInput {
  id: string;
  agentState: SyncAgentState;
}

export interface SetAccountSyncRootInput {
  id: string;
  syncRoot: string;
}

export interface AppStatusSnapshot {
  appVersion: string;
  platform: string;
  syncEngineReady: boolean;
  authConfigured: boolean;
  activeAccount: string | null;
  lastSyncAt: string | null;
  health: "ok" | "degraded" | "error";
  accounts: AccountProfile[];
}

export interface UpdateCheckResult {
  currentVersion: string;
  latestVersion: string;
  updateAvailable: boolean;
  releaseUrl: string;
}

export interface SyncRuntimeTransfer {
  id: string;
  direction: string;
  path: string;
  bytesDone: number;
  bytesTotal: number | null;
  startedAt: string;
  updatedAt: string;
}

export interface SyncRuntimeRecentItem {
  id: string;
  direction: string;
  path: string;
  bytesTotal: number | null;
  finishedAt: string;
  status: string;
  error: string | null;
}

export interface SyncRuntimeAccountStatus {
  profileId: string;
  phase: string;
  phaseMessage: string;
  issueCode: string | null;
  issueMessage: string | null;
  issueActions: string[];
  issuePath: string | null;
  issueSecondaryPath: string | null;
  inProgress: SyncRuntimeTransfer[];
  recentCompleted: SyncRuntimeRecentItem[];
  recentFailed: SyncRuntimeRecentItem[];
  updatedAt: string;
}

export interface SyncRuntimeSnapshot {
  generatedAt: string;
  accounts: SyncRuntimeAccountStatus[];
}

export type ToastType = "success" | "error" | "info";

export interface ToastMessage {
  id: number;
  message: string;
  type: ToastType;
  durationMs: number;
}

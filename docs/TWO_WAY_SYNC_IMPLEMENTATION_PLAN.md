# Two-Way Sync Implementation Plan

## Goal

Implement a real two-way synchronization engine that mirrors remote OneDrive changes locally and pushes local changes back to OneDrive, replacing the current heartbeat-only stub.

## Current State (Gap)

- The current sync worker only updates `lastSyncAt` and writes heartbeat events.
- No Microsoft Graph `/delta` calls are made.
- No file download/upload/delete actions are executed.
- No persisted sync cursor/state exists for incremental reconciliation.

## Target Behavior

When an account is in `syncing` state:

1. Validate account/auth prerequisites.
2. Fetch remote incremental changes via Graph `/delta` using stored cursor.
3. Apply remote changes to local filesystem.
4. Detect local changes since previous snapshot.
5. Push local creates/updates/deletes to OneDrive.
6. Persist updated sync state and account `lastSyncAt`.
7. Emit rich diagnostics to session/activity logs.

## Architecture

### 1) Persistent Sync State (per account)

Store at:

- `~/.config/onedrive/accounts/<profile-id>/sync_state.json`

State fields:

- `deltaLink` (latest Graph delta checkpoint)
- `remoteById` (id -> metadata)
- `remotePathToId` (path -> id)
- `localSnapshot` (path -> local metadata)
- `lastCycleAt`

### 2) Graph Client Usage

Use existing auth session token from `auth.json` and Graph endpoints:

- Delta read: `GET /v1.0/me/drive/root/delta`
- Download content: `GET /v1.0/me/drive/items/{id}/content`
- Upload file: `PUT /v1.0/me/drive/root:/<path>:/content`
- Create folder: `POST /v1.0/me/drive/root:/<parent>:/children`
- Delete item: `DELETE /v1.0/me/drive/items/{id}`

### 3) Reconciliation Strategy

Two-phase each cycle:

1. **Remote -> Local**
   - Apply deletes, directory creates, file downloads.
   - If both sides changed, use mtime-based winner (local newer -> upload, remote newer -> download).

2. **Local -> Remote**
   - Upload new/modified local files.
   - Create new local directories remotely.
   - Propagate local deletions to remote when known remote id exists.

### 4) Worker Lifecycle

Keep current worker model (one per account), but replace heartbeat tick with full sync cycle execution.

### 5) Logging / Diagnostics

For each cycle, log:

- Start/end with account id/name and elapsed time
- Delta fetch stats (`changed`, `nextLink` pagination, `deltaLink` updated)
- Counts for downloaded/uploaded/created/deleted/conflicts
- Hard failures (auth missing/expired, network, filesystem)

## Implementation Steps

1. Add auth session reader helper for sync engine token access.
2. Replace `sync_engine.rs` heartbeat logic with real two-way cycle.
3. Implement sync state load/save helpers.
4. Implement Graph delta pagination and delta item parsing.
5. Implement remote-to-local application functions.
6. Implement local scan and local-to-remote propagation functions.
7. Wire counters + logs + activity events.
8. Update account `lastSyncAt` only on successful cycle completion.
9. Validate with `cargo check` and frontend `typecheck`.

## Validation Checklist

- Authenticated account + `syncing` state triggers real Graph traffic.
- New remote file appears locally.
- Updated remote file replaces local file.
- Deleted remote file is removed locally.
- New local file uploads to remote.
- Updated local file uploads to remote.
- Deleted local file deletes remote item.
- `sync_state.json` persists and incremental cycles use `deltaLink`.
- Session log contains full cycle diagnostics.

## Known Follow-ups

- Token refresh flow (using refresh token) for expired access tokens.
- Conflict policy hardening beyond mtime heuristic.
- Ignore patterns / selective sync rules.
- Hash-based equality checks for large files.
- Batched and parallel transfer optimization.

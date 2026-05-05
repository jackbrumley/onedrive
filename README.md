# OneDrive (Linux-first)

OneDrive is a Linux-first desktop client for Microsoft OneDrive built as a single, self-contained Tauri application.

This project baseline uses modular Rust command boundaries, Preact UI pages/components/hooks, clear runtime state ownership, and an in-app update-check workflow.

## Product Principles

- GUI-only product: no terminal commands, no manual config editing, and no required command-line workflow.
- Setup by button clicks: onboarding, login, sync setup, recovery actions, and diagnostics are all in-app.
- Single app runtime: one desktop application manages all account profiles.
- Multi-account from day one: personal, business, and additional accounts in one install.

## User Experience Goals

- Simple installation with package formats users expect.
- First-run guided setup for non-technical users.
- Account management in one screen: add, remove, rename, pause, resume.
- Clear status and actionable errors without asking users to open a terminal.
- Accounts-first landing screen with one card per account profile.
- Account detail workspace with local tabs (`overview`, `sync`, `activity`, `settings`).

## Project Goals

- Deliver a clean Linux desktop GUI for OneDrive Personal and OneDrive for Business.
- Keep the entire application in one repository and one app runtime (no external helper services).
- Prioritize maintainability with explicit module boundaries and incremental milestone delivery.
- Support multiple account profiles in one app instance with independent sync control per profile.
- Default sync root convention: `~/OneDrive-OSS/<profile-slug>/`.

## Scope (Baseline)

This baseline includes:

- Tauri v2 + Rust backend + Preact frontend structure.
- App shell with routed pages (`status`, `files`, `activity`, `settings`, `debug`).
- Shared runtime hook and typed frontend models.
- Backend command modules with initial status + update-check commands.
- Session logging foundation.
- Baseline architecture docs and development guardrails (`AGENTS.md`).

This baseline does **not** yet implement OneDrive auth or sync logic.

## Architecture

### Frontend (Preact + TypeScript)

- `src/app/` route parsing and app-level wiring.
- `src/components/` reusable UI shell and feedback components.
- `src/hooks/` runtime orchestration and UI behavior hooks.
- `src/pages/` feature pages by domain.
- `src/types/` strict shared UI types.

### Backend (Rust + Tauri Commands)

- `src-tauri/src/app/commands/` command modules by responsibility.
- `src-tauri/src/app/state.rs` shared app state container.
- `src-tauri/src/app/session_log.rs` runtime/session logging.
- `src-tauri/src/lib.rs` orchestration only: plugins/state/command registration.

### Multi-Account Architecture Baseline

- `AccountProfile`: source-of-truth record for each linked account.
- Per-profile sync root: `~/OneDrive-OSS/<profile-slug>/`.
- Per-profile runtime state: auth health, cursor state, last sync, sync mode.
- Independent sync agents: one worker loop per profile managed by a central orchestrator.
- Global app shell: unified status/activity view across all profile agents.

### Storage Model (Planned)

- User files:
  - `~/OneDrive-OSS/personal/`
  - `~/OneDrive-OSS/work/`
  - `~/OneDrive-OSS/personal-2/`
- App metadata (internal):
  - `~/.config/onedrive/accounts/<profile-id>/`
  - auth/session metadata
  - sync cursors and profile-specific settings

Current baseline implementation stores account profiles in:

- `~/.config/onedrive/accounts/profiles.json`

Current baseline authentication implementation stores per-profile auth sessions in:

- `~/.config/onedrive/accounts/<profile-id>/auth.json`

### OneDrive Reference Inputs

Reference systems were reviewed from `reference-systems/` for OneDrive technical constraints such as:

- Delta-based synchronization lifecycle.
- Local cache/database responsibilities.
- Monitor mode and remote-change update strategy.
- Webhook and connectivity edge cases.

## Milestone Plan

### M0 - Baseline (this phase)

- Project scaffold, docs, command boundaries, typed UI foundation.
- Account profile persistence and local sync root provisioning.
- Debug UI Lab route for frontend state/screen simulation (`#/ui-lab`).
- GUI folder picker for per-profile sync root changes.
- Microsoft device-code sign-in flow scaffolding (in-app initiation + polling).
- Activity feed persistence and sync-agent heartbeat events.

### M1 - Auth + Account Status

- In-app Microsoft OAuth2 sign-in (popup/webview flow where needed).
- Multi-account onboarding in one runtime (personal/work/additional).
- Account/session status panel and token health per profile.

### M2 - File Browser + Sync Preview

- Remote tree listing.
- Local sync root selection per account profile.
- Dry-run style planned actions preview.

### M3 - Controlled Sync Operations

- Manual sync actions.
- Conflict and deletion safety handling.
- Basic activity timeline.
- Independent per-profile controls (start, pause, resume, retry).

### M4 - Ongoing Monitor + Live Updates

- Local file watch integration.
- Remote change pull strategy.
- Optional webhook-assisted remote events.

## Development

Prerequisites:

- Node.js + npm
- Rust + Cargo
- Tauri v2 build dependencies for Linux

Commands:

```bash
npm install
npm run tauri:dev
```

Build:

```bash
npm run tauri:build
```

## Standards Used

- Structural standards focused on clean module boundaries and maintainability.
- Agent and architecture standards documented in `AGENTS.md`.
- Product philosophy: desktop-first, simple UX, no command-line dependency for end users.

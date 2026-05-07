# SomeDrive Frontend Architecture

This document defines the frontend structure and where new code should live.

## High-Level Structure

- `App.tsx`: shell composition only (shell, workspace mounting, toast mounting).
- `components/*`: reusable UI and feature components.
- `pages/*`: page-level rendering components only.
- `layout/*`: routing/page composition and structural wrappers.
- `hooks/*`: stateful orchestration and side-effect logic.
- `routes/*`: URL/hash route parsing and route state mapping.
- `types/*`: shared frontend domain models and API-facing types.
- `styles.css`: import aggregator only.
- `styles/*`: focused style files grouped by concern (shell, pages, accounts, forms, sync, toast).

## Guardrails

- Keep `src/main.tsx` wiring-only.
- Keep `src/App.tsx` composition-only.
- Keep file size under 1000 lines (`.ts`, `.tsx`, `.css`).
- Keep visual concerns out of hooks; keep data/side effects out of styling modules.
- Keep Tauri command calls in hooks/runtime modules, not deeply nested presentational components.

## Current Hook Responsibilities

- `useAppRuntime`: top-level runtime orchestration facade for route state, backend snapshots, account actions, and lifecycle wiring.
- `useToastManager`: toast queue and lifecycle management.
- `useWindowControls`: titlebar drag and window control actions.

## Verification

- Run `npm run typecheck` for frontend integrity.
- Run `npm run build` for frontend bundle verification.

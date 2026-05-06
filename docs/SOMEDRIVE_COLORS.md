# SomeDrive Color Reference

This document defines the primary SomeDrive palette used by the app UI.

## Source Colors

Brand direction anchored to `docs/logo.svg` with the current app palette:

- `#1D4ED8` (primary action blue)
- `#1A57D3` (hover/pressed blue)
- `#22C1C3` (secondary accent)
- `#0891B2` (soft/link accent)
- `#FFFFFF` (cloud/S mark)

Preferred text treatment:

- headings: `#F3F7FF`
- links/icons: `#0891B2`

## Token Mapping

Core brand tokens:

- `--brand-primary: #1D4ED8`
- `--brand-strong: #1A57D3`
- `--brand-highlight: #22C1C3`
- `--brand-soft: #0891B2`
- `--brand-on-color: #FFFFFF`

UI context tokens:

- `--bg-page: #121722` (dark charcoal)
- `--bg-card: #222D3D`
- `--text-main: #F3F7FF`
- `--text-muted: #A7B6CC`

Action accent token:

- `--success: #28C268` for the add-account plus card CTA

## Usage Guidance

- Use `--brand-primary` for primary buttons and active controls.
- Use `--brand-strong` for pressed/hover states and stronger outlines.
- Use `--brand-highlight` sparingly for focus/secondary accent moments.
- App background uses a logo-aligned gradient blend from `#1D4ED8` to `#0891B2`.
- Use `--brand-on-color` for icon/label content that sits on brand gradients.
- Use dark charcoal surfaces for primary backgrounds to keep brand blues vibrant.
- Reserve green for positive, creation-oriented actions (for example, Add Account card).

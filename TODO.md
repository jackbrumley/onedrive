# TODO

## Auth Registration Migration (Microsoft Entra)

- [ ] Create a new Microsoft Entra app registration owned by this project (do not use third-party client IDs).
- [ ] Set app type to support both account types:
  - Personal Microsoft accounts (MSA)
  - Work/school accounts (Entra ID)
- [ ] Configure branding before release:
  - App name (final product name)
  - Publisher/support URLs
  - Privacy policy URL
  - Terms of service URL (if available)
  - Logo/icon so consent screens are trustworthy

## Redirect + Client Configuration

- [ ] Add redirect URI used by desktop interactive auth:
  - `https://login.microsoftonline.com/common/oauth2/nativeclient`
- [ ] Verify public client/native flow settings are enabled as required by this flow.
- [ ] Confirm the app supports authorization code flow used by desktop sign-in.

## API Permissions + Consent

- [ ] Add Microsoft Graph delegated permissions required by the app.
- [ ] Baseline permissions currently expected by the app:
  - `offline_access`
  - `User.Read`
  - OneDrive/Files scopes needed for sync
- [ ] Review and reduce scopes to least privilege where possible.
- [ ] Grant/admin-consent where needed for organization scenarios.

## App Integration

- [ ] Set local environment variable for testing with project-owned app registration:
  - `ONEDRIVE_INTERACTIVE_CLIENT_ID=<new-client-id>`
- [ ] Verify app no longer shows third-party consent app identity.
- [ ] After validation, update default interactive client ID in code to project-owned ID.
- [ ] Keep env var override for emergencies and staging.

## QA Validation Matrix

- [ ] Personal account flow:
  - Add account (`personal`)
  - Open in-app sign-in window
  - Complete login + consent
  - Verify token exchange succeeds
  - Verify account shows authenticated in app
- [ ] Business account flow:
  - Add account (`business`)
  - Open in-app sign-in window
  - Complete login + consent (tenant policies permitting)
  - Verify token exchange succeeds
  - Verify account shows authenticated in app
- [ ] Redirect safety checks:
  - Validate callback state verification still passes
  - Validate mismatch/error paths are logged clearly

## Logging + Diagnostics

- [ ] Confirm logs contain auth routing details for each attempt:
  - selected account kind
  - `domain_hint`
  - `client_id` used
  - authorize/token/redirect URLs
- [ ] Confirm `Copy Session Log` includes startup environment + auth lifecycle entries.
- [ ] Confirm `Open Session Log File` opens the live log path correctly.

## Documentation

- [ ] Add a section in `README.md` describing auth setup ownership and the required app registration.
- [ ] Document required redirect URI and permissions in a dedicated auth setup doc.
- [ ] Document local override variable:
  - `ONEDRIVE_INTERACTIVE_CLIENT_ID`

## Release Hardening

- [ ] Rotate out all non-project-owned auth IDs from defaults before release.
- [ ] Verify consent text and branding match final product naming everywhere.
- [ ] Run final regression on add-account, auth completion, log copy/open, and account settings UX.

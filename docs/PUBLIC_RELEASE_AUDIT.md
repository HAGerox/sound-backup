# Public release audit — Sound Backup 0.1.0

Reviewed 30 September 2026 before publishing the source and macOS DMG.

## Scope and checks

- Gitleaks scanned all local Git history, including checkpoint refs, with no secrets found. TruffleHog independently scanned Git history with verification disabled and reported zero verified or unverified secrets.
- Gitleaks separately scanned the publishable working files, including new providers. No secrets found. A broad scan of ignored build output flagged six copies of a `muda` Rust metadata documentation example; these are not credentials and `.rmeta` files are not distributed.
- Manually reviewed credentials, settings serialization, SSH commands and host identity verification, OSC/HTTP/AH-Net traffic, discovery, deep links, frontend rendering, build inputs, documentation, and the tracked file inventory. Inspected historical URL, email, and absolute-path references.
- npm audit reported zero known vulnerabilities. Cargo audit reported zero vulnerabilities. The lockfile retains upstream Unicode maintenance warnings through Tauri. GLib unsoundness and proc-macro-error maintenance advisories concern platform dependencies absent from the macOS dependency tree. Updated the yanked libssh2-sys 0.3.2 to 0.3.3.
- Ran Rust workspace tests and frontend regression tests. One Rust test is intentionally skipped because it requires an open QLab workspace.
- Inspected the app bundle and executable, verified the ad-hoc signature, and verified the DMG. Gitleaks also scanned extracted executable strings; its sole match was the upstream libssh2 error text `invalid/unrecognized private key file format`, not a credential. Checked the website route, assets, download link, and layout at desktop and mobile widths.

## Findings and changes

- Failed Remote Login attempts previously cleared the saved SSH host fingerprint. Retaining it prevents a retry from silently trusting a changed remote host. Added regression coverage for failure and successful connection.
- Removed the developer's name from current test fixtures. Older commits retain harmless examples (`/Users/finn/Show` and `finn@qlab-mac.local`); these are test data, not credentials or production account configuration.
- Rust panic locations and vendored OpenSSL paths included the builder's home directory. The packaging command remaps Rust paths and builds native dependencies outside the home directory; the distributed executable was checked for `/Users/` paths.
- The app stores Remote Login passwords and QLab OSC passcodes in macOS Keychain. Saved settings contain endpoints, usernames, workspace names, host fingerprints and the chosen backup folder, but no passwords. User settings and Keychain contents are not packaging inputs.
- No analytics, telemetry, cloud upload endpoints, private show archives, audio recordings, or Allen & Heath binaries are included. Network traffic goes to discovered or configured systems. Optional Avantis trace logging can print endpoint and Show names locally.
- Corrected the read-only description: QLab saves the workspace before archiving its directory; SLink-Rack creates an export job. Avantis only downloads stored Shows.
- Added explicit app icons and bundled project/third-party license notices. Screenshot assets show the actual packaged desktop app and contain no account names, IP addresses or personal file paths.

## Remaining limits

This is a publication/privacy review and targeted security review, not an independent penetration test. Secret scanners and manual review cannot prove the absence of every issue.

The DMG is an Apple silicon preview build, ad-hoc signed and not Apple-notarized. macOS may require approval through Privacy & Security on first launch. No trusted Developer ID distribution certificate was available.

Avantis still needs physical-console validation, and remote QLab transfers need a second Mac. Mic-Wise and SLink-Rack use unauthenticated LAN HTTP APIs; QLab OSC is also not encrypted. Use the app on a trusted production network. QLab archives its workspace directory; externally referenced media is not collected. HTTP download failures may leave an incomplete archive file; check the app's success result before relying on a backup.

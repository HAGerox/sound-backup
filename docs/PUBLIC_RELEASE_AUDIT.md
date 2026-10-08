# Public release audit — Sound Backup 0.3.0

First reviewed 30 September 2026 for 0.1.0, re-reviewed 7 October 2026 for 0.2.0 and 8 October 2026 for 0.3.0, before publishing the source and macOS DMG.

## What changed in 0.3.0

- **Avantis Show download now works on a real console.** The file-transfer function numbers taken from Director were swapped: the console sends the header in `0x0112` and the remaining packets in `0x0113`. Verified against a physical Avantis by listing its User Shows and downloading single and multiple Shows. Each archive passed a gzip and tar check. The mock console now matches the captured traffic.
- **Newer Show prompt.** If a selected Show ends in a version such as `v11` and the console has a higher version of the same name, Sound Backup asks before backing up. A declined version is saved in settings (`declinedNewerShows`, Show names only) so it isn't offered again.
- **QLab without Bonjour.** Discovery also probes the local IPv4 subnet on TCP 53000 and lists a host only after it answers OSC `/workspaces`; Settings → QLab can connect by IP address.
- **Clearer remote QLab error.** When macOS privacy protection on the QLab Mac blocks Remote Login access to the project folder, the error explains the Remote Login full-disk-access setting.

Re-ran the checks for 0.3.0. Gitleaks found no leaks in Git history. In the working files it flagged only the nine known `muda` `.rmeta` documentation examples in ignored build output. TruffleHog reported zero findings in Git history. `npm audit` reported zero vulnerabilities, and `cargo audit` reported the same seven allowed upstream warnings. The app bundle's ad-hoc signature and the DMG checksum were verified. The executable contains no `/Users/` paths, and its only Gitleaks match is the known libssh2 error text. New test fixtures use Show naming patterns from a real console with personal names removed.

## What changed in 0.2.0

0.2.0 is a bug-fix release on top of 0.1.0. The review below focused on the new code:

- **Avantis discovery** now uses Allen & Heath's UDP unit finder — the same broadcast Avantis Director sends — on UDP 51320 from each active IPv4 interface, instead of sweeping TCP 51321. Only units that answer the finder with a name are listed; a saved address is still checked with an AH-Net connection. The old TCP sweep was unreliable: its 90 ms connect timeout failed on Wi-Fi, and unrelated devices accept that port.
- **Avantis Show list** now reads Director's User Show location (`1`) rather than the internal working location (`4`), which had hidden every stored Show. Only names and locations reported by the console are read; Show data is not parsed.
- **Settings can remove any saved connection.** Removing a remote QLab login deletes its macOS Keychain password through a new `forget_remote_access` command; a missing Keychain entry is not an error.
- **Avantis protocol trace** (connection steps, message headers and Show names — never Show data) is written to `~/Library/Logs/uk.stagebackup.desktop/avantis.log`, capped at 512 KiB with a single rotated `avantis.previous.log`. Nothing is sent off the machine.
- **Error messages** now surface the underlying connection or protocol failure instead of a generic "try again".

Re-ran the checks for 0.2.0: Gitleaks scanned all Git history and the publishable working files with no leaks; TruffleHog scanned Git history with verification disabled and reported zero verified or unverified secrets; `npm audit` reported zero vulnerabilities, and `cargo audit` reported the same allowed upstream maintenance warnings as 0.1.0. The full Rust workspace and frontend regression tests pass, with the one QLab test skipped because it needs an open QLab workspace.

## Scope and checks

- Gitleaks scanned all local Git history, including checkpoint refs, with no secrets found. TruffleHog independently scanned Git history with verification disabled and reported zero verified or unverified secrets.
- Gitleaks separately scanned the publishable working files, including new providers and the Avantis discovery module. No secrets found. A broad scan of ignored build output flagged nine copies of a `muda` Rust metadata documentation example (the count grows with build artifacts); these are not credentials and `.rmeta` files are not distributed.
- Manually reviewed credentials, settings serialization, SSH commands and host identity verification, OSC/HTTP/AH-Net traffic, discovery, deep links, frontend rendering, build inputs, documentation, and the tracked file inventory. Inspected historical URL, email, and absolute-path references.
- npm audit reported zero known vulnerabilities. Cargo audit reported zero vulnerabilities. The lockfile retains upstream Unicode maintenance warnings through Tauri. GLib unsoundness and proc-macro-error maintenance advisories concern platform dependencies absent from the macOS dependency tree. Updated the yanked libssh2-sys 0.3.2 to 0.3.3.
- Ran Rust workspace tests and frontend regression tests. One Rust test is intentionally skipped because it requires an open QLab workspace.
- Inspected the app bundle and executable, verified the ad-hoc signature, and verified the DMG. Gitleaks also scanned extracted executable strings; its sole match was the upstream libssh2 error text `invalid/unrecognized private key file format`, not a credential. Checked the website route, assets, download link, and layout at desktop and mobile widths.

## Findings and changes

- Failed Remote Login attempts previously cleared the saved SSH host fingerprint. Retaining it prevents a retry from silently trusting a changed remote host. Added regression coverage for failure and successful connection.
- Removed the developer's name from current test fixtures. Older commits retain harmless examples (`/Users/finn/Show` and `finn@qlab-mac.local`); these are test data, not credentials or production account configuration.
- Rust panic locations and vendored OpenSSL paths included the builder's home directory. The packaging command remaps Rust paths and builds native dependencies outside the home directory; the distributed executable was checked for `/Users/` paths.
- The app stores Remote Login passwords and QLab OSC passcodes in macOS Keychain. Saved settings contain endpoints, usernames, workspace names, host fingerprints and the chosen backup folder, but no passwords. User settings and Keychain contents are not packaging inputs.
- No analytics, telemetry, cloud upload endpoints, private show archives, audio recordings, or Allen & Heath binaries are included. Network traffic goes to discovered or configured systems. Avantis trace logging writes endpoint and Show names to a local log file under `~/Library/Logs`. Nothing is sent off the machine.
- Corrected the read-only description: QLab saves the workspace before archiving its directory; SLink-Rack creates an export job. Avantis only downloads stored Shows.
- Added explicit app icons and bundled project/third-party license notices. Screenshot assets show the actual packaged desktop app and contain no account names, IP addresses or personal file paths.

## Remaining limits

This is a publication/privacy review and targeted security review, not an independent penetration test. Secret scanners and manual review cannot prove the absence of every issue.

The DMG is an Apple silicon preview build, ad-hoc signed and not Apple-notarized. macOS may require approval through Privacy & Security on first launch. No trusted Developer ID distribution certificate was available.

Avantis Show listing and download are verified on one physical console; the lost-comms keepalive and AH-Net v2 negotiation are not (that console used v1). Remote QLab has connected to a second Mac, but its first transfer was blocked by macOS privacy protection on the project folder, so a complete remote transfer is still unverified. Mic-Wise and SLink-Rack use unauthenticated LAN HTTP APIs; QLab OSC is also not encrypted. Use the app on a trusted production network. QLab archives its workspace directory; externally referenced media is not collected. HTTP download failures may leave an incomplete archive file; check the app's success result before relying on a backup.

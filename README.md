# Stage Backup

A small macOS-first Tauri app for one-click backups of Allen & Heath Avantis and QLab.

The current Avantis provider:

1. Searches active local IPv4 networks for Avantis consoles and verifies them over AH-Net.
2. Reads the console's stored **User Show** catalogue without recalling a Show.
3. Lets the user search for and select one or more Shows.
4. Downloads the selected Shows in one console session.
5. Saves the received bytes unchanged under:

   `AllenHeath-Avantis/Shows/<Show>_YYYY-MM-DD_HH-mm-ssZ.tar.gz`

The `AllenHeath-Avantis` folder can then be placed at the root of a USB drive for Avantis Show access. The timestamp uses UTC and contains no `:` or filesystem separators.

## Important behaviour

- The app is read-only with respect to the console: it does not recall, store, overwrite, rename, or delete a Show.
- It backs up the **stored Show file**, not uncommitted live desk state. Store/Overwrite on the Avantis first if current changes need to be in the backup.
- Show archive bytes are not decompressed, recompressed, converted, or otherwise re-encoded.
- Output filenames begin with a UTC timestamp and are restricted to simple ASCII characters.
- Network discovery scans the active local subnet (wide networks are limited to the computer's local `/24`) and only accepts devices that expose the Avantis Show File Manager. A manual address remains available as a fallback.
- The home screen is deliberately limited to connection status, one backup button, progress, and completion feedback. Backup location, console selection, and Show selection live in Settings.
- QLab is discovered through Bonjour. Stage Backup automatically includes its open projects, tells each one to save, archives the directory containing the workspace, and transfers the resulting zip. Media referenced from outside that directory is not collected like QLab's native Bundle Workspace feature.
- Local QLab instances do not require Remote Login. For another Mac, turn on **System Settings → General → Sharing → Remote Login**. No Stage Backup agent is installed on that Mac.
- Remote Login passwords and QLab OSC passcodes are stored in macOS Keychain. They are never written to `settings.json`.
- Remote Mac identity fingerprints are pinned after the first successful connection. A changed fingerprint stops the backup and asks the user to set the Mac up again.

## Run on macOS

Requirements:

- macOS 11 or later
- Xcode Command Line Tools
- Rust stable
- Node.js 20 or later

```sh
npm install
npm run tauri dev
```

Build an application bundle / DMG:

```sh
npm run tauri build
```

## Tests

The Avantis network/protocol implementation lives in the dependency-free `avantis-protocol` workspace crate. Its integration tests run simulated AH-Net consoles and cover catalogue filtering, native-location preference, single-Show backup, multi-Show backup in one session, and USB-layout output:

```sh
cargo test -p avantis-protocol
```

Run the complete test suite:

```sh
cargo test --workspace
```

The ignored QLab app integration test can be run while a local QLab workspace is open:

```sh
cargo test -p stage-backup discovers_local_qlab_by_open_workspace_name -- --ignored
```

The static frontend can be built without npm dependencies:

```sh
node scripts/build.mjs
```

## Hardware validation status

The Avantis implementation was derived for interoperability from the user-supplied Avantis Director V2.01 application. The protocol path and file layout have unit/integration coverage, including a mock console, but this environment does not contain a physical Avantis. The first real-console run should therefore be treated as hardware validation, particularly for AH-Net routing details that are not publicly documented by Allen & Heath.

The local QLab integration test has queried a real open QLab 5 workspace over OSC/TCP. Remote Login authentication, transfer, and host-key pinning are covered structurally but still need validation against a second Mac on the target production network.

Set `STAGE_BACKUP_TRACE=1` before launching from a terminal to print protocol-level diagnostics if a real console behaves differently. Automatic discovery also depends on the Mac and Avantis being reachable on the same local IPv4 network; routed or firewalled networks may require the manual address fallback.

## Repository layout

```text
crates/avantis-protocol/   AH-Net, Show catalogue and file-transfer logic
src-tauri/                 Tauri shell, saved settings and provider boundary
src-web/                   Minimal HTML/CSS/JS UI
scripts/                   Dependency-free frontend build/dev server
docs/                      Interoperability notes
```

No Allen & Heath binaries or Show content are included in this repository.

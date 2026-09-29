# Sound Backup

[Download for macOS](https://apps.finnstanley.com/sound-backup/) — Apple silicon, macOS 11+. Version 0.1.0 is an unsigned preview; see the release notes for first-launch instructions and hardware validation limits.

A small macOS-first Tauri app for one-click backups of live-production systems: Allen & Heath Avantis, QLab, Mic-Wise and SLink-Rack.

One **Back up now** button archives every connected system into a single backup folder:

```text
<backup folder>/
├── AllenHeath-Avantis/Shows/<UTC ts>_<Show>.tar.gz
├── QLab/<UTC ts>_<Project>.zip
├── Mic-Wise/<UTC ts>_<Show>.micwise.zip
└── SLink-Rack/<UTC ts>_<Instance>.zip
```

Everything is discovered automatically over the local network (Bonjour where the system advertises it, a bounded subnet probe where it does not), with a manual address as fallback. See [docs/INTEGRATION.md](docs/INTEGRATION.md) for the contract.

## The Avantis provider

1. Searches active local IPv4 networks for Avantis consoles and verifies them over AH-Net.
2. Reads the console's stored **User Show** catalogue without recalling a Show.
3. Lets the user search for and select one or more Shows.
4. Downloads the selected Shows in one console session.
5. Saves the received bytes unchanged under:

   `AllenHeath-Avantis/Shows/YYYY-MM-DD_HH-mm-ssZ_<Show>.tar.gz`

The `AllenHeath-Avantis` folder can then be placed at the root of a USB drive for Avantis Show access. The timestamp uses UTC and contains no `:` or filesystem separators.

## Important behaviour

- There is no restore path. Avantis downloads stored Shows without recalling or storing console state. QLab is asked to save its open workspace before archiving; SLink-Rack creates a backup export job.
- Avantis: it backs up the **stored Show file**, not uncommitted live desk state. Store/Overwrite on the Avantis first if current changes need to be in the backup.
- Show archive bytes are not decompressed, recompressed, converted, or otherwise re-encoded.
- Output filenames begin with a UTC timestamp and are restricted to simple ASCII characters.
- Network discovery scans the active local subnet (wide networks are limited to the computer's local `/24`) and only accepts devices that identify themselves correctly. A manual address remains available as a fallback.
- The home screen is deliberately limited to connection status, one backup button, progress, and completion feedback. Backup location and per-system selection live in Settings.
- QLab is discovered through Bonjour. Sound Backup automatically includes its open projects, tells each one to save, archives the directory containing the workspace, and transfers the resulting zip. Media referenced from outside that directory is not collected like QLab's native Bundle Workspace feature.
- Local QLab instances do not require Remote Login. For another Mac, turn on **System Settings → General → Sharing → Remote Login**. No Sound Backup agent is installed on that Mac.
- Remote Login passwords and QLab OSC passcodes are stored in macOS Keychain. They are never written to `settings.json`.
- Remote Mac identity fingerprints are pinned after the first successful connection. A changed fingerprint stops the backup and asks the user to set the Mac up again.

## Mic-Wise and SLink-Rack

Both are same-LAN HTTP services and are found automatically.

- **Mic-Wise** advertises `_micwise._tcp` and answers `GET /api/health`. Sound Backup pulls `GET /api/showfile/export?format=archive`, a self-contained show archive carrying the showfile, every channel photo, and a SHA-256 manifest. Restoring that archive into any Mic-Wise session reproduces the show exactly.
- **SLink-Rack** advertises `_slink-rack._tcp` and answers `GET /api/identity`. Rack exports run to about a gigabyte, so Sound Backup starts a job (`POST /api/backups`), watches it, and downloads the finished archive. Progress is shown in the app while it runs.

Each app's own **Back up now** link uses the `stage-backup://` URL scheme to trigger a backup of that system from the machine running Sound Backup. See [docs/INTEGRATION.md](docs/INTEGRATION.md).

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
npm run package:macos
```

## Tests

The Avantis network/protocol implementation lives in the dependency-free `avantis-protocol` workspace crate. Its integration tests run simulated AH-Net consoles and cover catalogue filtering, native-location preference, single-Show backup, multi-Show backup in one session, and USB-layout output:

```sh
cargo test -p avantis-protocol
```

Provider and settings unit tests cover the shared subnet scanner, mDNS resolution, endpoint normalisation, output naming, and settings migration:

```sh
cargo test --workspace
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

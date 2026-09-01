# Stage Backup

A small macOS-first Tauri app for backing up a stored Allen & Heath Avantis Show over the network.

Version 0.1 does one job:

1. Connect to an Avantis by address.
2. Find one configured **stored User Show** without recalling it.
3. Ask the console to send that Show archive over AH-Net.
4. Save the received bytes unchanged under:

   `AllenHeath-Avantis/Shows/<Show>_YYYY-MM-DD_HH-mm-ssZ.tar.gz`

The `AllenHeath-Avantis` folder can then be placed at the root of a USB drive for Avantis Show access. The timestamp uses UTC and contains no `:` or filesystem separators.

## Important behaviour

- The app is read-only with respect to the console: it does not recall, store, overwrite, rename, or delete a Show.
- It backs up the **stored Show file**, not uncommitted live desk state. Store/Overwrite on the Avantis first if current changes need to be in the backup.
- Show archive bytes are not decompressed, recompressed, converted, or otherwise re-encoded.
- Output filenames are restricted to simple ASCII characters plus the timestamp.
- Version 0.1 backs up one configured Show. The provider boundary is intentionally small so later versions can add all-Shows backup and other systems such as QLab without complicating this screen.

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

The network/protocol implementation lives in the dependency-free `avantis-protocol` workspace crate. Its integration test runs a simulated AH-Net Avantis and checks the complete path from handshake through selected Show download and USB-layout output:

```sh
cargo test -p avantis-protocol
```

The static frontend can be built without npm dependencies:

```sh
node scripts/build.mjs
```

## Hardware validation status

The implementation was derived for interoperability from the user-supplied Avantis Director V2.01 application. The protocol path and file layout have unit/integration coverage, including a mock console, but this environment does not contain a physical Avantis. The first real-console run should therefore be treated as hardware validation, particularly for AH-Net routing details that are not publicly documented by Allen & Heath.

Set `STAGE_BACKUP_TRACE=1` before launching from a terminal to print protocol-level diagnostics if a real console behaves differently.

## Repository layout

```text
crates/avantis-protocol/   AH-Net, Show catalogue and file-transfer logic
src-tauri/                 Tauri shell, saved settings and provider boundary
src-web/                   Minimal HTML/CSS/JS UI
scripts/                   Dependency-free frontend build/dev server
docs/                      Interoperability notes
```

No Allen & Heath binaries or Show content are included in this repository.

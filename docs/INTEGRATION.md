# Integrating with Sound Backup

Sound Backup is the one-click backup hub. It discovers live-production systems
on the local network and files each one into a single backup folder. This
document is the contract any system needs to meet to be backed up.

## Output layout

Everything lands under the configured backup folder using a UTC timestamp and a
sanitised name (ASCII alphanumerics, space, `-`, `_`):

```text
<backup folder>/
├── AllenHeath-Avantis/Shows/<UTC ts>_<Show>.tar.gz    # USB-drive layout
├── QLab/<UTC ts>_<Project>.zip
├── Mic-Wise/<UTC ts>_<Show>.micwise.zip
└── SLink-Rack/<UTC ts>_<Instance>.zip
```

`providers/util.rs` (`dated_path`, `safe_stem`) produces these names, so a second
writer can match them exactly.

## Discovery

Two mechanisms, merged and de-duplicated:

1. **mDNS** (`providers/mdns.rs`) — browse the service type for 2 s, resolve a
   preferred IPv4 (skipping link-local/loopback), and read TXT records.
2. **Subnet probe** (`providers/net_scan.rs`) — sweep the local IPv4 subnets
   (`/24` clamped, max 1022 hosts, 32 workers, 90 ms connect) on the service's
   HTTP port and identify whatever answers.

A manually entered endpoint is always merged in as a fallback for routed or
firewalled networks.

| System | mDNS service type | Probe port | Identify with |
| --- | --- | --- | --- |
| Avantis | — (Director advertises nothing) | `51321` | AH-Net handshake + Show File Manager |
| QLab | `_qlab._tcp.local.` | `53000` | OSC `/workspaces` |
| Mic-Wise | `_micwise._tcp.local.` | `8000` | `GET /api/health` |
| SLink-Rack | `_slink-rack._tcp.local.` | `8080` | `GET /api/identity` |

## HTTP service contract (Mic-Wise, SLink-Rack)

Both are plain-HTTP LAN services with no authentication. Sound Backup's client
(`providers/http_client.rs`) does not follow redirects and uses bounded timeouts.

### `GET /api/health` — Mic-Wise

```json
{"app": "micwise", "status": "ok", "version": "…", "show_name": "Sunday",
 "show_filename": "default.micwise", "audio_engine_running": true}
```

`app` is optional but must be `"micwise"` when present. `show_name` names the
resulting archive.

### `GET /api/identity` — SLink-Rack

```json
{"app": "slink-rack", "version": "…", "instance": "front-of-house",
 "hardware_verified": false, "racks": ["main", "…"], "api": 1,
 "discovery": {"service_type": "_slink-rack._tcp.local.", "advertising": true,
               "available": true, "advertised_on_lan": true}}
```

`instance` names the resulting archive. This route must answer even while a
restore holds the service's data lock, so discovery survives maintenance.

### Pulling an archive

| System | Call | Notes |
| --- | --- | --- |
| Mic-Wise | `GET /api/showfile/export?format=archive` | Streams a complete `.micwise.zip`. A `200` with zero bytes is rejected. |
| SLink-Rack | `POST /api/backups` → `GET /api/backups` → `GET /api/backups/{id}/download` | Job-based because exports run to ~1 GB. Poll until `status` is `complete` or `failed`, using `message`/`bytes` for progress. |

### mDNS TXT records

| Key | Meaning |
| --- | --- |
| `app` | `"micwise"` or `"slink-rack"` |
| `version` | Service version string |
| `show` / `showFilename` | Mic-Wise show name and file |
| `hardwareVerified` | `"1"`/`"0"` — SLink-Rack's hardware gate |
| `path` | HTTP base path (`/`) |

## One-click trigger: `stage-backup://`

Registering a **Back up now** link is the in-app affordance. The scheme is owned
by Sound Backup (`tauri-plugin-deep-link`):

```text
stage-backup://backup?target=micwise&host=10.0.0.4&port=8000
stage-backup://backup?target=slink-rack&host=192.168.130.18&port=8080
stage-backup://backup?target=all
```

| Parameter | Meaning |
| --- | --- |
| `target` | `micwise`, `slink-rack`, `slinkrack`, `avantis`, `qlab`, or `all` |
| `host`, `port` | Hints for the caller's own endpoint (informational) |
| `show` | Optional show/instance name (informational) |

Sound Backup brings its window to the front and runs the normal backup flow for
the requested target. This is a **same-machine** affordance — the scheme cannot
reach another machine's Sound Backup. Across the network, use the hub's own
**Back up now** button, which pulls from every discovered system.

## Backup-only

Sound Backup has no restore path. Avantis downloads stored Shows without
recalling or storing console state. QLab saves the open workspace before
archiving its project folder; SLink-Rack creates an export job. Mic-Wise restores its own show archives, SLink-Rack
restores its own rack archives, and Avantis/QLab restores stay with the console
and QLab.

## Adding a provider

1. `src-tauri/src/providers/<name>.rs` exposing `test`, `discover`, `backup`.
2. Register commands in `src-tauri/src/main.rs` and add a `DeviceSettings` section.
3. Add a home indicator and a settings card in `src-web/index.html`, wire them in
   `src-web/app.js` (`PROVIDERS`, `configuredProviders`, `checkConnections`,
   `runBackup`, `configurationIssue`, `guideTo`).
4. Emit `backup://progress` `{provider, detail}` from the backup command so the
   progress panel shows real status.
5. Document the contract here.

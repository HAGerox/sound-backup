# Avantis interoperability notes

These notes document the small subset of Director V2.01 behaviour used by Sound Backup. They are implementation notes, not an Allen & Heath protocol specification.

## Connection

- AH-Net rendezvous: TCP 51321.
- Base utility hello advertises the client's UDP port.
- Director then attempts the AH-Net v2 hello/ack exchange; the implementation falls back to v1 if that negotiation is not answered.
- Sound Backup listens for NET traffic on both the TCP connection and the negotiated UDP pair. Requests are sent on TCP; file acknowledgements are returned on the transport that delivered the packet.

## Local discovery

Director V2.01 finds consoles with Allen & Heath's UDP unit finder (`cUnitFinder`), not Bonjour:

- From an ephemeral socket bound to each active IPv4 interface, it broadcasts to the subnet on UDP 51320.
- Console finder requests are `TLD Find\0V2\0` and `Squirrel Find\0V2\0`. Director appends `\0V2\0` to every non-legacy finder string.
- A unit replies to the sending socket with its unit name as a NUL-terminated ASCII string. The datagram source is the console address.
- Director repeats the broadcast on a timer. It also binds UDP 51319 for its multi-unit finder.

Sound Backup sends the same two requests on every active IPv4 interface for about 1.6 seconds and lists each unit that replies, by name. A saved address also receives a unicast finder request. If it does not answer, it is checked with an AH-Net connection, so routed networks still work through the manual address.

An earlier build found consoles by sweeping TCP 51321. That was unreliable: the 90 ms connect timeout was too short on Wi-Fi, and unrelated devices accept that port (an Extron CrossPoint, for example, accepts almost any TCP port).

## Object discovery

Director addresses remote objects as `<host>/<object>`. Its registry router (`cRegistryRouter::FindObject`) splits off the host, selects that connection, and sends function `4` to target `0` (the remote registry) with only the object name. The reply is function `2` with the handle: 4 bytes on AH-Net v2 connections and 2 bytes on v1. Function `3` means not found. `cShowManagerClient` looks up `Show File Manager`, and Sound Backup sends the same request. Receivers replace the connection field with their own connection index, so the sender's value is not significant.

## Show catalogue

The Show File Manager is sent function `0x0100` (sync). Director handles catalogue additions as function `0x1001`. Each payload begins with a 42-byte `sShowKey` serialization:

```text
0x00  17 bytes  Show name, single-byte, NUL terminated (max 16)
0x11   1 byte   Show location
0x12   2 bytes  key value, big endian
0x14  21 bytes  auxiliary string, NUL terminated (max 20)
0x29   1 byte   legacy flag
```

Director's `GetShowDirectory(eShowLocation)` maps locations to storage:

| Location | Storage |
| --- | --- |
| `0` | Factory Shows |
| `1` | User Shows |
| `2` | USB |
| `4` | Internal working directory |

Director's console Show list (`cShowManagerFormBackend::RefreshSBCShowsList`) shows locations `1` and `0`, and its USB list shows `2`. Sound Backup lists only location `1`. An earlier build treated `4` as User Shows and discarded `1`, which hid every stored Show.

The server side, `cShowFileManager`, is also in Director. Its dispatch table confirms `0x100` = `Sync(connection, source)`, `0x118` = `UploadShowToConnId`, and `0x119` = `StoreShowAndUploadToConnId`. `Sync` answers with one `0x1001` message per Show, written by `sShowKey::PutShowKeyInMessage` in the layout above.

## Direct stored-Show download

Director contains a direct `UploadShowToConnId` path. Sound Backup uses Show File Manager function `0x0118` with the selected Show key. It does **not** use `0x0119`, which stores current state before upload.

This distinction is why Sound Backup can download a stored Show without recalling it and without changing live console state.

Multiple selected Shows are downloaded sequentially through one AH-Net session. The catalogue is read once, requested names are matched case-insensitively and deduplicated, and each console key is reused for its corresponding `0x0118` request.

## File transfer

The Show sender uses these functions, confirmed against a real Avantis on 2026-10-08 (an earlier build had `0x0112` and `0x0113` swapped and failed with "Show data before its file header"):

- `0x0112`: header / first payload
- `0x0113`: subsequent payload
- `0x0002`: receiver ACK (the console waits for it before each next packet)
- `0x0003`: receiver error

The sender is a new object for each transfer (for example `0x1CBE`, then `0x1CBF`), so replies go to the message's source rather than the Show File Manager.

The `0x0112` payload begins:

```text
0x00  u16 BE  header length
0x02  u16 BE  total packet count
0x04  u32 BE  file size
0x08  char[]  basename, NUL terminated
...            first archive bytes, beginning at header length
```

Subsequent packets contain raw archive bytes. Director's file-transfer packet size is capped at 40,000 bytes. Sound Backup writes each payload directly to disk and checks the declared byte/packet counts before finalising it.

## Archive and USB layout

Director resources contain factory Show archives as `.tar.gz` files whose archive tree begins with `Show/`. Director's file-location strings also expose the removable-media layout:

```text
AllenHeath-Avantis/
└── Shows/
```

Sound Backup therefore writes the exact received archive into that folder structure. It checks the gzip signature but does not unpack or repack the Show.

## Deliberate scope

The Avantis protocol provider does not implement:

- remote Store / Overwrite
- remote Recall
- Show deletion or rename
- Scene or Library transfer

Other systems belong in separate providers rather than broadening the Avantis protocol path.

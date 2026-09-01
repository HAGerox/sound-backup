# Avantis interoperability notes

These notes document the small subset of Director V2.01 behaviour used by Stage Backup. They are implementation notes, not an Allen & Heath protocol specification.

## Connection

- AH-Net rendezvous: TCP 51321.
- Base utility hello advertises the client's UDP port.
- Director then attempts the AH-Net v2 hello/ack exchange; the implementation falls back to v1 if that negotiation is not answered.
- Stage Backup listens for NET traffic on both the TCP connection and the negotiated UDP pair. Requests are sent on TCP; file acknowledgements are returned on the transport that delivered the packet.

## Local discovery

Director V2.01 connects to an explicitly supplied host and does not expose Bonjour/mDNS discovery. Stage Backup therefore scans the Mac's active local IPv4 subnets for TCP 51321, then performs the normal AH-Net handshake and `Show File Manager` lookup before presenting a device. Wide networks are intentionally limited to the Mac's local `/24`; a manually entered address remains available for routed networks.

## Object discovery

The remote registry is queried for `Show File Manager`. The returned object handle is then used directly. This avoids emulating the full Director Show Manager UI or reciprocal Show Manager discovery.

## Show catalogue

The Show File Manager is sent function `0x0100` (sync). Director handles catalogue additions as function `0x1001`. Each payload begins with a 42-byte `sShowKey` serialization:

```text
0x00  17 bytes  Show name, single-byte, NUL terminated (max 16)
0x11   1 byte   Show location
0x12   2 bytes  key value, big endian
0x14  21 bytes  auxiliary string, NUL terminated (max 20)
0x29   1 byte   legacy flag
```

Director's User Show storage maps to location `4`. Stage Backup prefers that location, ignores Factory/USB locations (`0`/`1`), and retains a non-factory compatibility fallback.

## Direct stored-Show download

Director contains a direct `UploadShowToConnId` path. Stage Backup uses Show File Manager function `0x0118` with the selected Show key. It does **not** use `0x0119`, which stores current state before upload.

This distinction is why Stage Backup can download a stored Show without recalling it and without changing live console state.

Multiple selected Shows are downloaded sequentially through one AH-Net session. The catalogue is read once, requested names are matched case-insensitively and deduplicated, and each console key is reused for its corresponding `0x0118` request.

## File transfer

The Show sender uses the Director dual-transfer functions observed as:

- `0x0113`: header / first payload
- `0x0112`: subsequent payload
- `0x0002`: receiver ACK
- `0x0003`: receiver error

The `0x0113` payload begins:

```text
0x00  u16 BE  header length
0x02  u16 BE  total packet count
0x04  u32 BE  file size
0x08  char[]  basename, NUL terminated
...            first archive bytes, beginning at header length
```

Subsequent packets contain raw archive bytes. Director's file-transfer packet size is capped at 40,000 bytes. Stage Backup writes each payload directly to disk and checks the declared byte/packet counts before finalising it.

## Archive and USB layout

Director resources contain factory Show archives as `.tar.gz` files whose archive tree begins with `Show/`. Director's file-location strings also expose the removable-media layout:

```text
AllenHeath-Avantis/
└── Shows/
```

Stage Backup therefore writes the exact received archive into that folder structure. It checks the gzip signature but does not unpack or repack the Show.

## Deliberate scope

The current implementation does not implement:

- remote Store / Overwrite
- remote Recall
- Show deletion or rename
- Scene or Library transfer
- R1 backup
- QLab backup

Those should be added as separate provider/service operations rather than broadening the Avantis protocol path.

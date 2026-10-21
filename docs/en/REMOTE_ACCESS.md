# Remote Access and Devices

Pair a phone or another computer with Harness Studio, chat from a phone browser, point OpenCode at a remote Gateway with a scoped token, and see every paired device on one page (Connections > Devices).

Everything here goes direct between your devices. StrukLabs is only ever identity, discovery and signaling; it is never an inference relay (see [StrukLabs Signaling](#struklabs-signaling-interface-only)).

## Pairing

1. **Create a code.** On the host open Connections > Devices > Pair a Device. Choose the role, the access and (optionally) the allowed models, then press Create Pairing Code. You get an 8-digit code (`1234-5678`) and a link to open on the phone. The code works once and expires after 5 minutes (at most 15).
2. **Claim it.** On the phone open the link (or `http://<host>:<gateway port>/m` and type the code). The device sends the code, its name and a random nonce to the gateway. A wrong code is refused; five wrong codes in a minute lock claims for the rest of that minute. A right code is consumed. No token is issued yet.
3. **Verify and approve.** Both screens show the same 6-digit verification code. The host lists the request under Devices Waiting for Approval; press Approve only when the device name and the code match. Deny, or ignoring it for 5 minutes, drops the request and issues nothing.
4. **Receive the token.** After approval the device's next poll returns the scoped token exactly once. The host stores only a salted hash of it (the normal named-token store), like any other named token.

Every step (offer created, failed claim, request, approval, denial, revocation, access change) is written to the audit log (`pairing_audit.jsonl` in the config directory; the Devices page shows the newest 100 entries).

### Access presets

| Preset | Control API | Gateway |
|---|---|---|
| Chat Only | none | chat completions and the model list |
| Gateway Models | none | every gateway endpoint except metrics (embeddings, audio, images, pipelines...) |
| Admin | everything (`admin`) | everything |

Chat Only and Gateway Models can be narrowed with allowed model patterns (`qwen3-*, coder`). Admin needs an explicit acknowledgement when the code is created and again when the access of an existing device is raised to it. Rate limits can still be set per token under Settings > Developer. The preset is a normal named token (`device: <name>`): changing the access of a device changes that token's scopes and policy for its next request.

### Revocation and a lost phone

- **Unpair** removes one device; its token returns 401 on the next request.
- **Remove All Devices** removes every paired device in one step, cancels open codes and pending requests, and leaves tokens you made by hand alone.
- Deleting a device's token in Settings > Developer also removes the device from the list.

### Threat model and limits

- The channel is as private as the connection it runs over. Over plain HTTP on a trusted LAN the code, the verification code and the token are visible to someone who can sniff that network. Outside a trusted LAN put the gateway behind HTTPS (a reverse proxy, Tailscale, or a TLS-terminating tunnel) and use the `https://` address. Certificate pinning and a PAKE (SPAKE2) are not implemented; the protection is the single-use short-lived code, the lockout, the verification code and the owner's explicit approval before any token exists.
- The verification code is computed on the host from the code, both nonces and the claim id. It protects against a stranger who guessed or saw a code racing the real device: the owner sees a name and code that do not belong to the device in their hand and denies it.
- The claim and poll routes are public by design (the device has no token yet). They only ever act on a live offer and are rate-limited by the lockout.
- The phone page is static, sets a restrictive Content-Security-Policy and never loads outside resources. It keeps the token in the browser's local storage; Unpair on the page, or removing the device on the host, ends access.
- A QR code is not drawn yet; the link and the short code carry the same information.

## Chat from a Phone

The Gateway serves a mobile-friendly chat page at `GET /m`. It pairs (code from the link or typed), shows the allowed models, and chats over `/v1/chat/completions` with streaming, using the token it received. The gateway must be on and reachable from the phone: in Settings turn on network access for the gateway (a network bind always requires a token). The page works in English and Turkish following the phone's language.

## Devices Page

For every paired device: role (Inference Host, Client, Storage), reachability (online when it spoke to the gateway in the last 2 minutes), last seen, permissions, and what it reported about itself: models, engines, disk, memory, workload and its address. Devices report with their own token:

```
POST /v1/harness/device/report        (gateway; Authorization: Bearer <device token>)
{ "models": [...], "engines": [...], "components": ["runtime", "gateway", "models"],
  "disk_total_bytes": 0, "disk_free_bytes": 0, "ram_total_bytes": 0,
  "vram_total_bytes": 0, "vram_free_bytes": 0,
  "running_servers": 0, "queued_requests": 0, "address": "http://192.168.1.20:11435" }
```

Reports are informational and never grant access. Any request a paired device makes to the gateway also counts as presence.

### Roles and components

| Role | Needs |
|---|---|
| Inference Host | model engine, gateway, model store |
| Client | chat client |
| Storage | model storage |

A device only gets the components its role needs; the card lists what is still missing for its role.

### Where Should This Model Run?

Model Placement takes the memory a model needs (`GET /devices/suggest?required_bytes=...`). If this computer's memory (unified budget, else the largest GPU, else RAM) is enough it says so; otherwise it lists paired Inference Hosts that reported enough free VRAM (or RAM), online first, with the components each still needs.

## Remote OpenCode and Other Clients

On the computer that should use another host:

1. Connections > Devices > Connect to Another Harness: enter the host's address, the code from its Devices page, a name for this computer and a link name. Compare the verification code with the host's prompt and wait for approval.
2. The approved token is saved as a remote link (system keychain; see Remote Models). The host's models appear in the local gateway as `remote:<link>/<model>`, so OpenCode and every other tool using the local gateway can reach them.
3. To let OpenCode call the remote Gateway directly instead, add the shown block to `opencode.json` and export the token as `HARNESS_REMOTE_TOKEN`; the token is never written into the file.

Create the code with the Gateway Models preset and the models you want to share so the token cannot reach anything else.

## Control API and Gateway Routes

Control API (all `admin`): `POST/GET /pairing/offers`, `DELETE /pairing/offers/{id}`, `GET /pairing/pending`, `POST /pairing/pending/{id}/approve|deny`, `POST /pairing/connect`, `POST /pairing/connect/poll`, `GET /devices`, `PATCH|DELETE /devices/{id}`, `POST /devices/revoke-all`, `GET /devices/audit`, `GET /devices/roles`, `GET /devices/suggest`.

Gateway (public unless noted): `GET /m`, `POST /v1/harness/pair/claim`, `GET /v1/harness/pair/claim/{id}` (header `x-harness-pair-secret`), `POST /v1/harness/device/report` (device token).

## StrukLabs Signaling (Interface Only)

`harness_core::pairing::SignalingProvider` is the interface a future StrukLabs service can implement: `announce` a device (name, role, address) and `discover` peers, so a code can be exchanged across networks. The default `LocalOnlySignaling` does nothing: pairing happens by code on an address you can reach. The interface has no relay method on purpose; inference traffic always goes straight between the paired devices.

### Planned Relay

For devices that cannot reach each other directly, the owner plans to run the signaling and optional relay service on an existing Oracle Cloud Free Tier server; everything is free for now. Nothing in this release deploys or depends on it, direct connections stay the default, and no deploy scripts ship here.

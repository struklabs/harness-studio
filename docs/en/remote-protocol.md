# Remote App Protocol

Remote access is an opt-in preview, compiled with the `remote-access` feature and
disabled by default. Settings → Server Defaults → Advanced offers **Remote Access
via StrukLabs**. The account sign-in/ticket endpoint is not deployed yet; the
desktop uses a `TicketProvider` interface rather than a placeholder account URL.
The browser reference is `struklabs-web/packages/app-protocol`. Its executable
schemas include commit/reveal and a 30-second resume window; older prose in
`APP_PROTOCOL.md` describes the preceding draft.

## Existing HTTP Pairing (#399)

These routes remain compatible with existing devices. They are served by the
gateway, not the Control API. Outside a trusted LAN use HTTPS.

| Operation | Request | Successful response |
| --- | --- | --- |
| Claim | `POST /v1/harness/pair/claim` with `{code, device_name, platform?, client_nonce}` | HTTP 200 `{claim_id, poll_secret, verification, server_nonce, expires_at}` |
| Poll | `GET /v1/harness/pair/claim/{claim_id}`, header `x-harness-pair-secret: <poll_secret>` | HTTP 200 `{status, token, device_id, role, preset, scopes, endpoints, models}` |
| Self-report | `POST /v1/harness/device/report`, device bearer token, `DeviceSelfReport` JSON | HTTP 204 |

`platform` defaults to the empty string. `device_name` is trimmed, 1–48 characters,
without control characters. `client_nonce` is 16–128 ASCII alphanumeric characters.
The eight code digits may be grouped (the offer displays `1234-5678`). Timestamps
are Unix seconds. Default offer and claim TTLs are 300 seconds; the owner may set
an offer TTL of at most 900 seconds. Five wrong claims within a rolling 60-second
window lock claiming until the oldest failures leave that window. This is a
host-wide limiter, not a per-IP limiter.

Poll `status` is `pending`, `approved`, `denied`, or `expired`. All four use HTTP
200. Optional fields are `null` when absent; the arrays are empty. An approved
token is delivered once. Repeating that poll returns `pairing.not_found`.

| HTTP error | Code |
| --- | --- |
| 400 | `pairing.device_name_invalid`, `pairing.nonce_invalid`, other pairing failures |
| 403 | `pairing.invalid_code` (also consumed/expired offer), `pairing.not_a_device` |
| 404 | `pairing.not_found` (including incorrect poll secret or already delivered token) |
| 429 | `pairing.locked`, `pairing.too_many` |

The error body contains `error.code` and `error.message`. The existing lockout
response does **not** provide `retry_after_s`. This confirms the actual #399
paths, fields, and mapping requested in APP_PROTOCOL section 14 Q1/Q2; its draft
201/202/410 mappings and `name`/`nonce`/`verification_code` fields do not describe
these HTTP routes.

Legacy verification is the first four bytes of
`SHA256(code_digits || 0 || client_nonce || 0 || server_nonce || 0 || claim_id || 0)`,
interpreted as a big-endian u32, modulo 1,000,000, formatted as six decimal digits.
All four inputs use their UTF-8 text representations. This derivation remains
unchanged for existing clients.

## Browser Pairing Inside Noise

The encrypted `pair.request` / `pair.response` adapter uses the browser schemas
and their HTTP-style status field. It does not change the legacy HTTP contract.
Envelopes have `v: 1`, a strictly increasing per-direction `seq` beginning at 0,
and a request `id` echoed in the response. All operations require mode `pair`.

1. `op: "claim"`, `body: {code, name, commit}`. `code` is exactly eight digits.
   `commit = base64url(BLAKE2s("struklabs/pairing/sas-commit/v1" || L(nonce_c)
   || L(Kc) || L(h) || L(name)))` where `Kc` is the claimer's Noise static key, `h`
   is the Noise handshake hash of this session and `name` is the trimmed device
   name. The nonce is 32 bytes. The desktop consumes the offer atomically, fixes
   the commitment and generates a fresh 32-byte desktop nonce. `claim_id` is
   assigned by the desktop and is never a SAS input.
2. Status 201 returns `{claim_id, poll_secret, nonce_d, static_pub_d, expires_at,
   poll_interval_ms}`. Nonces and keys are unpadded base64url, `expires_at` is UTC
   RFC 3339 ending in `Z`, and polling is every 2000 ms. The browser checks `static_pub_d` against
   the key authenticated by Noise. A QR fingerprint is checked before claiming.
   Only a claim-salted hash of the poll secret is retained. The legacy HTTP poll
   route cannot deliver browser claims; delivery requires the bound Noise peer.
3. `op: "reveal"`, `body: {claim_id, poll_secret, nonce_c, static_pub_c}`. The
   desktop recomputes the commitment from its own Noise session (peer static key
   and handshake hash) and requires `static_pub_c` to equal the Noise peer key.
   Keys and `h` never come from message bodies. A failed reveal burns the claim; repeat reveals are rejected.
   No claim can be approved or displayed as pending before successful reveal.
4. Status 200 returns `{verification_code}`. Both screens display the code;
   only explicit owner approval creates a token.
5. `op: "poll"`, `body: {claim_id, poll_secret}`. Valid only after a valid reveal
   and only from a session whose Noise static key equals the claiming key;
   anything else is `pairing.not_found`. Pending: status 202
   `{status:"pending"}`. Approval: status 200 `{status:"approved", token,
   token_type:"bearer", device_id, desktop_id, desktop_name, scopes, token_policy,
   noise_static_pub}` exactly once. The desktop key is pinned with the token;
   subsequent app handshakes must verify it before sending `hello` and the token.

Browser errors use 403 for `pairing.invalid_code` / `pairing.pending_denied`, 410
for `pairing.expired` / `pairing.not_found`, and 429 for `pairing.locked` (with
`retry_after_s: 60`). Malformed crypto inputs fail closed. No legacy token can
authenticate a browser app session: its registry record must bind its Noise key.

### Verification and Fingerprint

Let `L(x) = u16be(byte_length(x)) || x` and `H` = BLAKE2s-256. The browser and the
desktop each derive the same six digits (first eight digest bytes as a big-endian
u64, modulo 1,000,000, zero-padded):

```text
SAS = H("struklabs/pairing/sas/v1" || L(nonce_c) || L(nonce_d)
        || L(Kc) || L(Kd) || L(h) || L(name))
```

`Kc`, `Kd` and `h` come from each side's own Noise session. A relay that
terminates Noise on both legs produces different `h` and keys per leg, and it
must fix its commitment before seeing `nonce_d`, so it cannot grind a matching
code (the old `claim_id` input was grindable and is removed). The reveal answer
still carries `verification_code` (the desktop's SAS) for older clients; the
browser must not treat it as authentication and must compare the displayed
numbers by eye. A bad reveal, a denial or a mismatch burns the offer.

Relay tickets are JWTs. Before dialing, the desktop pins `alg = EdDSA` (rejects
`none`/HS*), requires `aud = struklabs-relay`, `role = desktop`, `device` and
`noise_static_pub` equal to its own, `exp - iat <= 300`, `iat - 5 s <= now < exp`
and rejects a reused `jti`. Signature verification belongs to the relay (platform
JWKS).

The relay connection multiplexes sessions. The desktop accepts frames and
envelopes only for sessions it was told about (`session.incoming`); anything for
another session id is ignored, a duplicate `session.incoming` is fatal, and each
Noise channel additionally rejects frames carrying another session id.
Handshake frames are direction-aware: the desktop (responder) accepts exactly a
32-byte message 1 and then a 64-byte message 3.

The QR fingerprint is
`base64url(BLAKE2s("struklabs/pairing/fingerprint/v1" || static_pub_d)[0..16])`.
All labels are literal UTF-8 bytes without a trailing zero. QR links use
`https://app.struklabs.com/devices/pair#code=...&desktop=...&fp=...`; fragment
values do not reach the app server. Never put tokens in a URL.

## Noise and Relay

Noise suite: `Noise_XX_25519_ChaChaPoly_BLAKE2s` using `snow`; browser initiates,
desktop responds. Prologue: `StrukLabs-App/1|<pair-or-app>|<32-lowercase-hex-session>`.
Empty handshake payloads produce exactly 32, 96, and 64 bytes. The desktop's
long-lived secret is held by Windows Credential Manager, macOS Keychain, or Linux
Secret Service, never a JSON file. A locked/corrupt store is an error, not a key
rotation. Each connection uses fresh ephemeral keys. Failed decryption is fatal.

Each whole-message transport carries one frame:
`u32be(body_length) || kind:u8 || session:16 || Noise_message`.
Kinds are 1 (handshake) and 2 (transport); body maximum is 65,552 bytes. Encrypted
records contain `flags:u8 || data_length:u16be || data || zero_padding`, padded to
64-byte multiples. Only flag bit 0 (more fragments) is permitted, each record
holds at most 16,384 data bytes, and an assembled UTF-8 JSON envelope is at most
1 MiB. Session mismatches, replay, ordering errors, invalid padding and malformed
frames close the session. There is no plaintext application transport.

Desktop WSS is outbound to `wss://relay.struklabs.com:7843/v1/app`, subprotocol
`struklabs.app.v1`. A signed-in account ticket is single-use, expires within 300
seconds and is bound to the desktop key. The ticket issuer is an injected trait;
the default provider reports that account sign-in is unavailable. Auth uses
`{v:1,seq:0,session:<all-zero-id>,type:"auth",role:"desktop",credential}`.
Relay text envelopes handle presence, incoming sessions, fallback, ping/pong and
peer departure. Binary application frames are accepted only after
`transport.fallback`. At most eight sessions share a desktop relay connection.
The channel interface is independent of WSS so a future ordered, reliable
WebRTC data channel can carry identical frames.

Chat replay retains unacknowledged deltas for at most 256 KiB / 30 seconds per
chat. `chat.ack {id,index}` acknowledges the contiguous delta index; the outer
envelope `seq` still increases independently in each Noise session. Reconnection
uses `hello.resume [{chat_id,last_index}]`; accepted cursors are echoed in
`hello.resumed`, otherwise `chat.resume_unavailable` is returned. Retained deltas
are re-encrypted under the fresh Noise session, never replayed as old ciphertext.
Revocation clears retained state and cancels generation, including while offline.
`device.revoke {id}` revokes the authenticated
device's named token, clears its buffer and answers encrypted
`device.revoked {id}`. It cannot revoke an arbitrary other device.

`models.list` is filtered by the token policy. `chat.start` uses the existing
gateway scheduler and upstream chat stream, enforcing model/endpoint permission,
request rate and concurrency limits. `chat.cancel` drops the upstream stream and
its scheduler lease. The relay can observe connection metadata and padded frame
sizes, never pairing secrets, tokens, prompts or application envelopes. Account
ticket issuance and WebRTC remain unavailable in this preview.

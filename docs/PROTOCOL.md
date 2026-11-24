# Songbird protocol

Skeleton. Each section is filled by the prompt that builds it; every encoding has published test vectors.

## 1. Transport and framing (Prompt 2)

Implemented in `crates/proto` (shared by server and clients); decisions D-0019, D-0020.

- **Transport.** TLS 1.3 only (rustls, aws-lc-rs), ALPN `http/1.1`, then an HTTP/1.1 WebSocket
  upgrade (RFC 6455) to path `/v1/ws` with subprotocol `songbird.v1`. Another path gets `404`, no
  `songbird.v1` offer gets `400`, a request over 8 KiB gets `431`, a version other than 13 gets
  `426`. No extensions are negotiated (no permessage-deflate). Plain TCP is accepted only in
  development and CI.
- **Messages.** Binary messages only; a text message closes the connection (4003). Each message is
  one frame: a one-byte type, then big-endian fixed-width fields, then (for frames that carry one)
  a payload that runs to the end of the message.
- **Heartbeat.** The server pings after 25 s of inbound silence and closes after 60 s of silence.
  Clients answer pings and do not need to ping.

### 1.2 Frames

| Type | Direction | Frame | Body |
| --- | --- | --- | --- |
| `0x01` | C→S | `Send` | `req_id u32`, `idempotency_key [16]`, `flags u8`, [if flag `0x01`: `group [16]`, `epoch u64`], [if `0x04`: `access_key [16]`], [if `0x10`: `commitment [32]`], [if `0x20`: `expire_after_s u32`], [if `0x40`: `group [16]`], `count u16` (≥ 1; 0 with `0x40`), then per device (no flag `0x02`) `count ×` (`device_id [16]`, `len u32` (≥ 1), `envelope [len]`), or shared (flag `0x02`) `count × device_id [16]`, `len u32` (≥ 1), `body [len]` |
| `0x02` | C→S | `Ack` | `up_to_seq u64` |
| `0x03` | C→S | `Ephemeral` | `to [16]`, `payload` (≥ 1 byte) |
| `0x04` | C→S | `Attention` | `to [16]`, `payload` (≥ 1 byte): a nudge, routed within budgets per pair of accounts and blocks (§10.5); the recipient gets `0x84` |
| `0x10` | C→S | `Auth` | protobuf `Auth` (§2) |
| `0x81` | S→C | `Deliver` | `seq u64`, `envelope` (≥ 1 byte) |
| `0x82` | S→C | `SendResult` | `req_id u32`, `status u8`; status 0 accepted; 1 rejected: `reason u16`, `retry_after_ms u32`; 2 device mismatch: protobuf `DeviceMismatch` |
| `0x83` | S→C | `Ephemeral` | `payload` |
| `0x84` | S→C | `Attention` | `payload`: an event that came through the attention class (§10.5, D-0108); apps honour a nudge only from this frame |
| `0x90` | S→C | `Hello` | protobuf `Hello` (§2) |
| `0x91` | S→C | `AuthOk` | protobuf `AuthOk` |
| `0x92` | S→C | `Error` | protobuf `Error` (`code u32`, `retry_after_ms u32`), sent before a close when needed |

`Send` flags (other bits must be zero; each optional block appears only with its flag, in the order
above, so every frame has one encoding):

| Flag | Meaning | Rules |
| --- | --- | --- |
| `0x01` | Retired (was D-0026's MLS commit block); never reused | Refused since Prompt 5: commits go to `POST /v1/groups/commit` (§6.3) |
| `0x02` | One shared body for every device | |
| `0x04` | `ACCESS`: the XOR of the access keys of every addressed account that is not open (§5.10), per device or with a shared body | Not with `0x08` |
| `0x08` | `REQUEST`: a message request (§5.10) | Not with `0x04`, `0x02` or `0x01` |
| `0x10` | `FRANKED`: the sender's franking commitment (§5.3), stamped by the server (§5.13) | |
| `0x20` | `EXPIRES`: the disappearing hint, 1 s to 30 days (§5.13) | Zero or more than 30 days refused |
| `0x40` | `GROUP`: a group message; `group [16]` follows every other block, `count` is 0 and the server fans the shared body out to the group's roster (§6.5) | Needs `0x02`; not with `0x01`, `0x04` or `0x08` |
| `0x80` | `SILENT` (Prompt 8, D-0095): stored and delivered as usual, but an offline recipient gets no push wake-up. For delivery and read receipts, sync messages to one's own devices, and anything else the recipient need not see at once. The server keeps the bit only while it routes the send. | No block; goes with any other flag. Every bit of `flags` is now assigned: a future flag needs a new block. |

Rejection reasons: 1 rate limited, 2 too large, 3 too many recipients, 4 unknown recipient
(unused since milestone 2: unknown devices are reported in a device mismatch), 5 duplicate
recipient, 6 store unavailable, 7 idempotency key already used for other recipients (detected
when the earlier send shares a mailbox partition with this one; a reuse that cannot be detected is
stored as a new send, never dropped), 8 stale
MLS epoch (the group moved on; process the winning commit, then retry from the new epoch), 9 an
envelope begins with `F5 53 42`, the prefix reserved for what the server writes (system frames
§3.9, franking stamps §5.13; never retryable), 10 access required (a send to another account with
neither its access key, nor the right one, nor `REQUEST`: fetch the profile key again or send a
request, §5.10), 11 an invalid request (not franked, or for more than one other account, §5.10),
12 not a member of the group (or no such group, §6.5), 13 an invalid group message (§6.5), 14 the
group's roles do not let the sender send ("only admins can send", a channel's subscribers, §6.6).
1 and 6 are retryable after `retry_after_ms`, with the
same idempotency key. Unknown reason codes are kept (forward compatibility).

Decoders reject trailing bytes, so every frame has exactly one encoding.

### 1.3 Limits

Configured on the server (`[delivery.limits]`) and checked before any allocation proportional to
input. Defaults: envelope 64 KiB, 1,024 recipients per `Send`, `Send` frame 1 MiB, ephemeral
payload 4 KiB, control-frame body 4 KiB. The WebSocket layer refuses any message larger than the
largest legal frame (1 MiB by default) from its header, before buffering it, and fails the
connection. Per connection: 50 sends/s (burst 200; excess rejected with reason 1), 400 frames/s
(burst 800; excess closes with 4103), 20 ephemeral events/s (burst 40; excess dropped). Per
listener: 2,000 new connections/s (burst 4,000) and 100,000 open connections; beyond either the
connection is closed with 4101 and a retry hint.

### 1.1 Reaching Songbird through a proxy (Prompt 1)

Clients that cannot reach Songbird directly may connect through a `songbird-proxy`
(`services/proxy`, operator guide `deploy/proxy/README.md`).

**Connection.** The client opens TCP to the proxy host and port, then TLS with SNI set to the proxy
host, validating the proxy's certificate against the system roots. Inside that TLS session it
opens its normal TLS session to the Songbird endpoint, with SNI set to that endpoint's name,
exactly as it would directly. The proxy routes by the inner SNI to an allowlist of Songbird
endpoints and refuses anything else by closing the connection without a response. Everything after
the inner `ClientHello` is opaque to the proxy. The inner `ClientHello` must fit in
`max_client_hello_bytes` (default 16 KiB) and arrive within the handshake timeout (default 10 s).

**HTTPS through the proxy (D-0164).** With a proxy set, the client sends every request to
Songbird's own hosts (the api, which also serves media, GIFs, packs and the backup vault, and
billing) the same way: HTTP/1.1 inside the inner TLS session, redirects followed only to HTTPS and
only through the proxy. A proxy that can't be reached, or that refuses a host, fails the request;
the client never falls back to a direct connection. Backup destinations the user chose (their own
WebDAV, S3 or cloud drive) are reached directly, since a proxy carries only Songbird's hosts.
Call media (LiveKit and its WebRTC transport) does not go through a proxy.

**Proxy links.** UTF-8, at most 300 bytes, one of:

```
songbird://proxy#<host>[:<port>]
https://<web-domain>/proxy#<host>[:<port>]
```

- The address is in the fragment so web servers never receive it.
- `<host>`: DNS name, 1–253 bytes, labels 1–63 bytes of `[A-Za-z0-9-]`, no leading or trailing
  `-`, no trailing dot; compared case-insensitively and stored lower-case. IP literals are not
  accepted.
- `<port>`: decimal 1–65535, default 443.
- `<web-domain>`: from a fixed list compiled into the app (owner decision open, D-0011).
- Anything else (other schemes, paths, user info, query strings, extra characters) is rejected.

Test vectors (from `services/proxy/src/link.rs`):

| Input | Result |
| --- | --- |
| `songbird://proxy#Proxy.Example.org` | host `proxy.example.org`, port 443 |
| `https://songbird.example/proxy#proxy.example.org:8443` (web domain `songbird.example`) | host `proxy.example.org`, port 8443 |
| `songbird://proxy#p.example:0` | rejected (port) |
| `songbird://proxy#user@p.example` | rejected (host) |
| `https://evil.example/proxy#p.example` | rejected (domain not allowed) |

## 2. Device authentication and sessions (Prompts 2, 3)

Decision D-0018.

### 2.1 Handshake

1. After the upgrade the server sends `Hello`:
   `nonce` (bytes, 32, fresh per connection), `server_time_ms` (uint64),
   `protocol_versions` (repeated uint32, currently `[1]`), `store_generation` (uint64, §3.6),
   `server_name` (string, informational).
2. Within 5 s the device sends `Auth`: `device_id` (bytes, 16), `signature` (bytes, 64),
   `last_acked_seq` (uint64), `client_version` (string, ≤ 64 printable ASCII bytes),
   `protocol_version` (uint32, one of the offered versions). Otherwise the server closes with 4102.
3. The server verifies the Ed25519 signature with the device's registered authentication key over
   the transcript below. An unknown device and a bad signature both close with 4000.
4. The server treats `last_acked_seq` as an `Ack`, claims the device (closing any older
   connection with 4001), replies `AuthOk` (`store_generation`, `resume_after_seq`) and starts
   delivery after `last_acked_seq`.

### 2.2 Signed transcript

```
"songbird.v1/device-auth\0" || u8 len(server_name) || server_name || nonce[32]
  || device_id[16] || protocol_version u32 (big-endian) || channel_binding[32]
```

The client uses the server name from its own configuration, never the one in `Hello`, so a
signature for one endpoint is useless at another. `channel_binding` is the TLS exporter of the
connection (RFC 9266 `tls-exporter`: 32 bytes, label `EXPORTER-Channel-Binding`, no context), so
a signature obtained by relaying the nonce over another TLS session does not verify (D-0024); TLS
must end at the delivery server. Without TLS (development only) it is 32 zero bytes. The server
name is the only variable-length field and is length-prefixed. Test vector: §13.

### 2.3 One connection per device

A newer authenticated connection for the same device closes the older one with 4001 (invariant 4).
A client closed with 4001 does not reconnect by itself.

### 2.4 Close codes and reconnecting

| Code | Meaning | Client action |
| --- | --- | --- |
| 4000 | authentication failed | stop |
| 4001 | replaced by a newer connection | stop |
| 4002 | device removed | stop |
| 4003 | protocol violation | stop |
| 4004 | unsupported protocol version | stop |
| 4100 | slow consumer | back off, then reconnect and catch up |
| 4101 | overloaded | back off (honour the hint) |
| 4102 | authentication timeout | back off |
| 4103 | rate limited | back off (honour the hint) |
| 4104 | store unavailable | back off |
| 4200 | server draining for a deploy | reconnect now (any node) |
| other | standard or unknown codes, transport errors | back off |

The reason may carry a hint `retry_ms=<decimal>`: wait at least that long. Backoff is exponential
with full jitter (initial 250 ms, cap 30 s, never below the hint); "reconnect now" waits a random
0–500 ms. On a deploy the server closes every connection with 4200 at random times within its
drain window (default 2 s) so clients do not return all at once.

## 3. Envelopes, mailbox and delivery (Prompt 2)

Decisions D-0020, D-0021.

### 3.1 Sending

A `Send` carries either one opaque, client-encrypted envelope per recipient device (pairwise
sessions) or, with flag `0x02`, one ciphertext for every listed device (an MLS group or channel
message), which the server stores once with a small entry per device. The server checks limits,
rate limits and recipients, stores every envelope in the recipients' mailboxes with each device's
next `seq` (fsynced), and only then answers `SendResult` accepted (invariant 1) and routes the
envelopes to connected recipients. A send rejected for a reason other than 6 stored nothing. After
reason 6 (store unavailable) the send may already be recorded, and if so the server stores it for
every recipient by itself; retrying with the same idempotency key returns the original result and
stores nothing twice.

**Device lists.** Every addressed device must exist, and a send that addresses an account must
address all of that account's devices except the sending device. Otherwise the answer is
`SendResult` status 2 with a `DeviceMismatch`: `missing` (devices to add), `extra` (devices that do
not exist), `stale` (devices whose keys changed; filled from Prompt 3). The client updates its view,
re-encrypts and retries with the same idempotency key.

**MLS commits** no longer travel in a `Send` (Prompt 5): `POST /v1/groups/commit` validates a
commit against the group, orders it by epoch and fans it out (§6.3). The `Send` flag `0x01` that
carried one (D-0026) is refused.

### 3.2 Idempotency

`idempotency_key` is 16 random bytes per logical message. A retry with the same key and the same
recipient devices (after a timeout, a disconnect or a retryable rejection) within 6 hours is
stored at most once: the server answers it from a record of the send (a keyed fingerprint of the
recipient set and where the send is stored), kept under a keyed hash of the key (never the key
itself) and not linked to the sender; envelopes already acknowledged are not stored or routed
again. The same key for other recipients is refused with reason 7. A retry after 6 hours may be stored again;
recipients drop it by the message ID inside the envelope.

### 3.3 Delivery and ordering

Each device has a sequence `seq` starting at 1, strictly increasing with no gaps at allocation.
After `AuthOk` the server sends every stored envelope after `last_acked_seq` in `seq` order, then
streams new ones. A device can receive a `seq` again after a reconnect if its `Ack` had not reached
the server; clients drop any `seq` at or below the last one they handed to the app, so the app
sees each envelope exactly once and in order. A `seq` may be skipped only where an envelope expired
(30 days, §3.5). `seq`s are allocated in the same durable transaction as the envelope, so they are
never reused, even across restarts or several server nodes.

### 3.4 Acknowledgement

`Ack{up_to_seq}` is cumulative: everything up to and including `up_to_seq` is stored on the device
and is deleted from the server (invariant 3). Clients acknowledge only what the app stored
durably, coalescing ACKs for a few milliseconds and flushing them before a clean close. An `Ack`
beyond the last `seq` sent on the connection closes it with 4003.

At most 256 envelopes (server setting `max_unacked`) are sent on a connection and not yet
acknowledged; the server pauses delivery at that limit and resumes on the next `Ack`. A socket
that accepts no data for 30 s is closed with 4100; the device catches up after reconnecting.

### 3.5 Retention

Undelivered envelopes are deleted after 30 days. Mailboxes hold at most 10,000 unacknowledged
envelopes per device by default; sends to a full mailbox are rejected with reason 6 (retryable).

### 3.6 Store generation

`Hello.store_generation` grows at every restore from backup and at every loss of the mailbox
(D-0040: it jumps to at least the restore time in milliseconds, so it always exceeds every value
clients have seen, even when the registry itself was restored from an older backup). A client
that sees it grow:

1. resets its delivery cursor to 0 (the mailbox's `seq` counters may restart);
2. resends every message from its send log, in the order first sent, with each message's original
   idempotency key. A mailbox that survived the restore recognises the key and stores nothing
   twice; a lost one stores the message again. The log holds every accepted send until the
   message's end-to-end delivery receipt arrives (`Client::confirm_delivered`) or 30 days pass;
3. re-asserts its device registration and keys (Prompt 3).

Recipients drop repeats by the message ID inside the encrypted payload. A resend the server
refuses for a reason that needs the app (a changed device list, an MLS epoch that moved on) leaves
the log and is reported (`ResendRefused`), so the app re-encrypts it. The mailbox itself is never
backed up (D-0040). Tests: `tests/e2e/tests/restore.rs`.

### 3.7 Ephemeral events

Typing indicators, presence pings and call signals that are only useful live use `Ephemeral`.
They are never stored, only delivered if the recipient is connected, rate limited per connection,
and dropped first under pressure. The recipient learns the sender only from inside the encrypted
payload.

### 3.8 Sending without a WebSocket: `PUT /v1/messages`

Decisions D-0037, D-0044. For a client with no open WebSocket (a notification quick-reply, a
share extension, a socket that failed), `api` accepts the same send over HTTPS:

- **Request:** `PUT /v1/messages`, body = one `Send` frame (§1.2), byte for byte as it would go
  over the WebSocket, `Content-Length` required (at most `max_send_bytes`; otherwise 411 or 413),
  signed like every `api` request (`authorization` over method, path, server name, time, nonce
  and body digest; `x-songbird-content-sha256` required). The server verifies the signature from
  the headers before it reads the body, then checks the body against the digest (401 otherwise).
- **Response:** `200` with body = the `SendResult` frame (§1.2) the WebSocket would have sent
  (`application/octet-stream`): accepted, rejected with a reason and retry hint, or a device
  mismatch. Other statuses come only from before the send path: 400 (not a `Send` frame, or over
  a limit), 401, 411, 413, 503 (no send path configured).
- **Same rules:** the same limits, the same device-list check, the same idempotency (a retry with
  the same key over HTTP or the WebSocket, in either order, is stored once), the same rate limit
  per device (50 sends/s, burst 200, counted across every `api` node; reason 1 with a hint),
  the same routing and push wake-ups. The request ID in the frame is echoed.

### 3.9 System frames

Decisions D-0037, D-0044. A notice from the server itself (`Kind` 1: an account's device list
changed; body: the account ID) arrives as an ordinary stored envelope (`Deliver`, with a `seq`,
acknowledged like any other), in a format no user can send:

```
magic F5 53 42 53 59 53 00 01 | version u8 (1) | key_id [8] | device [16] | issued_at_ms u64
  | notice_id [16] | kind u16 | body_len u16 (<= 1024) | body | signature [64]
```

- The signature is Ed25519 by the server's system key over `songbird-system-frame-v1\0`
  followed by every byte before the signature. Apps pin the system public key; `key_id` is the
  first 8 bytes of its SHA-256 (rotation). A frame that does not verify, names another device, or
  repeats a `notice_id` already seen is dropped and never shown. Unknown kinds are verified, then
  ignored.
- The send path refuses any user envelope that begins with the magic (reason 9), over the
  WebSocket and HTTP alike, so a client never has to consider a forged notice.
- Format and test vector: `crates/proto/src/system.rs`; signer: `crates/send/src/system.rs`.

## 4. Identity, devices and key directory (Prompt 3)

Decisions D-0047 (the owner's Prompt 3 choices) and D-0048 (registration).

### 4.1 Registration: `POST /v1/registration`

Accounts have no phone number or e-mail. An account is a random ID, an **account identity key**
(Ed25519; it signs the device list from M4 and is what people verify) and its devices. Each
device has its own **authentication key** (Ed25519, §2), never used for end-to-end encryption.
The server chooses both IDs (16 random bytes, D-0003).

1. `POST /v1/registration/challenge` (no body) returns a 51-byte proof-of-work challenge
   (`application/octet-stream`):

   ```
   version u8 (1) | issued_ms u64 | memory_kib u32 | iterations u32 | bits u8 | solutions u8
     | nonce [16] | tag [16]
   ```

   The tag is the first 16 bytes of `HMAC-SHA256(key, "songbird.v1/pow-challenge\0" || the 35
   bytes before it)`, with a key every `api` node derives from the registration secret: no state
   is kept until a solution arrives. A challenge is valid for 10 minutes and for one attempt.
2. The client finds `solutions` strictly increasing `u32` counters `c`, each such that

   ```
   Argon2id(v1.3, m = memory_kib KiB, t = iterations, p = 1, 32-byte output)(
       password = "songbird.v1/pow\0" || challenge[0..35] || c (u32 big-endian),
       salt = nonce)
   ```

   begins with `bits` zero bits (on average `solutions × 2^bits` evaluations). A client refuses
   parameters outside: memory 8–262,144 KiB, iterations 1–16, bits 0–24, solutions 1–64.
   Defaults: 16 MiB, 1, 4, 8 (128 evaluations; 0.48 s median on an Apple M5 Pro core).
3. `POST /v1/registration` with `Content-Length` and the body:

   ```
   version u8 (1) | challenge [51] | k u8 | k × solution u32 | flags u8
     | [invite [15] if flags & 1] | identity_key [32] | auth_key [32]
     | identity_sig [64] | auth_sig [64]
   ```

   Unknown flags, trailing bytes and `k` outside 1–64 are refused. Each key signs, with Ed25519:

   ```
   context || u8 len(server_name) || server_name || challenge [51] || identity_key [32]
     || auth_key [32]
   ```

   with context `"songbird.v1/register-identity\0"` for the identity key and
   `"songbird.v1/register-device\0"` for the authentication key. The two keys must differ, and
   each must be the canonical encoding of a point of prime order (small-order and mixed-order
   keys are refused: under the identity point, `R = sB` verifies for any message).

Answers:

| Status | Body | Meaning |
| --- | --- | --- |
| 201 | `{"account": hex, "device": hex}` | Registered |
| 400 | `{"error": "request" \| "keys" \| "server name"}` | Does not decode; keys or signatures refused (the challenge is not spent) |
| 403 | `{"error": "challenge"}` | Forged, expired, already used or wrongly solved: get a new challenge |
| 403 | `{"error": "invite"}` | Invite-only mode: the code is missing, unknown, used up, expired or revoked (one answer for all) |
| 411, 413 | none | No `Content-Length`, or longer than 518 bytes |
| 429 | none, `Retry-After` | Throttled (per IP range or deployment-wide) |
| 503 | none | Registration is not configured, or a store is unavailable |

The server checks, cheapest first: length and decoding; throttles; the challenge's tag and age;
both keys and signatures; then spends the challenge (single use); then verifies the work; then
consumes the invite and creates the account in one transaction.

**Right after registering**, delivery nodes may not know the new device yet: they reload the
directory from the registry within `directory_poll_ms` (1 s by default). A client that connects
in that window gets 4000 like any unknown device. Apps therefore treat 4000 as retryable, with
backoff, for 10 seconds after registering (`ClientConfig::new_device_grace` in the Rust client).
The server never looks a device up on behalf of an unauthenticated connection.

### 4.2 Invites

Invite-only mode is a server setting (`[api.registration] invite_only`, the launch default);
switching it off needs no migration, and codes are then ignored.

- **Codes:** 15 random bytes (120 bits), shown as 24 Crockford base32 symbols in six groups of
  four (`0123-4567-89AB-CDEF-GHJK-MNPQ`). Parsing ignores case, hyphens and spaces and reads `O`
  as `0`, `I` and `L` as `1`. The server stores `SHA-256("songbird.v1/invite\0" || code)` with
  the uses left and the expiry, nothing else: **not who created the code** (D-0038, D-0047). The
  row goes at its last use.
- **Guessing:** a guess costs a solved challenge, and refused codes spend a per-range budget
  (10 an hour, burst 5) and a deployment-wide one (600 an hour); once spent, registrations from
  that range wait (429) before their challenge is even checked.
- **Users' invites**, signed like every device request (§2 of `api`, D-0036):
  - `POST /v1/invites`, optional body `{"ttl_days": n}` (capped at 7 by default): `201
    {"code": "...", "expires_in_secs": n}`, single use, one taken from the account's allowance (5
    per new account by default); `403 {"error": "allowance"}` when none is left.
  - `GET /v1/invites`: `{"allowance": n, "invite_only": bool}`.
  - `DELETE /v1/invites`, body `{"code": "..."}`: revokes it; `204` whether or not it existed.
    Whoever holds a code may revoke it (nothing records who created it).
- **Operators:** `songbird-admin invites create --count N --uses U --days D` prints codes once
  (audit-logged without them); `songbird-admin invites grant --account <hex> --add N`.

### 4.3 Usernames, the username key and links

Decision D-0049 (unique handles, the owner's choice). Every request is signed by a device.

- **A username** is 3 to 32 characters: ASCII letters, digits, `_` and `.`; it starts with a
  letter, has no `..` and does not end with `.`. Unique in lower case (`Radu` and `radu` are one
  username); shown as typed. Found only by exact match: there is no search, prefix or listing.
- **The server never sees it.** It stores and is asked about only
  `hash = s·G` on Ristretto255, with `s = SHA-512("songbird.v1/username\0" || lower-cased
  username)` reduced mod l and `G` the point `Sho("songbird.v1/username-generator")` squeezes
  (64 bytes, `from_uniform_bytes`). The SHO is libsignal's poksho `ShoHmacSha256`.
- **Claiming** needs a proof: a poksho proof (label `POKSHO_Ristretto_SHOHMACSHA256`, statement
  `hash = s·G`, encoded `01 01 01 00 02`, points base, hash, `G`, message the hash) that the
  client knows `s`: `challenge || response`, two canonical scalars, 64 bytes. libsignal's own
  verifier accepts these proofs (`tools/libsignal-vectors`).

| Request | Body | Answers |
| --- | --- | --- |
| `PUT /v1/username` | `hash [32] \|\| proof [64]` | 204; 409 `{"error":"taken"}` (another account's, held, or reserved); 400 `{"error":"proof"}`; 429 |
| `DELETE /v1/username` | none | 204 |
| `POST /v1/username/lookup` | `{"hash": hex, "key"?: digits, "link"?: handle hex}` (the link: §10.3) | 200 `{"account": hex}`; 404; 400 for a malformed hash or key; 429 |
| `PUT /v1/username/key` | `{"key": "4 to 10 digits"}` | 204 |
| `DELETE /v1/username/key` | none | 204 |
| `PUT /v1/username/link` | the link's ciphertext, 1 to 256 bytes | 200 `{"handle": 32 hex}`; 409 `{"error":"no username"}` |
| `DELETE /v1/username/link` | none | 204 |
| `GET /v1/username/link/<handle>` | none | 200 the ciphertext; 404 |

- **Changing or releasing** a username holds the old one for 30 days: nobody can claim or find
  it, so nobody can pose as its previous owner by grabbing it. The held row names no account.
  Account deletion releases the username the same way.
- **Reserved names** (`admin`, `support`, `songbird`, … plus `[api.usernames] reserved`) answer
  `taken`; the server compares hashes.
- **The username key** (optional): with one set, a lookup finds the account only with the right
  key. A missing key, a wrong key and a username whose guessing budget is spent all answer 404,
  exactly like an unknown or held username (the owner's decision, D-0049). A lookup that
  carries a key always costs one Argon2id verification, and one without never does, so time
  does not tell them apart either. The key is stored as `salt [16] || Argon2id(m = 19 MiB, t = 2,
  p = 1, secret = a pepper from the secrets store)(key, salt)`.
- **Throttles:** lookups and link reads 120 an hour per account (burst 30) and 600 per IP range
  (burst 100); claims 2 an hour per account (burst 5), counting every attempt; key guesses that
  find nothing, 1 an hour per username across all requesters (burst 10).
- **Links** let someone share a username without typing it, and reset the link without changing
  the username: the client encrypts the username under a key that stays in the link (the
  server never sees it) and uploads the ciphertext; the server keeps it under a new random
  16-byte handle each time (the old handle stops working). A username change deletes the link.
  The client's encryption follows Signal's username links; the server only stores bytes.

### 4.4 The key directory (PQXDH prekeys)

Decision D-0050. Every device has its own **device identity key** (X25519, used by PQXDH;
D-0048) and publishes prekeys for others to start sessions with it. Keys are libsignal's
serializations: an X25519 key is `0x05 || u [32]`; a post-quantum key is ML-KEM-1024 only,
`0x0A || encapsulation key [1568]` (libsignal's pre-standard Kyber1024, type `0x08`, is refused;
the owner's decision). Signatures are XEdDSA (libsignal's variant: the signer's Edwards sign bit in
the top bit of `s`) by the device identity key over the key's serialization.

| Request | Body | Answers |
| --- | --- | --- |
| `PUT /v1/keys` | the upload below (at most 169,177 bytes; signed over its digest, authenticated before the body is read) | 204; 400 `{"error": "request" \| "keys" \| "signature" \| "incomplete" \| "duplicate"}`; 409 `{"error": "identity" \| "full" \| "duplicate"}` |
| `GET /v1/keys` | none | 200 `{"ec": n, "kem": n}` (one-time prekeys left) |
| `POST /v1/keys/fetch` | `{"account": hex, "device"?: hex}` | 200 the bundles below; 404 (no device of the account holds keys, the requester's own excepted); 429 |

Upload (integers big-endian):

```
version u8 (1) | flags u8 (1 identity, 2 signed prekey, 4 last-resort KEM prekey)
  | [identity [33]] | [id u32 | key [33] | sig [64]] | [id u32 | key [1569] | sig [64]]
  | n u16 | n × (id u32 | key [33])                      one-time X25519, n <= 100
  | m u16 | m × (id u32 | key [1569] | sig [64])         one-time ML-KEM-1024, m <= 100
```

- The first upload must carry the identity key, a signed prekey and a last-resort ML-KEM prekey.
  The identity key is **set once and never replaced** (another one: 409 `identity`; a new
  identity is a new device, M4). Signed and last-resort prekeys replace the current ones.
- The server checks every key and signature before storing anything: X25519 keys must be honest
  (canonical `u`, on the curve, of prime order); ML-KEM keys must pass FIPS 203's modulus check
  (every coefficient below 3329); XEdDSA signatures must verify with a canonical `s` (stricter than
  libsignal, which reduces a larger one). A device holds at most 200 one-time keys of each kind.

Fetch answer: `version u8 (1) | n u8 | n × bundle`, one per device of the account that holds
keys (not the requester's own device):

```
device [16] | identity [33] | signed: id u32 | key [33] | sig [64]
  | flags u8 (1 a one-time X25519 key follows, 2 the KEM key is one-time, not last-resort)
  | [id u32 | key [33]] | kem: id u32 | key [1569] | sig [64]
```

- Each one-time key goes to **exactly one fetch** and leaves the store with it (the prompt's gate:
  `no_one_time_prekey_is_ever_served_twice_under_parallel_claims`).
- **Exhaustion:** fetches are limited per requesting account (240 an hour, burst 60) and per IP
  range; one-time keys per target account are limited across all requesters (60 an hour, burst
  30). Past that budget a fetch still answers, with the last-resort ML-KEM key and no one-time
  X25519 key, and the one-time keys stay for later.
- **Refill:** when a fetch leaves a device below 10 one-time keys of either kind, the server sends
  it a system notice (§3.9) of kind 2, `PrekeysLow`, body `ec u16 | kem u16`, at most once an hour.
  Clients also see their counts with `GET /v1/keys`.
- A client verifies both signatures with the identity key before PQXDH, as libsignal does.

### 4.5 MLS KeyPackages

Decision D-0051. Groups (Prompt 5) add a device with one of its KeyPackages (RFC 9420 §10). The
server checks each package before publishing it, with its own focused parser (the owner's choice
over running OpenMLS on the server), and **binds it to the uploading device**:

- RFC 9420's encoding, strictly (minimal variable-length integers, no trailing bytes, at most
  4,096 bytes, no duplicate extension types); version mls10; ciphersuite `0x0001` or `0x0003`
  (X25519 HPKE), or `0x004F`, X-Wing (ML-KEM-768 and X25519 HPKE, the suite of every group,
  D-0066); Ed25519 signatures throughout;
- `init_key` and the leaf's `encryption_key` honest keys of the suite's KEM, different from each
  other: X25519, or X-Wing (an ML-KEM-768 encapsulation key passing the FIPS 203 modulus check,
  then an honest X25519 key); the `signature_key` an honest Ed25519 key;
- a **basic credential whose identity is the uploading device's 16-byte ID**;
- capabilities listing mls10, the package's ciphersuite and basic credentials; leaf source
  `key_package`; a lifetime that has begun (an hour of skew allowed), has not ended and is at most
  100 days;
- both `SignWithLabel` signatures (`LeafNodeTBS`, `KeyPackageTBS`) by the signature key.

A package carrying the `last_resort` extension (type 10, as OpenMLS writes it) replaces the
device's last-resort package; the others are single use. Packages are stored under their
`KeyPackageRef` (RFC 9420 §5.2).

| Request | Body | Answers |
| --- | --- | --- |
| `PUT /v1/keypackages` | `version u8 (1) \| n u8 \| n × (len u16 \| KeyPackage)`, 1 to 100 packages | 204; 400 `{"error": "request" \| "unsupported" \| "keys" \| "identity" \| "lifetime" \| "signature" \| "duplicate" \| "last resort"}`; 409 `{"error": "full" \| "duplicate"}` |
| `GET /v1/keypackages` | none | 200 `{"single_use": n, "last_resort": bool}` (unexpired) |
| `POST /v1/keypackages/fetch` | `{"account": hex, "device"?: hex}` | 200 `version u8 \| n u8 \| n × (device [16] \| flags u8 (1 single use) \| len u16 \| KeyPackage)`; 404; 429 |

- Each single-use package goes to exactly one fetch (`no_key_package_is_ever_served_twice_under_parallel_claims`);
  an expired one is never served; beyond the single-use ones, the unexpired last-resort one.
- The same throttles as prekey fetches (§4.4), counted separately, with the same per-target
  budget and last-resort fall-back; a device left below 10 single-use packages gets a system
  notice of kind 3, `KeyPackagesLow`, body `n u16`, at most once an hour. At most 200 stored per
  device.
- Clients still validate every package fully before use (RFC 9420 §10.1).

### 4.6 Devices: QR provisioning, linking and the signed device list

Decision D-0052 (M4). An account has one **primary** device (the one holding the account identity
key, which registered it) and at most five linked devices (owner, D-0047; `max_devices`, until
Prompt 11's plans). Integers are big-endian.

**Linking a device.**

1. The new device draws an X25519 provisioning key and opens a channel
   (`POST /v1/provisioning`, unsigned, throttled per IP range). Its QR code shows the channel ID
   and the provisioning public key.
2. The primary asks for a link token (`POST /v1/devices/link-token`): 32 random bytes, stored
   only as `SHA-256("songbird.v1/link-token\0" || token)`, single use, ten minutes. It reserves
   a server-chosen device ID and counts toward the limit until it is used or expires.
3. The primary seals the provisioning message to the QR key and puts it in the channel
   (`PUT /v1/provisioning/<channel>`, signed). Seal: X25519 with a fresh ephemeral key;
   HKDF-SHA256 (salt `"songbird.v1/provisioning"`, info `ephemeral || recipient`) to an
   AES-256-GCM key; nonce zero (one key, one message); AAD `version || ephemeral || recipient`.
   Envelope: `version u8 (1) | ephemeral [32] | ciphertext || tag [16]`, at most 4,096 bytes. A
   channel holds one message, written once and read once, and lives ten minutes. The server sees
   only the envelope. Plaintext:
   `version u8 (1) | account [16] | account_identity [32] | token [32] | device [16] | confirm_key [32] | u8 len | server_name`.
   Version 2 (Prompt 9, D-0102) appends `storage_key [32]`, the account's storage key for the
   record store (§10.1).
4. The new device reads it (`GET /v1/provisioning/<channel>`, unsigned; waits up to 25 s, 204
   when nothing came, 404 once it has been read or the channel is gone) and opens it.
5. The new device links (`POST /v1/devices/link`, unsigned, throttled per IP range):
   `version u8 (1) | token [32] | device [16] | auth_key [32] | identity_key [33] | auth_sig [64] | identity_sig [64] | confirm [32]`.
   The transcript is
   `context || u8 len(server_name) || server_name || token || device || auth_key || identity_key`.
   `auth_sig` is Ed25519 by the authentication key with context
   `"songbird.v1/link-device-auth\0"`; `identity_sig` is XEdDSA by the identity key with context
   `"songbird.v1/link-device-identity\0"`. `confirm` is
   `HMAC-SHA256(confirm_key, "songbird.v1/link-confirm\0" || transcript)` over the identity
   transcript. The server checks both keys are honest and both signatures verify, then consumes
   the token for exactly that device ID. The device is created **pending**, with its identity key
   fixed (a later `PUT /v1/keys` cannot change it).
6. The primary lists the account's devices (`GET /v1/devices`). It checks the pending device's
   `confirm` tag with the `confirm_key` it sent. The server saw the token but never the
   confirmation key, so it cannot link keys of its own unnoticed. The primary then signs a list
   that names the new device.

While pending, a device authenticates but is outside the send check: a send to its account need
not address it, and a send that does is refused as extra (§3.1). Its keys are not served
(§4.4, §4.5). Once a list names it, a send that leaves it out is refused as missing, and the
sender fetches the list again.

**The device list.**

```text
version u8 (1) | account [16] | list_version u64 | issued_ms u64 | n u8 (1..=16)
  | n × (device [16] | auth_key [32] | identity_key [33] | flags u8 (1 primary)) | signature [64]
```

The signature is Ed25519 by the account identity key over `"songbird.v1/device-list\0"` followed
by every byte before it. Exactly one entry is the primary; no device appears twice.

`PUT /v1/devices/list` accepts a list only if all of these hold:

- it verifies under the account identity key;
- it names the requester's account;
- it has at most `max_devices` entries;
- its version is higher than the stored one (no rollback);
- every entry is an enabled device of the account with exactly its stored keys, and the primary
  entry is the account's primary.

On acceptance, devices it names stop being pending. Enabled devices it leaves out are removed:
they are disabled at once and a `DeleteDevices` job (outbox) deletes them from every store, as
account deletion does. Clients keep the highest `list_version` they have seen for each account
and refuse an older one, so a server cannot roll a list back to a device it removed.

A linked device may remove itself (`DELETE /v1/devices/self`); the primary cannot (it deletes the
account instead). **Only the primary deletes the account** (`DELETE /v1/account` from any other
device: 403 `{"error": "primary"}`, checked inside the deletion's transaction). **A pending device
changes nothing account-wide**: claiming, changing or releasing a username, the username key and
link, and invites answer 403 `{"error": "pending"}` (lookups and reading links are allowed). Linking, a new list and a removal send a `DeviceListChanged` system notice
(kind 1, §3.9) to the account's other devices.

| Request | Body | Answers |
| --- | --- | --- |
| `POST /v1/provisioning` | none (unsigned) | 200 `{"channel": hex, "expires_in": s}`; 429 |
| `PUT /v1/provisioning/<channel>` | envelope (signed) | 204; 400 `{"error": "size"}`; 404 (unknown, expired or written); 413; 429 |
| `GET /v1/provisioning/<channel>` | none (unsigned) | 200 envelope; 204 (ask again); 404; 429 |
| `POST /v1/devices/link-token` | none (the primary) | 200 `{"token": hex, "device": hex, "expires_in": s}`; 409 `{"error": "primary" \| "limit"}`; 429 |
| `POST /v1/devices/link` | link request (unsigned) | 200 `{"account": hex, "device": hex}`; 400 `{"error": "request" \| "keys" \| "signature"}`; 409 `{"error": "token" \| "limit"}`; 429 |
| `GET /v1/devices` | none | 200 `{"devices": [{"device", "auth_key", "identity_key", "primary", "pending", "confirm"}], "list_version"}` |
| `PUT /v1/devices/list` | list | 204; 400 `{"error": "list" \| "account" \| "limit" \| "signature"}`; 409 `{"error": "version" \| "devices" \| "identity"}` |
| `POST /v1/devices/list/fetch` | `{"account": hex}` | 200 list; 404 (none stored); 429 |
| `POST /v1/devices/identity` | `{"account": hex}` | 200 `{"identity_key": hex}` (the account identity key that signs the list; prove it with a KT lookup, §4.9; A2, D-0138); 404; 429 (the list-fetch budget) |
| `DELETE /v1/devices/self` | none | 204; 409 `{"error": "primary"}` |

Throttles (defaults), all as GCRA records in `valkey-hot` under keyed hashes:

- per IP range: 60 channels an hour, 1,200 channel reads an hour and 30 link attempts an hour;
- per account: 20 link tokens and provisioning messages an hour;
- per requesting account: 240 list fetches an hour.

### 4.7 Second factors and multiple accounts

Decision D-0053 (M4b). An account may set a two-step PIN and passkeys. Once it has either, linking
a device needs one of them, and so will account recovery (Prompt 10). Only the primary sets,
replaces or removes them.

**The link request's factor.** These bytes follow `confirm` (§4.6). They are not covered by the
link signatures; each factor binds itself:

```text
factor u8: 0 none
         | 1 PIN:     len u8 | 6 to 12 ASCII digits
         | 2 passkey: len u16 | credential_id | len u16 | authenticatorData | len u16 | clientDataJSON
                      | len u8 | signature
```

- **PIN.** Stored as `salt [16] || Argon2id(m, t, p = 1, secret = pepper)(pin, salt)`. The pepper
  is derived from `[api.keys] secret` (the secrets store, never the database or its backups).
  Each attempt is counted before it is checked, so parallel guesses cannot exceed the limit:
  - the first 5 attempts are free;
  - each further attempt locks the next for `60 s · 2^(k-1)`, at most a week;
  - after 20 attempts the PIN is unusable until the primary sets a new one;
  - a correct PIN resets the count.
  Refusals: 403 `{"error": "factor"}` for a missing or wrong factor, 403
  `{"error": "locked", "retry_after": s}` while locked.
- **Passkey.** WebAuthn with Songbird's own minimal verifier (`songbird_keys::webauthn`):
  - ES256 or EdDSA (Ed25519) credentials;
  - no attestation is trusted;
  - user presence always, user verification required by default;
  - the RP ID and allowed origins come from `[api.keys.devices]`;
  - the signature counter must grow unless it is always zero.
  The assertion's challenge is `SHA-256("songbird.v1/link-passkey\0" || identity-context
  transcript)`, so it approves exactly this token, device ID, pair of keys and server, and needs
  no server state.

| Request | Body | Answers |
| --- | --- | --- |
| `PUT /v1/account/pin` | `{"pin": "6 to 12 digits"}` (primary) | 204; 400 `{"error": "pin"}`; 403 `{"error": "primary"}` |
| `DELETE /v1/account/pin` | none (primary) | 204; 403 |
| `POST /v1/account/passkeys/challenge` | none (primary) | 200 `{"challenge": hex, "rp_id"}`, single use, five minutes; 503 without an RP ID |
| `PUT /v1/account/passkeys` | `{"client_data_json": hex, "attestation_object": hex}` (primary) | 200 `{"id": hex}`; 400 `{"error": "passkey"}`; 409 `{"error": "challenge" \| "limit" \| "duplicate"}` |
| `DELETE /v1/account/passkeys` | `{"id": hex}` (primary) | 204 |
| `GET /v1/account/factors` | none | 200 `{"pin", "pin_locked_secs", "pin_unusable", "passkeys": [hex]}` |

**Multiple accounts on one installation.** Each account is a separate account on the server:
its own device ID and authentication key, identity keys, mailbox and push registration. The app
holds one device per account and signs each request as that account's device. One push token
registered for several accounts is sealed and stored under a separate keyed index per device.
The push service still opens each registration to deliver, so it can see that one token belongs
to several devices: it links them. Where the platform allows several tokens per installation
(FCM instance IDs, Web Push subscriptions), the app registers one per account. APNs gives one
token per app installation, so on iOS the link remains (threat model §6).

### 4.8 Encrypted profiles and sealed sender

Decision D-0054 (M5).

**Profiles.** A profile is one blob, encrypted by the client under its 32-byte profile key, which
it shares only with contacts. The blob holds the name, avatar pointer, about, status line and
profile links; the server never sees inside it.

```text
PUT /v1/profile:
version u8 (1) | profile_version u64 | access_key [16] | flags u8 (1 anyone may send sealed)
  | len u16 | blob (1..=8192)
```

- The version must exceed the stored one (409 `{"error": "version"}`).
- A pending device may not change it (403 `pending`).
- `POST /v1/profile/fetch` `{"account": hex}` answers `{"version", "profile": hex}` and is
  throttled per requesting account.
- Avatars are encrypted by the client and uploaded with `PUT /v1/profile/avatar` (a signed body,
  at most 512 KiB). They are stored in RustFS under a fresh random 32-byte ID (`p/<hex>`), which
  is answered as `{"avatar": hex}` and goes inside the profile blob.
- `GET /v1/profile/avatar/<id>` serves only an account's current avatar. Replaced ones are
  deleted by the worker after an hour; all of them go when the account is deleted.

**Sealed sender** (behind `[api.profiles] sealed_sender`, off by default; Prompt 12 verifies it):

- **Access key.** `access_key = HKDF-SHA256(salt = "songbird.v1", ikm = profile_key,
  info = "unidentified-access")[..16]` (`songbird_keys::sealed::access_key`). Contacts derive it
  from the profile key. The server stores only it, plus the "anyone may send" flag.
- **Sender certificate.** `GET /v1/certificate`, from a listed device that has uploaded its
  identity key:
  `version u8 (1) | key_id [8] | account [16] | device [16] | identity_key [33] | expires_ms u64 | signature [64]`.
  - The signature is Ed25519 by the certificate key over `"songbird.v1/sender-certificate\0"`
    followed by every byte before it.
  - `key_id` is the first 8 bytes of SHA-256 of the key; lifetime 24 hours (configurable, at
    most a week).
  - The sender puts the certificate inside the end-to-end encrypted envelope. The recipient
    checks the signature against the pinned key, the expiry, and the identity key against the
    sender's.
- **Sealed send.** `PUT /v1/messages/sealed` carries an ordinary `Send` frame (no MLS commit),
  with no device signature, and the header `Unidentified-Access-Key: hex(16)`. The header holds
  the XOR of the access keys of every addressed account that is not open
  (`songbird_keys::sealed::combine`).
  - The server recomputes it from what it stores and compares in constant time.
  - An account with no access key that is not open cannot be reached sealed (401).
  - The device-list check applies as for any send.
  - Throttles: per IP range and per recipient account.
  - The server learns who receives, never who sent.

### 4.9 Key transparency and security notifications

Decisions D-0055 and D-0057. Key transparency runs on Meta's `akd` 0.13 (the owner's decision of
2026-11-16), a verifiable key directory with the WhatsApp v1 configuration:

- labels hidden behind a VRF;
- lookup proofs of a label's current value (and of non-membership);
- key-history proofs;
- append-only (audit) proofs between epochs.

What the directory records about each account:

- **Label** = `kind u8 (1 identity, 2 device list, 3 username) || account [16]`. Labels are
  deterministic, so each account has one label per kind. akd shows everyone the one value of a
  label, and its VRF keeps labels out of the tree.
- **Value** = `HMAC-SHA256(salt, "songbird.v1/kt-value\0" || kind || raw)`.
  - `raw` is SHA-256 of the identity key or of the signed device list, or the username hash (32
    zero bytes once released).
  - `salt` is a random per-account salt that whoever asks receives. A server that hands someone
    another salt cannot make the published value match another key.
  - The salt is deleted with the account, which leaves its values unlinkable (`akd` 0.13 cannot
    tombstone them). The labels keep the account ID and the epochs of its changes.
- **Publishing**: a registration, a new device list, or a username claimed or released bumps the
  account's generation in its own transaction. The worker publishes one epoch per sweep with the
  values that changed. There is one writer, enforced by the store: an epoch commits only from the
  epoch before it (a compare and swap in the same transaction). Nothing is missed, and a crash only delays it.
- **Signed head**: `version u8 (1) | epoch u64 | root [32] | time_ms u64 | signature [64]`. The
  signature is Ed25519 by the log key over `"songbird.v1/kt-head\0"` followed by the bytes
  before it. Apps pin the log key and the VRF key.
- **Proofs** travel in akd's own protobuf encoding (`akd::proto`), hex in JSON.

| Request | Body | Answers |
| --- | --- | --- |
| `GET /v1/kt/keys` | none | 200 `{"vrf": hex, "log": hex}` |
| `GET /v1/kt/head` | none | 200 signed head |
| `POST /v1/kt/lookup` | `{"account": hex, "kind": "identity" \| "device_list" \| "username"}` | 200 `{"salt", "head", "proof"}`; 404 (nothing published); 400; 429 |
| `POST /v1/kt/history` | the same | 200 `{"salt", "head", "most_recent": 64, "proof"}`; 404; 400; 429 |
| `POST /v1/kt/audit` | `{"from", "to"}` (at most 1,000 epochs apart) | 200 `{"proof"}`; 400 for epochs out of order or beyond the directory; 429 |
| `POST /v1/kt/roots` | the same | 200 `{"roots": [hex, …]}`, every epoch's root from `from` to `to` as the publisher recorded it (D-0171); 404 if one is missing; 400; 429 |

**What clients do** (with `akd_core`'s verifiers, `lookup_verify`, `key_history_verify` and
`audit_verify`):

- Check every head's signature; keep the newest verified head.
- Before trusting a contact's identity key or device list, verify a lookup proof for the
  contact's label (`kind || account`) against a verified head. The proven value must equal
  `HMAC(salt, context || kind || SHA-256(key or list in hand))`.
- Verify an audit proof between the last head seen and a newer one, with every root in between
  from `/v1/kt/roots` (the first and last must be the two signed heads' roots; D-0171). Two signed heads that no audit
  proof joins are evidence of a fork: show it and stop trusting the server.
- Owners check their own key history regularly. A version they did not publish is the warning.

**Security notifications.** An account identity key is never replaced (a new one is a new
account), and a device's identity key is fixed when it is linked. What changes is the device
list, and changes reach clients by three paths:

- the account's own devices get a signed `DeviceListChanged` notice (§3.9);
- a contact's next send is refused as stale by the device check (§3.1), and the contact fetches
  the signed list;
- the change appears in the directory with a new version.

In each case the app compares the new list with the last one it accepted and warns the user
about new devices.

### 4.10 GDPR: export, deletion and inactivity

Decision D-0056 (M7).

- **Export** (`GET /v1/account/export`, the primary, signed): JSON of everything the registry
  holds about the account:
  - the account row: creation, invites left, identity key, username hash, whether a username key
    is set, the signed device list, the profile, the access key, the PIN's state, the
    transparency salt, last use and the inactivity period;
  - username links, passkeys and avatars;
  - every device: keys, flags, prekey and KeyPackage counts, whether a sealed push registration
    exists, pending uploads;
  - groups (Prompt 5, D-0073): each group the account's devices are in, its epoch, those
    devices with their roles and its community; pending requests to join groups.
  Secrets derived from what the user knows (the PIN and username-key verifiers) are reported as
  present, not handed out. What cannot be attributed to the account (mailbox ciphertext, sent
  attachments, keyed throttle records, transparency leaves once the salt is gone, encrypted
  backups) is listed with its retention.
- **Deletion** (`DELETE /v1/account`, the primary): the outbox job deletes every Prompt 2 and 3
  store, then verifies it:
  - mailbox and push registrations;
  - upload grants and objects, and avatar objects;
  - devices, prekeys, KeyPackages and passkeys;
  - username (held 30 days, unlinked) and links;
  - link tokens;
  - the profile, device list, PIN and transparency salt on the account row;
  - pending requests to join groups (Prompt 5);
  - the record store's records and objects (Prompt 9). Group messages are no longer stored for the
    devices. Their leaves, and the account ID if it held a role, stay in each group's MLS state
    until a member commits the removal (D-0073).
  Encrypted backups keep the data until they expire (at most 93 days); a restore deletes it
  again through the journal (D-0041).
- **Inactivity** (owner, D-0047): each account chooses 6, 12 or 24 months
  (`PUT /v1/account/inactivity {"months"}`, the primary; default 12; a month is 30 days).
  - Any signed `api` request counts as use, recorded at most once a day.
  - A month and a week before the period ends, the worker sends every device a wake-up push. The
    app wakes, makes a request, and the clock restarts.
  - At the end, the account is deleted exactly as if it had asked.

### 4.11 Account recovery and the hand-over to a new phone (Prompt 10, D-0123)

The account identity key is in the user's backup (§11.5), so a brand-new device holding only
the recovery key can become the account's primary again; the transfer to a new phone (§11.9)
uses the same call with the old primary's consent instead of the second factor.

1. A **token**: `POST /v1/account/recover/token` (unsigned, `{"account": hex}`; throttled per
   IP range and per account) for a recovery, or `POST /v1/account/handover` (signed by the
   enabled primary; throttled per primary, apart from strangers' recovery requests) for a
   hand-over. Either answers `{"token", "device", "expires_in"}` like a link token (§4.6): 32
   random bytes stored only as their hash, single use, ten minutes, reserving a device ID. The
   token's kind is stored with it: a link never accepts a recovery or hand-over token, a
   recovery never accepts a link token, and only link tokens count towards the device limit.
   For an unknown or deleting account the recovery endpoint answers the same way with a token
   it never stores, so it tells a stranger nothing (D-0125).
2. `POST /v1/account/recover` (unsigned):
   `account [16] | account_sig [64] | link request` (§4.6, with the second factor of §4.7 when
   the token is a recovery token). `account_sig` is Ed25519 by the **account identity key** over
   `link_transcript("songbird.v1/recover-account\0", …)`, the same bytes the new keys sign.
3. The server checks, before anything changes: the range throttle; the new authentication and
   device identity keys' two proofs; `account_sig` under the account's stored identity key (an
   unknown account and a wrong signature: one answer, 400 `signature`); the token (unexpired,
   this account, this device, a recovery kind; 409 `token`); for a recovery token the PIN or a
   passkey when the account has one (counted and locked exactly as for linking, 403 `factor`,
   403 `locked`).
4. In one transaction: the token is consumed; **every enabled device of the account is
   disabled** (marked for removal from its groups, as a list removal does) and a
   `DeleteDevices` job goes to the outbox; other outstanding tokens are dropped; the new device
   is created as the enabled, non-pending primary. 200 `{"account", "device", "removed"}`.
5. The new primary signs a device list naming itself (version above the stored one, §4.6), and
   restores its backup. Contacts see a device change and the **same** safety number.

The recovery key is therefore the account unless a PIN or passkey is set: whoever holds it alone
can take the account over and sign out every device. The app asks for a PIN when backups are
turned on and says so where it shows the key (threat model §3.27).

## 5. Encrypted content schemas (Prompt 4)

Decision D-0059 (M1). Specification: `crates/content/proto/content.proto`; Rust types, padding,
franking and validation: `crates/content`. **No service links that crate** (a test checks every
manifest): the server never decodes content. What the server must act on travels in fixed fields
of the `Send` frame (§1.2), never inside the ciphertext.

### 5.1 Layers

```
Send frame (§1.2): per-device envelopes or one shared body, plus the server's fields (M3, M4)
  └─ envelope: the session layer's ciphertext (pairwise PQXDH and Double Ratchet, §4.4;
     MLS for groups, Prompt 5; sealed sender wraps it with the sender certificate, §4.8)
       └─ padded plaintext: encode(Plaintext) || 0x80 || 0x00*, to a multiple of 160 bytes
            └─ Plaintext { content: encode(Content), franking_key, profile_key }
                 └─ Content { version, required_version, message_id, sent_at_ms,
                              body: data | receipt | typing | sync | request_response }
```

- **Padding.** Exactly `0x80` then zeros up to the next multiple of 160 bytes (1 to 160 bytes of
  padding; one encoding per length). Readers refuse anything else. Padding hides lengths within a
  bucket from the server, which sees envelope sizes (threat model §1).
- **Bounds.** A padded plaintext is at most 64 KiB and its `Content` at most 61,440 bytes, both
  checked before decoding; after decoding every list, string and ID is held to the bound written
  next to it in `content.proto` (`songbird_content::validate`).
- **IDs.** Accounts, messages, groups, calls and sticker packs are 16 bytes; keys and digests 32.
  A message is named by (author account, `message_id`): `message_id` is 16 random bytes chosen
  by the sender, and a repeat of the same pair is dropped (this is how resends after a restore,
  §3.6, and retries beyond the 6-hour idempotency window are made harmless).
- **Text.** UTF-8. Formatting spans and mentions count UTF-8 bytes (not UTF-16 units as Signal
  does) and must start and end on character boundaries; a mention's span covers the placeholder
  U+FFFC, and names the account. Styles: bold, italic, strikethrough, monospace, spoiler.
- **Times** are the sender's clock (`sent_at_ms`), except where §5.3 gives the server's.

### 5.2 Versions

- `version` is the writer's schema version (1). `required_version` is the lowest version that can
  show the message; an older reader shows "update to see this message".
- Readers ignore unknown fields, unknown `oneof` members and unknown enum values (a newer writer
  added them) instead of refusing the message. Field numbers are never reused or retyped.
- The compatibility suite (`crates/content/tests/compat.rs`) decodes every published version's
  vectors with the current reader, and a simulated newer writer's fields at three depths.

### 5.3 Franking

A message to another account is franked: the sender draws a 32-byte `franking_key`, puts it in
the `Plaintext`, and sends the commitment

```
commitment = HMAC-SHA256(franking_key, "songbird.v1/franking\0" || content)
```

(`content` = the encoded `Content` bytes, exactly as carried) in the `Send` frame's `FRANKED`
block (§1.2). The server stamps it (§5.13); the recipient checks that the stamped commitment
opens to the content it decrypted, and **drops a franked message whose commitment does not
open**, so every message it shows can be reported (§5.14). Visible messages (`DataMessage`) to
another account must be franked; receipts, typing and sync messages need not be.

### 5.4 Chat messages (`DataMessage`)

`group_id` (16 bytes) marks a group or channel message; empty is a 1:1 chat. `expire_timer_s` is
the chat's disappearing timer the sender applied. Exactly one kind:

| Kind | Rules |
| --- | --- |
| `Message` | Text with spans; up to 32 attachments (voice notes and video notes are attachments with a flag; voice notes carry a waveform); a quote (`MessageRef`, a 2 KiB preview, a thumbnail); a forward counter; up to 8 contact cards; a location pin (degrees × 10⁷); a sticker reference (pack ID, pack key, sticker ID); a poll; an event; `view_once`; up to 4 link previews; an album (`Album`). At least one visible part. |
| `Reaction` | One per reactor and message; add, replace (a new emoji) or remove. The reactor's highest `counter` wins (§5.5). |
| `Edit` | The sender's own message, within **24 hours** (D-0058); the highest `revision` is shown and every revision is kept as history. |
| `Delete` | Delete for everyone, the sender's own message, within **24 hours** (D-0058); group admins delete others' (Prompt 5). |
| `PollVote`, `PollClose`, `Rsvp` | §5.5 |
| `Pin` | Pin for 24 hours, 7 days or 30 days, or unpin; at most 3 pinned per chat, the newest winning (§5.5). |
| `TimerChange` | The chat's disappearing timer: off, or 5 s to 4 weeks. |
| `Nudge` | A nudge (Prompt 9). |
| `CallEvent` | A call's outcome in the chat (Prompt 7). |
| `GroupUpdate` | A group or channel system message written by a client from what changed (Prompt 5 orders the changes themselves). |
| `ChatPolicy` | Restricted chat (§5.9). |

**Albums** (D-0186). Photos and videos sent together go one message each, so each can be
answered, reacted to, forwarded or deleted on its own, and each carries the same `Album`: a
16-byte random ID, its `index` from 0 and the album's `count` (2 to 100). Receivers draw the
album from the first item that arrives, with a place for each one still to come, in `index`
order whatever order they arrive in. An item may never arrive (its sender deleted it before it
was sent): apps show the album with what they have. The `Album` of a view-once message, or of a
message with no attachment, is invalid.

**Forwarding.** Forwarding a message sets `forward_count` to the original's plus one (a message
first forwarded has 1). From 5 the app shows "forwarded many times" and lets it be forwarded to
one chat at a time; otherwise to at most 5 chats at once. Apps enforce this; the server sees
nothing of it.

**Edit and delete windows** use the server's stamp time (§5.13) of both messages when both carry
one, otherwise the sender's `sent_at_ms`. The rule looks only at the two messages, never at when
they arrived, so every device agrees. A modified app can still ignore the window on its own
device, as in Signal and WhatsApp.

### 5.5 Order-independent state

Decision D-0060 (M2). Reference implementation: `songbird_content::state`; every app computes
the same. Every state is a function of the **set** of operations received, never of their
order or repeats (property tests: `crates/content/tests/convergence.rs`).

- **Last write per author, by the author's counter.** Reactions, poll votes and RSVPs keep, per
  (target, author), the write with the greatest `(counter, value)`. Each author numbers its own
  writes (any strictly increasing counter), so no clock is trusted, and a reused counter still
  breaks ties on the value's bytes.
- **A total order for state many people write.** Pins, the disappearing timer and the
  restricted-chat policy are decided by the greatest `(time, author, message ID)`. The time is the
  server's stamp (§5.13) when the message has one, else `sent_at_ms`.
- **Reactions:** one per reactor and message; a new emoji replaces, `remove` clears.
- **Polls:**
  - while open, each voter's vote with the highest counter counts. A vote for an option the poll
    lacks, or for several options in a single-choice poll, counts as none; no options retracts;
  - only the poll's author can close it, and a close names the poll (author and message ID).
    `heads` lists each voter's counter as the author saw it. From then on exactly those votes
    count, and nobody else's, so late votes cannot change a closed result on any device. Of two
    closes, the greater `(time, author, message ID)` wins;
  - votes may arrive before the poll and are judged once it is known;
  - a device keeps at most 64 votes per voter (the highest counters), which bounds what one
    voter can make it store.
- **Events:** RSVPs as votes (going, maybe, declined), the responder's highest counter winning.
- **Edits:**
  - the shown text is the author's valid edit with the highest revision; every valid revision is
    the history;
  - an edit is valid when its editor is the original's author and it falls within 24 hours of
    the original (§5.4);
  - at most 64 revisions are kept.
- **Delete for everyone:** by the author (or a group admin, Prompt 5) within 24 hours.
- **Pins:** per message, the greatest pin operation wins. A pin lasts its duration from its time,
  and at most 3 are shown, the newest pins first.

### 5.6 Receipts and message status

Decision D-0060 (M2).

| Status | Meaning | Produced by |
| --- | --- | --- |
| Sending | Not yet stored on the server | The sender's app |
| Sent (one tick) | Stored durably | The server's `Accepted` (§3.1) |
| Delivered (two ticks) | At least one device of the recipient (of every member) decrypted and saved it | Encrypted delivered receipts |
| Read (blue) | The recipient (every member) opened the chat | Encrypted read receipts, if enabled |
| Played | A voice or video note was played | Encrypted played receipts |
| Failed | Could not be sent after retries | The sender's app |

- **Receipts are content** (`ReceiptMessage`), end-to-end encrypted, so the server can neither
  fake nor hide them. They travel through the mailbox (never the ephemeral lane), so an offline
  sender gets its ticks later. Every device that saves a message sends a delivered receipt.
- **Only to the author.** A receipt goes to every device of the message's author, never to the
  rest of a group.
- **Forward only.** The status is computed from each recipient's greatest receipt (delivered <
  read < played), so it never moves back, whatever order receipts arrive in. A receipt also lifts
  a message the app had marked failed, since it proves the server stored it.
- **Groups ("Message info").** The audience is the members when the message was sent:
  - members who left since are no longer waited for, and members who joined later are not
    counted;
  - two ticks when every remaining audience member has a device that received it, and read when
    every one of them read it.
- **Batching.** The recipient's app gathers receipts per (chat, author, kind) for 1 second, or 10
  seconds in groups above 32 members, then sends one receipt:
  - delivered and played receipts list message IDs, at most 512 per receipt;
  - a read receipt says "up to here" (`up_to`), so opening a chat with 200 unread messages from
    one author sends one receipt;
  - the reader's own other devices get one `ReadSync` of what was read.
- **The read-receipt setting** is reciprocal and **applies to groups too** (D-0058): off means
  no read or played receipts sent, and none shown (at most two ticks). Delivered receipts are
  always sent.
- **Load** (the M2 report): in a 1,024-member group where members chat in bursts, receipts are
  47 % of all envelopes with the 10-second window (66 % with 1 second).

### 5.7 Typing

`TypingMessage` (started or stopped, optional group) travels only on the ephemeral lane (§3.7):
never stored, dropped first under pressure. Apps stop showing "typing" 15 seconds after the last
"started".

### 5.8 Sync messages

Sent only to the sender's own other devices (a send that addresses only the sender's account,
§5.10):

| Kind | Carries |
| --- | --- |
| `sent` | A transcript of what this device sent: the chat (or a broadcast list's accounts), the exact `Content` bytes the recipients got, its franking key, and when the server accepted it. The content must be a `DataMessage` (nesting stops at one level). |
| `read`, `viewed` | Messages read, or view-once media and voice notes played, with when. |
| `starred` | Messages starred and unstarred. |
| `chat_state` | Archived, muted until, pinned, marked unread. |
| `settings` | Read receipts, typing indicators, link previews, the default disappearing timer (Prompt 9 adds the rest); unset fields are unchanged. |
| `blocked` | The whole block list (accounts and groups), replacing the previous one. The server keeps only what it must to drop blocked senders (§5.11). |
| `call_history` | Call log entries (Prompt 7's calls), so every device shows the same history. |
| `request_response` | What the user did with a message request (§5.10). |
| `storage` | The account record store (§10.1): a new storage key after a rotation, or "the store changed, fetch now". Settings live in the store; the `settings` kind is superseded. |

### 5.9 Restricted chat

`ChatPolicy` (in a 1:1 chat, or in a group's settings from Prompt 5) tells every participant's
app to refuse exporting the chat and saving its media automatically. Apps enforce it; **the
server only carries it, and a modified app can ignore it.** It is a courtesy between honest apps,
not a protection against a participant: anyone can photograph a screen.

### 5.10 Message requests, contacts and lanes

Decisions D-0058 (the owner's rules) and D-0062 (M4).

**Contacts are proven by the access key.** An account's access key is derived from its profile key
(§4.8), and the profile key goes only to contacts:

- when a request is accepted (`RequestResponse{ACCEPT}` with `Plaintext.profile_key`: beside
  the content, never inside what franking commits to, so a report never reveals it);
- or when users meet by username, link or QR code;
- or with every message to an existing contact after a rotation.

The server keeps no contact list: a send carries the proof.

**Every send to another account** (identified, over the WebSocket or `PUT /v1/messages`) falls
in one lane, decided from the directory snapshot alone, so the hot path makes no store round
trip:

| Lane | What the server sees | Rule |
| --- | --- | --- |
| Sync | Only the sender's own other devices | Always allowed |
| Direct | One other account, a contact | The send carries `ACCESS` with its access key |
| Open | One other account that is open ("anyone may send") | No key needed; its blocks apply (dropped silently) |
| Broadcast | Several other accounts (per-device envelopes) | Every closed one has an access key, and `ACCESS` carries their XOR; at most 256 accounts by default; a budget of accounts per hour |
| Request | One other closed account, `REQUEST` set | Must be `FRANKED`; budgets per sender, per (sender, recipient) and per recipient; dropped silently if blocked or refused |
| Group | A shared body (MLS) | Until Prompt 5 authorizes membership: every closed account it reaches is a contact (`ACCESS` carries their XOR; groups share their members' profile keys) or the body is refused. No budget beyond the group-send rate |
| Sealed | `PUT /v1/messages/sealed` | The sealed endpoint checks the access keys (§4.8); `ACCESS` and `REQUEST` are refused there |

- A send to a closed account with neither a valid key nor `REQUEST` is refused with reason 10
  ("access required"). The app then fetches the profile key again (if it had one) or sends a
  request.
- An account with no profile (no access key) is reachable by requests only. **Every app
  publishes a profile, with its access key, at registration.**
- A request addressed to more than one other account, or not franked, is refused with reason
  11.
- A request is never sealed, so it can be blocked and reported with a sender the server can
  attest.

**Answering a request** (all inside encrypted content, §5.8):

- **accept**: share the profile key with the requester; from then on it sends as a contact;
- **delete**: nothing reaches the requester;
- **block**: §5.11;
- **report**: §5.14.

Every response is synced to the user's own devices.

**The request policy** (`GET`/`PUT /v1/account/requests`, `{"from": "everyone" | "nobody"}`):
with "nobody", requests are accepted and dropped silently; contacts are unaffected. Refusing
requests may be done from any listed device, accepting them again only from the primary (D-0058).

**Budgets** (`[api.requests]` and `[delivery.requests] limits`, kept in `valkey-hot` under keyed
names, shared by every node and transport):

| Budget | Default |
| --- | --- |
| Requests per sender account | 20 an hour, burst 10 |
| Requests per (sender, recipient) | 10 an hour, burst 5 |
| Requests per recipient account | 100 an hour, burst 30 |
| Broadcast accounts per sender | 1,024 an hour, burst 256; at most 256 a send |

Over budget: reason 1 with a retry hint. The budgets are spent **before** the block and policy
checks, so a blocked sender gets exactly the answers an unblocked one gets.

**A dropped send leaves the same traces as a delivered one** (D-0064):

- the whole send is stored under its idempotency key with a deadline already passed, so it is
  never fetched or routed and the next sweep deletes it;
- its idempotency record names the same devices, and a reused key gets the same answers (reason
  7, a replay);
- the sender's own envelopes (its sync transcript) are stored live under a key derived from the
  send's.

**Who may loosen:** opening the account ("anyone may send" in the profile, false to true) is the
primary's, like unblocking and accepting requests again; a linked device may keep it open or
close it. A send from a device not yet in the snapshot is refused as unavailable (never treated as
sealed).

**Rate limits per conversation type** (the prompt's requirement), counted per device unless said:

| Lane | Limit |
| --- | --- |
| Every send | 50 a second, burst 200 (per connection, and across `api` nodes for HTTP) |
| Group sends (shared bodies) | Also 10 a second, burst 40 |
| Requests and broadcasts | The budgets above, per account across the cluster |
| Channels | Prompt 5, once the server can tell a channel from a group |

What is not gated, stated plainly:

- **Ephemeral frames** (typing) to a stranger's device are not checked. They are end-to-end
  encrypted, so a client without a session with the sender drops them; per-connection limits
  bound them.
- **Group invitations from strangers** go as a message request first (a Welcome to a
  non-contact needs that member's key, like any shared body).

### 5.11 Blocking

- **`PUT /v1/blocks`** `{"account": hex}` (any listed device) and **`DELETE /v1/blocks`** (the
  primary only, D-0058).
- **What the server keeps:** one row named by `HMAC(k_pair, blocker ‖ blocked)[..16]`, a keyed
  tag of the owner `HMAC(k_owner, blocker)[..16]`, the time, and a per-owner counter (at most
  1,000). No account ID is stored. The keys derive from the requests secret every node holds,
  so the server (with its key) can test whether one account blocked another; a database or
  backup thief cannot.
- **What it does:** a request from a blocked account, or any send to an open account that blocked
  the sender, is accepted and never delivered. A blocked
  contact still holding the access key is cut off by a client rule: **blocking rotates the
  profile key**, sharing the new one with every remaining contact (as Signal does), so the old
  access key fails (reason 10) and only the request lane, where the block applies, is left.
- **Sealed senders** are blocked by the client: it drops sealed messages whose certificate names
  a blocked account. Rotating the profile key also ends their sealed access.
- **The full block list** (accounts and groups) syncs between the user's own devices as an
  encrypted `blocked` sync message (§5.8).
- **Deletion:** an account's block rows are deleted with it (found by its owner tag) and
  verified. A blocked account's deletion leaves the rows naming it as keyed hashes until the
  blocker unblocks or is deleted.

### 5.12 Client-only features

Lock chat, free up storage, the media and links overview, in-chat search, save to photos, export
chat and groups in common live entirely on the device: they read the app's own database. Nothing
in the protocol prevents them, and the server gains nothing for them:

- export and save to photos are refused only by a restricted chat's policy (§5.9);
- groups in common are computed from groups the device belongs to (Prompt 5), with no server
  query.

### 5.13 Franking stamps and disappearing envelopes (server)

Decision D-0061 (M3).

**Franking stamps.** For a `FRANKED` send, the server stores every envelope addressed to another
account behind a stamp; the sender's own devices get their envelopes as sent:

```
magic F5 53 42 46 52 4B 00 01 | version u8 (1) | key_id [8] | commitment [32]
  | stamped_at_ms u64 | tag [32] | the client's envelope
tag = HMAC-SHA256(franking key, "songbird.v1/franking-stamp\0" || key_id || commitment
                  || stamped_at_ms || sender account [16] || recipient account [16])
```

- The sender account is all zeros for a sealed send (the server does not know it). The recipient
  is all zeros for a shared body, which is stamped once.
- `key_id` names the franking key: `HKDF-SHA256(salt "songbird.v1", secret, info "franking")`,
  identified by the first 8 bytes of `SHA-256("songbird.v1/franking-key-id\0" || key)`. Every
  node that sends holds the current key (secrets store); `api` also keeps earlier keys for
  reports.
- The tag is a MAC only the server can check. It names neither account in the clear; whoever
  holds the key could test candidate senders for an envelope still undelivered (threat model
  §1).
- A recipient app splits off the stamp (`songbird_proto::envelope::Stamp::split`), decrypts, and
  **drops the message unless the stamped commitment opens to the content** (§5.3). It keeps the
  stamp with the message: a report needs it (§5.14). `stamped_at_ms` is the server's time, used
  for the edit and delete windows (§5.4).
- User envelopes beginning with `F5 53 42` are refused (reason 9), so no client can forge a stamp
  or a system frame.

**Disappearing envelopes.** An `EXPIRES` send gives each of its envelopes a deadline,
`expire_after_s` after the store:

- every mailbox (in memory, SurrealDB, Iggy) **never delivers it after the deadline**, and its
  `seq` is skipped, as §3.3 allows for expired envelopes;
- the mailbox **deletes it at the next sweep** even if no device ever connected. In Iggy its key is
  punctured, so it cannot be read again, nor come back on a restart;
- apps set it to the chat's disappearing timer for messages in a chat with one, and for view-once
  media. Nothing about such an envelope is kept beyond delivery: the server keeps no copy of any
  delivered envelope, and backups never hold the mailbox (D-0040). View-once attachments are
  deleted after one fetch by Prompt 6's upload grants;
- a resend after a restore (§3.6) carries the time left, and apps do not resend what has already
  disappeared.

### 5.14 Reporting

Decision D-0063 (M5). A recipient shows the server one message it received; nothing else about
the conversation is revealed.

```
POST /v1/reports (a listed device, signed body, at most 61,602 bytes):
version u8 (1) | reason u8 (1 spam, 2 harassment, 3 illegal, 4 impersonation, 5 other)
  | key_id [8] | commitment [32] | stamped_at_ms u64 | tag [32]     (the stamp, §5.13)
  | sender [16] | franking_key [32] | content_len u32 | content       (1 to 61,440 bytes)
```

The app keeps each franked message's stamp and franking key, and its encoded `Content` exactly
as received. To report, it sends them with the sender it decrypted. The server then checks, in
order:

1. **The reporter's budget:** 20 reports an hour, burst 10, keyed in `valkey-hot`. It is spent
   before anything else, so reports cannot probe stamps. Over it: 429.
2. **The franking key:** the current key or one still accepted (`[api.send]
   previous_franking_keys`). Otherwise 403 `key`.
3. **The window:** the stamp is at most 30 days old (`window_days`) and not from the future.
   Otherwise 403 `expired`.
4. **The stamp,** which binds the sender and the recipient. The MAC must verify for the named
   sender (attested), else for no sender (a sealed send: the sender is only the reporter's claim);
   and for the reporter's account, else for no recipient (a group's shared body, which any member
   may report). Otherwise 403 `stamp`. So a message can be reported only by a recipient, and
   only against the sender who sent it.
5. **The commitment:** it must open to the content under the franking key (constant time).
   Otherwise 403 `commitment`.

Then the server seals a **minimal report** to the moderation key and queues it (204):

```json
{"version": 1, "reason": "harassment", "sender": hex, "sender_attested": true,
 "conversation": "direct" | "group", "stamped_at_ms": …, "reported_at_ms": …, "content": hex}
```

- **Sealing:** X25519 with a fresh ephemeral key, HKDF-SHA256 under the domain
  `songbird.v1/report`, AES-256-GCM
  (`songbird_keys::provisioning::seal_with(Domain::Report, …)`), to
  `[api.reports] moderation_key`. The private half stays offline with the moderators
  (`songbird-admin reports keygen`).
- **What the report leaves out:** the reporter, any other message, the conversation's members.
- **Storage:** the row is named by a keyed hash of the stamp's tag, so the same message is
  reported once (409 `duplicate`). Rows are kept `retention_days` (30 by default, at most 90),
  then swept by the worker.
- **Moderators** list, open (decrypt, and decode the content with `songbird-content`) and
  delete reports with `songbird-admin reports`.
- **In memory only:** `api` holds the content in memory while it verifies a report; it never
  decodes it, and never stores it unsealed.
- **A report does not block:** an app that reports usually also blocks the sender (§5.11).

## 6. Groups, roles and channels (MLS) (Prompt 5)

Decisions D-0065 (the owner's), D-0066 (the delivery-service core) and D-0067 (its integration).
Milestone 1: groups, commits, Welcomes and group messages. Roles, invite links, communities and
channels follow in M2–M4.

### 6.1 The model

- **Members are devices.** Each device is one MLS leaf whose basic credential is its 16-byte
  device ID, and only devices their account's signed list vouches for (§4.6) may be in a group.
- **Every group uses X-Wing**, `MLS_128_MLKEM768X25519_AES128GCM_SHA256_Ed25519` (`0x004F`):
  post-quantum key exchange and Ed25519 signatures. KeyPackages for it are published as in
  §4.5.
- **Handshakes are `PublicMessage`s** (OpenMLS's pure-plaintext wire policy), so the server can
  check them. **Application messages are `PrivateMessage`s**; the server reads only their group,
  epoch and content type.
- **The server keeps** each group's public state (OpenMLS `PublicGroup`), its epoch, its roster
  (member devices), the `GroupInfo` of the current epoch, and the hash of the commit that made
  it. Never a group secret, a name or any content (threat model §1).

### 6.2 Creating a group: `POST /v1/groups`

`version u8 (1) | group [16] | info_len u32 | GroupInfo | tree_len u32 | ratchet tree`, both parts
`MLSMessage`/TLS as OpenMLS exports them, at most 4 MiB in all.

- The group is at epoch 0, uses `0x004F`, and its only leaf is the requesting device, which
  signed the `GroupInfo`. Its context carries the extensions of §6.6 and `external_senders`
  naming the delivery service, exactly (§6.11).
- Answers:
  - 204;
  - 409 `exists`;
  - 400 with one of `request`, `malformed`, `kind`, `group`, `ciphersuite`, `sender`,
    `credential`, `group_info` or `invalid`;
  - 403 `pending`;
  - 429 after the per-device commit budget (2 a second, burst 10, shared with commits).

### 6.3 Committing: `POST /v1/groups/commit`

`version u8 (1) | group [16] | bundle`, where the bundle is
`version u8 (1) | commit_len u32 | commit | welcome_len u32 | Welcome | info_len u32 | GroupInfo`
(`welcome_len` is 0 when nobody is added).

The server validates the commit fully (RFC 9420, OpenMLS `PublicGroup`), then checks that:

1. a member's commit is signed by the leaf whose credential is the requesting device;
2. every added KeyPackage names a listed device that is not already in the group;
3. updates keep their leaf's identity;
4. the proposals are add, remove, update, self-remove and (§6.6) group-context changes only.
   Pre-shared keys, re-init and custom proposals are refused. A proposal by reference is a
   member's, or a `Remove` from the delivery service (§6.11);
5. an external commit comes from a device whose account already has a device in the group, and
   removes at most that device's own old leaf. A reinstalled device (a new device ID) joins
   without removing anything, and a member then removes the dead leaf;
6. the group stays within 1,024 leaves;
7. the Welcome holds exactly one secret per added KeyPackage, and nothing else;
8. the `GroupInfo` is the new epoch's, signed by the committer's leaf.

The new state is written only if the group is still at the commit's epoch, in the same
transaction as its fan-out. So of several concurrent commits from one epoch exactly one wins.

**Fan-out.** The commit (`MLSMessage` bytes, as uploaded) goes to every device in the group
before or after it, except the committer and the added devices. Removed devices get it, so they
learn they were removed. The Welcome goes to the added devices.

The answer comes only once every recipient's mailbox holds it (invariant 1):

- 200 `{"epoch": n}`, the new epoch;
- 409 `{"error": "stale", "epoch": n}`: another commit won. Process it (it is in your mailbox),
  then commit again from epoch `n`;
- 404: no such group, or the device may not read it (neither in it nor a device of an account in
  it: the same answer, before any work; D-0074);
- 400 with a reason (as §6.2, plus `proposal`, `external`, `duplicate`, `size`, `welcome`);
- 403 `pending`; 403 `key_package` if an added KeyPackage is not one its device published to the
  key directory (a claimed single-use package or its last-resort one; D-0074);
- 429 or 503: retry the same bundle.

A commit that already won is answered 200 again, with its fan-out finished. Fan-outs a crash
interrupted are finished by every `api` node within seconds. They are stored under idempotency
keys only the servers can derive, so a member cannot pre-empt them.

### 6.4 Joining: `POST /v1/groups/state`

Reads (`/state`, `/log`, `/v1/communities/state`) take 5 a second per device, bursting to 20
(D-0074).

`version u8 (1) | group [16]`. Answers 200 `epoch u64 | info_len u32 | GroupInfo | tree_len u32 |
ratchet tree` to:

- a device in the group (a device added by a Welcome fetches the tree here);
- a listed device of an account with a device in the group (it joins by external commit, §6.3,
  rule 5).

Anyone else gets 404, the same answer as for no group.

- **Member accounts** (D-0155): for a device in the group, the answer to `POST
  /v1/groups/state` ends, after the meta blob, with `n u16 | n × (device [16] | account [16])`:
  each member device's account. Devices check every pair against that account's signed device
  list before using it, so the server can leave a device out but not attribute it to another
  account. A link's state (`/v1/groups/links/open`) has no such part.
- **Clients check commits too** (D-0155): before merging a member's commit, a device builds
  the commit's change in accounts from pairs it verified and runs the same `authorize` the
  server ran; a commit it refuses ends its following of the group (it shows why). It also drops
  group messages from accounts the roles do not let send, and pins from accounts that may not
  pin.

### 6.5 Group messages: `Send` with `GROUP` (`0x40`)

Flags `SHARED | GROUP`; the `GROUP` block (`group [16]`) comes after every other block; `count`
is 0 and no device is listed. The shared body is one `MLSMessage` carrying a `PrivateMessage`.
The server reads its prefix, never decrypting it, and requires:

- that it is an application message of the frame's group;
- that the sending device is in the roster;
- that its epoch is the group's current epoch or the one before (a message sent just as a commit
  landed).

It stores the body once for the roster minus the sender. `FRANKED` and `EXPIRES` may be set;
`ACCESS`, `REQUEST` and `MLS` may not. Refusals:

- 12 not a member (also for no such group);
- 13 an invalid group message;
- 8 a stale epoch;
- 6 unavailable.

Delivery nodes cache rosters for at most a second, or less when a message names a newer epoch.
A removed device can therefore still post at its last epoch for up to a second on a node that
has not refreshed. Members who processed the removal know it was removed.

The flag `0x01` (the MLS commit in a `Send`, D-0026) is retired: the decoder refuses it, and
the sealed endpoint refuses `GROUP` (a sealed send hides the device membership is checked
against). The mailboxes still hold their unused epoch table; it goes with their next storage
format change.

### 6.6 Roles and permissions (M2, D-0070)

Every group's context carries two extensions from creation:

- **`required_capabilities`**, naming the roles extension and the `SelfRemove` proposal. Every
  member must support both; OpenMLS refuses to add a KeyPackage that does not.
- **The roles extension** (`0xF5B0`):

  ```text
  version u8 (1) | send u8 | add u8 | edit_info u8 | links u8 | call u8 | default_role u8 |
  flags u8 | n u16 | n × (account [16] | role u8)    accounts strictly ascending
  ```

  Version 2 (D-0076) is the same followed by the invite links' proof keys (§6.8): `m u8 | m ×
  (handle [16] | Ed25519 public key [32])`, 1 ≤ m ≤ 16, handles strictly ascending. A group
  with no link keys uses version 1, so each value has one encoding. Publishing or withdrawing a
  key needs the `links` role.

  - **Roles:** 0 read-only, 1 member, 2 moderator, 3 admin, 4 owner.
  - **Flags:** `0x01` channel, `0x02` joining needs approval.
  - **Unlisted accounts** have `default_role` (1 in a group, 0 in a channel). An entry never
    names the default role.

  A new group's only entry is its creator's account, as owner.

Roles are per account: every device of an account has its role. The server and every client
apply the same rule to a commit (`songbird_groups::roles::authorize`):

| Action | Who may |
| --- | --- |
| Add a person (an account not in the group) | Role ≥ `add` |
| Add a device of an account already in it | Member and above (one's own: anyone) |
| Remove another person's device | An admin or owner who outranks that person |
| Remove one's own devices, or a device no longer listed | Anyone |
| Commit the delivery service's proposal to remove a device (§6.11) | Anyone, and only for a device no longer in its account's signed list: one for a listed device refuses the commit, whoever commits it |
| Change the policy (send, add, edit info, links, call, default role, flags) | Admin and above |
| Change someone's role | An admin who outranks both the old and the new role, or an owner (who may make owners) |
| Lower one's own role | Anyone |
| Afterwards | At least one owner; entries only for accounts still in the group. When no owner is left (they left, or their devices are gone), the committer may name new owners from the highest role left. Changing the default role is a role change for every member it applies to |
| Send a message | Role ≥ `send`: the send path refuses others with reason 14 |
| Edit the group's info | Role ≥ `edit_info`: `POST /v1/groups/meta` |
| Manage invite links | Role ≥ `links` (§6.8) |
| See, approve (add) and decline join requests | Role ≥ `add` (§6.8) |
| Start a group call | Role ≥ `call` (Prompt 7) |
| Pin a message, delete others' messages | Moderator and above, **enforced by clients only** (inside the ciphertext) |

A commit the rule refuses gets 403 `role`, and clients drop it.

### 6.7 Leaving, metadata and the change log (M2)

- **Leaving:** `POST /v1/groups/propose` with `version u8 (1) | group [16] | MLSMessage`, a
  `PublicMessage` with the member's own `SelfRemove`. No other proposal is taken standalone.
  - The server queues it in the group's state and stores it in every other member's mailbox;
    the answer is 204.
  - The next committer includes it by reference and needs no role to. Members store it with
    OpenMLS's `store_pending_proposal`.
  - 409 `stale` with the epoch if a commit came first: propose again from that epoch.
- **The group's info:** `POST /v1/groups/meta` with `version u8 (1) | group [16] | expected u64
  | blob` (1 byte to 16 KiB). The blob is encrypted by the members; the server never reads it.
  - 200 `{"version": n}`;
  - 409 `stale` (the current version);
  - 403 `role` below `edit_info`.

  `POST /v1/groups/state` now ends with `meta_v u64 | meta_len u32 | meta`.
- **The change log** ("View member changes", for devices that join later):
  - **Writing:** a commit bundle of version 2 adds `log_len u32 | entry`, where entry is
    `ciphertext_len u16 | ciphertext (≤ 2 KiB) | signature [64]`. The signature is the
    committer's leaf key (Ed25519) over `"songbird.v1/change-log\0" | group | from_epoch u64 |
    ciphertext`.
  - **Checking:** the server verifies it and refuses the commit otherwise (400 `log`). An entry
    cannot be forged by the server, or replayed onto another epoch.
  - **Keeping:** the newest 1,000 entries per group.
  - **Reading:** `POST /v1/groups/log` with `version u8 (1) | group [16] | after u64`
    (`u64::MAX` for all) answers `n u16 | n × (epoch u64 | len u16 | entry)`, at most 200, for
    the same devices as the state.

- **The log key and entries on the device** (D-0156): a group's creator draws a random 32-byte
  **log key**; it reaches members in a group message (`GroupUpdate.log_key`) sent after every
  add (the "created" update for the first members) and by a member that processes a join by
  link. An entry's ciphertext is `nonce [12] | ChaCha20-Poly1305(log key, nonce, 1 | at_ms u64 |
  GroupUpdate{kind, actor, subjects}, aad = "songbird.v1/change-log-entry\0" | group |
  from_epoch u64)`. Apps write one with each add, removal, committed leave and settings change
  they commit, and show the entries they can open as "Member changes". Entries that do not
  open (written before a member had the key, or with another) are left out.

### 6.8 Invite links and join requests (M3, D-0071)

- **A link** is `handle [16]` (random, chosen by the app) and a 32-byte secret. Both travel
  only in the link itself (`https://…/g#<handle><secret>`, in the fragment). The app derives
  three keys from the secret with HKDF-SHA-256 (salt `"songbird.v1/group-link"`):
  - `auth` (info `"auth"`), shown to the server to open or use the link;
  - a key for the group's info (info `"info"`), which the server never sees;
  - `proof` (info `"proof"`), the seed of the link's Ed25519 key pair (D-0076). Before sharing
    the link, its creator commits the public key under the handle into the roles extension
    (§6.6, version 2); revoking the link also withdraws it.

  The server keeps `SHA-256(auth)`, the group, whether a join must be approved, a use count,
  an optional limit and an optional expiry (at most a year).
- **Managing:** for roles at least the policy's `links` role (admins, by default):
  - `POST /v1/groups/links` with `version | group | handle | SHA-256(auth) [32] | ttl u64
    (seconds, 0: none) | max_uses u32 (0: any) | approval u8`. 204; 409 `exists` (draw another
    handle) or `full` (16 links per group).
  - `POST /v1/groups/links/revoke` with `version | group | handle`. 204.
- **Opening:** `POST /v1/groups/links/open` with `version | handle | auth`. Every link that
  cannot be used (unknown handle, wrong `auth`, used up, expired, revoked) gets one answer: 404.
  The secret is compared in constant time. Otherwise 200:
  - `0 | group [16] | state` (as `/v1/groups/state`) if the device may join now;
  - `1 | meta_v u64 | meta_len u32 | meta` if a join needs approval: the encrypted name and
    picture only, no members.
- **Joining:** `POST /v1/groups/join` with `version | handle | auth | bundle`. The bundle is an
  external commit adding the device (`Sender::NewMemberCommit`), with the group's default role
  (it cannot change roles: RFC 9420 forbids external commits from changing extensions).
  - **The link proof** (D-0076): the external commit's authenticated data is `version u8 (1) |
    handle [16] | signature [64]`. The signature is the link's `proof` key over
    `"songbird.v1/link-proof\0" | group [16] | epoch u64 (the GroupInfo's) | handle | device
    [16] | key_len u16 | the new leaf's signature key`. The server requires it to name this
    link and to verify under the key the roles extension publishes for it.
  - Answers as `/v1/groups/commit`, including a retried join that already won.
  - 404 for a link that cannot be used; 403 `approval` if joins need approval; 403 `proof`
    without a valid proof (nothing is spent).
  - The link's use is counted in the same transaction as the commit, so two joins can never
    both take the last use.
- **Asking:** a link needs approval if the link says so or the group's policy has the approval
  flag (`0x02`).
  - `POST /v1/groups/requests` with `version | handle | auth`: the device's account asks to
    join. 204, also when it asked before (the first request's time is kept). 409 `open` (join
    instead), `member`, or `full` (256 pending per group). Requests lapse after 7 days.
  - `POST /v1/groups/requests/list` with `version | group`, for roles at least the policy's
    `add` role: `n u16 | n × (account [16] | age_secs u64)`, oldest first.
  - **Approving** is adding the account's devices in an ordinary commit. That commit removes
    the request.
  - `POST /v1/groups/requests/decline` with `version | group | account [16]`, the same role.
    204.
- **Budgets:** opening, joining and asking share a per-account budget of 1 a second, bursting
  to 5. Managing links and requests uses the commit budget.
- **What members see:** every join by link is an external commit from a new account, with the
  default role (OpenMLS refuses an external commit that changes extensions). Apps show it in
  the chat. Before following any external commit, an app applies the rule the server applies
  (`songbird_groups::ds::check_external_commit`, D-0078): only the joiner's own `ExternalInit`
  and `Remove`, inline; the joiner's leaf is its device; the only leaf removed is that
  device's own old one. From an account not yet in the group, it also checks the link proof
  against the keys its own roles extension publishes (`songbird_groups::links::verify`), and
  refuses one without a valid proof: a server cannot admit someone who never held a link the
  members made. Link keys are honest Ed25519 points (no small order, canonical), and apps
  verify proofs strictly (RFC 8032), so every implementation agrees.

### 6.9 Communities (M4, D-0072)

A community is a container of groups around an **announcement group**: a group whose policy lets
only admins send. The server keeps opaque links between IDs and an encrypted info blob (name,
description, picture: ciphertext under a key the members share). The community's admins are the
announcement group's admins, as its roles extension says, which every member verifies (§6.6).

- `POST /v1/communities` with `version | community [16] | announcement [16]`. For an admin of
  the announcement group.
  - 204;
  - 400 `announcement` if its members may send;
  - 409 `exists` if the community ID is taken or the group is already in a community.
- `POST /v1/communities/groups/add` with `version | community | group [16]`. For a device that
  is an admin of both the announcement group and the group (a community admin cannot claim
  someone else's group).
  - 204;
  - 409 `exists` (the group is in a community) or `full` (100 groups).
- `POST /v1/communities/groups/remove` with `version | community | group`. For an admin of the
  announcement group or of the group (a group may leave). 204.
- `POST /v1/communities/meta` with `version | community | expected u64 | blob` (1 byte to
  16 KiB), for the announcement group's admins. 200 `{"version": n}`; 409 `stale`.
- `POST /v1/communities/state` with `version | community`, for the announcement group's members
  (and their accounts' other devices). It answers `announcement [16] | meta_v u64 | meta_len u32
  | meta | n u16 | n × group [16]`.
- `POST /v1/communities/delete` with `version | community`, for the announcement group's owners.
  204. The groups stay.

Where the device is not in a group it must be in, or the community does not exist, the answer is
404. Joining a community is joining its announcement group (§6.8); its groups are joined one by
one, by their own links.

### 6.10 Private channels and group tiers (M4, D-0072)

- **A private channel** is a group whose policy has the channel flag (`0x01`): admins post, add,
  edit and manage links; subscribers join read-only (`Policy::CHANNEL`).
  - **Size:** up to 2,048 devices; a group is up to 1,024 (`Limits`). The limit follows the
    policy a commit leaves.
  - **Rate class:** a channel takes at most 1 post a second, bursting to 10, across the whole
    cluster and both transports (WebSocket and `PUT /v1/messages`), whoever of its admins
    posts (D-0077). This is on top of each device's own budget. Over it, reason 1
    `RateLimited` with the wait. While the cluster's store is unreachable, each node applies
    the class on its own.
  - **Subscribers see each other's devices** in the ratchet tree, as in any MLS group. Channels
    suit audiences that know each other; public channels are not built (D-0065 §4).
- **Tiers and receipts** (`songbird_groups::tiers`, the apps' rule; receipts are encrypted):

  | Tier | People | Delivered receipts | Read receipts | Window |
  | --- | --- | --- | --- | --- |
  | Small group | up to 32 | every device | if enabled | 1 s |
  | Large group | 33 and more | the primary device only | if enabled | 10 s |
  | Channel | any | none | none | — |

### 6.11 Server-proposed removals (D-0075)

When a device leaves the directory (its account is deleted, its primary's new list leaves it
out, or it removes itself), its leaf would stay in every group until a member removed it. The
delivery service proposes that removal itself, as the group's MLS external sender
(RFC 9420 §12.1.8.1). It can only propose; a member commits.

- **The service's key:** Ed25519, the same on every node. `POST /v1/groups/ds` with `version u8
  (1)` answers it (32 bytes); apps should also pin it.
- **Every group names it.** Its context's `external_senders` holds exactly one entry: that key
  with the basic credential `songbird.v1/ds`. The server refuses a group created without it
  (400 `kind`), and a commit that drops it or adds another sender (400 `proposal`).
- **The proposal:** a `PublicMessage` from `external(0)` carrying `Remove(leaf)`, at the
  group's current epoch, signed by the key (`SignWithLabel(…, "FramedContentTBS", …)`, no
  membership tag). The server queues it in the group's state and stores it in every listed
  member's mailbox. Signing is deterministic: the same leaf at the same epoch is the same
  bytes.
- **When:** within seconds of the device leaving, confirmed against the registry; and again
  after any commit that leaves a departed device in the group (proposals do not outlive an
  epoch).
- **What apps do** (the trust rule, `songbird_groups::roles::authorize`):
  1. On receiving a proposal from `external(0)`: keep it (`store_pending_proposal`) only if it
     is a `Remove` of a leaf that exists and whose device is no longer in its account's signed
     device list (after refreshing that list). Drop anything else: the service may propose
     nothing but removals. A removal of a device the app still sees listed is not stored, but
     kept aside: before processing a commit that references a proposal it lacks, the app
     refreshes the device lists and judges the kept proposals again (D-0078).
  2. Commit kept proposals soon (after a random delay of a few seconds, so members do not race;
     another member's commit may include them first). Committing them needs no role.
  3. Refuse a commit that includes a server removal of a still-listed device, whoever commits
     it, and any external proposal that is not a `Remove`.
  4. Refuse a commit whose context does not name exactly the service key the app pins
     (`songbird_groups::ds::extensions_allowed`).
- **The server applies the same rule** to commits. A commit removing a device the node's
  directory still lists is answered 503 (retry): the service proposes only removals the
  registry confirmed, and devices never come back, so that node's directory is seconds behind.

## 7. Attachments and packs (Prompt 6)

The server holds only ciphertext. It never parses media. It checks sizes and grants and counts
fetches (D-0079 to D-0084).

### 7.1 The attachment format (M1, D-0080)

Apps encrypt every attachment, thumbnail and pack asset like this. The reference implementation
is `songbird_content::attachment`; the sizes are `songbird_proto::media`.

- **Inputs:**
  - a fresh 64-byte random key (`AttachmentPointer.key`);
  - a fresh 32-byte random salt;
  - the plaintext of `n` bytes.

  Forwarding encrypts again under a new key and salt and uploads a new object. Reusing an object
  ID would let the server link two messages.
- **Keys:** `HKDF-SHA256(salt = salt, ikm = key, info = "songbird.v1/attachment\0" ‖ version, 64)`.
  - The first 32 bytes are the chunk key.
  - The last 32 bytes are the **key commitment**, sent as `AttachmentPointer.digest`.
  - Readers compare the commitment in constant time before opening any chunk. This makes the
    format key-committing: one object cannot open to two different files under two keys.
- **Version byte (the suite):**
  - `1`: ChaCha20-Poly1305 (RFC 8439);
  - `2`: AES-256-GCM, which apps write by default (D-0080: about 30 % faster than
    ChaCha20-Poly1305 on x86 with AES-NI).

  Readers accept both, so a device without AES hardware may write `1`. Any other value is
  refused.
- **Padding:** the plaintext is followed by zeros up to `padded = padme(max(n, 1024))`.
  - Padmé (PURBs, PETS 2019) costs at most 12 % and leaves O(log log n) bits of the size.
  - `n` itself travels only in the encrypted pointer (`size`).
  - Readers refuse padding that is not all zeros.
- **Chunks:** the padded plaintext is cut into 65,536-byte chunks. The last chunk may be shorter
  and is never empty, so `k = max(1, ⌈padded / 65,536⌉)`.
  - Chunk `i` is sealed with nonce `00 00 00 ‖ be64(i) ‖ last`, where `last` is `01` for chunk
    `k − 1` and `00` otherwise, and an empty AAD.
  - This is the STREAM construction (Hoang, Reyhanitabar, Rogaway, Vizár; as in `age`). A
    reordered, dropped, truncated or extended chunk fails at that chunk.
- **Layout:** `version (1) ‖ salt (32) ‖ chunk₀ ‖ tag₀ ‖ … ‖ chunkₖ₋₁ ‖ tagₖ₋₁`.
  - Total length: `33 + padded + 16k`.
  - Chunk `i` starts at byte `33 + i × 65,552`. Range requests fetch whole chunks, plus the
    33-byte header once.
- **What the server checks:** an upload grant is only issued for a length the format produces.
  `songbird_proto::media::padded_from_ciphertext` recognises such a length and refuses every
  other, so an uploader cannot choose an exact size.

Test vectors (§13) are in `crates/content/vectors/attachment/v1.txt`:

- key 64 × `11`, salt 32 × `22`, plaintext byte `i` = `i mod 256`;
- each suite at 0, 5 and 70,000 bytes;
- each line has: version, `n`, ciphertext length, commitment, SHA-256 of the ciphertext, and the
  ciphertext's first 32 bytes after the header.

`tools/attachment-vectors/check.mjs` reproduces every line with Node.js's OpenSSL, from this
section rather than the Rust code.

### 7.2 Upload grants and uploads (M2, D-0081)

Every request is signed by the device (§3). Upload bodies are the client's own ciphertext: they
are not signed, only held to their exact length.

| Request | Body | Answer |
| --- | --- | --- |
| `POST /v1/attachments` | `{"size": <ciphertext length>, "fetches": <recipient devices>}` | 201 `{"id": <64 hex>, "part_size": <bytes>, "parts": <count>}`; 400 for a length the format (§7.1) never produces, past the limit (2 GiB by default), or 0 or more than 1,024 fetches; 429 with `Retry-After` past a budget |
| `PUT /v1/attachments/<id>` | the whole ciphertext, `Content-Length` = `size` | 201; 400 for another length; 404 not this device's grant, or used |
| `PUT /v1/attachments/<id>/parts/<n>` | part `n` (1 to `parts`): `part_size` bytes, the last one `size − (parts − 1) × part_size` | 204; 400 for another length; 404 no such part, or not this device's grant |
| `GET /v1/attachments/<id>/parts` | — | 200 `{"part_size", "parts": [n, …]}`: the parts stored with exactly their length, in order |
| `POST /v1/attachments/<id>/complete` | — | 201: the parts become the object and the grant becomes the attachment; 409 while a part is missing |

- **Budgets** (per account, `[api.media.budgets]`):
  - 300 grants per hour, bursting to 60;
  - 10 GiB granted per 24 hours, all of which may be spent at once.

  These numbers stay until Prompt 11's plans set them per plan. The records are keyed and name
  no object.
- **Resuming:** after any interruption, a client asks `GET …/parts` and sends only the parts
  that are missing. Parts may go in any order and in parallel; four at a time is what D-0081
  measured.
  - A broken part stores nothing; the client sends it again.
  - Each stored part renews the grant for its idle time (1 hour). A grant never lasts more than
    24 hours after it was issued.
  - Parts of an upload whose grant is gone are aborted by the next sweep.
- **Small attachments** (one part or less) use the single `PUT`.
- **tus 1.0** was built and measured over the same parts, then removed (D-0081). Clients that
  want a library can map tus's `PATCH` to consecutive parts themselves.

### 7.3 Downloads and the lifecycle (M3, D-0082)

Downloads carry no account and no device signature (D-0079). The 256-bit object ID, which
travels only inside end-to-end encrypted content, is the capability.

| Request | Answer |
| --- | --- |
| `GET /v1/attachments/<id>` | 303 `Location: /v1/media/<id>?e=<expiry>&z=<size>&s=<MAC>`, with `Cache-Control: no-store`; 404 if the attachment is unknown, fetched out or past 30 days; 429 past the IP range's budget |
| `GET /v1/media/<id>?e=…&z=…&s=…` | The object: 200, or 206 with `Content-Range` for one `Range: bytes=` range (`a-b`, `a-`, `-n`); 416 with `Content-Range: bytes */<size>` past the end. Several ranges, or a malformed one, get the whole object. `Accept-Ranges: bytes`; `Cache-Control: private, max-age=<until expiry>, immutable`. 403 for a wrong or expired signature; 404 once the object is gone |
| `POST /v1/attachments/<id>/fetched` | 204: one recipient device has the whole file and has verified it. The last confirmation deletes the record and queues the object's deletion. 404 once nothing is due |

- **The signed URL.** The MAC is HMAC-SHA256 over `"songbird.v1/media-url\0" ‖ id ‖ be64(e) ‖
  be64(z)`, truncated to 16 bytes, under a key derived from `[api.media].secret`.
  - The URL lives `url_ttl_secs` (10 minutes by default). A URL dated further ahead than that
    is refused.
  - Serving it needs no database access. It carries the size, so `Content-Range` can name the
    total.
- **Seeking.** A player fetches the 33-byte header once, then the chunks it needs:
  - chunk `i` is bytes `33 + i × 65,552` to `33 + i × 65,552 + 65,551` (shorter for the last
    chunk);
  - `Decryptor::chunks_for` maps plaintext offsets to chunks (§7.1).
- **The lifecycle.**
  - The grant's `fetches` is the number of recipient devices, the uploader's other devices
    included.
  - Each device confirms once, after its last chunk verified. Downloading alone counts nothing,
    so a player may read parts of a file many times.
  - The last confirmation deletes the attachment. Anything unconfirmed goes after 30 days (the
    worker's sweep).
  - Each source counts once per object: an IPv4 address, or an IPv6 /64 (D-0084). A recipient
    cannot delete a file before the others by confirming many times. Recipients behind one
    address count as one, and their file is kept until 30 days.
- **Throttles.** Redirects and confirmations are budgeted per IP range in keyed records: 6,000
  per hour, a burst of 600, /24 for IPv4 and /48 for IPv6. Signed-URL requests (every range a
  player asks for) have their own budget: 36,000 per hour, a burst of 3,600 (D-0084).
- **Forwarding** encrypts again under a fresh key and uploads a new object (§7.1). The server
  never copies an object, so two messages never share an ID.

### 7.3a Map tiles (D-0142)

`GET /v1/maps/v1/<z>/<x>/<y>` (z 0–14, x and y within the grid): the OpenStreetMap vector tile
(Mapbox Vector Tile 2.1, OpenMapTiles schema), 200 `application/vnd.mapbox-vector-tile`; 204 for
an empty tile; 404 outside the grid; 429 past the IP range's serving budget. Unsigned: no account
or device is named. Clients over-zoom beyond 14 and show "© OpenStreetMap contributors".

### 7.4 Packs (M4, D-0083)

One generic service carries sticker packs, chat scenes (IMVironments), sound stickers and
emoticon packs. The kind exists only inside the encrypted manifest (`PackManifest` in
`content.proto`; `songbird_content::pack`).

- **Objects.**
  - Object 0 is the manifest; objects 1 to n are the assets. There are at most 200 assets.
  - Each object is in the attachment format (§7.1) and at most 16 MiB; a pack is at most
    300 MiB in all.
  - The key of object `n` is `HKDF-SHA256(salt = "", ikm = pack key, info =
    "songbird.v1/pack-object\0" ‖ be16(n), 64)`.
  - The manifest's plaintext is `be32(length) ‖ PackManifest`, then zeros. A reader opens it at
    its padded length (`padded_from_ciphertext`) and takes its commitment from its header
    (`attachment::commitment`). Only the pack key opens it.
  - Each asset is opened against the commitment the manifest lists for it.
- **The link:** `https://<web domain>/pack#<pack ID, 32 hex>.<pack key, base64url>`. The key is in
  the fragment, which browsers and apps never send to a server. A sticker in a message is a
  `StickerRef` carrying the pack ID, the pack key and the item's object number.
- **Requests:**

  | Request | Body or header | Answer |
  | --- | --- | --- |
  | `POST /v1/packs` (signed) | `{"sizes": [each object's ciphertext length, the manifest first], "token": <64 hex: SHA-256 of a random 32-byte management token>}` | 201 `{"pack", "objects"}`; 400 outside the bounds or for a length the format never produces; 429 past the account's budgets (the whole pack counts as one grant and its bytes) |
  | `PUT /v1/packs/<pack>/<n>` (signed, unsigned body) | object `n`, exactly its size | 201; 400 for another length; 404 not the creating device, or the pack is complete or expired |
  | `POST /v1/packs/<pack>/complete` (signed) | — | 201 once every object is in the store at its size; 409 otherwise. The pack then forgets the creating device |
  | `GET /v1/packs/<pack>/<n>` | no account | 303 to the object's signed media URL (§7.3); 404 for an unknown or unfinished pack |
  | `DELETE /v1/packs/<pack>` | `x-songbird-pack-token: <the token, 64 hex>`, no account | 204; 404 for no pack or a wrong token, which look the same |

- **Lifetime (D-0079 §3):**
  - An unfinished pack goes after 24 hours.
  - A complete pack goes when deleted with its token, or one year after the last fetch of any of
    its objects, which the server records to the day.
  - A pack has no link to any account, so deleting an account cannot find its packs. The
    creator's devices keep the token, in their encrypted sync, to delete it.

### 7.5 GIF search (M4, D-0083)

`api` proxies the provider (GIPHY, D-0079 §4). The provider sees queries, never who sent them.

| Request | Body | Answer |
| --- | --- | --- |
| `POST /v1/gifs/search` (signed) | `{"q": <1–64 characters>, "offset": <0–4,999>, "lang": <2 lowercase letters, default "en">}` | 200 `{"gifs": [{"id", "width", "height", "gif", "still", "mp4"}], "next": <offset or null>}`; 429 past the account's budget (300 per hour, burst 60); 503 with `Retry-After` while the provider fails |
| `POST /v1/gifs/trending` (signed) | `{"offset", "lang"}` | as above |
| `GET /v1/gifs/media/<token>` | no account | the file (`image/gif`, `image/webp`, `image/png`, `image/jpeg`, `video/mp4`); 404 for a bad or expired token; 429 per IP range |

- Queries go in a signed body, never a URL, so no access log or proxy sees them.
- The provider's request is built from scratch:
  - `api_key`, `q`, `limit`, `offset`, `rating` and `lang`;
  - only the headers `Host`, `User-Agent: songbird` and `Accept`.

  No address, header or identifier of the app reaches the provider, and GIPHY's per-user
  `random_id` is never sent.
- Every file URL in an answer is a token `hex(be64(expiry) ‖ HMAC16 ‖ URL)`, valid one hour. It
  only ever names the provider's own hosts (`cdn_hosts`), so the proxy cannot be pointed
  anywhere else. Files the provider puts elsewhere are dropped from the answer.
- Answers are cached in memory for 10 minutes, under a keyed hash of language, offset and query.
  Files are cached in memory for a day (256 MiB at most). Queries are never written to disk or
  logged.
- Each node has a circuit breaker: after five consecutive provider failures (transport, timeout,
  5xx, 429) it leaves the provider alone for 30 s. File fetches never move it; they have their
  own budget and run at most 32 at a time (D-0084). Across the cluster, the provider gets at most `provider_per_sec` requests. The
  runbook is `docs/RUNBOOKS/media.md`.
- No link previews on the server: apps build them (§5).

## 8. Calls and the call key schedule (Prompt 7)

Decisions D-0085 (the owner's) and D-0086 (M1). Calls run on self-hosted LiveKit, always through
its SFU: there is no peer-to-peer path, so participants never learn each other's IP addresses.
Every audio and video frame and every data packet is encrypted end to end by LiveKit's frame
cryptor under keys only the participants hold. **No server component ever receives a media
key.** The reference implementation of everything in §8.1 to §8.5 is
`songbird_content::call`; nothing in it is linked by a service.

### 8.1 Calls, rooms and participant identities

- A call has a random 16-byte **call ID**. Its LiveKit room is named by the call ID in lowercase
  hex. The server chooses it (§8.6), so two calls never share a room.
- A device in a room appears under a **participant identity** the server chooses: an HMAC of the
  room and the device under a server key (§8.6), 32 lowercase hex characters. It is stable within
  one room, so a device that reconnects keeps its keys, and unlinkable across rooms. LiveKit and
  everyone in the room see only this identity; who it is travels inside encrypted content.
- Keys go to LiveKit's key provider per identity and **key index** (`0` to `15`, the size of
  LiveKit's key ring), never as one shared key.

### 8.2 The key schedule

Each key epoch has a 32-byte **epoch secret**; each sender's key in that epoch is derived from it
and the sender's identity:

```text
sender key = HKDF-Expand(PRK = epoch secret,
                         info = "songbird.v1/call-sender\0" ‖ u8(len(identity)) ‖ identity, 32)
key index  = epoch mod 16
```

The sender key is what apps hand to LiveKit's key provider (`set_key(identity, key_index, key)`);
LiveKit then derives its frame key from it as on every platform. Because each sender's key is
bound to its identity, no two senders encrypt under one key, so LiveKit's per-frame IVs (SSRC,
timestamp, counter) cannot collide across senders.

- **1:1 calls.** The caller draws a random 32-byte `call_secret` and sends it in the encrypted
  offer (§8.5) over the Signal session. There is one epoch, `0`:

  ```text
  epoch secret = HKDF-SHA256(salt = call ID, ikm = call_secret, info = "songbird.v1/call-1to1\0", 32)
  ```

- **Group calls.** The epoch secret is the group's MLS exporter at its current epoch `e`:

  ```text
  epoch secret = MLS-Exporter("songbird.v1/call", call ID, 32)      key index = e mod 16
  ```

  Every commit (an add, a removal, an update) starts a new MLS epoch and so a new secret that a
  removed device cannot compute. On processing a commit, a participant installs the new keys for
  every identity at the new index at once and **switches its own sending to the new index 1
  second later**, so others have installed it before frames arrive; it keeps the old keys in the
  ring for late frames. The server also disconnects removed devices from the SFU (§8.6).
- **Call links** are §8.4.

- **The verification code** (D-0152) both people compare: the first 24 bits of
  `HKDF-Expand(PRK = epoch secret, "songbird.v1/call-code\0", 32)`, four symbols of 6 bits
  (indices into the apps' list of 64). It changes with every group epoch.
- **Hellos** (D-0152): on joining, and to each newcomer, a device sends on the room's encrypted
  data channel (topic `songbird.v1/call-hello`) `account (16) ‖ device (16) ‖ XEdDSA signature
  (64)` by its device identity key over `"songbird.v1/call-hello\0" ‖ call ID ‖ u8(len) ‖
  identity ‖ account ‖ device`. Receivers check the device in the account's signed device list
  before showing a name; until then the tile names nobody. A raised hand is one byte (1 or 0)
  on topic `songbird.v1/call-hand`.

### 8.3 Rules for apps

- Never reuse a `call_secret` or a call ID; never send a sender key or secret outside a Signal
  session or MLS.
- Encrypt data packets as well as media (LiveKit's E2EE covers both); a participant whose frames
  do not decrypt is shown as "can't decrypt", never played in the clear.
- A group's call ID changes only when a new call starts; the MLS exporter already gives a fresh
  secret per epoch.

### 8.4 Call links

A call link is `https://<web domain>/call#<root, base64url>`: a random 32-byte **root** in the
fragment, which never reaches a server.

```text
link ID   = HKDF-SHA256(salt = "", ikm = root, info = "songbird.v1/call-link-id\0", 32)   sent to the server
link key  = HKDF-SHA256(salt = "", ikm = root, info = "songbird.v1/call-link-key\0", 32)  never sent
stored    = SHA-256("songbird.v1/call-link-hash\0" ‖ link ID)                         what the server keeps
```

- **Cards.** Each participant seals a `CallLinkCard` (its account, its device, its participant
  identity and an optional display name; `proto/content.proto`) under the link key:

  ```text
  version u8 (1) | nonce [12] | ChaCha20-Poly1305(key = link key, nonce, card, aad = "songbird.v1/call-link-card\0")
  ```

  at most 512 bytes. Cards go on LiveKit's data channel and, while someone waits in a lobby,
  through the server (§8.7), which cannot open them.
- **Keys** (the owner's choice, D-0085 §2: Signal's design). Every participant draws its own
  random 32-byte sender key and sends it to each other participant's device over a Signal
  session, as `CallMessage.sender_key` (identity, key index, key). On every join and every leave,
  each participant draws a new key at the next index and sends it to those still present, so a
  newcomer cannot decrypt what came before and a leaver cannot decrypt what follows. Joining
  needs a Songbird account (sessions need one).
- **On the device** (D-0153). The Signal session's envelope carrying a `sender_key` travels on
  the room's data channel (topic `songbird.v1/call-key`, addressed to that participant), not
  through the message lanes: those would refuse a stranger whose account is closed, and a link
  call is mostly strangers. A key counts only if it came, over the session, from the account and
  device the sender's card names, for the identity the card names. A link call's data channel is
  **not** encrypted by LiveKit (its packets would be under sender keys a newcomer does not hold
  yet); everything sent there is sealed by the app: cards (topic `songbird.v1/call-card`, the
  card's identity must be its sender's), key envelopes, and other data such as a raised hand
  (topic `songbird.v1/call-hand`) as `nonce [12] | ChaCha20-Poly1305(link key, nonce, data,
  aad = "songbird.v1/call-link-data\0")`. Media stays encrypted by LiveKit under the sender keys.
  No hello (§8.2) is sent in a link call.
- A card sent to a lobby is made before the device has a participant identity; it carries the
  identity `lobby`.
- The link's URL is `https://songbird.chat/call#<base64url, no padding, of the root>`.

### 8.5 Signalling

Ringing, accepting, declining, busy and hanging up are encrypted content (`Content.call`,
`CallMessage`; §5):

| Kind | Sent to | Meaning |
| --- | --- | --- |
| `offer` | the callee's devices (1:1), or the group (MLS) | `media` (audio or video); `call_secret` (32 bytes) in a 1:1 call, `group_id` (16 bytes) in a group call, exactly one; `ring_until_ms` |
| `answer` | the caller, and the callee's own devices | accepted on this device; the others stop ringing |
| `decline`, `busy` | the caller | |
| `hangup` | everyone in the call | with a reason (normal, answered/declined/busy elsewhere, failed) |
| `sender_key` | one device, over a Signal session | a link call's key (§8.4) |

- **Live ringing** uses the ephemeral lane (§3.7). An offer to a device that is offline is also
  sent stored, with `EXPIRES` of 60 seconds, and the server wakes the device (§8.6). An app rings
  until `ring_until_ms`, never more than 2 minutes after the offer's `sent_at_ms`, and not at all
  once that has passed (`call::ring_left_ms`).
- **Non-contacts never ring.** An offer that arrives by the request lane (§5.10) is shown as a
  request, silently; the server queues no wake-up without the contact proof (§8.6).
- A missed, declined or finished call is recorded as `CallEvent` in the chat and synced to the
  user's devices as `call_history`.

### 8.6 Rooms, tokens and who may join (M2, D-0087)

Every request is signed by the device (§2, like every `api` request). A pending device (linked,
not yet in its signed list) gets 403 `pending`. Answers are small JSON.

**A ticket** is what a device needs to join:

```json
{"call": "<call ID, 32 hex>", "url": "wss://calls.<domain>", "token": "<JWT>", "identity": "<32 hex>"}
```

- The **token** is LiveKit's (HS256, signed with the LiveKit API secret), valid **2 minutes**: the
  time to connect. LiveKit refreshes a connected participant's token itself.
- It admits one identity to one room with the least grants: join, subscribe, and for those who
  may speak, publish data and only the sources the call allows (microphone, camera, screen share;
  the microphone only in a group's audio call); a read-only member publishes nothing, data
  included. No admin, recording, ingress,
  SIP or agent grant; no name, metadata or attributes, and no right to set them.
- The **identity** is `hex(HMAC-SHA256(k_identity, room ‖ device)[..16])` (§8.1): the key
  schedule binds sender keys to it.

**`POST /v1/calls`** `{"callee": hex, "media": "audio" | "video", "access_key": hex?, "ring": bool}`:
a 1:1 call.

- The callee must be another account whose devices exist and that is **open**, or whose access
  key the caller presents (a contact, §5.10). Otherwise 404, the same answer as for an account
  that does not exist.
- The server creates the room (random ID; 32 participants for video, 128 for audio; it closes 20
  seconds after the last participant leaves, or after 3 minutes if nobody joins) and keeps the
  call's record (§8.8) until the room closes, at most 24 hours.
- `ring: true` queues a wake-up for each of the callee's listed devices (`VoIP` on iOS,
  high-priority FCM on Android, Prompt 8) **only when the caller presented the access key**: a
  `VoIP` push rings, so a stranger's call to an open account rings nobody, and its offer arrives as
  a message (D-0091). Wake-ups count against the callee's budget and the (caller, callee) pair's.
  The ringing itself is the encrypted offer (§8.5).
- 200 with the caller's ticket.

**`POST /v1/calls/group`** `{"group": hex, "media": …, "ring": bool}`: the group's call.

- The requesting device must be in the group's roster (404 otherwise, as for no group).
- If the group has a live call, the device gets a ticket for it, whatever `media` says.
- Otherwise starting one needs the role the group's policy sets for `call` (§6.6; 403 `role`).
  Two members starting at once get the same room.
- `ring: true` wakes every other member account's devices, if the group has at most 32 accounts.
- Read-only members (a channel's subscribers) get tickets that publish nothing.

**`POST /v1/calls/join`** `{"call": hex, "group": hex?}`: a ticket for a live call.

- A 1:1 call admits only listed devices of its two accounts.
- A group call needs `group` (the call's record holds only a keyed tag of it) and admits only
  devices in the group's **current** roster.
- Anything else, and a call that does not exist or has ended, is 404.

**Removal mid-call.** When `api` accepts a commit to a group with a live call, it asks LiveKit for
the room's participants, records every identity that is not one of the roster's devices' in the
room's deny set (§8.8), and disconnects them (three attempts). A removed device is therefore cut
off at once; its next ticket is refused; if it reconnects with a token it still holds (LiveKit does
not revoke tokens), the `calls` service disconnects it again on that join's webhook; and the new
MLS epoch's keys (§8.2) are ones it cannot compute anyway (D-0091).

A group's call pointer younger than 15 seconds is trusted without asking LiveKit (its starter may
still be creating the room); an older one is checked with `ListRooms`. Each join keeps a live
call's records for another 24 hours.

**Budgets** (`[api.calls.limits]`, keyed records in `valkey-hot`):

| Budget | Default |
| --- | --- |
| Calls started per account | 60 an hour, burst 20 |
| Wake-ups received per account | 60 an hour, burst 20 (over it: no wake-up; the stored offer still arrives) |
| Wake-ups one caller causes one account | 20 an hour, burst 6 |
| Tickets per device (every ticket, group joins and link tickets included) | 600 an hour, burst 60 |

Answers: 200 ticket; 400 `request`, `callee`, `group`, `call`, `access_key`; 403 `role`,
`pending`; 404; 429 with a wait; 503 (LiveKit or `valkey-hot` unavailable: retry).

### 8.7 Call links (M3, D-0088)

The link and its keys are §8.4. The server stores a link under `SHA-256("songbird.v1/call-link-hash\0"
‖ link ID)` with its room, `SHA-256("songbird.v1/call-link-admin\0" ‖ admin token)`, audio or
video, whether a lobby is on, and an expiry. **No account**: the creator keeps the random 32-byte
admin token (and may share it with co-admins inside encrypted messages).

- **`POST /v1/calls/links`** `{"link": hex32, "admin": hex32, "media": "audio" | "video",
  "lobby": bool, "ttl_secs": n}`: creates it. `ttl_secs` defaults to a day, at most 30 days
  (400 `ttl_secs`). 204; 409 `exists`. Counts against the call budget.
- **`POST /v1/calls/links/join`** `{"link": hex32, "admin": hex32?, "lobby": hex16?, "card": hex?}`:
  - with the right admin token, or on a link without a lobby: 200 ticket;
  - otherwise with a `card` (a sealed card, §8.4, 29 to 512 bytes): 202 `{"lobby": handle}`, a
    random 16-byte place in the lobby;
  - polling with that handle, from the same device: 202 while waiting, 200 ticket once admitted
    (once), 404 once declined;
  - a wrong admin token, another device's handle, an unknown, expired or revoked link: 404.
- **`POST /v1/calls/links/lobby`** `{"link", "admin"}`: 200 `{"waiting": [{"handle": hex,
  "card": hex}]}`. Admins open the cards with the link key and decide.
- **`POST /v1/calls/links/admit`** `{"link", "admin", "handle", "admit": bool}`: 204; 404 for no
  such handle.
- **`POST /v1/calls/links/revoke`** `{"link", "admin"}`: deletes the link and its lobby and ends
  its call for everyone (LiveKit `DeleteRoom`). 204.

A lobby holds at most 64 people (409 `full`) for 10 minutes, **one place per device** (asking
again returns the same handle); each entry holds a keyed tag of the waiting device (so only it can
redeem the place) and the sealed card. A declined entry is deleted at once, freeing its place.
Every link request spends a per-device budget (1,800 an hour, burst 120) and every ticket the
ticket budget. A link revoked while its room was being opened gets the room closed again. The
worker deletes expired links.

Cards are not signed: anyone holding the link can write any name in one. Apps show a joiner as
unverified until the Signal session with its device is established and, for strangers, until the
users compare safety numbers; media keys go only over those sessions (§8.4).

### 8.8 Call records and LiveKit webhooks (M2, M3, D-0087, D-0088)

Live calls are records in `valkey-hot` under keyed names, never in SurrealDB:

```text
cl:r:<tag(room)>   version (1) | kind | media | created_ms | caller [16] callee [16]   (1:1)
                                                         | group tag [16]            (group)
                                                         | link tag [16]             (link)
cl:g:<tag(group)>  version (1) | room [16] | media | created_ms      the group's live call
cl:q:<tag(link)>   lobby: handle → version | state | device tag [16] | sealed card
cl:w:<tag(event)>  a webhook event already handled
cl:x:<tag(room)>   identities removed from the room mid-call (its deny set)
```

They expire after the call's longest life (24 hours), the lobby after 10 minutes, the webhook
marks after an hour. LiveKit sends its webhooks to the `calls` service on the same host, over
loopback. A webhook is handled only if its token verifies with the LiveKit API secret, its
`sha256` claim is the body's hash, its time is within 5 minutes of ours, and its ID is new;
retries and replays are answered 200 and do nothing; an event whose cleanup failed is unmarked
and answered 503, so LiveKit's retry runs it. `room_finished` deletes the call's record and the
group's pointer (compare-and-delete, so a newer call's pointer survives); `participant_joined` of
an identity in the room's deny set disconnects it. Room, participant and track events feed aggregate counters
(`songbird_calls_*`), never a label naming a room. Webhook bodies are not stored or logged (they
carry LiveKit's per-room TURN credential).

LiveKit runs with `auto_create: false`, so a token for a room that does not exist admits nobody: a
finished 1:1 call cannot be rejoined. If a webhook is lost, the next group start finds the room
gone (`ListRooms`) and starts a new call.

### 8.9 Low-data mode and the SFU (M4, M5, D-0089, D-0090)

"Use less data for calls" is the app's choice; the server only has to allow it:

- **Publishing:** in low-data mode an app publishes video at most 320×180 (one simulcast layer,
  about 150 kbit/s) and Opus at 16–24 kbit/s with DTX on; in normal mode up to 1280×720 with three
  simulcast layers. Screen sharing is never sent in low-data mode unless the user asks.
- **Subscribing:** in low-data mode an app subscribes to the lowest simulcast layer of every video
  (`setVideoQuality(LOW)`) and pauses video it does not show; LiveKit's dynacast then stops layers
  nobody receives.
- **The SFU** (`deploy/ansible/roles/calls/templates/livekit.yaml.j2`): simulcast and dynacast are
  allowed (they are client options), Opus RED and DTX are allowed, congestion control is on with
  pausing allowed, and the codecs are Opus, RED, VP8, H.264, VP9 and AV1.

The bandwidth each mode uses is measured in M5 (`P07-gate.md`).

## 9. Push (Prompt 8)

## 10. Settings sync and presence (Prompt 9)

### 10.1 The account record store (M1, D-0102)

An account's settings, per-chat preferences, contacts and lists are **records**: encrypted by
the account's devices under a storage key the server never holds, stored opaque under random
IDs. The client library is `crates/sync`; the wire types are `songbird_proto::storage`.

**Records are never changed in place.** A new value is a new record under a fresh random 16-byte
ID, and the record it replaces is deleted in the same write, so the set of IDs names the whole
state. Every write is a compare-and-swap of the store's version:

| Method and path | Body | Answer |
| --- | --- | --- |
| `GET /v1/storage` | — | `version u64 \| mlen u16 \| manifest` |
| `GET /v1/storage/ids` | — | `version u64 \| n u32 \| id [16] * n` |
| `POST /v1/storage/read` | `n u16 (1..=1024) \| id [16] * n` | `n u16 \| (id [16] \| len u16 \| record) * n`, IDs that no longer exist left out |
| `PUT /v1/storage` | `version u8 (1) \| expected u64 \| mlen u16 \| manifest \| ndel u32 \| id [16] * ndel \| nins u32 \| (id [16] \| len u16 \| record) * nins` | 200 `{"version"}`; 409 `{"error": "version", "version"}` when another write won; 409 `full`; 409 `exists` for an ID already used; 400 `storage` |
| `PUT /v1/storage/object` | an encrypted object, at most 2 MiB | `{"object": hex}` (a server-drawn 32-byte ID); 409 `full` |
| `GET` and `DELETE /v1/storage/object/<id>` | — | the account's own objects only; 404 for any other |

- Every endpoint needs a listed device of the account; a pending device gets 403 `pending`.
- A record's length must be a Padmé value of 256 to 4,096 bytes plus a 16-byte tag
  (`valid_record_len`), so most settings records share one size. A manifest is 16 to 256 bytes.
- Caps per account (`[api.storage]`): 10,000 records, 16 MiB of records, 16 objects. A write may
  delete and insert up to 20,000 records each, so a key rotation fits in one write.
- Throttles per account, in `valkey-hot` under keyed names (`st:<kind>:<hmac>`): writes and reads
  per hour, and records written (one unit per record a write deletes or inserts; 60,000 an hour,
  burst 40,000, at least one key rotation of a full store; D-0108).
- No write time is stored; the server keeps the version counter only.

**Keys.** The storage key is 32 random bytes made by the primary at registration, given to a
linked device in the provisioning message (version 2, §4.6), and rotated when a device is
removed. A device that adopts the key starts from an empty replica and reads the store from
scratch, so its own first-run defaults never overwrite the account's settings. Until app-core
sends the new key to the other devices end to end (A2), the primary rotates only when it is left
as the account's only device (D-0106):

- `key_id = HMAC-SHA256(storage key, "songbird.v1/storage-key-id")[..8]`;
- a record's key: `HKDF-SHA256(salt = "songbird.v1/storage", ikm = storage key,
  info = "record\0" ‖ id)`; AES-256-GCM-SIV, zero nonce (one key per record, never rewritten),
  AAD `"songbird.v1/storage-record\0" ‖ id`;
- the manifest's key: the same HKDF with `info = "manifest"`; AES-256-GCM-SIV, random nonce,
  AAD `"songbird.v1/storage-manifest\0" ‖ key_id`.

**Record plaintext:** `be32(len) ‖ RecordPlain ‖ zeros` to the padded length. `RecordPlain`
(protobuf): `key` (the logical key, §10.2, at most 256 bytes), the stamp (`wall_ms`, `counter`,
`node` of 16 bytes), `deleted` (a tombstone, empty value) and `value`.

**Manifest:** `key_id [8] ‖ nonce [12] ‖ AES-256-GCM-SIV(version u64 ‖ digest [32]) ‖ tag`
(76 bytes); `digest = SHA-256("songbird.v1/storage-ids\0" ‖ sorted IDs)`; `version` is the store
version the manifest was written at.

**A device accepts a store only if:** the manifest opens under its storage key (another `key_id`:
the key was rotated, wait for the new one); the manifest names the head's version; its digest
matches the IDs; every record opens under its ID; and the version is not below one the device
already saw. A store that fails is refused whole: a server that withholds, swaps, replays or
rolls back records is caught. A new device trusts the first manifest it sees.

**Merge.** Per logical key, the record with the greatest stamp wins, local edits included. A
stamp is a hybrid logical clock `(wall ms, counter, node)`: a device's clock never goes
backwards, follows remote stamps at most 24 hours ahead of its own time, and a write always gets
a stamp above the key's current one, so a bad clock can never pin a key. Deletions are tombstones,
collected 90 days later; a device away longer drops its own unwritten edits older than that.
Convergence is proved by `crates/sync/tests/convergence.rs`.

### 10.2 Logical keys

Inside the ciphertext only:

| Prefix | Holds |
| --- | --- |
| `s/<name>` | An account-wide setting |
| `c/<chat>/<name>` | A per-chat preference; `<chat>` is the hex of the peer's account ID or the group ID |
| `g/<group>` | The account is in this group (`1`): its other devices join it by external commit (D-0167) |
| `c/<chat>/state` | The chat's list state: `a=,p=,m=<muted until ms>,u=,f=,l=<lists>` joined by `;` (D-0163) |
| `k/<id>` | A saved contact (name, notes, buddy-list groups) |
| `l/<id>` | A custom chat list |
| `b/<id>` | A broadcast list |
| `x/<id>` | A starred message |
| `p/<id>` | An installed pack |

The values of each setting are defined with the app's settings (M5).

### 10.3 Server-enforced settings (M2, D-0103)

The few settings the server must see to enforce (THREAT_MODEL §4). Tightening is any listed
device's; loosening is the primary's, checked in the write's transaction (D-0058), so a stolen
linked device cannot open the account up. A pending device changes nothing (403 `pending`).

| Request | Body | Answers |
| --- | --- | --- |
| `GET /v1/account/settings` | — | `{"discoverable": bool, "groups": "everyone" \| "contacts", "requests": "everyone" \| "nobody", "inactivity_months": 6 \| 12 \| 24}` |
| `PUT /v1/account/discoverable` | `{"on": bool}` | 204; 403 `primary` for turning it on from a linked device |
| `PUT /v1/account/groups` | `{"from": "everyone" \| "contacts"}` | 204; 403 `primary` for "everyone" from a linked device |
| `PUT /v1/account/requests` | `{"from": "everyone" \| "nobody"}` (§5.10) | as before |
| `PUT /v1/account/inactivity` | `{"months"}` (§4.10) | as before |

- **Not discoverable:** `POST /v1/username/lookup` answers 404, exactly as for an unknown
  username (same budgets, one statement of the same work), unless the request carries
  `"link": <handle hex>` naming one of the account's own username links: a link or QR code the
  user handed out keeps working. Existing contacts need no lookup.
- **Group adds from contacts only:** a `POST /v1/groups/commit` that adds devices of such an
  account must carry the header `Group-Add-Access-Key: hex(16)`, the XOR of the access keys of
  every such account it adds (as broadcasts do, §5.10), compared in constant time; otherwise 403
  `access`, the same whichever account was missing. Exempt: the committer's own account, and an
  account with a pending request to join that group (it asked). Apps offer "Nobody" by
  answering adds with an invitation the user accepts (a client rule). The app sets "contacts" at
  registration (D-0101 question 9); the server's default for older accounts is "everyone".

### 10.4 The profile's contents (M2, D-0103)

The blob of `PUT /v1/profile` (§4.8) is, for every app (`songbird_content::profile`):

```text
blob = nonce [12] ‖ AES-256-GCM-SIV(key, nonce, aad = "songbird.v1/profile\0" ‖ be64(version),
                                    be32(len) ‖ ProfilePlain ‖ zeros to a multiple of 256)
key  = HKDF-SHA256(salt = "songbird.v1/profile", ikm = profile key, info = "profile")
```

`ProfilePlain`: name (≤ 128 bytes), about (≤ 280), status (text ≤ 140, emoji ≤ 32, `expires_ms`,
0 for until changed), avatar (object ID and key, 32 bytes each), up to 5 links. The version is
bound in, so an old profile cannot be served under a newer version.

- **Visibility** of photo, about and status is "My contacts" or "Nobody": only contacts hold the
  profile key, so a field shared with nobody is left out of the blob. Narrowing one rotates the
  profile key (sent to remaining contacts with their next message, §5.10).
- **A status** stops showing at its expiry on every reader, with no further message.
- **After any change** the app sends its contacts `Content.profile_changed {version}`: sealed,
  stored, `SILENT` (no wake-up). They fetch the profile again if the version is newer.

### 10.5 Presence, last seen and nudges (M3, D-0104)

The server never reveals connection state: no answer, header or timing tells a sender whether a
recipient is online. Apps tell their allowed contacts themselves (`songbird_content::presence`).

**Presence** (`Content.presence`: online, away, busy, or offline with `last_seen_ms` to the
minute; `wants_reply` when just come online) goes **sealed**, in one batch to every allowed
contact's devices:

```text
PUT /v1/ephemeral/sealed   (no device signature)
Unidentified-Access-Key: hex(16)   the XOR of the access keys of every addressed account that is not open
body: version u8 (1) | n u16 (1..=1024) | (device [16] | len u16 (1..=4096) | payload) * n   (at most 512 KiB)
```

- The server maps devices to accounts from its directory snapshot, recomputes the combined key
  and compares it in constant time: "contacts only" is enforced cryptographically, and a stranger
  cannot probe anyone. A wrong key refuses the whole batch, with the same 204 (D-0108); devices
  that are not listed are left out.
- Throttle: per IP range, counted per device addressed (1,000,000 an hour, burst 20,000).
- Never stored; routed to connected devices only; the answer is always 204 with no body.
- The server learns who receives presence and when, never who sent it, beyond correlating the
  request's address and time with the sender's own connection (sealed sender's known limit).

**The sender's rules** (`Presencer`): at most one batch per 5 s, the latest state wins and is
always sent after the debounce; the same state is not sent twice; going background sends
"offline" with last seen (if shared); contacts that asked get one reply each, at most once a
minute; **invisible sends nothing at all**, and neither does "Nobody" for both online and last
seen. A contact who was offline all along keeps the last value it saw, or shows "last seen
recently": the server keeps no presence state. Reciprocity: a user who shares last seen with
nobody sees nobody's.

**Nudges** (`Content.attention {NUDGE}`) go identified on the WebSocket in the attention frame
(`0x04`): the connection's ephemeral bucket, then budgets in `valkey-hot` per (sender,
recipient) account pair (30 an hour, burst 3) and per sender (300 an hour, burst 20), under keyed
names `q:n:` and `q:m:`, then the recipient's blocks. Over a budget, blocked, offline or the store
down: dropped silently, like any ephemeral event (fail closed). The recipient gets it in the
attention frame (`0x84`): the server marks each event it routes with its class, so one a client
sent as an ordinary ephemeral event (`0x03`, outside these budgets) arrives as `0x83` and is
never taken for a nudge (D-0108). The server learns that a nudge
happened between two accounts, never its content. A nudge to an offline contact is lost. The
receiving app shakes the window (desktop) or vibrates, unless nudges are off, globally or for
that contact. The stored `DataMessage.nudge` kind is superseded and never sent.

### 10.6 Parental supervision (M4, D-0105)

**Linking, in person** (D-0107, D-0108). Three QR codes, read only by the live camera; nothing
passes through the server between the two accounts, so it never learns who parents whom.

1. The parent's device makes a fresh Ed25519 **policy key** for this child (never linked to the
   parent's account) and a 32-byte link secret, and shows only a commitment to them:
   `SONGBIRD:SUPERVISE:<hex(SHA-256("songbird.v1/link-commit\0" ‖ policy key ‖ link secret))>`.
2. The child's phone scans it. The child enters their age; the phone computes the unlink date
   from it and the country's age of digital consent (THREAT_MODEL §5); the birth date never
   leaves the phone. It shows `SONGBIRD:CHILD:<account hex>:<unlink_at_s decimal>`.
3. The parent's device scans it, signs the first policy (version 1, for that child, with that
   date) and shows it with the secret: `SONGBIRD:POLICY:<link secret hex>:<policy hex>` (the
   policy names the key; at most 644 bytes, about 200 with the app's client rules). It shows the
   six-digit code (`SHA-256("songbird.v1/link-code\0" ‖ policy key ‖ link secret ‖ child)`,
   first 4 bytes mod 10⁶).
4. The child's phone scans it and checks that the key and secret match the commitment, the
   signature, that the policy is version 1, for this account, with the date it computed, and not
   unlinking. It shows the six digits.
5. Parent and child compare the digits. Only then does the child's phone install the policy
   (`PUT /v1/supervision`, as the primary), and the parent's device keep the link. The parent
   committed before seeing the child's account, and the child's account was fixed before the key
   and secret were revealed, so nobody relaying the codes can choose values that make two
   different links show the same digits.

**The policy** (`songbird_proto::supervision::Policy`):

```text
version u8 (1) | child [16] | policy_key [32] | policy_version u64 | flags u8
  | unlink_at_s u64 | clen u16 | client_rules (0..=512) | signature [64]
signature = Ed25519(policy key, "songbird.v1/supervision\0" ‖ every byte before it)
```

| Flag | The server enforces |
| --- | --- |
| `0x01` no message requests | `requests_off` set and locked |
| `0x02` group adds need approval | `grp_contacts` set and locked; every commit adding the child's devices (its own joins included) carries an approval |
| `0x04` not discoverable | `hidden` set and locked |
| `0x80` unlink | supervision ends; the locks lift, the settings stay for the child to change |

`client_rules` is AES-256-GCM-SIV ciphertext under a key from the link secret (HKDF, salt
`"songbird.v1/supervision"`, info `"client-rules"`; AAD `"songbird.v1/client-rules\0" ‖ child ‖
be64(version)`): approving each new contact, declining calls from non-contacts, nudges off, quiet
hours. Only the two devices read it; the child's app enforces it and shows it (D-0166). A new
contact is approved in person: the child's phone shows `SONGBIRD:APPROVE:<account hex>:<username>`,
the parent's phone shows eight digits, `HMAC-SHA256(HKDF-SHA256(salt "songbird.v1/supervision",
link secret, info "contact-approval"), "songbird.v1/contact-approval\0" ‖ child ‖ contact)`, the
first 8 bytes big-endian mod 10^8, which the child types.

| Request | Who | Answers |
| --- | --- | --- |
| `PUT /v1/supervision` (the policy), signed | The child's own devices: the first from the child's primary (the child consents; the parent's signature proves the parent does), or a later one a child's device carries | 204; 403 `account` (a policy for another account, whatever its state), 403 `primary` (a first from another of the child's devices), 403 `signature`; 409 `exists`, `version` (not newer, or another key), `date` |
| `PUT /v1/supervision/update` (a later policy), **unsigned** (D-0108) | Anyone: the parent's device. Authorized by the parent's signature against the key in force alone, so the server never learns which account the parent is (only the request's address, as for sealed sends); throttled per IP range | 204; 403 `signature` (a bad signature, or no policy in force: one answer after the same work); 409 `version`, `date` |
| `GET /v1/supervision` | The child's devices | the policy as signed; 404 when none |

- A locked setting answers 403 `supervised` to every device of the child, the primary included.
- **Group approvals:** `Group-Add-Approval: hex(expires_s u64 | signature [64])` (several
  comma-separated, at most 8), the signature by the policy key over
  `"songbird.v1/group-approval\0" ‖ group [16] ‖ child [16] ‖ expires_s`; valid only if not
  expired and at most a week ahead. Without one: 403 `approval`.
- **The date:** the worker removes a policy past `unlink_at_s` and lifts its locks. A later
  policy may move the date earlier, never later (409 `date`): supervision never outlives the date
  set at linking (D-0108).
- **No message requests** also closes the account to "anyone may send": the policy sets
  `uak_open` off, and a profile upload opening it answers 403 `supervised` (D-0108).
- **Group adds** check the contacts-only rule before the approval, so a stranger is refused
  (403 `access`) as for any such account and never learns of the supervision.
- **The child may delete their own account** (the owner's decision): the policy goes with it.
- `GET /v1/account/settings` reports `"supervised"`.

## 11. Backup format and vault access (Prompt 10)

Decisions D-0119 (the owner's plan) and D-0120 (M1). The library is `crates/backup-format`;
`proto/backup.proto` specifies the plaintext schemas; `vectors/v1/vectors.json` holds the
byte-exact vectors and `vectors/check.py` re-derives them with the Python standard library alone.
A backup is the same on every platform: nothing in it depends on the operating system, the
locale, the time zone or the width of native integers.

### 11.1 The recovery key and the key schedule

A recovery key is 64 characters of Crockford base32 (`0123456789ABCDEFGHJKMNPQRSTVWXYZ`), most
significant bit first, encoding 40 bytes: a 32-byte **secret** and an 8-byte **check**,
`SHA-256("songbird.v1/recovery-key\0" || secret)[0..8]`. It is shown as 16 groups of four
joined by `-`. A reader ignores case, whitespace and `-`, reads `I` and `L` as `1` and `O` as
`0`, refuses any other character (`U` included) and anything but 64 symbols, and refuses a key
whose check does not match before any request (a typo, not a missing backup).

Keys: `PRK = HKDF-Extract(salt = "songbird.v1/backup", secret)`, then
`HKDF-Expand(PRK, "songbird.v1/backup/" || label, n)`:

| Label | n | Use |
| --- | --- | --- |
| `vault-id` | 32 | The vault's ID on every store (§11.8) |
| `vault-access` | 32 | The Songbird vault's bearer secret; the vault stores `SHA-256("songbird.v1/vault-access\0" \|\| it)` |
| `manifest` | 32 | XChaCha20-Poly1305 key of manifests |
| `chunk` | 32 | XChaCha20-Poly1305 key of chunks |
| `chunk-name` | 32 | HMAC-SHA256 key of chunk names and media references |
| `chunker` | 2,048 | The gear table: 256 big-endian `u64` |

A new recovery key is a new vault (a full backup, then the old vault is deleted). The transfer to
a new phone (§11.9) uses a one-time key of the same form.

### 11.2 Chunks

```text
chunk object = "SBC" | version u8 (1) | nonce [24] | ciphertext | tag [16]
ciphertext   = XChaCha20-Poly1305(chunk key, nonce, padded, AAD = the 28 header bytes || name [32])
padded       = length u32 | plaintext | zeros, to Padmé(max(4 + length, 1024)) bytes
name         = HMAC-SHA256(chunk-name key, plaintext)
```

Integers are big-endian; Padmé is §7.1's. A chunk's plaintext is at most 8 MiB. A reader opens a
chunk only against the name and plaintext length the manifest gives: the ciphertext length must
be exactly the padded length of that plaintext length (a truncated or extended object fails as
damaged before decryption), the tag must verify, the padding must be zeros, the length field must
match, and the HMAC of the plaintext must equal the name. A store serving one chunk under
another's name fails the tag (the name is in the AAD). Names are written as 64 lowercase hex
characters.

### 11.3 Cutting streams into chunks

Content is written as byte streams (§11.5) and cut with FastCDC (normalized chunking, level 2):
for each stream, from the start of the unchunked rest,

```text
if rest <= 1 MiB: the rest is the last chunk (only at the stream's end)
h = 0; for i from 1 MiB, h = (h << 1) + gear[byte[i]] (wrapping, u64):
  before 2 MiB: cut after byte i when h & (!0 << 41) == 0   (top 23 bits zero)
  from 2 MiB:   cut after byte i when h & (!0 << 45) == 0   (top 19 bits zero)
  at 8 MiB:     cut
```

The hash at byte `i` depends only on the 64 bytes before it, so a writer may start it 64 bytes
before 1 MiB. The table is keyed (§11.1): boundaries reveal nothing about content to the store,
while the same content cuts the same way in every backup of the same key, so new messages change
only the chunks near them. Measured on an Apple M5 Pro: 3.4 GB/s, chunks of 2.35 MiB on average
(D-0120). Readers accept any chunk of 1 byte to 8 MiB; only writers follow these sizes.

### 11.4 The manifest

```text
manifest object = "SBM" | version u8 (1) | generation u64 | nonce [24] | ciphertext | tag [16]
ciphertext      = XChaCha20-Poly1305(manifest key, nonce, padded protobuf `Manifest`,
                                     AAD = the 36 header bytes || vault ID [32])
```

`generation` is the only field in the clear: stores compare-and-swap on it (§11.8). It is
authenticated, so a store cannot relabel a manifest, and the vault ID in the AAD keeps one vault's
manifest from opening as another's. The plaintext (`Manifest`, at most 48 MiB) holds the writer's
format version and the oldest reader version that may read it (`min_reader_version`), the time,
the app's version and platform (informational only), `previous` (the SHA-256 of the previous
manifest object, empty for the first) and the streams: each with its kind, its ID (a media
reference for a media stream), its length, its SHA-256, and its chunks in order (name and
plaintext length). Bounds a reader checks: at most 2^20 streams and 2^21 chunk references, names
of 32 bytes, chunk lengths of 1 byte to 8 MiB adding up to the stream's length, exactly one
records stream and at most one small-media stream.

**Rollback.** A device keeps, per vault, the generation and hash of the newest manifest it has
read. It refuses an older generation (`Rollback`), the same generation with another hash
(`Fork`), and the next generation unless its `previous` is the hash it kept (`Fork`). A brand-new
device has nothing to compare with, so a store can serve it an older backup of the same key (never
a forged one; threat model §6).

### 11.5 Streams and records

| Stream | Holds |
| --- | --- |
| records (exactly one) | Length-delimited `Record`s (a protobuf varint length, then the message), at most 4 MiB each |
| small media (at most one) | Length-delimited `Blob`s: media files under 1 MiB, each once |
| media (one per file) | The bytes of one media file of 1 MiB or more, ID = its reference |

The records stream follows a canonical order, so equal content gives equal chunks and a new
backup changes as few chunks as possible: the `Account`, the `Profile`, the `Setting`s, the
`Contact`s, the `Group`s, every `Chat`, then **every `Message` of every chat by time** (sent
time, then chat ID, then message ID), each message's `Media` record just before the first
message that shows it. New messages then land at the end of the stream and cleared old ones at
its start: ordering messages chat by chat instead made one new message in each of 2,000 chats
re-upload the whole messages stream (D-0123). A media file is stored once however often it is forwarded: a small one
under `HMAC-SHA256(chunk-name key, "songbird.v1/backup-media\0" || bytes)`, a streamed one under
`HMAC-SHA256(chunk-name key, "songbird.v1/backup-media-stream\0" || length u64 || its chunk
names)`.

**What a backup holds**: the account (account ID, the account identity key's Ed25519 seed, the
profile key, the record store's storage key, the access key, the username and username key, the
device list's version, the server name); the profile; settings (the record store's logical keys
with their values, and device-only settings under `d/`); contacts; groups (ID, name, metadata
key, members, role: enough to rejoin by re-add or external commit, §6); chats; messages (the
content protobuf of §5 as sent or received, with media references, status and star); media.
**Never**: ratchet sessions, prekeys, device identity keys, MLS epoch secrets, KeyPackages,
disappearing messages, view-once messages, deleted messages (D-0165). A restored device starts
fresh sessions. A device keeps its rollback state (§11.4) per vault and per destination.

### 11.6 Restoring

Two phases. **Verify**: fetch every chunk the manifest names, open and check each (§11.2), keep
the still-encrypted objects in a staging area (memory or a folder), then read every stream
through, checking its length and SHA-256 and decoding every record and blob. **Apply**: only
after verify succeeded, read the backup from staging (each chunk opened and checked again) and
write it to an empty store in one transaction. Any failure stops the restore with a named error
(`Missing`, `Damaged`, `WrongName`, `StreamMismatch`, `Malformed`, `TooNew`, `Rollback`,
`Fork`) and the device keeps what it had.

### 11.7 Versions

Readers ignore unknown fields, stream kinds and record bodies. A writer adding something an
older reader must not skip raises `min_reader_version`; an older reader then refuses the whole
backup ("update Songbird") instead of restoring part of it. Every published version's vectors
stay in `vectors/v*/` and are restored by every later reader
(`this_reader_restores_every_published_version`).

### 11.8 Stores and the Songbird vault (M2, D-0121)

A backup lives in any **store** (`crates/backup-storage`): one interface, `BackupStore`, with
`open`, `put`/`get`/`list`/`delete` of chunk objects by name, `manifest`, `swap_manifest(expected,
object)` (stored only if the current manifest's generation is `expected`, 0 for none, and the
object's own generation is `expected + 1`; otherwise a conflict with the current generation;
of racing writers exactly one wins each generation) and `destroy`. Adapters run on the device
only. A backup run reads the manifest and the chunk list, uploads only chunks the store lacks,
swaps the manifest after every chunk is stored (and every chunk it reused from the listing is
still there), then deletes chunks no manifest references; a run that stops anywhere leaves the
previous backup intact. Runs to one kind of store take turns on a device. A restore refuses
an object larger than a chunk can be before keeping it. WebDAV and S3 take `https://` only,
except to the device itself. One conformance suite
(`songbird_backup_storage::conformance`) checks every rule on every adapter.

Local export: a folder (`chunks/<hex>`, `manifest`, a `manifest.lock` held while swapping and
broken after five minutes), and a single `.songbird-backup` file:
`"SBX" | 1 | manifest_len u32 | manifest | n u32 | n × (name [32] | len u32) | objects`; a file
whose length does not match its index is refused before anything is read.

**The Songbird vault** (`api`, objects in their own RustFS bucket). Every request carries the
vault ID and the access secret (§11.1), both from the recovery key:

| Header | Value |
| --- | --- |
| `x-songbird-vault` | the vault ID, 64 hex |
| `x-songbird-vault-access` | the access secret, 64 hex |
| `x-songbird-vault-expected` | (manifest writes) the generation replaced, decimal |

Prompt 11 (D-0130) re-decided D-0119 §3: **no vault names an account**. Writes need an
anonymous day pass, creation an anonymous creation token (§14.4, kinds 2 and 3).

| Request | Needs (besides the two headers) | Answer |
| --- | --- | --- |
| `POST /v1/vault/tokens` | signed by the account's **primary**; no vault headers; a §14.4 request of kind 2 (a day within the next week, the account's level, Plus only through its last day) or 3 (this 180-day window, level 0) | 200 the §14.4 answer; 403 `primary`, `level`; 409 `period`, `cap` (two creations a window); 429 past `passes_per_day` |
| `POST /v1/vault` | `{"token": <creation token>}` (this window's or the last) | 200 `{"created"}` (`false`: it exists with this secret, nothing spent); 409 `exists` (the ID taken with another secret), `spent`, `period`, `invalid` |
| `POST /v1/vault/day` | `{"token": <day pass>}` (today's or yesterday's) | 204: writes allowed that day and the next, the quota the pass's level; 409 `spent`, `period`, `invalid` |
| `GET /v1/vault` | — | 200 `{"generation", "bytes", "chunks", "quota_bytes", "quota_chunks"}` |
| `DELETE /v1/vault` | a current pass | 204: the vault is gone at once; its objects are deleted by the worker |
| `GET /v1/vault/manifest` | — | 200 the manifest object; 204 before the first |
| `PUT /v1/vault/manifest` | a current pass | 200 `{"generation"}`; 409 `{"error": "generation", "generation"}`; 409 `full`; 400 `manifest`/`generation` |
| `PUT /v1/vault/chunks/<name>` | a current pass | 204; 409 `full`; 409 `size`; 400 `chunk` |
| `GET /v1/vault/chunks/<name>` | — | 200 the object; 404 |
| `GET /v1/vault/chunks?after=<name>` | — | 200 `n u16 \| name [32] × n \| more u8`, at most 1,000 names in order |
| `POST /v1/vault/chunks/delete` | a current pass | `n u16 (1..=1000) \| name [32] × n` → 204 |

A write without a current pass is 403 `{"error": "pass"}`; the app presents its day pass again
and retries once.

- The server never stores the vault ID: rows and objects are keyed by `HMAC(server key, vault
  ID)`, objects named `<that>/<epoch>/c/<name>` and `<that>/<epoch>/m/<random>`, where the
  epoch is random per vault incarnation (a vault turned off and opened again with the same key
  never shares objects with the old one). It stores `SHA-256("songbird.v1/vault-access\0" ||
  secret)` and compares in constant time. No time is stored.
- **Reads need only the two headers**, so a new device with only the recovery key finds and
  restores its backup before it has an account. An unknown vault and a wrong secret answer the
  same 404 after the same lookup and count against the requester's IP range (`misses_per_hour`,
  /24 and /48); past it, every vault request from the range waits (429).
- **Writes** need a current day pass on the vault (Prompt 11, D-0130): passes are issued only to
  an account's primary, blinded, so the vault never learns which account presented one. Whoever
  holds the recovery key can write, as whoever holds it can already recover the account
  (D-0119 §2).
- The server checks only shapes: a chunk object starts `SBC 01` and its length is a header, a
  Padmé length of at least 1,024 and a tag, at most the largest chunk; a manifest starts
  `SBM 01`, is at most `max_manifest`, and its generation is `expected + 1`. A chunk is recorded
  with its size (and counted against the quota) before its object is stored, and checked to be
  still recorded after: an upload whose chunk was forgotten, or whose vault was turned off,
  meanwhile deletes its object again. A name already recorded must come with the same size
  (409 `{"error": "size"}`), so uploads racing for one name can't store more than they count. A
  manifest object is stored under a fresh random name before the compare-and-swap, and the
  loser's object is deleted. Every write checks the vault incarnation (epoch) it opened is still
  the current one (D-0125).
- Quotas per vault: the level of its latest day pass (§14.1: 5 GiB and 100,000 chunks on Free,
  50 GiB and 1,000,000 on Plus) under `[api.vault]`'s ceiling. Two creation tokens per account
  per 180 days bound how many vaults one account keeps alive. Budgets (GCRA in `valkey-hot`
  under keyed records): reads and writes per vault, creations and misses per range, passes per
  account.
- A vault keeps the day of its latest pass and of its last use, never a time. One nobody uses
  for **365 days** is retired by the worker (rows, then every object under its prefix). Deleting
  an account no longer reaches its vault: the app deletes the vault first (it holds the access
  secret). A vault turned off is listed again by every sweep for 15 minutes (longer than any
  request) before it is forgotten, so an upload in flight when it was turned off is deleted too.
  The GDPR export says vaults are not attributable.

**Third-party stores** (M3, D-0122), each on the device with the user's own credentials:

| Store | Where | Compare-and-swap | Atomic |
| --- | --- | --- | --- |
| S3 (any compatible bucket) | `<prefix>chunks/<hex>`, `<prefix>manifest`; SigV4 | `If-None-Match: *`, then `If-Match: <ETag>` (412: lost) | yes, where the provider honours conditional writes (the conformance race checks it; RustFS 1.0.5 does) |
| `WebDAV` | `chunks/<hex>`, `manifest`, `manifest.lock` | an exclusive `LOCK`, then `If-None-Match: *` / `If-Match` | **no**: Apache `mod_dav` lets racers through both checks |
| Google Drive | `appDataFolder`, flat: `<prefix>c.<hex>`, `<prefix>m.<generation 20 digits>.<random>` | create the next generation's file, keep the first name of that generation | **no**: Drive v3 has no conditional write |
| OneDrive | `special/approot`: `<root>/chunks/<hex>`, `<root>/manifest` | `conflictBehavior=fail`, then `If-Match: <eTag>`; upload sessions above 4 MiB; downloads follow the redirect without the token | yes |
| Dropbox | App Folder: `<root>/chunks/<hex>`, `<root>/manifest` | `files/upload` `mode: add`, then `mode: update(rev)` | yes |
| iCloud Drive | the native bridge's `chunks/<hex>`, `manifest` | `replace_if` in one coordinated write (the interface the app's native half must meet) | yes, by contract |

Where a swap is not atomic, the run reads the manifest back and reports a conflict if it was
overtaken; a device overtaken after that sees a fork at its next run (§11.4), never a silent
loss. Only the primary writes, so such a race needs two primaries (around an account recovery).

**Signing in to the clouds** (OAuth 2.0 for native apps, RFC 8252, with PKCE S256, RFC 7636):
public client IDs, no secret; a random `state`; the loopback redirect on desktop; refresh
tokens kept in the app's keychain; a 401 refreshes once. Scopes: Google
`drive.appdata`, Microsoft `Files.ReadWrite.AppFolder offline_access`, Dropbox an App Folder
app with `files.content.read files.content.write`.

### 11.9 Device-to-device transfer (M4, D-0123)

The new phone listens on the local network and shows a QR code; the old phone scans it,
connects over TCP and streams a backup straight to it (`crates/backup-storage::transfer`):

- **Code**: `songbird-transfer:` and base64url (no padding) of
  `"SBT" | 1 | psk [32] | port u16 | n u8 (1..=8) | n × (family u8 (4 | 6) | address [4 | 16])`.
- **Commitment**: on each connection the new phone first sends `len u16 | 32 bytes`,
  `SHA-256("songbird.v1/transfer-commit\0" || its ephemeral public key)`, before it sees the
  old phone's; the old phone refuses a reply whose key doesn't match. Without it a man in the
  middle who had seen the code could try keys until both phones showed the same six digits
  (about 2²⁰ tries, seconds); with it each try is one connection with a one-in-a-million chance
  (D-0125).
- **Channel**: Noise `NNpsk0_25519_ChaChaPoly_SHA256` (revision 34) on aws-lc-rs, prologue
  `"songbird.v1/transfer\0" || the code's payload`, the old phone initiating. Only a phone that
  read the code completes the handshake; a connection with another code, or one silent for 20
  seconds, is dropped and the new phone keeps listening (several handshakes at once, so one
  stalled connection holds up nothing). Both show a 6-digit code,
  `u32(SHA-256("songbird.v1/transfer-code\0" || handshake hash)[0..4]) mod 10⁶`, and the user
  confirms it on **both**: the old phone sends nothing before, and the new phone answers `OK`
  and applies nothing before. Transport messages are Noise's (`len u16 | ciphertext`, at most
  65,535 bytes), carrying a byte stream of records `kind u8 | len u32 | body` (at most the
  largest manifest object): `1 HELLO` (account, hand-over token, its device ID,
  a one-time recovery key's secret), `2 CHUNK` (name and object), `3 MANIFEST`, `4 DONE`, and
  `5 OK` from the new phone once **every chunk is verified and the backup restored** (§11.6).
- The backup is the format of §11 under the one-time key, written by the same run as every
  store. Then the new phone recovers the account with the hand-over token (§4.11): no PIN, the
  old primary consented. Nothing passes through Songbird's servers except that call.
- Vector: `crates/backup-storage/vectors/transfer.json`, re-derived from the Noise
  specification by `vectors/transfer_check.py` (X25519, ChaCha20-Poly1305 and HMAC written from
  RFC 7748, RFC 8439 and RFC 2104 with the Python standard library).

## 12. Versioning and compatibility rules

- **Wire frames** (§1.2): one encoding per frame, unknown flag bits refused; new optional blocks
  come behind new flag bits, in a fixed order (D-0061). `protocol_versions` changes only for an
  incompatible frame.
- **Encrypted content** (§5.2): protobuf, unknown fields, kinds and enum values ignored,
  `required_version` for what an older app must not misread; every published version's vectors
  decode with every later reader (`crates/content/tests/compat.rs`).

## 13. Test vectors

Frames (hex; from `crates/proto` tests and the fuzz seeds in `fuzz/corpus/proto_*`):

| Frame | Encoding |
| --- | --- |
| `Ack{up_to_seq: 42}` | `02 000000000000002a` |
| `Deliver{seq: 1, envelope: "envelope"}` | `81 0000000000000001 656e76656c6f7065` |
| `SendResult{req_id: 7, accepted}` | `82 00000007 00` |
| `SendResult{req_id: 7, rejected: rate limited, retry 500 ms}` | `82 00000007 01 0001 000001f4` |
| `Ephemeral{to: 44…44, "typing"}` | `03 44444444444444444444444444444444 747970696e67` |
| `Send{req_id 7, key 11…11, [(22…22, "abc"), (33…33, "z")]}` | `01 00000007 11111111111111111111111111111111 00 0002 22222222222222222222222222222222 00000003 616263 33333333333333333333333333333333 00000001 7a` |
| `Send{req_id 8, key 11…11, group message (group 44…44), shared "grp", no devices}` (Prompt 5; replaces D-0026's MLS commit vector, whose bytes are now refused) | `01 00000008 11111111111111111111111111111111 42 44444444444444444444444444444444 0000 00000003 677270` |

Authentication transcript for server name `chat.example`, nonce 32 × `11`, device 16 × `22`,
version 1, channel binding 32 × `33`:

```
736f6e67626972642e76312f6465766963652d6175746800 0c 636861742e6578616d706c65
1111111111111111111111111111111111111111111111111111111111111111
22222222222222222222222222222222 00000001
3333333333333333333333333333333333333333333333333333333333333333
```

Registration (Prompt 3, D-0048), from `crates/pow` and `crates/proto` tests:

- Proof-of-work challenge for registration secret 32 × `42`, issued at 1,763,164,800,000 ms,
  memory 64 KiB, 1 iteration, 4 bits, 2 solutions, nonce 16 × `11`:

  ```
  01 0000019a84cfc400 00000040 00000001 04 02 11111111111111111111111111111111
  0062f574373fcffeceeac3d8e2446d66
  ```

  Its solutions are `[7, 31]`; the Argon2id output for counter 7 is
  `039ed8433f9111cfc95132559389a12a974d312b0f22e013cacb546aff63fd56`, which the reference
  `argon2` command (phc-winner-argon2) reproduces.
- Invite code bytes `00 01 02 … 0e` (15 bytes counting up) display as
  `000G-40R4-0M30-E209-185G-R38E` (checked independently against a Python implementation of
  Crockford base32).
- Registration request with challenge 51 × `11`, solutions `[7, 31]`, that invite code, identity
  key 32 × `22`, authentication key 32 × `33`, signatures 64 × `44` and 64 × `55` (269 bytes):

  ```
  01 | 51 × 11 | 02 | 00000007 0000001f | 01 | 000102030405060708090a0b0c0d0e
    | 32 × 22 | 32 × 33 | 64 × 44 | 64 × 55
  ```
- The identity key's transcript for server name `chat.example` and the same challenge and keys:

  ```
  736f6e67626972642e76312f72656769737465722d6964656e7469747900 0c 636861742e6578616d706c65
    | 51 × 11 | 32 × 22 | 32 × 33
  ```

  The authentication key's differs only in its context, `"songbird.v1/register-device\0"`.
- Username (Prompt 3, M2, D-0049), from `crates/keys/tests/username_vectors.rs`: the
  generator `G` is `9c921379c3fdb3bf5dc4810f37d8fb50ea2e9392fa28b908ca6fdd849f0c627c`; the username `Radu` hashes to `b0aff097b1eb85225fbb895965cb81ff2757a8e39f458e731743270b1fb5ad09`; its proof with randomness 32 ×
  `11` is

  ```
  77c74bbf972c93f70681b1b75869c7538915a43910e79341499dc4428559710d
  0bd820c1436c8e463b66283e3b805440c59905fc025c964fa709c7be6a031f04
  ```

  libsignal's poksho (commit `8be00c2`) produces the same proof for the same statement and
  randomness, and verifies it.
- XEdDSA (Prompt 3, M3, D-0050), from `crates/keys/tests/xeddsa_vectors.rs`: private key 32 ×
  `31`, message `songbird.v1/xeddsa-vector`, signing randomness 64 × `42`. libsignal (commit
  `8be00c2`) and Songbird both give the public key
  `0504f5f29162c31a8defa18e6e742224ee806fc1718a278be859ba5620402b8f3a` and the signature

  ```
  5bdccfa0250b116ef988e11c678a3562f8b37884cbc3c2ee720d87760a0e8cf0
  68aba378d09d70b2ad14b47a44709b3b72400c10bd2bdffbe84ee24b29373e82
  ```

  and each verifies the other's signature.

Sender certificate (Prompt 3, D-0054), from `services/api` `certificate_vector`, cross-checked
with OpenSSL 4.1. The inputs are: Ed25519 seed 32 × `42`; account 16 × `01`; device 16 × `02`;
identity key `05` || 32 × `03`; expiry 1,800,000,000,000 ms. The certificate:

```
01 3097e2dee2cb4a34 01010101010101010101010101010101 02020202020202020202020202020202
05 0303030303030303030303030303030303030303030303030303030303030303 000001a3185c5000
e2ba85e62f8006c7ff540bbed16ff64f1636cb28133f05da28b9604b0d4d32596d486dd773916abf4a674b75813703d273e747b840ed8288da465c69f4c98804
```

WebAuthn (Prompt 3, D-0053): `crates/keys/tests/webauthn_vectors.rs` carries the W3C WebAuthn
Level 3 test vectors (§16.2, §16.11) verbatim.

Key transparency (Prompt 3, D-0057): `akd` 0.13's proofs and their protobuf encoding are akd's
own. `crates/akd/tests/akd_live.rs` checks Songbird's storage with akd's client verifiers. The
proofs are accepted, and refused under another label, another root, another VRF key or a
rewritten history.

Encrypted content (Prompt 4, D-0059): `crates/content/vectors/v1/vectors.json` holds 33 vectors,
one or more per kind of content, each as hand-written protobuf JSON and the hex the Rust types
produce. `tools/content-vectors` checks with an independent compiler (protox) that
`content.proto` encodes each JSON to exactly that hex. Two of them:

- `reaction` (a 👍 from the author of message 16 × `33` by 16 × `22`, counter 7; message ID 16 ×
  `01`, sent at 1,800,000,000,000 ms):

  ```
  0801 1a10 01010101010101010101010101010101 2080a0f1c2b134
  5230 5a2e 0a24 0a10 22222222222222222222222222222222 1210 33333333333333333333333333333333
  1204 f09f918d 2007
  ```
- `plaintext_franked`: that content under franking key 32 × `42`, padded to 160 bytes; its
  commitment is `4cc19e38508a8cdbcbac905629cee83e217741cf3cc0878bd00fee0bb9fb2c50`
  (recomputed with Python's `hmac`).

Send frames with hints (Prompt 4, D-0061), from `crates/proto/tests/vectors.rs`:

| Frame | Encoding |
| --- | --- |
| A franked message request that disappears after an hour: `Send{req_id 9, key 11…11, REQUEST, commitment 55…55, expires 3600, to 22…22 "abc"}` | `01 00000009 11111111111111111111111111111111 38 5555555555555555555555555555555555555555555555555555555555555555 00000e10 0001 22222222222222222222222222222222 00000003 616263` |
| A contact's send: `Send{req_id 10, key 11…11, ACCESS 66…66, to 22…22 "z"}` | `01 0000000a 11111111111111111111111111111111 04 66666666666666666666666666666666 0001 22222222222222222222222222222222 00000001 7a` |

Franking stamp (D-0061), from `crates/send/src/franking.rs` `stamp_known_answer`, recomputed
independently with Python's `hmac` and `hashlib`: secret 32 × `46`, commitment 32 × `01`, time
1,800,000,000,000 ms, sender 16 × `0a`, recipient 16 × `0b`. The key ID is
`3df2c8855889e63b` and the tag is
`114c33cc2b8f1a5e0ad8943b34ebb738c10f3397504e858b44b8e06e45a8b3b6`.

Attachments (Prompt 6, D-0080): `crates/content/vectors/attachment/v1.txt`, described in §7.1,
checked by `crates/content/tests/attachment.rs` and independently by
`tools/attachment-vectors/check.mjs` (Node.js and OpenSSL). For example, AES-256-GCM (version 2)
over the empty file gives the commitment
`4d3849c61240da287ae2a50f294f8ca593a037dc678a57f7846e425936206aac` and a 1,073-byte ciphertext
with SHA-256 `fc5c71776298012c8c116848d978cc7068a420c207ca7824f6891f5c70969cb5`.

Packs (Prompt 6, D-0083): for pack key 32 × `33`, object 0's key is
`b4b86d6d8340ed7c0242fbee69938c04cff4d62f59120a703df1ec02124c3c92a328512b4a91c35aee31481cae363c507eed2d7cf6a4fd4907e1643bda6af388`
and object 1's is
`259a6b7cb7cf7d091b07cae12cf177127ebb19396d61a4b6bf3f2993f5f7a23f498f371981a38260ef70a53d15521470114e39017c42b3c995a8fa6a5aa788e7`
(`crates/content/src/pack.rs`; reproduced by `tools/attachment-vectors/check.mjs`).

Calls (Prompt 7, D-0086): `crates/content/vectors/call/v1.txt` (§8.2, §8.4), checked by
`crates/content/tests/call.rs` and independently by `tools/call-vectors/check.mjs` (Node.js and
OpenSSL). For call ID `00 01 … 0f` and `call_secret` `40 41 … 5f`, the sender key of identity
`0123456789abcdef0123456789abcdef` is
`9abfb1f35fa88b5c072d6410879d17a86fff15a196b82fe917a2bb113e490aeb`. For link root `00 01 … 1f`,
the link ID is `5f88f381e38c44f13807d6c1ca1268a94fdd7437f745cca65ac91b8a29b5480d` and the server
stores `19a4a59124d6c69329890a7fe6be812afda73dbf92737d7873f18d4c85af3853`.

## 14. Plans, entitlements and abuse (Prompt 11)

### 14.1 Levels, the catalogue and what is never gated

Plans are levels: `0` Free (the default, never stored) and `1` Plus. Every Family seat is
Plus: the server cannot tell a seat from a subscription (D-0126 §5). Teams will add a source
(an organisation), not a new kind of entitlement.

The **catalogue** (`[api.plans]`, `songbird_entitlements::Catalog`) gives each level its limits
and flags, with a version the apps compare. Defaults (D-0126 §3):

| | Free | Plus |
| --- | --- | --- |
| `linked_devices` (besides the primary) | 5 | 10 |
| `file_bytes` (largest attachment ciphertext) | 2 GiB | 4 GiB |
| `upload_bytes_per_day` | 10 GiB | 50 GiB |
| `vault_bytes`, `vault_chunks` | 5 GiB, 100,000 | 50 GiB, 1,000,000 |
| `packs_per_month` (new packs per 30 days) | 5 | 50 |
| flags `app_icons`, `chat_themes`, `priority_support` | — | ✓ |

A catalogue is refused at start-up when a higher level allows less than a lower one, or a
number is outside its bounds. **Never in the catalogue, on any plan:** encryption, safety
numbers and verification, disappearing messages, app lock, the PIN and passkeys, the proxy,
backups and restore, every privacy setting, calls, groups, reporting, blocking, export and
deletion. The flags gate only extras whose settings are end-to-end encrypted, so apps check
them; the server enforces every limit.

Each `[api…]` number that a plan now sets stays as the **deployment's ceiling**:
`[api.keys.devices] max_devices` (default 16), `[api.attachments] max_bytes` (4 GiB; it must
be at least every plan's `file_bytes`), `[api.media.budgets] bytes_per_day` (1 TiB),
`[api.vault] quota_bytes` and `quota_chunks` (1 TiB, 10,000,000). An account's limit is its
plan's under that ceiling.

### 14.2 Entitlements and the snapshot

The registry keeps one row per account not on Free: `ent:[account]` with `level`, `last` (the
last UTC day it holds, days since the Unix epoch, inclusive), `source` (`grant` from an operator,
`token` from an anonymous redemption, §14.4) and `at`. Every change, and the account's deletion,
bumps `entver:v`. Nothing about a payment is ever stored here.

Every service that enforces a limit reads a **snapshot** of those rows
(`songbird_entitlements::Live`, `arc-swap`): only the exceptions, reloaded whole when `entver`
changes (`api` polls every `entitlements_poll_ms`, 1 s by default). Rules:

- **Expiry is by date, at the reader.** An entry past its last day reads as Free whatever the
  snapshot says; the worker's sweep deletes such rows only to keep things small. A stale
  snapshot or a stopped worker can never extend an entitlement.
- **Downgrades never delete.** Over a limit, what exists stays and keeps working (linked
  devices, a vault's chunks, which still read and restore); only additions are refused until
  the account is back under. A signed device list is checked against the deployment's
  ceiling, so a downgraded account can still sign its list.
- **Each level has budgets of its own** (upload bytes, new packs), so an upgrade gives the new
  level's whole budget at once.

`songbird-admin plans grant --account <hex> --level plus --days <n>` sets an account's level
through a last day; `revoke` puts it back on Free; `show` prints the row. Both writes need
`--confirm` and are audit-logged with the account.

### 14.3 Requests

| Request | Body | Answer |
| --- | --- | --- |
| `GET /v1/entitlements` | signed, a listed device | `{"level": "free" \| "plus", "last_day": <UTC day> \| null, "standing": "good" \| "limited" \| "suspended", "catalog": <version>, "limits": {...}, "flags": {...}}`; 403 `pending` for a device not yet in the signed list |
| `POST /v1/entitlements/redeem` | signed, a listed device; a token (§14.4) | 200 `{"level", "last_day"}`; 400 `token`, `level`, `kind`; 409 `period` (not this month or the next), `invalid` (not the PRF of its nonce), `spent`; 429 past 30 an hour (burst 10) per account |
| `GET /v1/tokens/keys` | none | `{"current": id, "keys": [{"id", "public": <64 hex>}]}`, the keys apps pin |

Plan refusals name the plan so the app can explain the moment (`docs/design/flows/plans-paywall.md`):

| Where | Answer |
| --- | --- |
| `POST /v1/attachments` over the plan's `file_bytes` (under the ceiling) | 413 `{"error": "plan", "file_bytes": n}` (400 still for past the ceiling or a length the format never produces) |
| `POST /v1/attachments`, `POST /v1/packs` past the daily bytes | 429 with `Retry-After` (unchanged) |
| `POST /v1/packs` past the plan's new packs | 429 `{"error": "plan", "packs_per_month": n}` with `Retry-After` |
| `POST /v1/devices/link-token`, `POST /v1/devices/link` at the plan's device limit | 409 `{"error": "limit"}` (unchanged) |
| Vault writes past the plan's space or chunks | 409 `{"error": "full"}`, as for the quota before (`GET /v1/vault` reports the owner's plan's quota); deletions always pass |

### 14.4 Anonymous tokens (D-0126 §2, D-0128)

Payments and accounts share no identifier. Billing (M3) issues **tokens** for what was paid;
the account redeems them. The primitive is RFC 9497's **POPRF** with `ristretto255-SHA512`
(`crates/tokens`, written from the RFC and checked against all of its Appendix A.1 vectors):

- **Keys.** One 32-byte secret, shared by `api` and billing only (`[api.tokens] secret`). Key
  ID `n`'s pair is `DeriveKeyPair(secret, "songbird.v1/token-key\0" || n)` in POPRF mode. The
  highest listed ID issues; every listed ID redeems. Apps pin the public keys (`GET
  /v1/tokens/keys` is how they are first published; a build ships them).
- **Metadata (`info`).** `"songbird.v1/token\0" || kind u8 || level u8 || period u32` (24
  bytes, big-endian). Kinds: 1 plan (period: months since January 1970, UTC), 2 a vault day
  (UTC day), 3 a vault creation (180-day window); 2 and 3 are M4's.
- **Request** (client → issuer): `{"key", "kind", "level", "period", "blinded": [<64 hex>…]}`,
  1 to 16 blinded elements, each `blind · HashToGroup(nonce)` for a fresh random 32-byte nonce
  and blind.
- **Answer** (issuer → client): `{"evaluated": [<64 hex>…], "proof": <128 hex>}`, one batched
  DLEQ proof. The client checks it against the **pinned** key tweaked by `info` before
  unblinding: an issuer evaluating under another key (tagging one user) or other metadata is
  refused.
- **Token** (client → redeemer): `{"key", "kind", "level", "period", "nonce": <64 hex>,
  "output": <128 hex>}`. The redeemer recomputes `Evaluate(sk, nonce, info)` and compares in
  constant time.
- **Nullifier**: `SHA-256("songbird.v1/token-nullifier\0" || key || info || nonce)`, kept in
  `tnull` until the token could no longer be redeemed. Burnt in the same transaction as the
  entitlement it buys.

**Plan tokens.** One per month per level; a month's token is redeemable during that month and
the one before (renewals are fetched in the last days of a month). It gives the level through
the month's last day plus **3 days of grace**. A refund or a chargeback stops further tokens,
so it costs at most the rest of a month. Caps per subscription per month (billing): Plus 2 (one
and a spare for a reinstall), Family 7 (six seats and a spare).

**Unlinkability.** The issuer sees the metadata and blinded elements; the redeemer sees the
metadata, the nonce and the output. With the blind uniformly random, the blinded element is
independent of the nonce, so the two views share no value and cannot be joined, even by one
operator holding both databases (RFC 9497 §7.1). What remains is **timing and the network
address** of an issuance followed by a redemption (neither is logged, invariant 6; the app
redeems renewals at a random time) and the **metadata itself** (a month and a level are shared
by everyone on that plan that month).

**Vector** (`crates/tokens/vectors/v1.json`): secret `07…07`, key ID 1, a Plus token for month
683 (December 2026), nonce `11…11`, blind and proof scalars from 64 bytes of `22` and `33`
reduced; every intermediate value is listed. `vectors/check.py` reproduces RFC 9497 A.1.3.1 and
this vector with the Python standard library alone (ristretto255 from RFC 9496).

### 14.5 Billing (D-0129)

`billing` is a service of its own (`services/billing`): its own SurrealDB database and
credential, its own uid on the host, no access to the registry, and the only holder of the
providers' credentials. It never sees an account.

| Request | Who | Body | Answer |
| --- | --- | --- | --- |
| `POST /v1/billing/tokens` | the app (no account) | `{"proof": {"apple": <signed transaction>} \| {"google": <purchase token>} \| {"mollie": <64 hex subscriber secret>}, "request": <§14.4 request>}` | 200 the §14.4 answer; 400 `body`, `request`; 402 `not_paid`; 403 `proof`; 409 `period`; 429 `cap` or `budget`; 503 `provider` |
| `POST /v1/billing/apple` | Apple | App Store Server Notifications V2 (`{"signedPayload"}`) | 200 (also for a replay); 400 not Apple's, not 2.0, another app or environment; 503 retry |
| `POST /v1/billing/google` | Pub/Sub push, `Authorization: Bearer <Google OIDC>` | real-time developer notification | 200 (also for a replay); 401 the token; 400 another package; 503 retry |
| `POST /v1/billing/mollie/checkout` | the app | `{"plan": "plus" \| "family", "interval": "month" \| "year", "subscriber": <64 hex>}` | 200 `{"checkout": <Mollie URL>}`; 409 `exists`; 409 `revoked` (refunded or charged back: the app starts over with a new subscriber secret) |
| `POST /v1/billing/mollie/webhook` | Mollie | `id=tr_…` (form) | 200 (unknown IDs too); 503 retry |
| `POST /v1/billing/mollie/state`, `/cancel` | the app | `{"subscriber"}` | state `{"plan", "interval", "paid_through", "renews", "revoked"}`; cancel 204; 404 |
| `POST /v1/billing/mollie/resume` | the app | `{"subscriber"}` | 204: a new subscription on the customer's mandate, its first charge the day the paid period would renew (D-0169); 404; 409 `revoked`, `expired`, `renews`, `mandate` |

**Rows.** `sub` under `HMAC(billing secret, provider ‖ 0 ‖ ID)[..16]`, where the ID is Apple's
original transaction ID, Google's purchase token, or the app's Mollie subscriber secret: the
provider, the plan, the last UTC day paid for, revoked, renews, and the provider time of the
newest state applied. Apple's and Google's IDs are never stored. Mollie's customer and
subscription IDs are (they are needed to cancel). The row's `stamp` is the provider's time of
a pushed message (an answer from Apple's or Google's API keeps the row's stamp; a transaction the
app presents counts by its day) and `at` is a whole UTC day: nothing records when a token
request came, so issuance cannot be joined to a redemption (`ent.at` is a day too; D-0134). `subtok` counts tokens per subscription and
period; `notif` remembers handled message IDs for 45 days.

**Verification.**
- **Apple:** every JWS's `x5c` must be exactly `[leaf, intermediate, root]` with the root
  byte-equal to the pinned one (Apple Root CA - G3 in production, checked by its SHA-256 at
  start), the leaf carrying `1.2.840.113635.100.6.11.1` and the intermediate
  `1.2.840.113635.100.6.2.1` (Apple's intermediates also sign developer certificates), the chain
  valid now (rustls-webpki), then `ES256` by the leaf. Bundle ID and environment must match; a
  transaction must be an auto-renewable subscription without `appAccountToken`. With the App
  Store Server API configured (ES256 JWT, `aud` `appstoreconnect-v1`), the state comes from Get
  All Subscription Statuses (5 revoked, 2 expired) every time; else from the signed transaction
  and renewal info (`expiresDate`, `revocationDate`, `gracePeriodExpiresDate`).
- **Google:** the state always comes from `purchases.subscriptionsv2.get` under the service
  account (`RS256` assertion, scope `androidpublisher`): active, in grace or cancelled are paid
  through `expiryTime`; anything else is not. A test purchase is refused in production; a
  purchase with `obfuscatedExternalAccountId` is refused. Pending purchases are acknowledged.
  A notification's OIDC token must be Google's (`RS256`, keys from Google's set, refreshed on an
  unknown key), issuer `accounts.google.com`, the configured audience and push service account,
  e-mail verified, not expired. A voided purchase revokes for good.
- **Mollie:** customers have no name or e-mail; a webhook's payment is fetched back by ID; the
  first payment's metadata names the row only by its key; a paid first payment starts the
  subscription (once, even if Mollie calls twice); **each payment counts once** (its ID kept 400
  days, however often its webhook is replayed) and moves `paid_through` to a period after the day
  it was paid (`paidAt`, UTC) plus 7 days (SEPA Direct Debit takes days); only the first payment
  turns renewal on; a refund or chargeback revokes, and a revoked row takes no new checkout
  (D-0134).

**Ordering and replays.** A state is applied only if its provider time is not older than the
row's; an answer from Apple's or Google's API counts as now. Handled message IDs are remembered
and skipped. Caps are spent in one transaction (retried on write conflicts), so concurrent
requests cannot pass them together.

**Caps** per subscription per month: Plus 2, Family 7 (§14.4). A token for the current month
needs the subscription paid today; for the next month, paid on that month's first day.

### 14.6 Family seats (D-0126 §5, D-0130)

A Family subscription's owner hands each member a month's plan token through a **family
mailbox** in `api`. No request carries an account and no row names one.

| Request | Needs | Answer |
| --- | --- | --- |
| `POST /v1/family` | `{"manage": <64 hex: SHA-256("songbird.v1/family-manage\0" ‖ management token)>}` | 200 `{"family": <32 hex>}` |
| `PUT /v1/family/<family>/slots/<slot>` | `x-songbird-family-token: <management token>`; `{"auth": <64 hex: SHA-256("songbird.v1/family-bearer\0" ‖ bearer)>, "sealed": <hex, at most 4,096 bytes>}` | 204 (a new slot, or the box replaced when `auth` matches); 409 `full` (five slots) |
| `GET /v1/family/<family>/slots/<slot>` | `x-songbird-family-bearer: <bearer>` | 200 `{"sealed": <hex> \| null}` |
| `DELETE /v1/family/<family>/slots/<slot>` | the management token (removal) or the slot's bearer (leaving) | 204 |
| `DELETE /v1/family/<family>` | the management token | 204 |

Every miss (no family, no slot, a wrong token or bearer) is the same 404. One budget per IP
range (600 an hour). A mailbox untouched for 60 days is deleted.

**Seats** (`crates/tokens::family`): a slot ID and a 32-byte secret, shared as
`https://songbird.chat/family#<family>.<slot>.<secret>` (the fragment never reaches a server).
`HKDF-SHA256(secret, salt "songbird.v1/family-seat", info label ‖ family ‖ slot)` gives a
**bearer** (label `bearer`, shown to the server) and a **key** (label `key`, never shown) that
seals the token with ChaCha20-Poly1305 (`nonce 12 ‖ ciphertext ‖ tag`, AAD `family ‖ slot`).

**Each month** the owner's app fetches seven Family tokens from billing (§14.5), redeems one,
seals one into each member's slot, and keeps the spare. Members fetch their slot and redeem on
their own account: each is Plus, and the server cannot tell a seat from a subscription.
**Removal** deletes the slot at once; the member keeps Plus through the month already redeemed
and its grace. **Leaving** deletes one's own slot.

### 14.7 Abuse controls and standings (D-0126 §7–§8, D-0131)

**Escalating proof of work.** A registration challenge's `bits` is the configured base plus one
for each of 3, 10 and 30 registrations from the requesting range (IPv4 /24, IPv6 /48) in the
current hour, plus one at `global_step` (300) registrations across the deployment and another
at four times that, at most `max_extra_bits` (4) above the base. Challenges carry their
parameters (§4.1), so clients need no change. The hourly counters are keyed records in
`valkey-hot`.

**Standings.** `standing:[account]`: `limited` or `suspended`, a last UTC day or none (until
lifted), a reason category and its source (`auto` or `moderator`). Changes bump `entver`, so
`api` and every delivery node apply them within a second (the entitlements snapshot, §14.2).

- **New accounts** (created in the last `young_days`, 30): 5 requests an hour per sender,
  burst 10.
- **Limited**: 1 request an hour, burst 2; `POST /v1/groups/links` answers 403 `standing`.
  Contacts are not affected.
- **Suspended**: every send (WebSocket and `PUT /v1/messages`) is rejected with reason **15
  `Suspended`** (`SendResult` status 1); `GET /v1/certificate` answers 403 `suspended` (sealed
  sends stop when the certificate held expires); `POST /v1/calls`, `/v1/calls/group` and
  `/v1/calls/links` answer 403 `suspended`. Receiving, `GET /v1/account/export`, `DELETE
  /v1/account` and support (appeals) work.

**Signals** (keyed tags, evaluated on arrival; thresholds in `docs/RUNBOOKS/abuse.md`):

- `rsig`: a verified report of a direct message the server attests (`sender_attested`, not a
  group's shared body), per keyed tag of the sender and day, 31 days; each reporter counts once
  per sender in that time (a keyed pair mark, `ab:r:`), so a limit needs five people (D-0134).
- `bsig`: a new block of an account that sent the blocker a message request in the last week
  (the gate's keyed pair mark `rp`, seven days, **consumed** by the block it counts: unblocking
  and blocking again counts nothing), per keyed tag and day; weighed against the account's
  requests per day (`rq`, keyed, eight days).

Only an automatic `limited` with a reason `reports` or `blocks` is ever set without a moderator;
it never overrides or shortens a moderator's row.

### 14.8 Support tickets (D-0126 §11, D-0132)

A ticket is opened by the app, signed or not, and read with a bearer secret. Messages are sealed
on the device and opened only by staff with the support key, or by the app with the ticket's
reply key: the server stores ciphertext. `kind` is `help`, `appeal` (signed only), `billing` or
`crash` (unsigned only: a crash report, scrubbed on the device and sent only when the user turned
them on; D-0170).

| Request | Auth | Body | Answer |
| --- | --- | --- | --- |
| `GET /v1/support/key` | — | — | 200 `{"key"}`: the support key (X25519, hex) |
| `POST /v1/support/tickets` | optional (signed, §2) | `{"kind","reply_key","sealed"}` | 201 `{"ticket","bearer","priority"}` |
| `GET /v1/support/tickets/<id>` | `x-songbird-ticket-bearer` | — | 200 `{"kind","messages":[{"from","sealed","day"}]}` |
| `POST /v1/support/tickets/<id>/messages` | `x-songbird-ticket-bearer` | `{"sealed"}` | 204 |
| `DELETE /v1/support/tickets/<id>` | `x-songbird-ticket-bearer` | — | 204: the ticket and every message deleted |

- `kind`: `help`, `appeal` or `billing`. **Signed**, the device's account is attached and
  `priority` is true on Plus or above; **unsigned**, no account. An `appeal` must be signed
  (401 otherwise); a suspended account may open one.
- `reply_key`: an X25519 public key the app makes for this ticket alone (hex); staff seal their
  replies to it.
- `sealed`: hex of `seal_with(Domain::Support, support key, plaintext)` (the user's) or
  `seal_with(Domain::SupportReply, reply key, plaintext)` (staff's): version byte, ephemeral
  X25519 key, ChaCha20-Poly1305 under HKDF (salts `songbird.v1/support`,
  `songbird.v1/support-reply`). At most 32 KiB sealed.
- Plaintext (JSON): `{"text"}`, and from the user, only with their consent after a preview,
  `"logs"` (the device's scrubbed log excerpt) and `"app"` (version and platform).
- `ticket` (16 bytes) and `bearer` (32 bytes) are random, drawn by the server and returned once
  (hex). The registry keeps `SHA-256("songbird.v1/ticket-bearer\0" || bearer)`. A wrong or
  missing bearer and a missing ticket are the same 404.
- At most 50 messages a ticket (409 `full`). Budgets per keyed IP range (IPv4 /24, IPv6 /48): 6
  opened an hour (burst 3), 240 other calls an hour (burst 60); 429 with `Retry-After`.
- A ticket untouched for 90 days is deleted with its messages. Deleting the account unlinks its
  tickets (they stay with their bearer). The account's export lists the tickets it attached
  (ID, kind, message count, last day), never their contents.

### 14.9 Legal requests and transparency (D-0126 §9–§10, D-0132)

No wire protocol: operator tooling (`songbird-admin legal`, `transparency`;
`docs/RUNBOOKS/legal-requests.md`). A legal export of an account is exactly its own data export
(`GET /v1/account/export`), wrapped with the case reference and a scope statement. The registry
keeps only counts: `legal:[half, country, kind, outcome]` → `n` (`half` is `YYYY-H1` or
`YYYY-H2`, UTC).

### 14.10 Invites: links and status (D-0126 §12, D-0132)

**Invite link.** `https://songbird.chat/i#<base64url, unpadded>` of `version (1) || flags (1;
bit 0: a code follows) || [registration code (15)] || inviter account ID (16) || inviter profile
key (32) || SHA-256 fingerprint of the inviter's identity key (32)`. Parsing is strict (one
version, no unknown flag, exact length, canonical encoding, at most 256 characters;
`crates/proto/src/invite_link.rs`). The fragment never reaches a server. After sign-up the app
fetches the inviter's profile and keys like any contact's, checks the identity fingerprint, and
opens the chat.

**Status.** `POST /v1/invites/status` (signed) `{"codes": [hex SHA-256("songbird.v1/invite\0" ||
code), …]}` (1 to 50) answers `{"live": [bool, …]}` in order: whether each code still has a use
left and has not expired or been revoked; 60 asks an hour per account (burst 20, keyed). The
inviter's app counts its invites used from the codes it made; the server keeps no per-inviter
count (D-0038).

# A2 — app-core: the gap inventory and the plan (D-0136)

Date: 2026-11-19. Owner's request: "implement everything that remained unimplemented in terms of
functionality, fill all the gaps on the backend that the frontend has implemented, make this app
production ready to launch."

## 1. Where the gaps are

The **server** is complete for what the apps need through Prompt 11: registration, usernames, the
key directory, devices and the signed device list, profiles and sealed sender, key transparency,
content franking and reporting, groups and channels (MLS delivery service), attachments and packs,
calls (LiveKit), push, the record store and presence, backups and the vault, plans, billing, abuse
and support. Every app screen has a server endpoint behind it, except the WhatsApp bridge
(Prompt 14, waiting for its legal gate).

The **app** (`apps/ui`, D-0092) has every screen, but 77 `// mock:` markers stand where
**app-core (A2)** should be. A2 was never built. Without it no two people can exchange a message:
there is no client-side session encryption, no local store, no message pipeline. The mocks group
as follows.

| Area | Mocked today | Needs |
| --- | --- | --- |
| 1:1 messaging | sending, receiving, ticks, edits, deletes, reactions, receipts, typing (`store/ext_chat.rs`, `ext_chats.rs`, `backend/ws.rs` never acknowledges) | PQXDH + Double Ratchet, the envelope, the pipeline, the store |
| Groups, communities, channels | creation, membership, roles, change log (`store/ext_groups.rs`) | an MLS client (OpenMLS) over `POST /v1/groups*` |
| Attachments, voice notes, view-once, stickers | upload, download, playback (`chat/sheets.rs`, `bubbles.rs`, `media/viewer.rs`) | streaming encryption (PROTOCOL §7.1) and the upload grants |
| Calls | media, participants, call links (`calls/*`) | the call key schedule (§8.2) and LiveKit's client SDK |
| Presence, nudges | send nothing until contacts' devices are known (`backend/presence.rs`, `nudge.rs`) | sessions (sealing per device) |
| Search, export | local search, chat export | the store's encrypted full-text index |
| Platform | photo/document pickers, share sheet, passkeys, biometrics, store purchases, launch at login | native modules (`apps/ui/DESKTOP_GAPS.md`); App Store / Play steps the owner deferred (D-0099) |
| WhatsApp chats | the bridge | Prompt 14 (legal gate) |

## 2. The owner's decisions (2026-11-19)

1. **1:1 protocol: implement from the specifications** (PQXDH, the Double Ratchet, sealed
   envelope), as D-0047 did for the server; libsignal (AGPL-3.0) is not linked. An external
   cryptographic audit is required before public launch.
2. **Local store: SQLCipher** (the owner, after asking why the agent first recommended SQLite with
   column sealing: SQLCipher encrypts the whole file, schema, counts, sizes and the search index
   included). OpenSSL enters only inside SQLCipher's page cipher (CommonCrypto on Apple), keyed
   from a secret wrapped by the Secure Enclave or Android Keystore; recorded as an exception to the
   locked stack's wording.
3. **Milestones in one session, one branch, one PR**.

## 3. Milestones

| | Scope | Status |
| --- | --- | --- |
| M1 | `crates/app-core` crypto: X25519/ML-KEM/HKDF/AES-GCM on aws-lc-rs; PQXDH; the post-quantum Double Ratchet; the sealed per-device envelope; safety numbers; sessions per device, prekey generation in the server's upload format | **done** (21 tests, clippy clean) |
| M2 | The SQLCipher store: identity, prekeys, sessions, chats, messages, receipts; versioned migrations; FTS5 search | **done** (D-0137) |
| M3 | The pipeline: key upload and refill (`PrekeysLow`), bundle fetch, device lists, `Send` with device-mismatch retries, receive → decrypt → store → `Ack`; content encode/decode with padding and franking; the status machine; receipts; requests and access keys; the backend gap `POST /v1/devices/identity` | **done** (D-0138; `tests/e2e/tests/gate_a2.rs` against the real stack) |
| M4 | App wiring: `apps/ui` 1:1 chats on app-core (the NSH `Net`, deliveries stored then acknowledged, sends and read receipts, new chat by username) | **done** (D-0139); keystore modules and a two-app run still open |
| M5 | Groups: OpenMLS client over the delivery service | |
| M6 | Attachments: streaming encryption, grants, resumable transfers | |
| Later | Calls media (LiveKit SDK), platform modules, push hosts (deferred by the owner), Prompt 12 (verification and the audit), Prompt 13 (certification), Prompt 14 (legal gate) | |

## 4. What "production ready" still needs that code cannot give

- An **external audit** of app-core's cryptography, and a Tamarin or ProVerif model of the
  post-quantum ratchet step (Songbird's own construction, D-0136) and the envelope.
- The owner's open items from earlier gates: OAuth app registrations, App Store Connect / Play /
  Mollie accounts, the alert receiver, `songbird.chat` names and certificates for production, the
  GitHub environments and SOPS secrets, counsel on GDPR ages and VAT.
- Prompt 13's soak, RTO drill and load tests on production hardware.

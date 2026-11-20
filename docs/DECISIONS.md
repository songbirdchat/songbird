# Decisions

Every "X versus Y" choice, new dependency, `unsafe` exception and owner decision is recorded here,
newest first. Performance decisions cite the benchmark run (hardware, kernel, configuration, numbers).

## Template

```
## D-NNNN — <title>

- Date: YYYY-MM-DD
- Status: proposed | accepted | superseded by D-NNNN
- Decided by: owner | agent (with owner approval)
- Context: what forced the decision
- Options considered: X, Y, ...
- Evidence: benchmark runs, tests, proofs (with links)
- Decision: what was chosen
- Consequences: what this rules in or out; what to revisit and when
```

## Open owner decisions

- Prompt 11 gate (`P11-gate.md`, D-0127 to D-0134): **approved by the owner on 2026-11-19**
  ("Approve all"), with the threat-model rows (§1, §3.27, §3.28, §4, §6). PR #70 waits for the
  owner to merge. Still the owner's: App Store Connect, Google Play and Mollie test accounts
  (the real sandbox runs); counsel on VAT records and the legal-request process; before release,
  the shipped token and support keys and the Apple root's hash.
- Prompt 11 plan (`P11-plan.md`, D-0126): **approved by the owner on 2026-11-19** ("Approve
  all"), every recommendation: a separate `billing` service, anonymous monthly tokens, the plans
  and limits table, Mollie, unlinked Family seats, the vault unlinked from the account, outreach
  limits automatic and suspension by moderators, sealed support tickets, the legal tooling,
  Teams schema only, the app's M7.
- Prompt 10 gate (`P10-gate.md`, D-0120 to D-0125): **waiting for the owner**. M1–M5 and the
  security review's fixes are done; the gate's items pass. For the owner: approve the gate with
  the threat-model rows (§1, §3.27, §6); register the Google, Microsoft and Dropbox OAuth apps.
- Prompt 10 plan (`P10-plan.md`, D-0119): **approved by the owner on 2026-11-19** ("Approve
  all"), every recommendation: account recovery in scope, the 64-character recovery key with a
  check, the vault owned by an account (unlinking re-decided in Prompt 11), recovery by the
  identity key plus the PIN or passkey when set, 5 GiB and 100,000 chunks per vault until Prompt
  11, third-party clouds' sign-in on desktop once the OAuth apps are registered, `mod_dav` in CI.
  Still the owner's: registering the Google, Microsoft and Dropbox OAuth apps. M1 is next.
- Prompt 9 gate (`P09-gate.md`, D-0102 to D-0108; PR #67): **approved by the owner on
  2026-11-18** ("Approve the gate"): M1–M6 and the security review's fixes, with the threat-model
  rows (§1, §3.26, §4, §5). Still open: the GDPR Article 8 ages to confirm with counsel before
  release (§5); device and parental linking between two real phones; presence and nudges from the
  app once app-core (A2) exists.
- Prompt 9 plan (`P09-plan.md`, D-0101): **approved by the owner on 2026-11-18** ("Approve
  all"), every recommendation, the scope inventory included (every Settings row made real except
  Prompts 10, 11 and 14's; host-only rows after the whole app is built). A supervised child may
  delete their own account. M1 (the record store) is next.
- Native camera module (`CAM-plan.md`, D-0110): **approved by the owner on 2026-11-18**
  ("approve all on the camera plan", relayed by the Prompt 9 session), every recommendation:
  the minimal Android host now (its push parts stay deferred), "connect a camera" for a
  primary without one, video clips on Windows and Linux later, portal then V4L2 on Linux,
  `unsafe` in `songbird-camera`'s platform modules only, link codes only from the live camera.
  Still the owner's: a physical iPhone and the personal team for the iOS checks (question 4).
- Camera gate (`CAM-gate.md`, D-0111 to D-0117; PR #68): **approved by the owner on 2026-11-18**
  ("Approve both"): M1–M5 and the security review's fixes, with the gate's open items left open;
  the IJG licence for `jpeg-encoder` only (D-0117). Still the owner's: a physical iPhone and the
  personal team for the iOS camera check.
- Prompt 8, M4 and M5 rest (D-0099): **deferred by the owner on 2026-11-18 until the whole app
  is built** ("lets do this after we have the whole app built"): the Xcode and Gradle host
  projects (APNs token, PushKit with CallKit, FCM, UnifiedPush, ConnectionService), the Apple
  bundle IDs and filtering entitlement, the Firebase Android app, and the real-provider checks
  (APNs sandbox wake-up, VoIP on a physical iPhone, FCM on the emulator).
- Prompt 8 gate (`P08-gate.md`, D-0095 to D-0100; PR #65): **approved by the owner on 2026-11-18**
  ("Approve all"): M1, M2, M3, M6 and the first part of M4 and M5 with the security review's
  fixes; the four threat-model §1 rows (`SILENT`; push registrations in their own database; call
  rings on their own job stream; a ring's timing seen by Apple). Still the owner's: where alerts
  go (email, ntfy or Matrix). The rest of M4 and M5 is deferred until the whole app is built.
- Prompt 8 plan (`P08-plan.md`, D-0094): **approved by the owner on 2026-11-18** ("Approve
  all"), every recommendation, desktop without a push service included. Still the owner's: where
  alerts go (email, ntfy or Matrix; M3 builds with the receiver left to configure); before M4,
  the bundle IDs `chat.songbird.app` and `chat.songbird.app.notify`, the App Group and the
  filtering entitlement request (and whether to enrol an organisation first); before M5, the
  Android app in Firebase `songbird-6fab1`.
- App UI and A1 design (D-0092, `apps/ui`, `docs/design/`): the four-tab IA without Status, the brand
  (feather mark, Dusk violet palette, system fonts), and the mock-first plan; usability tests still to run.
- Prompt 7 gate (`P07-gate.md`, D-0086 to D-0091; PR #62): **approved by the owner on 2026-11-17**
  ("all green you can merge"): M1 to M5 and the
  security review's fixes; nine threat-model §1 rows (a live 1:1 call with its two accounts for at
  most 24 h, a live group call, LiveKit's state, call budgets, the LiveKit secret, call links,
  lobbies, webhook marks, the deny set). Before production: the `calls.` and `turn.` names, their
  certificates and DNS, `[api.calls]` and `[calls]` with their secrets, load tests on the
  production host class, a fuzz run of the two new targets.
- Prompt 7 plan (`P07-plan.md`, D-0085): **approved by the owner on 2026-11-17**, every
  recommendation: 1:1 calls name the callee for the call's life only, Signal-style keys for call
  links, 32 video / 128 audio-only participants, a dedicated Valkey for LiveKit and a second IPv4
  per calls host for TURN/TLS on 443.
- Prompt 6 gate (`P06-gate.md`, D-0079 to D-0084; PR #61): **approved by the owner on 2026-11-17** ("i approve"): M1 to M5
  and the security review's fixes; the benchmarks and the choices they drove (AES-256-GCM, parallel
  16 MiB parts with tus removed, no caching proxy); five threat-model §1 rows (resumable grants up
  to 24 h, upload budgets, downloads by capability, packs, GIF queries). Before production: the
  GIPHY key and terms, and `[api.media]` on every `api` node. Prompt 6 is done once PR #61 merges.
- Prompt 6 plan (`P06-plan.md`, D-0079): **approved by the owner on 2026-11-17**, every recommendation: downloads without
  device authentication, size limit and budgets, pack lifetime, GIF provider and design.
- Prompt 5 follow-ups (`P05-followups.md`, D-0075 to D-0078; PR #60): **approved by the owner
  on 2026-11-17** ("approve all"): the three milestones and the review's fixes; the four
  threat-model §1 rows (the service key and its queued removals in group state, the prune
  marks, link proof keys in the roles extension, the keyed channel record in `valkey-hot`);
  D-0077's fallback to the per-node budget while `valkey-hot` is down; and not rotating the
  router's `cluster_secret` while groups exist, until key rotation is built (D-0075). Done
  once PR #60 merges.
- Prompt 5 gate (`P05-gate.md`, D-0070 to D-0074; PR #59): **approved by the owner on 2026-11-17**
  ("approve all"): M2–M5, the benchmarks and the caps they set (1,024 devices per group, 2,048
  per private channel, X-Wing by default), the seven threat-model §1 rows (roles, metadata,
  change log, invite links, join requests, communities, claimed KeyPackage references), and the
  three follow-ups as future work: server-proposed removal of deleted devices
  (`external_senders`, D-0073), a link proof members can check (D-0071), a cluster-wide channel
  budget (D-0072). Prompt 5 is done once PR #59 merges.
- Prompt 5, M1 (`P05-M1.md`, D-0066 to D-0068): **approved by the owner on 2026-11-16**
  ("Approve all three"): the milestone, the two threat-model §1 rows (group state; pending
  fan-outs) and the three MPL-2.0 licence exceptions for the test harness. M2 (roles,
  permissions, metadata, change log) is next.

- Prompt 5 plan (`P05-plan.md`, D-0065): **approved by the owner on 2026-11-16** ("Approve
  all"), every recommendation. Delivered receipts in large groups: primaries only from 33
  members (D-0065 §5).
- Public channels: not in Prompt 5 (D-0065 §4); whether to build them stays the owner's, after
  legal review.
- WhatsApp interop legal gate (Prompt 14).
- Second EU provider (D-0009): Hetzner only for now (D-0042); the Scaleway code stays dormant.
- Public domains: the owner holds `songbird.chat` (D-0046). Still to decide: which names under it
  serve the client endpoints and the web domain for proxy links (D-0011). The upstream names in
  `deploy/proxy/proxy.example.toml` are placeholders until then.
- Licence for the repository (README says all rights reserved until chosen).
- Deployment access model: SSH as root with a key, through one bastion restricted to admin ranges
  (D-0014). Alternatives: WireGuard-only access, a non-root deploy user with sudo.
- D-0045 (CI on GitHub-hosted runners for the public repository): branch `ci/hosted-public` merged
  on 2026-11-15 (it carried PR 37, closed with it). Still the owner's: do **not** register the D-0039 runners for this
  repository; set the organization's Actions budget to $0 with "stop usage"; in the repository's
  Actions settings require approval for workflows from all external contributors (it was
  "first-time contributors" on 2026-11-14).
- D-0046 (public repository): enable secret scanning with push protection and private
  vulnerability reporting (the security-advisory link needs it; both were off on 2026-11-14),
  protect `main` (required checks and a review; a collaborator with write access exists), and
  choose a licence (none yet: the code is visible, all rights reserved).
- GitHub environments `staging` and `production` with required reviewers, and their secrets
  (`SOPS_AGE_KEY`, `DEPLOY_SSH_KEY`), must be created by the owner before `deploy.yml` can run.
- Prompt 2, milestone 2 (report `docs/reports/P02-M2.md`):
  - D-0029: the mailbox store is Apache Iggy (owner's decision). Still to confirm: Iggy is
    pre-1.0 (storage format changed between 0.9 and 0.10); its container needs io_uring allowed
    in seccomp. (The 14 hours acknowledged ciphertext stayed on disk is answered by D-0031:
    unopenable within about a second of the ACK, deleted within minutes.)
  - D-0030: official Iggy SDK (owner's decision) brings 153 crates (exempted, audit backlog) and
    `ring` as an unused transitive dependency. Proposed wording for the constitution's locked
    stack: "All TLS and cryptographic operations we perform use rustls with aws-lc-rs. Another
    crypto library may appear only as a transitive dependency of an owner-approved crate, unused
    by our TLS or crypto paths, named in DECISIONS." Not written into the constitution until the
    owner approves it.
  - D-0025: own minimal SurrealDB client instead of the official SDK (264 crates); SurrealDB no
    longer holds the mailbox, but keeps accounts and devices from Prompt 3.
  - D-0028: benchmark compio (owner's suggestion) against per-core Tokio in M3; changing the
    runtime touches the locked stack. Decided: per-core Tokio stays (owner, after D-0032's
    measurement: compio +2–4 %); io_uring is measured again in M7.
- Prompt 2, milestone 3 (report `docs/reports/P02-M3.md`): gate approved by the owner on 2026-11-12.
- Prompt 2, milestone 4 (report `docs/reports/P02-M4.md`): gate approved by the owner on 2026-11-12. D-0034
  (Valkey routing, owned partitions over node links, replicated key stores) decided by the agent
  under the owner's delegation; the owner may revisit it. New dependency `redis` (with `arcstr`,
  `async-lock`, `xxhash-rust` under BSL-1.0), exempted pending audits.
- Prompt 2, milestone 5 (report `docs/reports/P02-M5.md`): gate approved by the owner on 2026-11-13.
  The two further threat-model rows (the sealed job queue and the hashed request nonces) were
  approved by the owner on 2026-11-15 with the Prompt 2 final gate; D-0036 decided by the agent under the owner's standing
  instruction. Provider credentials (2026-11-13): an APNs sandbox key and a least-privilege FCM
  service account exist outside the repository and were verified against Apple and Google;
  waking a real device waits for the apps; production keys go to SOPS at deployment.
- D-0037 / D-0044 (report `docs/reports/P02-send.md`): **approved by the owner on 2026-11-14**
  ("Approve both"): D-0044's routing (`api.` admitted to node links for the store of a send
  only) and its two threat-model rows. Prompt 2's final gate (report `docs/reports/P02-final.md`):
  **approved by the owner on 2026-11-15** ("Approve all four"). Prompt 2 is done; its go-live checks
  (24-hour soak, durable latency on the `ccx33`, the 2-hour RTO drill) run after every phase is finished.
- Prompt 3, milestone 1 (report `docs/reports/P03-M1.md`, D-0048): gate **approved by the owner
  on 2026-11-15**, with its four threat-model §1 rows (account identity key and invite count;
  invite code hashes; keyed IP-range throttle records; used challenge nonces).
- Prompt 3, milestone 2 (report `docs/reports/P03-M2.md`, D-0049): gate **approved by the owner
  on 2026-11-15** ("Approve both"), with its four threat-model §1 rows (username hashes;
  username key verifiers; username links; keyed throttle records).
- Prompt 3, milestone 3 (report `docs/reports/P03-M3.md`, D-0050): gate **approved by the owner
  on 2026-11-15** ("Approve both"), with its two threat-model §1 rows (device public keys and
  prekeys; keyed throttle records).
- Prompt 3, milestone 3b (report `docs/reports/P03-M3b.md`, D-0051): gate **approved by the owner
  on 2026-11-15** ("Approve both"), with its threat-model §1 row (KeyPackages). M4 split by the
  owner (2026-11-15): M4 (devices: QR linking, the signed device list, limits, removal) then M4b
  (two-step PIN, passkeys with Songbird's own minimal WebAuthn verifier, multiple accounts on one
  device).
- Prompt 3, milestone 4 (report `docs/reports/P03-M4.md`, D-0052): gate **approved by the owner
  on 2026-11-15** ("Approve both"), with its four threat-model §1 rows (the signed device list
  and device flags; link token hashes; provisioning channels; keyed throttle records).
- The rest of Prompt 3 in one pass (owner, 2026-11-15: "lets do the rest of milestones in one"):
  M4b (D-0053), M5 (D-0054), M6 (D-0055) and M7 (D-0056, the final gate `P03-gate.md`) on one
  branch and one PR (#54). **Approved by the owner on 2026-11-16** ("i approve everything"): the
  Prompt 3 gate, M4b–M7 and the six threat-model §1 rows. The owner also decided to **adopt
  `akd` for key transparency now** (D-0057, built in PR #55, **approved by the owner on 2026-11-16**).
  Prompt 3 is done.
- D-0035 (RustFS replaces Garage, owner's decision): the `rustfs/rustfs:1.0.5` image is pinned by
  tag only, because Docker Hub was unreachable from the agent's sandbox. Pin its digest in
  `deploy/compose/compose.yaml` and `deploy/ansible/group_vars/all.yml` before the next deployment
  (`docker buildx imagetools inspect rustfs/rustfs:1.0.5`).

## Decisions

## D-0173 — Screen protection and the app switcher cover

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision: "Block screenshots" keeps Songbird out of screenshots and recordings where the
  system allows: Android `FLAG_SECURE`, macOS `NSWindowSharingNone`, Windows
  `WDA_EXCLUDEFROMCAPTURE`, applied again every second to windows opened since. iOS lets no
  app block screenshots: there the setting says so, and a cover hides the app while the screen
  is recorded, mirrored or shared (`UIScreen.isCaptured`). Linux has no such API, so the
  setting isn't shown there. "Hide content in app switcher" (iOS and Android only, where the
  switcher shows a snapshot) covers the app whenever it isn't in front, and on Android 13+
  also blanks the recents thumbnail without blocking screenshots. The cover has no motion: it
  must be whole in the frame the system snapshots.

## D-0172 — The device's own services: device secrets in the secure store; biometric unlock

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision: a new crate, `apps/device` (`songbird-device`), holds every call into the
  platforms' own services, like `songbird-camera` (D-0110): objc2 and Security.framework on
  Apple, JNI to `chat.songbird.device.DeviceBridge` in the Gradle host on Android, the `windows`
  crate on Windows, D-Bus on Linux. `unsafe` stays in its platform modules.
  - **Device secrets at rest** (the identity and device seeds, the message store's key, the
    recovery key, the profile key, the records' replica, the passcode's hash, the subscriber
    secret, Family and seat tokens, support tickets, supervision links, the backup
    destination's sign-in): encrypted in app storage under a **vault key** (AES-256-GCM, the
    key name as associated data), which the system keeps: the Keychain (iOS: this device only,
    after first unlock, never in backups; macOS: the login keychain, as a development build
    can't use the data-protection keychain), the Android Keystore (a non-exportable AES key,
    StrongBox where present, wraps it), DPAPI (wraps it), the Secret Service. Values written
    before are encrypted on first read. A Linux without a keyring keeps them as before, and
    the app can tell (`vault::protected`).
  - **Biometric unlock**: "Unlock with Face ID / Touch ID / fingerprint / Windows Hello" in App
    lock, turned on only after a match. The lock screen prompts at once and on a button; a
    miss counts nothing against the passcode's tries, and the passcode always works. Linux has
    no system biometrics, so the option doesn't show there.
- Consequences: the D-0165 note (secrets in app storage until the keychain) is closed.

## D-0171 — Audits on the device; the new device gets the log key; dead leaves go

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision:
  - **Audits.** The KT publisher records every epoch's root as it publishes (and the current
    one first on each pass, so a crash leaves no gap); `POST /v1/kt/roots` serves them. When a
    device's head moves forward it checks akd's append-only proof over those roots, whose two
    ends must be the signed heads' roots; a failure is a fork, noticed and refused.
  - **Pinned log keys.** Release builds carry the log and VRF keys
    (`SONGBIRD_KT_LOG_KEY`, `SONGBIRD_KT_VRF_KEY` at build time); a release without them checks
    nothing rather than trust the server. Development builds pin the first keys they read.
  - **The new device's log key and title.** When a member account's new device joins, that
    account's other devices send a `DEVICE_JOINED` group update (a new kind, 13) carrying the
    change log's key and the title; no line shows.
  - **Dead leaves.** After following its account's new device, a device removes every leaf
    whose device no member account's signed list names (read fresh), never its own. Only lists
    it verifies count, so the server can't make it remove a device someone still lists.
- Consequences: the D-0167 and D-0168 gaps are closed. Calls through the proxy, keychain storage
  of device secrets, and client-side parental rules on modified phones stay as they were.

## D-0170 — Crash reports: opt-in, scrubbed, sealed, unlinked

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision: a panic hook writes one report on the device: version, platform, the UTC day, the
  panic's message and place, and Songbird's own function names on the stack, each scrubbed
  (quoted text, IDs, long numbers, tokens, usernames, emails, URLs and paths replaced; only a
  `.rs` file name and line kept). At the next start it goes only if "Send crash reports" is on,
  as an **unsigned** support ticket of kind `crash` sealed to the support key (the server
  keeps ciphertext, no account, no device; signed crash tickets are refused); either way the
  file is deleted. The support channel's throttle and 90-day deletion apply.
- Consequences: staff read crash reports with the support key, like tickets.

## D-0169 — A cancelled renewal can be resumed

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision: `POST /v1/billing/mollie/resume` turns renewal back on while the paid period lasts.
  Mollie can't reactivate a cancelled subscription, so billing starts a new one on the
  customer's mandate, its first charge the day the paid period would have renewed (never
  before tomorrow): nothing is charged twice. One resume per paid period at a time; cancelling
  again allows another. Refused when revoked, expired (buy again), already renewing, or when
  Mollie refuses the mandate (pay again by checkout). If recording fails, the new subscription
  is cancelled at once. StoreKit and Play resume in their own pages (native hosts, later).

## D-0168 — Key transparency checked on the device; security notices shown

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision: app-core checks the log (PROTOCOL §4.9) with akd's own verifier: the log's keys
  are pinned on first read; every signed head is checked and must move forward (an older
  epoch, or the same epoch with another root, is a fork); whenever a device accepts a newer
  signed device list (a contact's or its own) it verifies a lookup proof that the log publishes
  exactly that list. A list the log hasn't published yet is checked again with the next fetch;
  the same list contradicted twice is noticed. New devices on an account the device already
  knew are noticed too. The app shows: a line in the contact's chat for their new devices or a
  list the log doesn't vouch for, a warning for the user's own new devices, and a lasting
  problem banner for a fork. The A2 gate now runs with the real log.
- Consequences: audit proofs need every epoch's root, which no endpoint serves one by one, so
  devices rely on forward-only heads and leave audits to auditors (`POST /v1/kt/audit`). The
  log's keys are trusted on first use until release builds ship them pinned (A12).

## D-0167 — A new device joins its account's groups by external commit

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision: every device keeps an encrypted record `g/<group ID hex>` per group the account is
  in (§10.2); a device that finds one for a group it isn't in joins by external commit through
  `POST /v1/groups/commit` (§6.3 rule 5: a listed device of an account in the group, removing
  nothing). Members follow such a join only when the joiner is on the signed device list of an
  account that already has a device in the group, checked from lists they verify themselves, so
  the server can't slip a device in under someone's account. No line shows in the chat (it's
  the same person). A restore joins the restored groups the same way. Leaving or being removed
  deletes the record.
- Consequences: the joining device doesn't get the change log's key until the next key handoff
  (D-0156); the dead leaf of a reinstalled device stays until a member removes it (§6.3).

## D-0166 — The parent's app-side rules are enforced on the child's app

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision: the child's app keeps the signed policy (checked against the key scanned at linking
  at every read; a newer version only; fetched at start and every 30 minutes) and enforces the
  sealed client rules (PROTOCOL §10.6):
  - **Calls only from contacts:** an offer from someone who hasn't shared their profile with
    the child is answered busy.
  - **Nudges off:** none shows, none can be sent.
  - **Quiet hours:** no notifications inside the window, in the device's local time.
  - **Each new contact needs approval, in person:** starting a chat with someone new shows a
    code (`SONGBIRD:APPROVE:<account>:<username>`); the parent's phone scans it and shows eight
    digits, `HMAC(HKDF(link secret, "contact-approval"), child ‖ contact)`, which the child
    types. Nothing goes through the server, so it never learns who parents whom or who is
    approved. Five wrong codes make the next try wait a minute, doubling. Approvals stay on the
    child's device until supervision ends.
  A policy that ends supervision (or its date passing) lifts every rule and forgets approvals.
- Consequences: these rules are the child's app's to enforce, as §10.6 says: a child who
  modifies their phone can get around them. The server-enforced flags (no requests, group
  approval, hidden) hold regardless.

## D-0165 — Backups hold app-core's chats; schedule, media and a second copy are real

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision:
  - **What a backup holds** for a real account: every chat and message app-core keeps, as its
    `Content` protobuf with real IDs (PROTOCOL §11.5), read on the backup's own thread from a
    second connection to the SQLCipher store (WAL lets it read while the app writes). Before
    this, backups held only what the screen showed, as text, and restored into the screen alone
    (gone at the next start). A restore writes chats, messages and files back into app-core,
    holding app-core's lock so the store isn't opened twice, then shows them.
  - **Never in a backup:** disappearing messages (their timer is a promise to the whole chat),
    view-once messages, deleted ones, sessions, keys.
  - **Include media** puts the files this device has into the backup (deduplicated by the
    format); off, messages keep their pointers only.
  - **Frequency** runs backups: checked a minute after start and every ten minutes; a backup is
    due a day, a week or 30 days after the last.
  - **A second destination** gets the same content after the main one; its failure is reported
    without undoing the main backup. Rollback state (§11.4) is kept per destination, since
    each place has its own generations.
  - **The settings are kept on this device** (they weren't: a restart turned backups off): on,
    destination, media, frequency, second copy, last time and size (both real now; the size was
    a made-up "1.2 GB"). The destination's sign-in sits with the device's other secrets and
    moves to the keychain with them.
- Consequences: a backup made before this restores as before (its IDs are the app's own); a
  restored group shows its history but needs a re-add or external commit to send again (§6).

 to Songbird's hosts goes through the proxy too

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision: with a proxy set, the app's HTTPS requests to Songbird's own hosts (api, media and
  the vault on it, billing) go through it like the connection (`songbird_client::https::Tunnel`:
  TCP and TLS to the proxy, the endpoint's TLS 1.3 inside, HTTP/1.1, redirects only to HTTPS
  and only through the proxy). Fail closed: an unreachable proxy, or one that doesn't carry a
  host, is an error, never a direct connection. Destinations the user chose for backups are
  reached directly, since a proxy is not an open relay. The proxy example adds billing's host.
- Consequences: before this, only the delivery connection used the proxy, and every HTTP
  request showed the user's IP address to Songbird. Calls still connect directly (WebRTC needs
  UDP, which the proxy doesn't carry); the proxy page says only that it hides the IP address
  from Songbird's servers.

## D-0163 — Chat-list state follows the account across devices

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision: each real chat's archived, pinned, muted-until, marked-unread and favourite state
  and the custom lists it is in are one encrypted record `c/<account or group ID, hex>/state`
  (PROTOCOL §10.2), last writer wins. Changes are written when the chat list changes; a sync
  from another device applies them. A chat this device has not written yet takes the synced
  state instead of overwriting it with its own defaults. Locked chats stay device-only: a lock
  protects whoever holds this device, and the passcode never leaves it.
- Consequences: the records server sees one more opaque record per chat that has state; it
  learns nothing of which chat (the logical key is inside the encrypted record, D-0102).

## D-0162 — Locked chats open with the app passcode; real storage and network usage

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision:
  - **Locked chats** open with the app passcode (the six digits kept only on this device as a
    slow hash, D-0101), with the lock screen's growing wait after wrong tries, served on the
    Locked chats page too. Locking a chat needs a passcode to exist; without one the user is
    sent to Privacy → App lock. Before this, "Unlock" opened them with a press. Face ID and
    fingerprints join with the native host projects.
  - **Manage storage** lists what app-core keeps on this device (every decrypted file in the
    SQLCipher store, by chat, with its real size, type and name) and deletes files from it (the
    messages stay). A real account no longer sees the made-up files.
  - **Network usage** counts what crosses the network on this device, by lane (messages,
    media, calls), kept on the device until reset: every HTTP request and answer (by its path:
    attachments, packs, GIFs, map tiles and storage URLs are media), every delivery-connection
    frame sent and received, and each call's bytes as WebRTC counted them on its transports
    (`Call::traffic`), added when the call ends. Header sizes are estimated (a flat 200 bytes an
    HTTP request and answer); bodies are exact.
- Evidence: `apps/ui` 214 tests (the lanes of URLs, the date); the app builds with every
  counter wired. Not exercised against a live session.

## D-0161 — Voice notes: a recorder on every platform, WAV, played receipts

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Context: D-0158 found that libwebrtc's audio device module does not record outside a call.
- Decision:
  - **`apps/recorder`** (its own workspace, no `unsafe`): the default microphone through
    `cpal` 0.16 (Apache-2.0: CoreAudio on macOS and iOS, WASAPI on Windows, ALSA on Linux,
    AAudio on Android), one API instead of a native module per platform. Whatever the device
    gives is mixed to mono and averaged down (or repeated up) to 16 kHz, kept in memory as
    16-bit PCM, at most 10 minutes; the level of the last 100 ms; at the end a waveform of at
    most 256 bytes and a WAV file. Nothing touches the disk before encryption.
  - **Why WAV, not Opus**: no codec library to vendor and review, played by every platform's
    own player; 32 KB a second (a minute is about 1.9 MB, inside the attachment limits). An
    Opus encoder can come later without changing the protocol (the pointer's content type says
    what it is).
  - **Sending**: the composer's record button records in real chats (the system asks for the
    microphone the first time, with the purpose string added to `nsh.json` in D-0152); release or
    Send sends a `FLAG_VOICE_NOTE` attachment with its duration and waveform, to a 1:1 chat or a
    group; under half a second is not sent; deleting or sliding away discards it.
  - **Receiving and playing**: a voice note is a voice note in the chat; it plays in the
    platform's audio player (from a decrypted copy in the app's `media-playing` folder, D-0158),
    its bar moved by the player's own progress events, at the chosen speed, then the next unplayed
    note; the bubble draws the waveform the sender measured. Playing a received note keeps it
    played and sends the author a played receipt (`Messenger::mark_played`).
- Evidence: the recorder's unit tests (48 kHz stereo to 16 kHz mono at the right level, slower
  devices brought up, the WAV header, the waveform's bounds, the 10-minute stop); a live capture
  on this Mac (two seconds recorded, a WAV and a 256-byte waveform); `gate_a2::a_voice_note_
  arrives_with_its_waveform_and_is_marked_played` against the real stack (flag, duration and
  waveform arrive; the file opens; the played receipt reaches the sender); the A2 gate (19) and
  `apps/ui` (213) pass. `cargo deny` passes licences, bans and sources for the recorder.
- Not exercised: recording inside the packaged app on iOS, Android, Windows and Linux (cpal
  builds there; Linux needs `libasound`'s headers to build; iOS needs the record audio session
  category, which cpal sets for an input stream).

## D-0160 — Broadcast lists, presence and nudges on the real backend

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision:
  - **Broadcast lists** send: a list's message goes to each person separately, as a 1:1
    message in their chat with the user, and only to people who shared their profile key (the
    lists' own promise: "only people who saved you as a contact get it"); the others are counted
    and named in the toast. Lists can hold real contacts (the picker offered only demo ones), and
    New chat → New broadcast list now makes the list it announced. Sending each person a direct
    message (rather than one send in the broadcast lane) keeps each copy in its own chat; the
    broadcast lane's single send is not used yet.
  - **Presence** (§10.5): the presence rules already in the app (`Presencer`) now reach real
    contacts: app-core lists the contacts who shared their profile key with their devices and
    access keys (`presence_peers`), seals each payload for each device in its Signal session
    (`seal_for`), and the app sends the sealed batch as before. Presence received updates the
    contact, and a contact who asks gets one answer. The contacts are read when app-core opens
    and every five minutes.
  - **Nudges** go sealed per device in the attention class (`Messenger::nudge`, new
    `Net::attention`), and are taken only from it (`receive_attention`): the same payload as an
    ordinary ephemeral event is not a nudge. A nudge shows unless nudges are off globally or for
    that person, or they are blocked. A real nudge is a line in the chat, never a stored message;
    the demo's "nudge back" stays in demo chats only.
- Evidence: `gate_a2::presence_and_nudges_reach_contacts_sealed` (presence reaches exactly the
  contact who shared a profile key; a sealed presence arrives as it was sent; a nudge arrives
  only through the attention class) and the whole A2 gate (18); `apps/ui` 213 tests.
- Not exercised: `PUT /v1/ephemeral/sealed` from the app (P09's gate covers the endpoint).

## D-0159 — Sticker packs on the device; stickers in real chats

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision:
  - **Installing** from `https://songbird.chat/pack#<id>.<key>` (§7.4): app-core fetches the
    manifest and every asset with no account, opens each with keys derived from the pack key,
    checks each asset against the manifest's commitment, and keeps the pack (key, manifest,
    assets) in the SQLCipher store (schema 7). Packs are listed, reordered and removed there; a
    real account starts with none (the sample packs are the demo's).
  - **Sending** an installed pack's sticker carries the pack ID, key and emoji, and the asset as
    an encrypted attachment (`StickerRef.data`), so the receiver needs nothing installed; the
    receiver shows the artwork.
  - **The built-in "Songbird friends" pack** is emoji: its stickers carry a fixed public pack ID
    (`songbird-friends`), an all-zero key that hides nothing, and no data; each device draws
    the emoji. Before this, built-in stickers could not be sent to real chats.
  - The UI's pack link format was not the protocol's (`/pack/<id>#<key>`); it is now.
  - Received videos and view-once messages are registered as media (D-0158 had missed it, so
    they could not be opened).
- Evidence: `gate_a2::sticker_packs_install_from_a_link_and_stickers_travel` (a pack made on
  the server as P06 makes it, installed from its link with every asset checked; a sticker
  reaches someone with nothing installed, with its image; a built-in sticker; removal);
  `apps/ui` 214 tests.
- Consequences: making a pack on the device (uploading one's own artwork) is not offered yet.

## D-0158 — View-once photos and videos; real video playback

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision:
  - **View once** (`Message.view_once`, §5): a "View once" attach option sends one photo; the
    sender keeps no copy to view again. A receiving device reads the file, deletes it from its
    store at once (and refuses to fetch it again), and shows it in a viewer that keeps no copy;
    the message then reads "Opened" on that device. A view-once video plays from a copy in the
    app's own data folder that goes when the viewer closes.
  - **Videos** received in real chats are videos (not files); the media viewer plays them in
    the platform's player with its own controls, from a decrypted copy in the app's data folder
    (`media-playing`, emptied whenever app-core opens; the player needs a file). Real photos in
    the viewer are the photos themselves (the viewer showed the demo illustration for every
    message before).
- Not done: voice notes. libwebrtc's audio device module does not record without a call's send
  stream (tried: a local track from the platform's audio delivers no samples, and recording
  fails to start for a process without microphone access); recording needs each platform's own
  recorder, a native module like the camera's.
- Evidence: `gate_a2::a_view_once_photo_is_shown_once_then_gone` (the sender keeps nothing; the
  receiver reads it once; after that it is gone and cannot be fetched again); the A2 gate (16)
  and `apps/ui` (214) pass.
- Consequences: a decrypted video exists on disk, inside the app's container, while it plays.

## D-0157 — Retries: the outbox, group operations, offline sends

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Decision:
  - Messages a budget (429) or a busy server held back already waited in the outbox, but went
    out again only on a reconnect. The app now flushes the outbox every 15 seconds while it has
    entries, and updates each message's ticks.
  - A send that fails for want of a network keeps its message (status Sending) and its outbox
    entry instead of showing it failed; the flush sends it.
  - Group operations (add, remove, leave, answer a request) are tried up to four times, 2, 4
    and 8 seconds apart, when the failure may pass (409: another commit came first; 429; 503;
    the network). The core is unlocked in between, so the commit that came first is received
    and the next try starts from the new epoch.
- Evidence: the A2 gate (15) passes with the changes; not exercised against an induced 409 or
  429 (the test stack's budgets are raised for the gate).

## D-0156 — The group change log on the device

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent, within D-0070 ("ciphertext under a key the members share")
- Decision: a per-group random **log key**, made by the creator, kept in the encrypted store and
  handed to members in a group message (`GroupUpdate.log_key`, a new field 5): the "created"
  update, an update after each add, and one from any member that processes a join by link.
  Each commit a device makes (adds, removals, committed leaves, link-key settings) carries a
  version-2 bundle entry sealed under the key, bound to the group and the commit's epoch, and
  signed by the committer's leaf as §6.7 already required (the server checks the signature, and
  cannot read or forge an entry). "Member changes" reads the log, opens what it can and names
  people from the store.
- Rejected: the epoch's exporter as the key (no device that joins later could read anything
  before it joined, which is the log's purpose).
- Also fixed: `songbird-content`'s guard test refused the client-only app-core crate it was
  written before; client-only crates are now named, and nothing else may depend on them.
- Evidence: `gate_a2::the_change_log_is_written_encrypted_and_read_by_members` (the creator's
  key reaches the member; the add is in the log with actor and subject; a removal is logged
  after it); the whole A2 gate (15) and P05 (14) pass.
- Consequences: a change naming more people than 2 KiB of ciphertext holds is left unlogged.

## D-0155 — Devices enforce group roles: verified member accounts

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent
- Context: roles (§6.6) were enforced only by the server; a device took any commit the server
  fanned out, because the MLS tree names devices, not accounts (D-0147 recorded the gap).
- Decision:
  1. `POST /v1/groups/state` ends with each member device's account, for members only.
  2. The device keeps a per-group device→account map (in the encrypted store), refreshed from
     that answer, and uses a pair only after finding the device in the account's signed device
     list (a device no longer listed is dead and has no account). The map is warmed on joining
     and after every merged commit, so a later removal can still be checked.
  3. Before merging a member's commit, the device computes the change in accounts (before,
     after, gained, lost, left, server removals) exactly as `ds` does and runs
     `roles::authorize`. A refused commit is not merged; the app says the device stopped
     following the group. A commit removing this device is accepted (the server merged it; the
     device cannot follow the group any further either way).
  4. Group messages from an account the roles do not let send are dropped; pins from an
     account that may not pin are ignored.
- Rejected: carrying accounts in leaf credentials (changes the credential every server and
  harness checks) or in each commit's change-log entry (unreadable to a device that joins
  later, and the log's key is not settled).
- Evidence: every A2 group test (creation, adds, removals, leaving, links, requests,
  communities, channels, calls, polls) passes with the check active (14); a joiner's verified
  map names the creator's device and account; P05 (14) and P07 (7) pass with the extended state
  answer.
- Consequences: a server can make a device refuse a valid commit only by leaving its committer
  out of the answer, which every member would notice; it cannot make one accept a commit the
  roles forbid. The change log ("View member changes") is still open.

## D-0154 — Disappearing messages, polls, pins, safety numbers, mutes and GIFs in real chats

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent, under the owner's no-mocks rule
- Decision:
  1. **One place for events** (`Messenger::apply_event`), used by 1:1 and group receive alike:
     reactions, edits, deletes, poll votes and closes, pins and timer changes. Before this, a
     group's reactions, edits and deletes were stored as empty messages. Sending an event to a
     group goes through MLS (`send_event` picks the group path for a group ID).
  2. **Disappearing messages** (§5.9): each chat's timer is kept (schema 6, `chat_timers`);
     every message and event carries it; a message goes `expire_timer_s` after it is stored on
     each device (with its files, reactions, votes, pins and franking proof). The app sweeps
     every 15 seconds. The UI's "90 days" choice is now "4 weeks": the protocol's longest timer
     (`MAX_TIMER_S`); a synced setting of "quarter" reads as 4 weeks.
  3. **Polls** (§5.8): a vote counts only for an open poll, for options it has, one unless it
     allows several; the highest counter wins; only the author closes it, and then only each
     voter's vote at the listed counter counts. The UI holds several choices (`mine` is a list)
     and offers "Close poll" to the author, "Final results" after.
  4. **Pins**: for everyone in the chat, 7 days (the protocol offers 24 hours, 7 days or 30
     days), only for a message of that chat.
  5. **Safety numbers from real keys** (D-0136's construction over both accounts' identity
     keys); "verified" kept in the store; a changed identity key, which used to block sending
     for good, can now be accepted from the safety-number screen (unverified until compared
     again).
  6. **Mute for 8 hours, 1 week or always**, lifted on time (checked every minute); backups
     keep when a mute ends.
  7. **GIFs** through the privacy proxy (`/v1/gifs/*`), in a GIFs tab of the sticker sheet in
     real chats; a GIF picked is fetched through the proxy and sent as an encrypted photo.
  8. **Mark all as read** marks every real chat read in app-core (and sends read receipts when
     they are on).
- Evidence: `gate_a2::timers_pins_polls_and_group_events` (a poll voted, a bad vote refused,
  closed and a later vote refused; a pin; a 5-second timer learnt by the other side and the
  next message gone from both devices; a group reaction and a multiple-choice vote arriving as
  events) and `gate_a2::safety_numbers_match_and_verification_is_kept` against the real stack
  (14 gate tests pass); `apps/ui` 214 tests.
- Consequences: built-in emoji stickers still cannot be sent to real chats (sticker packs from
  links are the next item); a group's pins follow no admin rule yet (roles on received commits
  and events wait for the device-to-account map).

## D-0153 — Call links on the device

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent, within the owner's design (D-0085 §2: Signal's link calls)
- Decision (`songbird_app_core::call_links`, `apps/ui/src/backend/calls.rs`):
  1. **Making a link**: a random root (the link, `https://songbird.chat/call#…`, new
     `songbird_proto::call_link`) and admin token; the server gets the link ID and admin token
     to hash (§8.7); the device keeps both in the SQLCipher store (schema 5, `call_links`). The
     Calls tab lists them; each can be copied, joined (as admin, skipping the lobby) or revoked
     (after a confirmation; the server ends its call for everyone).
  2. **Joining**: a link pasted into New call or New chat, or tapped in the list. With a lobby,
     the device's card waits (identity `lobby`, the device has none yet) and its place is asked
     again every 3 s until admitted (then the ticket) or turned away ("You weren't let in").
     Admins in the call see who waits (cards opened with the link key, unverified names unless a
     contact) and admit or turn them away.
  3. **Keys on the data channel, sealed in Signal sessions.** The message lanes refuse a
     stranger whose account is closed (reason 10), and a link call is mostly strangers, so the
     `sender_key` envelope (built by `link_key_envelope`, opened by the receiver's ordinary
     `receive`) travels on the room's data channel to that participant only. A key is used only
     if it came, over the session, from the account and device the sender's card names, for the
     card's identity; one that arrives before the card waits for it. Each participant draws a
     new key at the next index on every join and leave, installs it, sends it to everyone whose
     card arrived, and sends with it a second later.
  4. **The data channel is not LiveKit-encrypted in a link call** (`Call::join_with(…, false)`):
     LiveKit encrypts data packets under the sender's key, which a newcomer does not hold before
     the exchange that needs the channel. Everything sent there is sealed by the app instead
     (cards and other data under the link key, keys in Signal sessions); media stays encrypted
     under the sender keys. No hello is sent (it would name the device to the server in the
     clear; the sealed card does that job).
  5. **Names**: a contact's name once their key came from the device their card names; anyone
     else's card name marked "(unverified)", as §8.7 requires (anyone with the link can write
     any name). Link calls have no shared verification code; the sheet says why and points to
     safety numbers.
- Evidence:
  - `gate_a2::call_links_lobby_keys_and_revoke` against the real stack: a link with a lobby, a
    stranger waits, the admin sees his card's name and admits him, both get tickets to the same
    room with different identities; cards open only with the link key; a sender key reaches the
    other device in a Signal session naming the sender's device, though they are strangers; a
    turned-away joiner gets 404; a revoked link cannot be joined and the other link stays.
  - `apps/calls-media::a_link_calls_keys_arrive_by_data_and_unlock_media`: on the compose
    LiveKit, with the data channel not LiveKit-encrypted, a participant without the sender's key
    hears nothing, receives the key on the data channel, installs it and hears the sender.
  - App-core unit tests (cards, sealed link data, join answers); `apps/ui` 213 tests; the app
    starts.
  - Not yet exercised: two app instances in one link call through the UI.
- Consequences: the server learns, from the room's data channel, that participants exchange
  sealed blobs (as it would see message traffic); it reads none of them.

## D-0152 — Calls, part 3: the app's call screens on real calls

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent, under the owner's no-mocks rule
- Decision (`apps/ui/src/backend/calls.rs`, `features/calls`):
  1. **A call in a chat app-core owns is real.** Its phases come from signalling and the room,
     not timers: Calling until the ticket is in, Ringing (1:1) until the answer, Connecting until
     the other person is in the room, Connected; a group call is open as soon as the room is.
     Incoming offers ring for their own deadline, only from chats app-core owns (never a
     non-contact); an offer during a call answers "busy" (new `Messenger::busy_call`). Another
     of the account's devices answering stops the ring here. Whoever ends a 1:1 call tells the
     other side (hang-up, or decline while ringing); a ring that runs out ends as "No answer" or
     "Missed". New end lines: declined, on another call, no answer, couldn't connect / lost.
  2. **Keys without the core lock.** `Messenger::group_call_secret` gives the epoch secret and
     epoch; the media layer derives every sender key from it as newcomers arrive. Every commit
     in the call's group (received, or this device's own) installs the new epoch at once and
     sends with it a second later (§8.2).
  3. **Names are signed** (PROTOCOL §8.2 "Hellos"): a participant identity is unlinkable, so a
     group call's tiles would otherwise name nobody; any member could claim to be another if
     the hello were not signed by the device key and checked against the signed device list. A
     1:1 call's other participant can only be the person called (the only other key holder),
     and a hello naming anyone else is ignored.
  4. **Verification code from the key** (PROTOCOL §8.2): `songbird_content::call`'s
     `verification_code`, the same on both sides only with the same epoch secret.
  5. **Media in the UI:** the microphone through the platform's audio device module (macOS asks
     on first use; `NSMicrophoneUsageDescription` and the audio-input entitlement added); the
     self-view's camera session feeds the call's encoder (NV12 or BGRA); remote video shows as
     RGBA frames (`ImageSrc::rgba`, no encoding) at up to 15 a second, turned upright; a tile
     whose frames do not decrypt shows "Can't decrypt" and nothing of theirs; a ring around who
     speaks; the mic meter follows the level the room measures. Screen sharing uses macOS's
     system picker (elsewhere the first screen or window listed). The devices sheet lists the
     computer's microphones, cameras and speakers and switches to the one picked. Raised hands
     travel on the encrypted data channel.
  6. **Demo-only until real:** the demo's timed calls stay for demo chats; the Calls tab's "make
     it ring" rows and call links show only in demo builds until call links are real on the
     device (a link nobody can join must never be offered).
  7. **Build:** `apps/ui` links libwebrtc (`-ObjC` on Apple targets in `apps/ui/.cargo/config.toml`)
     and `apps/calls-media` refuses to build without `LK_CUSTOM_WEBRTC` (so `webrtc-sys` never
     downloads one unchecked).
- Evidence:
  - `gate_a2::calls_ring_answer_and_agree_on_keys` against the real stack, extended: equal
    verification codes; a hello verifies to its account, and fails for another identity or with
    a forged account; `group_call_secret` gives the same keys and code on both members; a busy
    reply reaches the caller.
  - `apps/calls-media`: the data channel carries a payload end to end; NV12 frames arrive as
    the right picture (three LiveKit tests pass).
  - `songbird-content`: codes agree only on the same secret; `apps/ui`: 213 tests, including
    frame rotation and participant IDs; the app starts with libwebrtc linked.
  - Not yet exercised: two app instances calling each other through the UI (the pieces each
    have their test; the end-to-end run needs two registered devices on screen).
- Consequences: call links on the device are next; the phone speaker route and the mobile
  builds of call media wait for the native hosts (P08).

## D-0151 — Calls, part 2: call media on LiveKit's Rust SDK, end-to-end encrypted

- Date: 2026-11-20
- Status: proposed (waiting for the owner); the SDK is the owner's choice (2026-11-20: "LiveKit
  Rust SDK")
- Decided by: owner (SDK); agent (the rest)
- Context: part 1 (D-0150) gives each device a ticket and per-sender keys; something must carry
  audio and video through the SFU with every frame encrypted under those keys, on macOS, iOS,
  Android, Linux and Windows.
- Decision (`apps/calls-media`, its own Cargo workspace so libwebrtc never enters the server's
  build):
  1. **`livekit` =0.12.0** over LiveKit's prebuilt **libwebrtc**, release `webrtc-a545de4`.
     The SDK's build script would download it unverified, so `scripts/fetch-libwebrtc.sh`
     fetches it, checks it against the SHA-256 pinned in `supply-chain/libwebrtc.sha256` (six
     platforms, as GitHub recorded them at upload; macOS also checked by download) and hands it
     to the build as `LK_CUSTOM_WEBRTC`. Apple targets link with `-ObjC`
     (`apps/calls-media/.cargo/config.toml`; every binary linking the crate needs it), or
     libwebrtc's Objective-C codec factories are dropped and the process aborts at start.
  2. **Keys.** A `KeyProvider` per call, never shared-key mode: each identity's key comes from a
     `KeySource` (app-core's `one_to_one_key` or `Messenger::group_call_key`) and is installed
     for this device before connecting, for everyone already in the room on joining, and for
     each newcomer when it connects, before its tracks are subscribed. `install_epoch` puts a new
     epoch's keys in the ring for everyone at once; `send_with` moves this device's senders to
     the new index (the app calls it a second later, §8.2). A (re)published track is put on the
     current index at once (LiveKit starts every sender at 0).
  3. **LiveKit's ratchet is off** (`ratchet_window_size = 0`): keys change only with epochs; a
     ratcheted key is one no epoch secret produced. Frame keys are derived from sender keys with
     LiveKit's default PBKDF2 and salt, so the web app interoperates.
  4. **"Can't decrypt" is reported** (§8.3): with LiveKit's default tolerance (-1) a failing
     frame is dropped silently; with 25 failed frames it reports the participant and marks the
     key invalid. The room then installs the participant's current key again, so a burst of bad
     frames never leaves a good key unused while a wrong key stays reported. Each change of
     state is reported once.
  5. **TLS on aws-lc-rs.** LiveKit's `rustls-tls-webpki-roots` feature turns on rustls's `ring`
     provider; instead the crate takes `__rustls-tls` and asks tokio-tungstenite and reqwest for
     Mozilla's roots directly, compiles rustls with aws-lc-rs only, and installs it as the
     process default before connecting (reqwest without a provider needs one). `ring` and
     `openssl-probe` are no longer in the shipped tree (`ring` remains only in `webrtc-sys-build`,
     a build dependency that downloads nothing when `LK_CUSTOM_WEBRTC` is set). No OpenSSL
     anywhere.
  6. **Media:** the microphone and speaker through WebRTC's platform audio device module;
     camera frames pushed by the app as I420 (VP8, simulcast); remote audio as PCM streams and
     remote video as RGBA frames for the app's canvas (libyuv's `ABGR`, which is the bytes R, G,
     B, A).
- What is not ours to choose: the frame cryptor (AES-GCM) and DTLS-SRTP run on the BoringSSL
  built into libwebrtc, not on aws-lc-rs (aws-lc descends from BoringSSL). Sender keys cross into
  it as a `Vec` LiveKit's API takes by value, which is dropped without being wiped; the key
  schedule's own copies are `Zeroizing`.
- Licences: `zlib-rs` (Zlib), flate2's backend that LiveKit selects, gets a `deny.toml`
  exception; libwebrtc's own `LICENSE.md` (BSD-3-Clause and its third-party notices) goes on
  the app's licences page with it.
- Evidence (`apps/calls-media/tests/media.rs`, against the compose LiveKit with rooms created as
  the `calls` service creates them, keys from `songbird_content::call`):
  - `audio_and_video_cross_encrypted`: B hears A's tone and sees A's grey frames; A's frames
    decrypt.
  - `a_wrong_key_is_never_played`: a participant under another secret is reported as failing
    to decrypt, and nothing of it is heard.
  - `a_new_epoch_locks_out_old_keys`: after a group epoch change, the members who installed it
    still hear the sender at the new index; a removed device that kept the old epoch's keys is
    reported failing and hears nothing.
  - `tls_runs_on_aws_lc_rs` (unit). Clippy with the workspace's lints is clean; `cargo deny`
    passes licences, bans and sources for the crate.
- Consequences: the app's call screens are wired to this crate next (signalling from app-core,
  media from here). Camera capture needs each platform's camera host (desktop through NSH's
  camera, mobile through the native hosts, D-0117); until then a call can be audio-only.

## D-0150 — Calls, part 1: tickets, encrypted signalling and the key schedule

- Date: 2026-11-20
- Status: proposed (waiting for the owner); the media stack is the owner's choice (2026-11-20:
  "LiveKit Rust SDK"), built in part 2
- Decided by: owner (media stack); agent (this part)
- Decision (`songbird_app_core::calls`): tickets from `POST /v1/calls` (presenting the callee's
  access key, so only a contact's call wakes anyone), `/v1/calls/group` and `/v1/calls/join`;
  `Content.call` signals sealed per device with `EXPIRES` 60 s (1:1) or as MLS messages (groups,
  not franked: not visible messages); offers ring no longer than `ring_left_ms` allows; sender
  keys from `songbird_content::call` (1:1: the offer's secret; groups: the MLS exporter at the
  current epoch). No key reaches a server.
- Evidence: `gate_a2::calls_ring_answer_and_agree_on_keys` against the real `calls` service and
  the compose LiveKit: a ticketed room, an offer that rings, an answer with a second identity,
  the same per-sender keys on both sides (and different keys per sender), a hang-up, a group call
  with exporter keys equal on both members (eleven gate tests pass).

## D-0149 — Communities and private channels on the device

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent, under the owner's no-mocks rule
- Decision:
  1. **Channels** (§6.10): a group created with `Policy::CHANNEL`; subscribers join by its
     link read-only; the server refuses their posts (reason 14). The app's "New channel" makes
     one and opens its link page.
  2. **Communities** (§6.9): a community's ID is its announcement group's ID (only admins post
     there), so members need no new content format to know it: a group whose roles let only
     admins send, and which is not a channel, is asked for its community
     (`POST /v1/communities/state`, 404 for a plain group). The community's name is the
     announcement group's title (an encrypted group update); the server's meta blob stays
     unused. Admins of both add a group (`/v1/communities/groups/add`).
  3. **No public directory** (public channels await the owner's legal review, D-0065 §4): the
     seeded directory shows only in the demo fixtures.
  4. The app's bridge gained the handlers four earlier edits had silently missed (a typing
     indicator received, joining a group, group updates, removal), found while wiring this:
     every bridge hook was then checked to exist.
- Evidence: `gate_a2::channels_and_communities` against the real stack: an owner posts in a
  channel a subscriber reads, the subscriber's post is refused, a community's announcement group
  and an added group are listed for a member (ten gate tests pass); `apps/ui` 211 tests.
- Consequences: joining a community's other groups is by their own links (the protocol's
  rule); a community's encrypted info beyond its name, and its picture, come with group info
  (`/v1/groups/meta`, not yet used).

## D-0148 — Group invite links and join requests

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent, under the owner's no-mocks rule
- Decision: links are `https://songbird.chat/g#<base64url(handle ‖ secret)>`
  (`songbird_proto::group_link`); `auth` and the `proof` key come from the secret by HKDF
  (salt `songbird.v1/group-link`). Making a link commits its proof key into the roles, then
  registers `SHA-256(auth)` with the server; revoking withdraws both. Opening a link joins by
  external commit whose authenticated data carries the link proof, or, where joins need
  approval, asks to join; admins list, approve (an ordinary add) and decline requests. Members
  follow an external commit only if it passes the structural rule (D-0078) and carries a valid
  proof under a key the group published, unless it only replaces the joiner's own old leaf. The
  group's context check gained `context_extensions_allowed` (the same rule on extensions a
  client holds). The app's invite page makes or reads the real link, resets it, shares it with
  the system sheet; join requests and pasted group links are real.
- Evidence: `gate_a2::invite_links_and_join_requests` against the real stack: a link joins Bob
  with a proof Alice's device checks, a revoked link opens nothing, an approval link files a
  request Alice lists and approves (nine gate tests pass). The test raises the commit and link
  budgets, which `gate_p05` tests; a real burst of group changes meets 429 (retrying is the
  app's follow-up).

## D-0147 — Groups: removing, leaving, adding later, media

- Date: 2026-11-20
- Status: proposed (waiting for the owner)
- Decided by: agent, under the owner's no-mocks rule
- Decision: removing a person removes every leaf of their devices in one commit (the server
  enforces the roles rule); leaving proposes the device's own `SelfRemove`
  (`POST /v1/groups/propose`) and forgets the group; a member who receives a `SelfRemove`
  stores it and commits it at once (no role needed); a commit that removes this device ends the
  group here; adding people later is the same commit as creating; photos and files in groups are
  uploaded once with `fetches` = the other devices in the group. The app's Exit group, Remove
  and Add members act on real groups.
- Evidence: `gate_a2::group_photos_removals_and_leaving` against the real stack (eight gate
  tests pass).
- Consequences, recorded rather than faked: a device does not learn other members' accounts
  from the MLS tree (credentials are device IDs), so it cannot yet apply the roles rule to
  received commits or judge the delivery service's removal proposals itself; the server applies
  both today. A device-to-account map signed into the group (or carried in the roles extension)
  is the follow-up.

## D-0146 — The group client: create, join, commit and talk over the delivery service

- Date: 2026-11-19
- Status: proposed (waiting for the owner)
- Decided by: agent, under the owner's no-mocks rule
- Decision (`songbird_app_core::mls::client`, on D-0145's provider):
  1. KeyPackages with the capabilities every Songbird leaf has, the device ID as credential,
     single use plus one last resort (`PUT /v1/keypackages`); the private halves saved first.
  2. Creating: epoch 0, the roles extension (this account owner), the delivery service pinned
     from `POST /v1/groups/ds` as the one external sender; `POST /v1/groups`; then one commit
     adding every device of each member, each KeyPackage validated in full and its device on
     the account's verified list (`POST /v1/groups/commit`, merged only on 200).
  3. Group messages: `account ‖ padded Content` in a `PrivateMessage`, shared body, franked.
     The receiver accepts it only if the MLS sender device is on that account's verified list
     (fetched when unknown) and the franking stamp opens.
  4. Receiving: a Welcome joins with the server's tree, only if the group names exactly the
     pinned service key; commits merge only under the same rule; group state saved after each.
  5. Titles travel as `GroupUpdate` data messages (members' devices only).
- Evidence: `gate_a2::a_group_is_created_joined_and_talked_in` against the real `api` (groups
  service, rosters) and `delivery`: the server validates the group and commit, the Welcome
  reaches Bob, the title and messages cross both ways (seven gate tests pass).
- Consequences: not yet: the roles rule on received commits (`songbird_groups::roles::authorize`;
  the server enforces it today), server-proposed removals, removing members and leaving,
  invite links and join requests, external commits for a member's new devices, the stale-epoch
  retry, communities and channels. The app's group screens are next.

## D-0145 — Groups on the device: an OpenMLS provider on aws-lc-rs; the third `unsafe` exception

- Date: 2026-11-19
- Status: accepted (owner, 2026-11-19: "aws-lc-rs provider", over shipping OpenMLS's RustCrypto
  provider with `hpke-rs` (MPL-2.0) and its `crypto-bigint` advisory); the `unsafe` exception
  is proposed, for the owner's review
- Decided by: owner (the provider); agent (its construction)
- Context: groups need a full MLS provider on the device (the server's only verifies). aws-lc-rs
  does not expose seeded ML-KEM key generation (X-Wing derives node keys from path secrets) or
  SHAKE256.
- Decision:
  1. `songbird-aws-lc-ext`: two FFI calls into the aws-lc build aws-lc-rs already links
     (`EVP_PKEY_keygen_deterministic` for ML-KEM-768 from `d ‖ z`; SHAKE256), each object freed
     by a guard, each buffer owned and sized by Rust. `unsafe` is denied in the crate and allowed
     only on its FFI module (as D-0033, D-0118). Tests: SHAKE256 against FIPS 202's empty-input
     vector; seeded keys deterministic, the expanded key embedding its public key.
  2. `songbird_app_core::mls::crypto::DeviceCrypto`: X-Wing (draft-06: SHAKE256 expansion,
     `SHA3-256(ss_M ‖ ss_X ‖ ct_X ‖ pk_X ‖ label)`), HPKE base mode (RFC 9180) with X-Wing's
     HPKE code point `0x647a` in the suite ID, HKDF-SHA256 and AES-128-GCM, the exporter;
     AEAD, HKDF, HMAC, SHA-2 and Ed25519 from aws-lc-rs. Only
     `MLS_128_MLKEM768X25519_AES128GCM_SHA256_Ed25519`.
  3. `DeviceProvider`: that crypto, aws-lc-rs randomness, OpenMLS's in-memory storage (MIT),
     snapshotted into the SQLCipher store.
- Evidence: `tests/groups/tests/device_provider.rs` against OpenMLS's RustCrypto provider
  (libcrux, `x-wing`, `hpke-rs`): identical X-Wing keys from the same path secrets; HPKE
  ciphertexts opened in both directions; equal exporter secrets; a whole group with one member
  on each provider exchanging messages, the aws-lc-rs member's state restored from its snapshot
  in between. The first cross-check failed twice and found the suite-ID code point (OpenMLS's
  `0x004D` is not HPKE's `0x647a`) — the reason the cross-check exists. `cargo deny`: ok.
- Consequences: the group client over `/v1/groups…` (next) uses this provider; the provider and
  the FFI crate join the external audit's scope.

## D-0144 — Block, report and typing through app-core

- Date: 2026-11-19
- Status: proposed (waiting for the owner)
- Decided by: agent, under the owner's no-mocks rule
- Decision:
  1. **Block** (§5.11): `PUT /v1/blocks`; the device keeps its own block list (store schema 4)
     and drops anything a blocked account still sends; the profile key is rotated, the profile
     republished, and the new key shared with every remaining contact as a silent
     `ProfileChanged`, so the blocked account's old access key fails. Unblock:
     `DELETE /v1/blocks` (the primary only). A block of an account never met leaves no fake
     identity: an empty key is "not pinned yet".
  2. **Report** (§5.14): every franked message from another account is kept with its stamp,
     franking key and content exactly as received (`franks`); "Report" sends the newest five
     from that person, each verified by the server; a repeat counts as reported.
  3. **Typing** (§5.7): sealed per device on the ephemeral lane, only to devices with a session
     (typing never starts one); "started" at most every 10 s; shown for 15 s. With "Typing
     indicators" off nothing is sent or shown. Signed `api` requests now cover any request with a
     body, `DELETE` included (unblock carries one).
- Evidence: `gate_a2::typing_reports_and_blocks_work_for_real` against the real stack with the
  production send gate and the reports service (six gate tests pass).

## D-0143 — Reactions, edits and deletes for everyone through app-core

- Date: 2026-11-19
- Status: proposed (waiting for the owner)
- Decided by: agent, under the owner's no-mocks rule
- Decision: reactions, edits and deletes travel as franked `DataMessage` events
  (`Reaction`, `Edit`, `Delete`, PROTOCOL §5.4) through the outbox like messages, but are not
  messages themselves. A reaction is one per reactor and message, the highest counter wins
  (store schema 3, `reactions`); an edit or delete is applied only when its sender is the
  message's author, on both sides (the sender refuses before sending); a deleted message keeps
  its row with no text, content, files or reactions, shown as "deleted". With "Read receipts"
  off, reading a chat marks it read on this device and sends nothing.
- Evidence: `gate_a2::reactions_edits_and_deletes_reach_the_other_side` against the real stack
  (five gate tests pass); `apps/ui` 211 tests.
- Consequences: the 24-hour window for edits and deletes (D-0058) is not yet enforced on
  receipt; edit history is not kept (only the newest text).

## D-0142 — Maps from self-hosted OpenStreetMap vector tiles

- Date: 2026-11-19
- Status: accepted (owner, 2026-11-19: "Self-hosted OSM tiles"); the threat-model row below is
  proposed, for the owner's approval
- Decided by: owner (the source); agent (the design)
- Context: sharing a location needs a map. Public OSM tile servers forbid app traffic and would
  learn where every user looks; Apple's and Google's map SDKs would too.
- Options considered: OSM's public tiles (policy, privacy), platform maps (Apple and Google see
  every place shown), self-hosted vector tiles (chosen by the owner).
- Decision:
  1. **Data:** OpenStreetMap in the OpenMapTiles schema, built with Planetiler (Europe, z0–14),
     loaded by `tools/maps-import` from an MBTiles file into RustFS as raw MVT objects
     (`maps/v1/z/x/y.mvt`; TMS rows flipped; gzip removed so clients need no inflater). New
     dependency for that tool only: `flate2` with its pure-Rust backend.
  2. **Serving:** `GET /v1/maps/v1/<z>/<x>/<y>` on `api`, unsigned (no account, no device, like
     attachment downloads), on the media serving budget per IP range (kind `t`); 204 for an empty
     tile; publicly cacheable.
  3. **Device:** `songbird_app_core::maps` decodes tiles (bounded: 4 MiB, 20,000 features,
     400,000 points per tile), styles them (water, green, urban, buildings at z14, roads by class,
     rail, rivers, borders, place names) and projects Web Mercator; the app draws them on an NSH
     canvas. A picked place goes inside the end-to-end encrypted message as a `LocationPin`.
     "© OpenStreetMap contributors" shows on every map (ODbL).
  4. **Threat-model row (proposed):** the server sees which map tiles an IP address asks for
     (an area being looked at), never which account; tile requests carry no identifier.
- Consequences: the operator runs Planetiler and the import before maps work (a runbook entry);
  the user's own GPS position needs a native location module (NSH has none), so picking on the
  map comes first; live location stays out of scope (the prompt set).

## D-0141 — A2 app-core, milestone 6 (first part): attachments end to end

- Date: 2026-11-19
- Status: proposed (waiting for the owner)
- Decided by: agent, under the owner's no-mocks rule
- Context: every photo, document and voice note in the app was invented; the server's
  attachment service (Prompt 6) and the format in `crates/content` existed but no client used them.
- Decision: `songbird_app_core::attachments` encrypts with `songbird_content::attachment`
  (AES-256-GCM STREAM, key-committing, Padmé-padded), takes a grant for every recipient device
  plus this account's other devices, uploads in one `PUT` or in parts then `complete`, and puts
  the pointer in the encrypted message; downloads follow the signed URL, verify every chunk, and
  only then confirm `fetched`. `Net` gained `upload` (signed, body not covered, §7.2), `fetch`
  and `fetch_post` (unsigned, §7.3). Held in memory up to 256 MiB per file for now.
- Evidence: `gate_a2::a_photo_goes_encrypted_and_arrives_verified` against RustFS and the real
  `api`: a 300,000-byte photo crosses five chunks, Bob's app-core downloads and verifies it
  byte for byte, and a pointer with a changed digest is refused.
- Then (same day): the app's Photos and Document buttons use the system pickers in real chats;
  photos show from their bytes, files save through the save dialog; received files are kept in
  the SQLCipher store (schema 2, `files`), never as plaintext files on disk. Auto-download
  follows Settings (phones follow the mobile choice: NSH cannot tell Wi-Fi from mobile data);
  "HD uploads" off scales photos to 1,600 px. The gate test reads files through
  `Messenger::file`, and shows the object deleted once its one recipient device confirmed it.
- Consequences: streaming from disk for files above 256 MiB, thumbnails and blurhash, voice
  notes (the microphone needs a native module) and video playback come after.

## D-0140 — Nothing mocked: profiles, photos and username links made real

- Date: 2026-11-19
- Status: proposed (waiting for the owner)
- Decided by: agent, under the owner's rule (2026-11-19): "everything must be implemented. no
  mock allowed or placeholder"
- Context: the app's mocked layer (D-0092) included the seeded demo, fake invite and username
  links, a photo button that only toasted, generated portraits on every avatar, and contacts
  named "Songbird user …" because no one fetched their profiles.
- Decision:
  1. A real account starts empty on the real backend; the demo data serves only screenshot
     fixtures and unit tests (`store::demo`).
  2. Avatars: the system picker (NSH 0.6 `pick_images`, scaled to 640 px), sealed on the device
     under a fresh key (`songbird_app_core::blob`: HKDF to an AES-256-GCM key, a random nonce,
     context `songbird.v1/avatar`), uploaded with `PUT /v1/profile/avatar`, named with its key
     inside the encrypted profile only when the photo's audience is not Nobody. Contacts' profiles
     are fetched with the profile key they shared and opened on the device: real names, about,
     status and photo.
  3. Username links: `https://songbird.chat/u#<base64url(handle [16] || key [32])>`
     (`songbird_proto::username_link`); the username sealed under the link key (same blob
     construction, context `songbird.v1/username-link`); `PUT /v1/username/link` stores only the
     ciphertext. Opening a link passes its handle to the lookup, so a link works when its owner
     is not findable by username (§10.3). Changing the username claims it on the server and
     makes a new link.
  4. Invites use the real invite card (§14.10); sharing uses the system share sheet (NSH
     `share`), copying where a platform has none.
- Consequences: telling contacts a profile changed (`ProfileChanged`, sealed and silent) still
  needs to be sent by app-core; the photo crop is the platform picker's.

## D-0139 — A2 app-core, milestone 4: the app's 1:1 chats on app-core

- Date: 2026-11-19
- Status: proposed (waiting for the owner, with the A2 report)
- Decided by: agent (owner: "Yes, do M4")
- Context: the app's chat screens ran on the mock store (D-0092); the delivery reader counted
  envelopes and never acknowledged them.
- Decision: `apps/ui/src/backend/chat.rs` bridges the UI store and `songbird-app-core`:
  1. With the real backend on and the device registered, app-core opens a SQLCipher file in the
     app's data folder on connect; on its first run it publishes the device's keys, the signed
     device list (the primary, with the account key) and the profile, whose key is the one the
     app's profile code already publishes.
  2. Each delivery is decrypted and stored by app-core, then acknowledged, in `seq` order; one
     it cannot store stays on the server.
  3. Sending in a real chat goes through app-core (no simulated ticks); ticks follow the server's
     answer and the recipient's receipts; reading a real chat sends read receipts. Only text (and
     text with a link) is sent; other kinds fail with a plain message until M5/M6.
  4. New chat → "Message this username on Songbird": the username's hash is looked up and the
     account's identity and devices are checked before the chat opens.
  5. Real accounts and messages get UI IDs from their first 8 bytes with the top bit set, so they
     never collide with the mock's small IDs and need no table on disk.
- Evidence: `cargo build` and `cargo test --lib` (208 tests) in `apps/ui`; the pipeline itself is
  proven by `tests/e2e/tests/gate_a2.rs` (D-0138). Not yet run: two app instances against
  `scripts/dev-app-backend.sh` or the phone test environment (D-0135).
- Consequences: the store key and the device identity seed sit in NSH app storage (marked
  `mock:`) until the Keychain/Keystore modules exist. The mock's seeded chats still show beside
  real ones; a live account should start empty (follow-up). Message times use real minutes
  relative to the UI's "now".

## D-0138 — A2 app-core, milestone 3: the message pipeline; `POST /v1/devices/identity`

- Date: 2026-11-19
- Status: proposed (waiting for the owner, with the A2 report)
- Decided by: agent (owner's request: fill the backend gaps the app needs)
- Context: a client must verify a contact's signed device list (PROTOCOL §4.6) before encrypting
  to their devices, which needs the account identity key that signs it. No endpoint served it:
  the server stored it (registration) but only the account's own devices and recovery used it.
  A backend gap, found while wiring app-core.
- Options considered: put the key inside the device-list answer (changes a format clients
  already parse); learn it in-band from links and QR codes only (a username lookup would then
  give no way to verify a list); a separate endpoint (chosen).
- Evidence: `tests/e2e/tests/gate_a2.rs` against SurrealDB and `valkey-hot`, the real `api` and
  `delivery` with the production send policy (franking key, contact gate): two app-cores with
  their own SQLCipher stores publish keys, signed device lists and profiles; Alice's first
  message goes as a franked request through PQXDH; Bob decrypts, checks the franking stamp,
  stores it, and sends a delivery receipt; a read receipt follows; Bob's reply accepts the request
  and both then send as contacts with each other's access key; six more messages both ways;
  search finds the first over the encrypted store. `cargo clippy -D warnings` clean for
  `songbird-app-core`, `songbird-api` and the test.
- Decision:
  1. `POST /v1/devices/identity` `{"account"}` → `{"identity_key"}`, signed like every device
     request, on the list-fetch budget. The key is public and already on the threat model's list;
     clients pin it on first sight and prove it with a KT lookup (§4.9). A changed key is refused
     (`IdentityChanged`) until the user accepts it.
  2. `songbird_app_core::messenger`: the pipeline behind a `Net` trait (signed `api` requests and
     `Send`), so the app and the tests share it. Sends: sealed content with a franking key, stored
     as `Sending` with its idempotency key in the outbox before any network; the verified device
     list; sessions from fetched bundles whose identity must equal the list's; `ACCESS` with the
     contact's access key, else `REQUEST`; device mismatches refresh the list and retry with the
     same key; retryable rejections stay in the outbox; a `SentTranscript` to the account's own
     devices. Receives: franking stamp split and checked (an unfranked or mismatched visible
     message from another account is dropped), system frames verified with the pinned system
     key (`PrekeysLow` refills, `DeviceListChanged` refreshes), the sender's identity from their
     verified list, everything stored before the caller acknowledges.
  3. Writing to someone shares this account's profile key with them (a request is an invitation
     to talk; a reply accepts theirs, §5.10); receipts and sync messages never carry it.
- Consequences: the app's screens still read the mock store until M4 wires them to app-core.
  KT proofs of the pinned key (`akd_core` verifiers on the device) are not yet checked: TOFU
  until then, with the identity-change warning.

## D-0137 — A2 app-core, milestone 2: the SQLCipher store

- Date: 2026-11-19
- Status: accepted (owner chose SQLCipher, 2026-11-19); the non-Apple `libcrypto` build is to
  be wired in the platform builds
- Decided by: owner (SQLCipher over column sealing); agent (how it is built and keyed)
- Context: A0 asks for SQLCipher; the locked stack names aws-lc-rs; `deny.toml` bans the
  `openssl` crates; OpenMLS already pins `rusqlite 0.37` (one `libsqlite3-sys` per build).
- Options considered: `bundled-sqlcipher-vendored-openssl` (pulls `openssl-sys`, banned);
  `bundled-sqlcipher` with the platform's crypto (chosen); a custom SQLCipher build against
  aws-lc-sys's prefixed symbols (possible later, more build code).
- Evidence: `cargo test -p songbird-app-core` 27 tests, among them: the database file holds no
  plaintext and does not start with SQLite's header; a wrong key is refused (`Locked`); messages
  are stored once per (author, id); status never moves back; receipts from a group take the
  lowest level over the whole audience; device lists never roll back; FTS5 search folds
  diacritics ("buna" finds "Bună") and never parses user input as query syntax. `cargo deny
  check bans licenses`: ok.
- Decision: `rusqlite =0.37.0` with `bundled-sqlcipher` only. Apple: CommonCrypto (the OS's own
  library). Android, Windows, Linux: SQLCipher's OpenSSL provider linked against an AWS-LC
  `libcrypto` built for the target (`OPENSSL_LIB_DIR`/`OPENSSL_INCLUDE_DIR`), so no second
  crypto library ships. A raw 32-byte key (no password KDF), wrapped by the Secure Enclave or
  Android Keystore by the host; `cipher_memory_security`, `secure_delete`, WAL; schema by
  `user_version`. FTS5 lives inside the encrypted file.
- Consequences: the Android, Windows and Linux app builds need the AWS-LC `libcrypto` step (to
  be added to the app's build scripts with the platform hosts).

## D-0136 — A2 app-core, milestone 1: 1:1 sessions implemented from the specifications

- Date: 2026-11-19
- Status: accepted (owner, 2026-11-19, the three questions of `A2-plan.md` §2); the post-quantum
  ratchet step is proposed, for review and formal modelling before release
- Decided by: owner (protocol source, store, execution); agent (the constructions below)
- Context: every app screen exists (D-0092) but app-core (A2) does not, so no message can be
  exchanged. libsignal is AGPL-3.0 (D-0047).
- Options considered: link libsignal (AGPL, audited) versus implement from Signal's published
  specifications on aws-lc-rs (chosen by the owner, as D-0047 for the server). Local store:
  SQLite with column sealing versus SQLCipher (chosen by the owner: whole-file encryption hides
  schema, row counts, sizes and the search index). Post-quantum ratchet: none (PQXDH only, Signal
  2023–2025), Signal's SPQR (erasure-coded ML-KEM braid; complex), or a KEM step on every DH
  ratchet step (chosen; PQ3's idea, simpler than SPQR, ~2.3 KB per message of a chain).
- Evidence: `cargo test -p songbird-app-core`: 21 tests, among them PQXDH agreement with and
  without a one-time key, forged prekey signatures refused, the ratchet's round trips, out of order
  and lost messages, tampering leaving state unchanged, a stripped KEM ciphertext refused, a
  property test over random send schedules and deliveries, simultaneous session starts converging,
  every state surviving its encoding. Prekey uploads are signed by the same XEdDSA code the server
  verifies with. `cargo clippy -D warnings` clean.
- Decision:
  1. **PQXDH** per the specification (X25519, SHA-256, ML-KEM-1024, info
     `Songbird_PQXDH_X25519_SHA-256_ML-KEM-1024`); `AD` also binds `SHA-256(PQPK || CT)`.
  2. **Double Ratchet** per the specification; AES-256-GCM with key and nonce from HKDF of the
     message key; at most 1,000 skipped per step and 2,000 kept.
  3. **Post-quantum step**: each sending chain carries its sender's fresh ML-KEM-768 key and a
     ciphertext to the peer's newest; `KDF_RK(rk, DH || SS)`. Only the initiator's first chain may
     lack a ciphertext (PQXDH covers it); later headers without one are refused.
  4. **Envelope**: the sender's account and device sealed to the recipient device's identity key
     (ephemeral X25519, HKDF, AES-256-GCM), since `Deliver` names no sender. A prekey message's
     identity must equal the one the sender's signed device list names.
  5. **Safety numbers** over account identity keys (Signal's numeric fingerprint, 5,200 SHA-512
     iterations), one number for all devices of both accounts.
- Consequences: an external audit and a formal model of 3 and 4 before public launch. M2 (the
  SQLCipher store) brings OpenSSL inside SQLCipher only, recorded then.

## D-0135 — A phone test environment on one Coolify server; TLS to delivery in the app

- Date: 2026-11-19
- Status: proposed (waiting for the owner)
- Decided by: agent (owner's request: "deploy the whole app including the needed services on my
  VPS to test the app on my iPhone", LiveKit included)
- Context: the owner's existing Hetzner server (x86, 8 cores, 15 GB) already runs Coolify, whose
  Traefik owns 80/443, and other services. `deploy/bin/env-up` builds new servers with OpenTofu and
  needs SOPS secrets that do not exist yet, so it does not fit. The app had never reached a running
  server (D-0092) and only spoke plain TCP to delivery.
- Options considered: the `beta` pipeline on new servers; a Coolify-managed compose resource built
  from GitHub (needs Coolify's GitHub app, builds Rust on the server through Coolify); a compose stack
  deployed over SSH and routed by Coolify's Traefik through its file provider (chosen). For
  delivery: TLS terminated by Traefik (breaks D-0018: the device signs the TLS exporter, which would
  be Traefik's) versus SNI passthrough with delivery's own certificate (chosen). For the app's TLS:
  a second setup with the operating system's verifier (tried first, then dropped) versus
  `songbird_client::web_tls()`, which the proxy route (Prompt 9) already uses (chosen: one TLS setup,
  no new dependency).
- Evidence (2026-11-19, from outside the server):
  - `api.songbird.chat` 200 over HTTP/1.1 and HTTP/2; `livekit.` 200; TURN/TLS on 5349 and LiveKit
    TCP 7881 reachable; Let's Encrypt certificates valid.
  - `ws.songbird.chat:443`: `101 Switching Protocols`, `songbird.v1`, served by delivery (TLS 1.3,
    ALPN `http/1.1`). Before the fix below Traefik answered `418` from `noop@internal`.
  - The owner's iPhone registered (`songbird_api_registrations_total 1`, `handles_claimed_total 1`)
    and authenticated to delivery over TLS (`songbird_delivery_auth_ok_total 1`, no failures).
  - With `web_tls()` on `main` (Prompts 8–11 merged): a second device (the iOS simulator) registered
    and connected ("Connected. Signed in with this device's key"; `registrations_total 2`).
  - App: `cargo test --lib` 206 passed, including `plain_tcp_only_to_this_machine`.
- Decision:
  1. `deploy/dev-vps/`: compose project `songbird` (SurrealDB, valkey-hot, valkey-jobs, RustFS, api,
     delivery, calls, LiveKit), images built on the server from the checkout, `environment = "dev"`,
     services as 65532 with read-only roots and no capabilities. Secrets are generated on the server
     into `/opt/songbird/secrets` (0640 root:65532) and never leave it. The stores publish no ports.
  2. Traefik routes in `/data/coolify/proxy/dynamic/songbird.yaml`: api and LiveKit terminated by
     Traefik; `ws.` passed through by SNI to delivery. No HTTP router may match `ws.`: one did (to
     obtain the certificate) and Traefik served it instead of the passthrough. Certificates for `ws.`
     and `turn.` come from a router on `turn.` with a `domains` list, and `certs.py` exports them
     daily (restarting delivery or LiveKit on change). Songbird's routers keep no access log
     (invariant 6).
  3. LiveKit single node without its Valkey; TURN/TLS on 5349 (443 belongs to Traefik); the deny set
     of D-0091 kept. Hetzner firewall: TCP 7881, UDP 7882, TCP 5349, UDP 3478 added.
  4. App: TLS 1.3 (`songbird_client::web_tls()`: aws-lc-rs, Mozilla's roots) to any delivery address
     that is not loopback; plain TCP only to loopback; the proxy route keeps its own. No new
     dependency. `SONGBIRD_SERVER=<domain>` at build time sets the default addresses (billing at
     `billing.<domain>`, not deployed here).
  5. App fix: `backend::run` and the delivery task now run under a detached owner. Register first
     saves the address, which re-renders its view and disposed the owner the task was made under; NSH
     cancels such a task before its first poll, so registration silently never ran.
- Consequences: not `beta`: no mTLS, no worker (account deletion is queued, not carried out), the
  mailbox in memory, push off, IP throttles see Traefik's address. iOS builds need
  `IPHONEOS_DEPLOYMENT_TARGET=17.0` (Rust's default iOS 10 lacks `___chkstk_darwin`, which aws-lc
  needs); to be set in the app's build. Revisit: replace with `beta` once secrets exist; the shared
  host also runs unrelated services and exposes Postgres (5432) and Coolify (8000–8080) to the
  internet, which the owner should restrict.

## D-0134 — Prompt 11, the security review's fixes

- Date: 2026-11-19
- Status: accepted; the threat-model rows approved by the owner on 2026-11-19
- Decided by: agent, after an independent read-only review of the whole Prompt 11 diff
- Context: `docs/reports/P11-gate.md` (the review and what was done about each finding).
- Decision:
  - **Mollie**: each payment counts once (its ID remembered 400 days); the period runs from its
    `paidAt`; only the first payment turns renewal on; a revoked row takes no new checkout
    (409 `revoked`; the app starts over with a new secret).
  - **No clock linkage**: billing's `at` and the registry's `ent.at` are whole days; a provider
    API answer keeps the row's stamp; a transaction presented by the app counts by its day.
    Kept tokens are redeemed at a random moment of their month's first two days; vault day
    passes are fetched up to a week ahead at start-up, never by a backup run.
  - **Trust in keys**: the app trusts only token keys and a support key shipped with the build;
    a development build without them pins the first seen and refuses any other ID; a release
    build without them refuses plans and support. Backups use the same pin.
  - **Abuse signals**: a reporter counts once per sender (keyed pair mark, 31 days); a block
    consumes the request mark it counts.
  - **Budgets**: `/v1/invites/status` 60 an hour per account; Google's keys refetched for an
    unknown key at most once a minute; billing's range budget keys IPv6 on the whole /48 and
    evicts only idle ranges (new ranges wait when the table is full of busy ones).
  - **Staff**: `support open` and `reply` need `--confirm` and are audit-logged; tickets are
    looked up by ID. The app treats only 409 `spent` as already redeemed.
- Consequences: a release must ship the deployment's token and support public keys in the app
  (a gate item for Prompt 13's release). Not changed: the app's secrets in plain app storage
  (the development stand-in for the Keychain / Keystore, like the identity seed); a first
  purchase's tokens are redeemed at once (the user is waiting), seen only by an observer of both
  services' live traffic.

## D-0133 — Prompt 11, milestone 7: the app on real plans

- Date: 2026-11-19
- Status: accepted by the agent within D-0126 (M7)
- Decided by: agent, within the owner's plan
- Context: `docs/reports/P11-M7.md`.
- Decision:
  - The app pins the token key on first sight (per key ID, kept in app storage) and refuses a
    different key under the same ID; tokens that fail their proof are refused.
  - Desktop purchases go through Mollie in the browser with a subscriber secret kept on the
    device; the app polls billing (5 s, at most 15 minutes) and then redeems this month's token;
    next month's is kept and redeemed in its month.
  - A support ticket names the account only when the user turns on "About my account" (and for
    appeals); logs only with consent, exactly the previewed scrubbed text.
  - Limited and suspended accounts see a banner and a status page with the appeal; the
    entitlement answer carries the standing.
  - Family seats' links are shared by the owner by hand (copy), never sent by Songbird.
- Consequences: StoreKit 2 and Play Billing wait for the native hosts; the live purchase run waits
  for a Mollie test account or a locally served emulator.

## D-0132 — Prompt 11, milestone 6: support, legal requests and invites without a data sink

- Date: 2026-11-19
- Status: accepted by the agent within D-0126 §9–§12; the threat-model rows (§1, §3.28, §6)
  approved by the owner on 2026-11-19; the legal process awaits counsel
- Decided by: agent, within the owner's plan
- Context: `docs/reports/P11-M6.md`.
- Decision:
  - **Support tickets** in the registry (`ticket`, `ticket_msg`): opened signed (the account
    attached; priority on Plus; required for appeals) or unsigned (anonymous); every message
    sealed on the device to the support key (`Domain::Support`) or by staff to the ticket's own
    reply key (`Domain::SupportReply`); a random ID and a bearer drawn by the server, the bearer
    stored hashed; one 404 for a wrong bearer and no ticket; 50 messages of 32 KiB at most;
    budgets per keyed IP range; deleted at close with every message, 90 days untouched at most;
    unlinked when the account is deleted; listed (never their contents) in the account's export,
    as is its standing. `songbird-admin support keygen | list | open | reply | close`.
  - **Legal requests**: `songbird-admin legal export` is the account's GDPR export wrapped with
    the case and a scope statement, written to a new 0600 file, audit-logged and counted;
    `legal record` counts the rest; `transparency` prints a half-year's Markdown report from
    counts only (`legal` table) and the standings now. `docs/RUNBOOKS/legal-requests.md`.
  - **Invites**: `POST /v1/invites/status` (signed, up to 50 code hashes, live or not); the
    invite link (`crates/proto/src/invite_link.rs`): everything in the fragment, versioned,
    parsed strictly.
  - The support throttle key is derived (HKDF, `songbird.v1/support-throttle`) from the push
    index secret every `api` node already shares, rather than a new secret to provision.
- Options considered: tickets in an external helpdesk (rejected: a third party holding users'
  accounts and logs); a ticket bearer chosen by the client (rejected: the server cannot vouch for
  its entropy); a per-inviter "invites used" counter (rejected, D-0038).
- Evidence: `crates/accounts/tests/support_live.rs` (4), `services/api/tests/plans_live.rs`
  (`support_tickets_are_sealed…`, `an_inviter_sees_only…`), `crates/proto` `invite_link` tests
  (RFC 4648 vectors) and fuzz target `invite_link`, `tools/admin` `support` and `legal` tests.
- Consequences: staff must keep retired support keys until their tickets close; the M7 app adds
  the Help form (consent and preview of logs), the appeal screen and the invite link.

## D-0131 — Prompt 11, milestone 5: abuse controls without content

- Date: 2026-11-19
- Status: accepted by the agent within D-0126 §7–§8; the threat-model rows (§1, §3.28, §4) approved by the owner on 2026-11-19
- Decided by: agent, within the owner's plan
- Context: `docs/reports/P11-M5.md`.
- Decision:
  - Escalating proof of work: +1 bit at 3, 10 and 30 registrations from a range in the hour,
    +1 at 300 across the deployment and +2 at 1,200, at most +4.
  - Standings `limited` (automatic, 7 days from the latest signal) and `suspended` (moderators
    only), in the registry and the entitlements snapshot, followed by `api` and delivery.
  - New accounts (30 days): 5 requests an hour, burst 10; limited: 1, burst 2, and no new group
    links; suspended: every send rejected with reason 15 `Suspended`, no sender certificate, no
    new calls; receiving, export, deletion and appeals always work.
  - Signals under keyed tags, evaluated on arrival: five distinct reported direct messages in 30
    days; five blocks in 7 days by people the account sent a request to, at least 20 % of its
    requests. Group reports never limit automatically.
  - `songbird-admin moderation decide | lift | show`, audited; a decision deletes its report.
- Evidence: `tests/e2e/tests/gate_p11.rs` (3), `registration_live::a_registration_flood…`,
  `crates/entitlements` tests, `tools/admin` tests.
- Consequences: a new wire reject code (15), read by older clients as unknown. M6 adds appeals
  through support.

## D-0130 — Prompt 11, milestone 4: Family seats without a family graph; the vault unlinked

- Date: 2026-11-19
- Status: accepted by the agent within D-0126 §5 and §6 (re-deciding D-0119 §3); the
  threat-model rows (§1, §3.27, §3.28) approved by the owner on 2026-11-19
- Decided by: agent, within the owner's plan
- Context: `docs/reports/P11-M4.md`.
- Decision:
  - **Family:** a mailbox in `api` of random families and up to five slots, managed by the hash
    of the owner's management token, each slot read by the hash of its member's bearer, holding
    a token sealed (ChaCha20-Poly1305) under a key from the invite link the server never sees.
    No account anywhere; removal at once (the month redeemed stays); a family 60 days unused
    goes.
  - **Vault:** no owner. `POST /v1/vault/tokens` issues day passes (a week ahead, the account's
    level) and creation tokens (two per 180 days per account) to the primary, blinded; a vault
    is created with a creation token and written under a day pass presented once a day; its
    quota is the pass's level; it keeps the day of its latest pass and last use and is retired
    after 365 days unused. `vault_owner` is removed. Deleting an account does not reach the
    vault (the app deletes it first).
  - The vault adapter takes `Passes` instead of a device signer; the app's `AppPasses` pins the
    token key per process (release builds ship it).
- Evidence: `services/api/tests/plans_live.rs::a_family_shares_plus_without_naming_anyone`,
  `crates/tokens/src/family.rs` tests, `tests/e2e/tests/gate_p10.rs` (8 of 8).
- Consequences: Prompt 10's PROTOCOL §11.8 and threat-model rows change (owner gone, passes
  added); M7 builds the app's Family screens on the mailbox.

## D-0129 — Prompt 11, milestone 3: billing, a service of its own

- Date: 2026-11-19
- Status: accepted by the agent within D-0126 §1–§4; the threat-model rows (§1, §3.28, §6) approved by the owner on 2026-11-19; the sandbox runs wait for the owner's accounts (D-0126 §11)
- Decided by: agent, within the owner's plan
- Context: `docs/reports/P11-M3.md`.
- Decision:
  - `services/billing`: its own database and credential, its own uid (`billing_uid`), the only
    holder of the providers' credentials and, with `api`, of the token secret; no registry
    access; no account anywhere.
  - Rows under `HMAC(billing secret, provider ‖ ID)`; Apple's and Google's IDs never stored;
    Mollie's customer and subscription IDs stored (needed to cancel); lapsed rows deleted 400
    days after the last paid day.
  - Apple: `x5c` exactly leaf, intermediate, pinned root (Apple Root CA - G3, its SHA-256 checked
    at start in production); Apple's marker extensions on leaf and intermediate; rustls-webpki
    for the path; `ES256`; bundle, environment, type and no `appAccountToken`; the App Store
    Server API as the source of truth when configured.
  - Google: the Play API as the only source of state; Google's OIDC token on every notification;
    test purchases refused in production; `obfuscatedExternalAccountId` refused; voided
    purchases revoke for good; pending purchases acknowledged.
  - Mollie: customers with no name or e-mail; webhooks fetched back; one subscription per row;
    paid through the next charge plus 7 days; refunds and chargebacks revoke.
  - Replays skipped by message ID; older states ignored; caps spent atomically (retried on write
    conflicts); a per-node budget per IP range for unauthenticated requests.
  - **New direct dependency:** `rustls-webpki` 0.103.16 (already in the tree under rustls,
    vetted), for certificates from the network; `rcgen` (in the workspace) as a dev-dependency
    for the test PKI.
- Evidence: `services/billing/tests/billing_live.rs` (7 live tests), unit tests (13), fuzz
  `billing_inputs`.
- Consequences: M4 adds Family seats and vault tokens on billing's and `api`'s tokens. The
  gate's sandbox item needs the owner's App Store Connect, Play Console and Mollie test accounts.

## D-0128 — Prompt 11, milestone 2: anonymous tokens on RFC 9497's POPRF

- Date: 2026-11-19
- Status: accepted by the agent within D-0126 §2; the threat-model rows (§1, §3.28, §6) approved by the owner on 2026-11-19
- Decided by: agent, within the owner's plan
- Context: D-0126 §2 chose anonymous tokens so billing and accounts share no identifier, and
  asked for the evaluation of libsignal's credential library. `docs/reports/P11-M2.md`.
- Options considered: libsignal `zkcredential` (AGPL, a port; hidden attributes not needed for
  payments); a VOPRF key per level and month (the plan's first idea: many keys to publish and
  pin, each a chance to tag a user); RFC 9497's POPRF with the metadata as public input; plain
  linkage in a stricter store (refused by the owner's plan).
- Decision:
  - `crates/tokens`: RFC 9497 `ristretto255-SHA512`, all three modes, written from the RFC on
    `curve25519-dalek` 5 and aws-lc-rs, checked against all of Appendix A.1. No new external
    dependency.
  - Tokens on the POPRF: metadata `"songbird.v1/token\0" || kind || level || period`; key ID
    `n` = `DeriveKeyPair(secret, "songbird.v1/token-key\0" || n)`; the highest ID issues,
    every listed ID redeems; apps pin the public keys; the client checks the batched DLEQ proof
    against the pinned key before unblinding; nullifier `SHA-256(context || key || info ||
    nonce)`.
  - Plan tokens: one a month per level, redeemable in their month and the one before, Plus
    through the month's end plus 3 days; nullifier and entitlement in one transaction.
  - `[api.tokens]` (the secret shared with billing only), `POST /v1/entitlements/redeem`
    (30 an hour per account), `GET /v1/tokens/keys`.
  - Songbird's vector re-derived in CI by `vectors/check.py` (Python standard library,
    ristretto255 from RFC 9496).
- Evidence: `crates/tokens/tests/rfc9497.rs`; `src/token/tests.rs` (9); `vectors/check.py`;
  `services/api/tests/plans_live.rs` (3); fuzz `tokens` (6.6 million runs); `tests/bench.rs`.
- Consequences: M3's billing issues plan tokens under its caps; M4 adds vault tokens (kinds 2
  and 3). Key transparency for the token keys is proposed for Prompt 12.

## D-0127 — Prompt 11, milestone 1: entitlements, one snapshot, every limit by plan

- Date: 2026-11-19
- Status: accepted by the agent within D-0126; the threat-model rows (§1, §3.28, §4) approved by the owner on 2026-11-19
- Decided by: agent, within the owner's plan
- Context: `docs/reports/P11-M1.md`.
- Decision:
  - `crates/entitlements`: levels (Free 0, Plus 1; every Family seat is Plus), a catalogue
    from `[api.plans]` with the owner's numbers by default (D-0126 §3), refused at start-up if
    a higher level allows less or a number is out of bounds; a snapshot of the accounts not on
    Free, swapped whole (`arc-swap`) when `entver` changes; expiry by date at every reader.
  - Registry `ent:[account]` (level, last UTC day, source `grant` or `token`) and `entver`;
    deleted with the account; in the GDPR export. Teams tables `org`, `org_admin`,
    `org_member`, `org_device` defined and empty.
  - `api` enforces every limit from the account's level: device links, the largest file (413
    `plan`), upload bytes a day, new packs per 30 days (429 `plan`), the vault's space and
    chunks (its owner's plan). Downgrades refuse additions and delete nothing.
  - The former configuration numbers stay as the deployment's ceilings, with defaults raised so
    the plans decide. Budgets of bytes and packs have a record per level.
  - `GET /v1/entitlements`; `songbird-admin plans grant | revoke | show`, audit-logged with the
    account (an operator's change to one account must be attributable).
  - No new external dependency (`arc-swap`, `serde`, `tokio` are in the workspace).
- Evidence: `crates/entitlements` tests (8, two of them property tests);
  `crates/accounts/tests/entitlements_live.rs`; `devices_live::the_plan_sets_the_device_limit…`;
  `media_live::the_plan_sets_file_size_daily_bytes_and_packs`;
  `gate_p10::the_quota_is_the_owners_plan_and_a_downgrade_keeps_the_backup`.
- Consequences: M2 redeems anonymous tokens into `ent` (`extend_entitlement`). `delivery`
  follows the snapshot from M5 (standings). PROTOCOL §14 is appended after §13.

## D-0126 — Prompt 11: the plan, and the owner's decisions it needs

- Date: 2026-11-19
- Status: accepted (owner, 2026-11-19, "Approve all": every recommendation)
- Decided by: owner, on the agent's plan `docs/reports/P11-plan.md`
- Context: Prompt 11 asks for Free, Plus, Family and (later) Teams plans enforced everywhere,
  payments verified server-side with minimal linkage to accounts, and abuse controlled without
  reading content. What exists: limits scattered in configuration (devices, vault, file size,
  uploads), GCRA budgets almost everywhere, fixed-difficulty proof of work, franked reports
  sealed to an offline key, `songbird-admin` with its audit log, the GDPR export, mocked plan,
  help and invite screens. Missing: a catalogue and snapshot, any payment verification, a
  credential scheme, families, standings, moderation decisions, legal tooling, support. libsignal's
  `zkcredential` is AGPL-3.0-only and never linked (D-0047).
- Options considered and evidence: in the plan ("What Prompt 11 starts from", "Findings").
  The credential evaluation the prompt asks for: `zkcredential` (port, as D-0049 did for
  usernames) against Privacy Pass on the RFC 9497 VOPRF (key per level and month) against plain
  linkage in a stricter store. Recommended: the VOPRF, which gives payments the same
  unlinkability with far less cryptography to own; hidden attributes are not needed.
- Decisions asked of the owner (recommendations first):
  1. the milestones as written, with a separate `billing` service and database;
  2. anonymous monthly tokens between billing and accounts, no shared identifier;
  3. the plans and limits table (Plus 10 devices, 4 GiB files, 50 GiB uploads a day and backup
     space, 50 packs a month; prices as the app's mock);
  4. Mollie as the EU provider (Songbird the merchant of record, VAT through OSS);
  5. unlinked Family seats through a family mailbox, removal at the month's end;
  6. the vault unlinked from the account on daily tokens, deleted after 12 months unwritten;
  7. automatic actions only limit outreach; suspension by moderators; a keyed counter of
     verified reports per reported account added to THREAT_MODEL §1;
  8. a suspended account receives, exports, deletes and appeals, but cannot send or call;
  9. sealed support tickets, the account only when attached, logs only with consent;
  10. the legal export tool, a counts-only register, a transparency report twice a year;
  11. the owner's App Store Connect, Google Play and EU provider test accounts for the sandbox
      item; counsel on VAT records and the legal process;
  12. Teams as reserved, empty tables;
  13. the app's M7, StoreKit 2 and Play Billing with the native hosts later.
- Agent decisions, unless the owner objects: in the plan's question 14.
- Consequences: M1 (entitlements) is next.

## D-0125 — Prompt 10: the security review's fixes

- Date: 2026-11-19
- Status: accepted by the agent within D-0119; the threat-model rows (§3.27, "review") await
  the owner
- Decided by: agent, within the owner's plan
- Context: an independent, read-only review of the whole Prompt 10 diff (by a separate agent
  given the code, not this session's conclusions) found no critical issue (the format's
  cryptography, the Noise handshake against revision 34, the vault's authorization and the token
  kinds held) and these problems, each fixed with a test:
  1. **Medium–high: the transfer's six digits could be ground.** A man in the middle who knew the
     code (filmed, or copied with "Copy code") could finish a handshake with the new phone, then
     try responder keys against the old phone until its digits matched (about 2^20 tries); and the
     new phone applied what it received without its user's confirmation.
  2. **Medium: the vault quota could be exceeded** by two uploads racing for one name (one counted
     small, the other stored large), or by an upload landing after its chunk was forgotten.
  3. **Medium: objects written after a vault was turned off survived it** (an upload between
     reservation and store, or after the worker's last listing), and a stale manifest swap could
     land in a re-created vault.
  4. **Medium: strangers could block a primary's hand-over and linking** (one shared per-account
     throttle; recovery tokens counted towards the device limit), and the recovery endpoint told
     whether an account exists.
  5. Low: a connection that sent one byte stalled the transfer listener forever.
  6. Low: a malicious store could make a restore keep huge objects (64 MiB each, or 4 GiB archive
     entries) before any size check.
  7. Low: two backup runs on one device could leave a backup naming a chunk the other collected.
  8. Low: WebDAV took `http://`, sending the app password in clear.
  9. Info: rollback and fork were judged on the unauthenticated header; the transfer's record
     limit (9 MiB) was below the largest manifest; the recovery key alone taking over an account
     without a PIN deserved saying in the app; OAuth `state` is not checked yet (nothing calls
     the OAuth flow yet: checked when the redirect handler is wired, Prompt 10 follow-up).
- Options considered (1): a longer code (more digits to compare, still grindable in principle);
  SAS with a commitment (ZRTP, Bluetooth numeric comparison); removing "Copy code". Chosen: the
  commitment, the new phone's own confirmation, and keeping "Copy code" (desktops have no
  camera), now harmless on its own: the code gives one attempt at 10⁻⁶.
- Options considered (4): cap outstanding recovery tokens per account, replacing older ones.
  Rejected: a stranger could then cancel a recovery in progress; since recovery tokens no
  longer count towards the device limit, the per-account throttle (20 an hour) bounds them.
- Decision:
  1. The new phone sends `SHA-256("songbird.v1/transfer-commit\0" || e_pub)` before the first
     message; the old phone refuses a reply with another key. The vector gains the commitment
     (the Noise messages are unchanged) and `transfer_check.py` re-derives it. The new phone asks
     "Does your old phone show this code?" and answers `OK`, recovers and applies only after
     "They match"; "They don't match" ends it with nothing changed.
  2. A chunk's reservation returns the size recorded for an existing name; another size is 409
     `size`. After storing, the upload checks its chunk is still recorded in the same epoch, and
     deletes its object if not.
  3. Reservation and manifest swap check the vault's epoch inside their transactions (gone
     otherwise). `vault_gone` rows carry their time; the worker deletes the prefix at every sweep
     and forgets the row only after `VAULT_GRACE` (15 minutes).
  4. Hand-overs throttle per primary (`ht`), recovery tokens per account (`rt`); the device limit
     counts only link tokens; an unknown or deleting account gets a token that is never stored.
  5. `accept` runs up to eight handshakes at once, each with 20 seconds; the old phone's
     handshake has the same timeout.
  6. A restore refuses an object larger than `max_chunk_object()` before staging it; archive
     index entries are bounded the same way.
  7. Backup runs to one kind of store take turns in a process; before the swap, every chunk the
     run reused from the listing is checked to be still there.
  8. WebDAV and S3 take `http://` only to the device itself (`localhost`, loopback).
  9. `open_manifest` checks the tag before judging rollback and fork; the transfer's record limit
     is the largest chunk or manifest object; the app says where it shows the key that it can
     move the account, and the Settings help says the key without a PIN is a takeover.
- Evidence: `a_reply_must_carry_the_key_committed_to_before_the_first_message`,
  `a_connection_that_stalls_does_not_hold_up_the_old_phone`, the vector and `transfer_check.py`;
  `Screens_backups.test.tsx` (the new phone waits for its own "They match");
  `a_name_keeps_its_size_and_late_uploads_leave_nothing`, the extended
  `turning_backups_off_and_deleting_the_account_leave_nothing`;
  `strangers_asking_for_recovery_tokens_block_neither_hand_over_nor_linking`;
  `an_oversized_object_is_refused_before_it_is_kept`,
  `runs_at_once_never_leave_a_backup_naming_missing_chunks`;
  `a_password_never_goes_over_plain_http_to_another_computer`,
  `plain_http_only_to_this_computer`; the garbage-manifest case in
  `a_store_cannot_roll_back_or_fork_a_device_that_has_seen_a_backup`.
- Consequences: the transfer protocol gains one message (PROTOCOL §11.9); the vault answers 409
  `size` (§11.8); the hand-over flow on the new phone has one more tap. Still open and stated:
  the recovery secret and credentials sit in NSH app storage until app-core's keychain (A0);
  the OAuth `state` check comes with the redirect handler.

## D-0124 — Prompt 10, milestone 5: the app's backups, restore and transfer

- Date: 2026-11-19
- Status: accepted by the agent within D-0119; the threat-model rows await the owner
- Decided by: agent, within the owner's plan
- Context: `docs/reports/P10-M5.md`.
- Decision:
  - The recovery key is real (D-0119 §2), made once and kept on the device; the onboarding
    shows 16 groups; restore checks the key's own check before fetching.
  - Backups go to the vault, an encrypted folder (the user's Documents in release builds, the
    app's data folder in debug builds), S3 and Nextcloud; the three OAuth clouds and iCloud say
    "Coming soon" (D-0119 §6).
  - Restore tries the chosen place, then the Songbird server, then this device's folder;
    applies only a fully verified backup; recovers the account when the device has none.
  - The transfer compares six digits on the old phone before sending; the new phone verifies
    the whole backup before saying yes and recovers with the hand-over.
  - Debug builds only: mod+alt+b on the restore screen backs this device up and fills in its
    key, for review and screenshots.
- Evidence: `apps/ui/src/backend/backup.rs` tests; `apps/ui/Screens_backups.test.tsx` (9 tests,
  headless and macOS); the app's 201 tests.
- Consequences: M6 is the gate.

## D-0123 — Prompt 10, milestone 4: account recovery, the transfer, and messages by time

- Date: 2026-11-19
- Status: accepted by the agent within D-0119 (§1, §4, §10); the threat-model rows await the
  owner
- Decided by: agent, within the owner's plan
- Context: `docs/reports/P10-M4.md`.
- Decision:
  - Account recovery (PROTOCOL §4.11) on the link request: a recovery token (unsigned,
    throttled) or a hand-over token (the primary's), the account identity key's signature over
    the link transcript under `"songbird.v1/recover-account\0"`, the PIN or a passkey for a
    recovery token; one transaction disables every old device and creates the new primary.
    Tokens carry their kind (`ltoken.kind`).
  - The transfer (§11.9): Noise `NNpsk0_25519_ChaChaPoly_SHA256` written on aws-lc-rs (no Noise
    library), the QR payload as prologue, a 6-digit comparison code, the backup format under a
    one-time key, `OK` only after the new phone verified everything.
  - **Records ordered by time across chats** (§11.5): the measurement showed chat-by-chat order
    re-uploads the whole messages stream for one message in every chat (158 MB a day on the
    large history); by time it is 1.5 to 3.7 MB. v1's vectors were regenerated (unpublished).
- Evidence: `services/api/tests/devices_live.rs` (three recovery tests, the existing eight
  unchanged); `crates/backup-storage/tests/{transfer,platforms,cost}.rs`; `gate_p10`'s
  `incremental_cost_on_the_vault`; `transfer_check.py`.
- Consequences: M5 wires recovery, restore and the transfer into the app.

## D-0122 — Prompt 10, milestone 3: the third-party stores; `WebDAV` and Drive without an atomic swap

- Date: 2026-11-19
- Status: accepted by the agent within D-0119 (§6, §7); the threat-model rows await the owner
- Decided by: agent, within the owner's plan
- Context: `docs/reports/P10-M3.md`.
- Decision:
  - Adapters for S3, `WebDAV`, Google Drive, OneDrive and Dropbox in `crates/backup-storage`,
    each on the device, over the `Http` trait; OAuth with PKCE (RFC 8252, RFC 7636), public
    client IDs, refresh once on a 401; the iCloud interface as `ICloudBridge` with its contract
    and a reference bridge that passes the suite.
  - `BackupStore::atomic_swap`: `false` for `WebDAV` (the suite showed Apache `mod_dav`'s
    `If-Match` and even its exclusive `LOCK` let racers through) and Google Drive (no
    conditional write); the run reads the manifest back after a swap; overtaken devices see a
    fork. The owner's alternative, if this is not acceptable: drop `WebDAV` and Drive, or warn
    in the app when a second primary appears.
  - Apache `httpd:2.4.66` (Debian, by digest `sha256:89a0b59e…cbea`) with `mod_dav` in compose
    for CI (Alpine's has no DBM driver); `WEBDAV_PASSWORD` in `.env`.
  - Emulators for Drive, OneDrive and Dropbox written from the documented APIs; the real
    providers are checked once the owner registers the OAuth apps.
- Evidence: `crates/backup-storage/tests/{clouds,adapters_live,conformance}.rs`; a manual run
  on Nextcloud 31.0.14.
- Consequences: M4 builds account recovery and the transfer on these stores.

## D-0121 — Prompt 10, milestone 2: one store interface, a conformance suite, the Songbird vault

- Date: 2026-11-19
- Status: accepted by the agent within D-0119; the threat-model rows await the owner
- Decided by: agent, within the owner's plan
- Context: `docs/reports/P10-M2.md`.
- Decision:
  - `crates/backup-storage`: `BackupStore` (async, `async-trait`, already a workspace
    dependency) with memory, folder, archive-file and vault adapters; the runs (`run`) and the
    conformance suite (`conformance`). HTTP behind an `Http` trait; `HyperHttp` on hyper and
    rustls (both already in the tree).
  - The vault in `api` (PROTOCOL §11.8): reads by the two headers alone; writes also signed by
    the owning account's primary; rows and objects keyed by `HMAC(server key, vault ID)`;
    a random epoch per vault incarnation in the object prefix; turning off deletes the rows at
    once and leaves `vault_gone` for the worker; one 404 for every miss, counted per range;
    quotas from `[api.vault]` (5 GiB, 100,000 chunks, two vaults per account, D-0119); shapes
    checked before counting; chunks recorded before storing; manifest objects under random
    names, swapped in one transaction, the loser's deleted.
  - Worker: vaults go with their account and when turned off, every object deleted and the
    prefix listed empty.
  - Chunks are buffered in `api` (≤ 8.25 MiB) for the signed body check.
- Evidence: `crates/backup-storage/tests/conformance.rs`; `tests/e2e/tests/gate_p10.rs` (six
  live tests); p50/p99 in the report; fuzz `backup_format`.
- Consequences: M3 adds S3, WebDAV, Google Drive, OneDrive and Dropbox on the same trait and
  suite.

## D-0120 — Prompt 10, milestone 1: the backup format

- Date: 2026-11-19
- Status: accepted by the agent within D-0119 (the owner's plan); the threat-model rows await
  the owner
- Decided by: agent, within the owner's plan
- Context: Prompt 10 asks for a versioned, chunked, encrypted, incremental, verified backup
  format readable on every platform, with byte-exact vectors and a cross-platform determinism
  test. `docs/reports/P10-M1.md`.
- Decision:
  - `crates/backup-format` (PROTOCOL §11.1–11.7): the 64-character recovery key with a 64-bit
    check; HKDF-SHA256 keys (vault ID, vault access, manifest, chunk, chunk name, chunker);
    XChaCha20-Poly1305 chunks and manifests with Padmé padding, the name or the vault ID in the
    AAD; chunk names by HMAC-SHA256 of the plaintext; the manifest's generation in the clear and
    authenticated, the previous manifest's hash inside.
  - Three stream kinds instead of a section per content type: one records stream in a
    canonical order (so the chunker resynchronises after an edit and small chats share chunks),
    one small-media stream of blobs under 1 MiB, and one stream per larger media file; each file
    stored once.
  - The keyed `FastCDC` chunker at 1/2/8 MiB (3.7 GB/s, 2.35 MiB average on an Apple M5 Pro).
  - A two-phase reader: verify into memory or an encrypted staging folder, then read with every
    chunk checked again.
  - New dependency: `chacha20poly1305` 0.10.1 (already in the lock and cargo-vet exempt; the
    exemption notes now name this use). `chacha20`, `poly1305` and this crate are optimised in
    dev builds (tests 41 s → 2 s).
  - Determinism: vectors in `vectors/v1/`, checked on macOS arm64 locally and on Linux x86_64
    and i686 in a new CI job, plus `vectors/check.py`, an independent re-derivation with the
    Python standard library.
- Evidence: `crates/backup-format/tests/{format,props,vectors,no_service_links_backups}.rs`;
  fuzz `backup_format` (about 1 million runs, no failure); `tests/bench.rs`.
- Consequences: M2 builds the stores on `ChunkSink` and `ChunkSource`. The vault's access hash
  moves to `songbird-proto` so that `api` can use it without linking the format.

## D-0119 — Prompt 10: the plan, and the owner's decisions it needs

- Date: 2026-11-19
- Status: accepted (owner, 2026-11-19, "Approve all": every recommendation)
- Decided by: owner, on the agent's plan `docs/reports/P10-plan.md`
- Context: Prompt 10 asks for a cloud-agnostic backup format (`crates/backup-format`), storage
  adapters (`crates/backup-storage`), the Songbird vault in `api` on RustFS, local export and a
  device-to-device transfer. What exists: an empty PROTOCOL §11, a mocked backup and transfer
  flow in the app with a 32-character mock key, our own S3 client, RustFS in compose, Padmé
  padding, the account deletion job. Missing: everything the prompt names, a WebDAV server and
  provider mocks for tests, and **account recovery**, which PROTOCOL §4.7 assigned to Prompt 10
  and without which a restore on a brand-new device cannot become a working account.
  aws-lc-rs has no XChaCha20-Poly1305.
- Options considered and evidence: in the plan ("What Prompt 10 starts from", "Findings").
- Decisions asked of the owner (recommendations first):
  1. the milestones as written, account recovery included;
  2. a 64-character Crockford base32 recovery key (256-bit secret, 64-bit check), created with
     the account and kept on the primary;
  3. the vault owned by an account (size, chunk count and generation known to the server, no
     times); unlinking re-decided with Prompt 11's anonymous credentials;
  4. recovery by the identity key's signature plus the PIN or a passkey when set; the transfer
     by a handover token from the old primary;
  5. a vault quota of 5 GiB and 100,000 chunks until Prompt 11;
  6. Google Drive, OneDrive and Dropbox adapters with mock servers now, the app's desktop sign-in
     once the owner registers the OAuth apps, mobile sign-in with the host projects;
  7. Apache httpd `mod_dav` as the WebDAV server in CI, one manual Nextcloud run.
- Agent decisions, unless the owner objects: `chacha20poly1305` 0.10.1 for XChaCha20-Poly1305;
  a keyed content-defined chunker (1/2/8 MiB), media per file, Padmé padding; chunk names by
  HMAC; the manifest's generation in the clear with the previous manifest's hash inside;
  protobuf sections with a minimum reader version; two-phase restore; backups by the primary
  only; an `Http` trait for adapters, SigV4 from `songbird-s3`; vault chunks streamed through
  `api` into a dedicated bucket; Noise `NNpsk0` on aws-lc-rs for the transfer; a new recovery
  key means a new vault; determinism checked on Linux x86_64, Linux i686 and macOS arm64; one
  branch and one PR.
- Consequences: M1 (`crates/backup-format`) starts once the owner approves.

## D-0118 — Camera: the `unsafe` exception marked for Semgrep, as D-0033's

- Date: 2026-11-18
- Status: accepted by the agent, following D-0033's precedent for an approved exception
- Decided by: agent
- Context: CI's Semgrep failed PR #68 with 55 blocking findings: `songbird-no-unsafe-allow` on
  the three platform modules' `#![allow(unsafe_code)]`, the registry's `unsafe-usage` on each
  `unsafe` block in them, and `args` on the probe's `std::env::args`.
- Decision: as `crates/alloc-count` does (D-0033), each module's allow carries a `reason` naming
  the owner's approval (D-0110 §5) and the review (D-0116), and each reviewed `unsafe` line its
  `// nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage` (62 lines in `apple.rs`,
  `android.rs`, `windows.rs`, each beside its `SAFETY:` comment). The probe's arguments choose
  its mode only (`// nosemgrep: rust.lang.security.args.args`). No rule was changed or
  disabled; any new `unsafe` still fails until it is reviewed and marked.
- Evidence: Semgrep 1.185.0 (CI's image, by digest) with `p/rust`, `p/secrets` and
  `.semgrep/` on everything PR #68 touches: 0 findings.

## D-0117 — The camera gate approved; the IJG licence for `jpeg-encoder` only

- Date: 2026-11-18
- Status: accepted (owner, 2026-11-18, "Approve both")
- Decided by: owner, on `docs/reports/CAM-gate.md`
- Decision:
  1. The camera gate: M1–M5 (D-0111 to D-0115) and the security review's fixes (D-0116). The
     gate's "Not done" items stay open: the iOS check on a physical iPhone, the Android scan on a
     device, Linux's portal and PipeWire, the app build on Windows ARM64, video clips, CI for the
     app workspaces and a pinned Gradle wrapper.
  2. `deny.toml` allows IJG as a per-crate exception for `jpeg-encoder` (desktop stills, pure
     Rust). IJG is permissive and asks for an acknowledgement in documentation; it is on the
     app's licences page (You › Help › App version and licences), with the camera's other new
     open-source parts.
- Consequences: no other crate may bring IJG without its own entry.

## D-0116 — Camera: the security review's fixes

- Date: 2026-11-18
- Status: accepted (owner, with the camera gate, 2026-11-18); `docs/reports/CAM-gate.md`, "Security review"
- Decided by: agent, on an independent read-only review of the whole camera diff
- Context: no high findings; three medium, three low, informational notes.
- Decision:
  1. Scanner and desktop reading threads are never joined on the UI thread: they end after
     their current frame or read, and publish nothing once stopped. QR decoding is capped at
     640×480; the `qr_decode` fuzz target reaches large, repeated patterns.
  2. V4L2 reads time out every 500 ms; Linux frame sizes are capped at 8192 and lengths checked
     before allocation.
  3. Apple: interruption, its end and runtime errors are observed and reported; a photo needs an
     active video connection (else `NotRunning`, never an Objective-C exception).
  4. Android: each session unbinds only its own use cases; the activity's stop closes every
     camera; the torch answer reflects the camera.
- Open: CI for the app workspaces (test, clippy, deny, audit); a pinned Gradle wrapper; Apple
  reconfiguration on the session queue; `jpeg-encoder`'s IJG licence (owner).

## D-0115 — Camera, milestone 5: photos in chat, the camera in calls

- Date: 2026-11-18
- Status: accepted by the agent within the approved plan (D-0110); report `docs/reports/CAM-M5.md`
- Decided by: agent
- Decision: the chat's camera sheet takes a photo through the session (stripped JPEG in memory,
  sent as an image message, bytes never on disk); a call's self-view shows the live camera while
  video is on and flips lenses. Sending the bytes and call frames wait for app-core (marked mock).
- Open: video clips (with the microphone module and encryption as they are written).

## D-0114 — Camera, milestone 4: Windows (Media Foundation) and Linux (V4L2)

- Date: 2026-11-18
- Status: accepted by the agent within the approved plan (D-0110); report `docs/reports/CAM-M4.md`
- Decided by: agent
- Decision:
  1. One shared desktop core (`desktop.rs`, no `unsafe`): a reading thread per capture, a pump
     routing frames, preview images with watchers, stills encoded in pure Rust
     (`jpeg-encoder`) and filtered.
  2. Windows: access from the camera consent store (no prompt exists for desktop apps),
     Media Foundation's source reader in NV12, a Direct2D preview.
  3. Linux: V4L2 through `v4l`'s safe API, a GTK picture preview.
- Evidence: Linux on a `v4l2loopback` camera: 1280×720 at the feed's rate, a clean JPEG, the
  link code read; tests and clippy clean on macOS, Ubuntu and Windows.
- Open: **Linux's camera portal with PipeWire** is not built. A Flatpak build needs it; native
  packages do not (the portal grants unsandboxed apps). Build it before a Flatpak exists. The
  full app was not built on Windows ARM64 (`aws-lc-sys` needs `clang-cl` in the VM).

## D-0113 — Camera, milestone 3: Songbird's Android host with CameraX

- Date: 2026-11-18
- Status: accepted by the agent within the approved plan (D-0110, question 1); report
  `docs/reports/CAM-M3.md`
- Decided by: agent
- Context: Android components are Java objects on `NshView`, and a packaged NSH app declares no
  permissions (D-0110 finding 2). The owner approved building the host's camera part now.
- Decision:
  1. `apps/android`: one Gradle module compiling NSH's Java sources from the `nsh-platform-android`
     the app resolves, Songbird's activity (NSH's `AppActivity` plus the camera) and the camera,
     with the Rust library and assets from `nsh package --target android` (`build.sh`).
  2. Java, not Kotlin; CameraX 1.6.2; no Play Services or ML Kit; QR decoded in Rust from the
     analysis frames' Y plane.
  3. A small shim in NSH's package reaches its three package-private activity helpers, so
     NSH's behaviour is used, not copied.
  4. Gradle dependency verification, strict, SHA-256 for all 272 components.
  5. No Google cloud backup or device transfer of the app's data (`allowBackup="false"`,
     data-extraction rules excluding every domain).
- Evidence: lint 0 errors; the six JNI entry points exported; on the emulator (Android 16): the
  explanation, the prompt only on "Continue", "No camera found", the live preview, the camera
  indicator.
- Consequences: the Gradle host exists before the rest of D-0099 (push) joins it. Scanning
  through the JNI frame path is still to be checked on a phone or an emulator with a camera
  macOS lets it use.

## D-0112 — Camera, milestone 2: AVFoundation from Rust on macOS and iOS

- Date: 2026-11-18
- Status: accepted by the agent within the approved plan (D-0110); report `docs/reports/CAM-M2.md`
- Decided by: agent
- Context: M2 of the camera plan. NSH 0.6 registers `#[native_components(ios)]` in the packaged
  app (D-0110 finding 1), so one Rust module serves both Apple platforms.
- Decision:
  1. `apps/camera/src/apple.rs`, the only module allowed `unsafe` so far: `objc2` bindings of
     AVFoundation, Core Media and Core Video, a `SAFETY:` comment on every block.
  2. One `AVCaptureSession` per capture, started and stopped on its own serial queue (both
     block); a registry by preview id lets the `songbird/camera` component attach a preview
     layer on the main thread.
  3. QR through `AVCaptureMetadataOutput` only when the device lists the QR type; otherwise NV12
     frames to the `rqrr` scanner (the built-in camera of this Mac lists none).
  4. Photo delegates are held by the capture until called; stills go through the M1 filter.
  5. Purpose strings in `nsh.json` `ios.infoPlist` and `macos.infoPlist`; the
     `com.apple.security.device.camera` entitlement on macOS.
- Evidence: the on-device probe on this Mac: 1080p NV12 at 30.6 frames/s, a 1920×1080 JPEG with
  no EXIF or GPS, a link code read once through Continuity Camera.
- Consequences: the iOS camera check needs a physical iPhone (the Simulator has none); the
  probe documents how to run without the app (inside a bundle with a purpose string).

## D-0111 — Camera, milestone 1: one interface, newest-wins mailboxes, real QR codes, strict code formats

- Date: 2026-11-18
- Status: accepted by the agent within the approved plan (D-0110); report `docs/reports/CAM-M1.md`
- Decided by: agent
- Context: M1 of the camera plan: the interface, fakes and QR, before any platform code.
- Decision:
  1. `apps/camera` (`songbird-camera`) holds the interface (`Backend`, `Capture`, `Sink`) and
     everything shared (mailboxes, frames, QR decoding, the JPEG filter, `FakeCamera`), each
     module `#![forbid(unsafe_code)]`; the crate denies `unsafe` and only platform modules (M2–M4)
     will allow it. `apps/ui` forbids `unsafe`.
  2. Camera threads reach the app through bounded mailboxes where the newest wins (32 events,
     2 frames): a stalled reader drops the oldest and never blocks the camera. No `futures` or
     channel crate: a mutex, a queue and a waker.
  3. Sessions run only while their screen shows them, the app is not in the background (a call
     on a desktop excepted) and access is granted; the system prompt only on a press.
  4. Codes: `SONGBIRD:LINK:<hex>:<hex>` and `SONGBIRD:SAFETY:<60 digits>`, upper case for QR's
     alphanumeric mode, parsed case-insensitively and strictly in `features::devices::codes`;
     link codes only from the live camera, with a confirmation before linking. The typed
     fallback on the new device is removed.
  5. `qrcode` 0.14.1 draws every code (one SVG path, light plate, 4-module quiet zone,
     error correction M); `rqrr` 0.11.0 decodes, frames capped at 1280×720 and 8192 a side.
  6. Stills are filtered to their image and orientation (a fresh 36-byte EXIF holding only the
     orientation, when not upright); everything after EOI is dropped.
- Consequences: Prompt 9's `link.rs` (PR #67) moves to `codes::link_code` and `codes::parse`.
  Platform code (M2–M4) implements `Backend` and `Capture` and is tested through the same
  screens. New dependencies: `rqrr` (with `g2p`, `lru`), `qrcode`.

## D-0110 — The native camera module: the plan, and the owner's decisions it needs

- Date: 2026-11-18
- Status: accepted (owner, 2026-11-18, "approve all on the camera plan": every recommendation).
  D-0101 to D-0109 are left to Prompt 9 (PR #67).
- Decided by: owner, on the agent's plan `docs/reports/CAM-plan.md`
- Context: the owner rejected linking a device by pasting a text code and asked for native
  camera code on each platform, since the camera also serves photos and videos in chat and
  video calls. NSH 0.6 has no camera API (`nsh-core` and `nsh-base` have only the system
  photo picker), and every scanner, preview and QR code in `apps/ui` is a mock.
- Findings (read in the NSH 0.6 sources, cited in the plan):
  - `#[native_components(ios)]` exists, and the iOS runner `nsh package` generates registers
    it; `nsh.json`'s `apple.infoPlist` takes the purpose string. So the iOS camera can be Rust
    in the packaged app, and Prompt 8's finding 7 is corrected for iOS components (not for the
    extension, PushKit or CallKit).
  - Android components are Java objects registered on `NshView`; the packaged `AppActivity`
    is `final` and the manifest takes no permissions. The Android camera needs Songbird's
    Gradle host.
  - Windows components draw into Direct2D (no child window); macOS gives an `NSView`, Linux a
    `gtk::Widget`.
- Options considered: Swift and Kotlin modules in both host projects (the brief) vs Rust
  everywhere NSH lets Rust register components (macOS, iOS, Windows, Linux) and Kotlin only
  on Android; Apple's metadata output vs a Rust decoder on Apple; `qrcode` vs `qrcodegen` vs
  `fast_qr` to encode; `rqrr` vs `rxing` vs `quircs` to decode; Media Foundation vs WinRT
  `MediaCapture`; the camera portal with PipeWire vs V4L2 on Linux.
- Recommendation (each for the owner):
  1. One crate `apps/camera` (`songbird-camera`) with one interface (`Camera`, `Session`,
     states for access denied, restricted and unavailable, a bounded newest-wins frame
     receiver) and a `FakeCamera` for headless tests; `apps/ui` keeps no `unsafe`.
  2. AVFoundation in Rust (`objc2-av-foundation`) for macOS and iOS with
     `AVCaptureMetadataOutput` for QR; CameraX in a minimal `apps/android` host now, with QR
     decoded in Rust (`rqrr`) and no Google service; Media Foundation on Windows; the camera
     portal and PipeWire on Linux, V4L2 without a portal.
  3. `qrcode` 0.14.1 without default features to draw real codes; the link code upper case
     (alphanumeric mode, version 5).
  4. Link codes read only from the live camera, a link scheme distinct from invites, and a
     confirmation naming what linking gives (the 2025 phishing of Signal's linking).
  5. `unsafe` allowed in `songbird-camera`'s platform modules only, as a recorded exception.
- Consequences: the Android host is built before the rest of D-0099's deferred work; the iOS
  host stays deferred. iOS checks need a physical iPhone. Nothing changes on the server or in
  THREAT_MODEL §1. Calls get the local preview now and send frames once app-core (A2) and
  LiveKit are in the app.

## D-0109 — The Prompt 9 gate approved

- Date: 2026-11-18
- Status: accepted by the owner
- Decided by: owner ("Approve the gate")
- Context: `P09-gate.md`: the four gate items pass. Fast CI is green. The live suites are green,
  except two api tests that need a 127.0.0.2 source address, which this Mac lacks. The security
  review's findings are fixed (D-0108).
- Decision:
  - M1–M6 and the review's fixes are approved.
  - So are the threat-model rows: §1 (the record store, storage objects and their throttles,
    discoverability, group adds from contacts, sealed presence, nudges, supervision), §3.26, the
    completed §4 and the §5 table.
- Consequences:
  - PR #67 may leave draft. Merging stays the owner's call.
  - Still open: the Article 8 ages to confirm with counsel before release; linking between two
    real phones; presence and nudges from the app once app-core (A2) exists.

## D-0108 — Prompt 9: the security review's fixes

- Date: 2026-11-18
- Status: accepted; part of the Prompt 9 gate (`P09-gate.md`, "Security review")
- Decided by: agent, on an independent read-only review of the whole Prompt 9 diff (a separate
  agent given the code and the specifications, not the reports' conclusions)
- Context: one high finding, four medium, seven low and four informational. Each was checked in
  the code before fixing.
- Decision (finding → fix):
  - **H1** A supervised child could turn "anyone may send" on and be reached without a request.
    Fix: a profile upload opening the account answers 403 `supervised` when message requests are
    locked off, and installing such a policy closes the account.
  - **M1** A later policy could push the unlink date out without limit. Fix: the date may only
    move earlier (409 `date`).
  - **M2** `PUT /v1/supervision` answered differently for supervised accounts. Fix: a signed
    policy for another account is refused with one answer (403 `account`) whatever its state.
  - **M3** A nudge sent as an ordinary ephemeral event got around the attention budgets and blocks.
    Fix: the server marks each event in the ephemeral lane with its class, and delivery hands
    attention events over in their own frame (`0x84`, `Event::Attention`). Apps honour a nudge only
    from it.
  - **M4** Linking a second child replaced the key for the first. Fix: a link in force is never
    replaced; the screen says to unlink first.
  - **L1** A parent's later policies named the parent's account to the server. Fix:
    `PUT /v1/supervision/update`, unsigned, authorized by the policy key alone and throttled per
    IP range; one answer for a bad signature and for no policy in force.
  - **L2** The group-add checks revealed supervision to strangers. Fix: the contacts-only check
    runs before the approval check.
  - **L3** The six digits could be ground by someone relaying the codes. Fix: commit then reveal
    (the parent's first code carries only a commitment; key and secret come after the child's
    code), and the child's phone installs only after the digits are compared.
  - **L4** The app lock's wait could be skipped by moving the clock. Fix: the wait is saved as time
    left and counted down on the monotonic clock. "Lock after" locks if either clock says the time
    is up, or the wall clock went backwards.
  - **L5** Storage write budgets counted requests, not records. Fix: a second, cost-weighted
    budget per record written (60,000 an hour, burst 40,000).
  - **L6** Client rules showed how many were on by their length. Fix: padded to 64 bytes.
  - **L7** Linking a device failed on accounts with a PIN. Fix: the new device asks for the PIN
    and sends it with the same request; the token is spent only after the factor checks out.
  - **I1** A device that never scanned the parent's code showed the server's rules unchecked.
    Fix: it says it can't check them.
  - **I2** An age giving no unlink date started supervision ending at once. Fix: linking stops.
  - **I4** Presence batches answered 401 or 204 depending on which devices exist. Fix: always 204;
    a refused batch reaches nobody.
  - **I3** (secrets in plain app storage) is the known A0 mock: the platform keystore comes with
    the host projects. Recorded, not changed.
- Evidence:
  - the parental gate (H1, M1, M2, L1);
  - the nudge gate (M3: a nudge sent as ephemeral never arrives as attention);
  - the presence gate (I4);
  - `a_child_accepts_only_the_first_policy_meant_for_it` and
    `the_link_code_matches_on_both_sides` (L3);
  - `client_rules_open_only_for_their_child_and_version` and
    `the_commitment_binds_the_key_and_the_secret` (L6);
  - `closing_the_app_does_not_reset_the_wait` (L4);
  - the frame proptests (`0x84`).
- Consequences:
  - The parental linking flow has five steps, and the parent's later policies go unsigned.
  - Not exercised between two real phones yet: device and parental linking, and the PIN step
    (P09-M6 "Not verified").

## D-0107 — Prompt 9, milestone 6: the app's presence, profiles, nudges, contacts, device and parental linking

- Date: 2026-11-18
- Status: accepted for the milestone (agent, under the approved plan D-0101); its threat-model rows
  were approved by the owner on 2026-11-18 with the gate (D-0109)
- Decided by: agent (with owner approval of the plan, and of the camera plan D-0110: link codes
  only from the live camera)
- Context: the app's side of M2–M4, and linking (devices and parent with child) now that the
  camera exists (PR #68, merged into this branch at the owner's word).
- Options considered:
  - Device linking: a pasted or typed code (rejected by the owner) or the live camera (chosen).
  - The first parental policy: relayed sealed through a provisioning channel, or shown as a
    third QR code. The relay was built first and replaced: the parent's signed write followed by
    the child's install would let the server tie the two accounts by timing, against D-0105.
    Chosen: the policy as a code; it is about 200 bytes.
  - A newly linked device: re-keying its replica, or an empty replica under the account's key.
    Re-keying was rejected: its first-run defaults are stamped later than the phone's choices and
    would overwrite them on the first sync. Chosen: an empty replica.
- Evidence:
  - `a_linked_device_reads_the_account_settings_and_a_rotation_shuts_out_the_old_key`;
  - `a_waiting_device_shows_a_link_code_the_main_phone_reads`;
  - `a_link_request_proves_its_keys_and_the_confirmation_key`;
  - `a_child_accepts_only_the_first_policy_meant_for_it` (another account, date or key, a later
    version or an unlinking: refused);
  - `parental_codes_round_trip_and_stay_apart` (no parental code is ever read as a link code);
  - the presence lane's tests (`invisible_sends_nothing_ever`, one send per debounce, a batch
    only for sealed devices with their combined key);
  - `nothing_is_sent_without_known_devices` (nudges);
  - the contacts, profile and supervision unit tests; the app's 199 tests.
- Decision:
  - `backend::presence`: the `Presencer` from the privacy settings and the lifecycle, sending
    sealed batches.
  - `backend::profile`: profiles sealed for contacts, a key rotation when a field narrows to
    Nobody.
  - `backend::nudge`: confirmed once, ever.
  - `features::contacts` and `store::ext_contacts`: saved contacts are `k/` records.
  - `backend::link`:
    - the new device shows `SONGBIRD:LINK`;
    - the main phone links what its camera read, after the confirmation;
    - the storage key goes in provisioning v2;
    - logging a device out signs a list without it;
    - the key is rotated when the main phone is left alone.
  - Parental linking: three codes (`SONGBIRD:SUPERVISE`, `SONGBIRD:CHILD`, `SONGBIRD:POLICY`,
    PROTOCOL §10.6).
- Consequences:
  - Presence and nudges send nothing until app-core (A2) can seal for contacts' devices, which
    the mock's contacts lack. No plaintext is ever sent.
  - With other devices still linked, a logged-out device keeps the storage key until A2 delivers a
    new one end to end (D-0106).
  - The live linking flows (devices, parent and child) are built and unit tested. They have not
    yet run between two real phones.

## D-0106 — Prompt 9, milestone 5: the app's settings saved, synced and enforced; app lock; proxy dialing

- Date: 2026-11-18
- Status: accepted for the milestone (agent, under the approved plan D-0101); its threat-model rows
  were approved by the owner on 2026-11-18 with the gate (D-0109)
- Decided by: agent (with owner approval of the plan: every Settings row made real except
  Prompts 10, 11 and 14's)
- Context: every setting saved, synced across devices or kept on one, and the server-enforced ones
  written to the server. Also the app lock, the two-step PIN, the account report, deletion, and
  dialing through a Songbird proxy.
- Options considered:
  - Settings: one JSON blob, or a record per setting on `songbird-sync` (chosen: per-key merge,
    PROTOCOL §10.2).
  - The app lock's passcode: kept, or only a salted slow hash (chosen: PBKDF2-HMAC-SHA256,
    600,000 rounds).
  - Wrong guesses: counted in memory, or saved on the device (chosen, after a first build that
    reset on restart).
  - Proxy dialing: a plain TCP relay or TLS-in-TLS with the inner session's channel binding
    (chosen).
  - The storage key when a device is removed:
    - rotate always, which strands the other devices until A2 can tell them;
    - rotate only when the main phone is left alone (chosen until A2).
- Evidence:
  - `every_synced_and_device_setting_round_trips`, `device_only_settings_are_never_records`,
    `two_devices_converge_through_the_store`, `settings_survive_a_restart` (headless, real
    engine);
  - `the_app_opens_locked_and_only_the_passcode_opens_it`, `closing_the_app_does_not_reset_the_wait`,
    `a_passcode_is_kept_only_as_a_salted_slow_hash`, `wrong_guesses_wait_longer_and_longer`;
  - `a_client_reaches_delivery_through_a_proxy_link` (e2e through the real relay to a TLS
    delivery node);
  - the storage live suite (4 of 4) and the P04/P05/P09 gates.
- Decision:
  - `backend::records`:
    - the codec tables `SYNCED` and `DEVICE`;
    - `restore`, `changed`, `schedule`, `periodic`, `under`, `put`;
    - the HTTP `Remote`;
    - `storage_key`, `adopt_key`, `rotate_key`.
  - `backend::account`: the server settings, the PIN, the export and its summary.
  - `backend::lock` and `features::lock`.
  - `crates/client`: `ProxyRoute`, with the proxy-link parser moved to `songbird-types`.
- Consequences:
  - The app lock is a gate in the app, not storage encryption: the device's own protection
    guards the app's storage (THREAT_MODEL §3.26).
  - Face ID and fingerprints come with the native host projects.

## D-0105 — Prompt 9, milestone 4: parental supervision with a per-child policy key

- Date: 2026-11-18
- Status: accepted for the milestone (agent, under the approved plan D-0101); its threat-model rows
  and the compliance table were approved by the owner on 2026-11-18 with the gate (D-0109)
- Decided by: agent (with owner approval of the plan, question 8: no parent identity or birth date
  on the server; a supervised child may delete their own account)
- Context: parental controls with in-person linking, a policy signed by the parent, server
  enforcement of what it can see (requests, discoverability, group adds), clients enforcing the
  rest, unlinking by the parent or at an age set at linking, GDPR Article 8.
- Options considered: storing the parent's account and checking that its devices sign (the server
  would learn who parents whom) or a fresh policy key per child (chosen); client rules in the clear
  (the server would learn quiet hours and the like) or encrypted under a secret shared in person
  (chosen); updates only through the child's devices (a child could withhold them) or from any
  device, authorized by the signature (chosen).
- Evidence: `a_parents_policy_locks_rules_and_only_the_parent_or_the_date_ends_it` (gate P09: the
  first policy only from the child's primary, forged 403, locks 403 `supervised` for every device,
  replay 409, the parent's update applies, the `superv` row's fields, unlinking, the date);
  `a_supervised_child_joins_a_group_only_with_the_parents_approval` (groups gate: no approval,
  another group, expired, too far ahead, another key: 403; the parent's: 200); unit tests of the
  policy encoding, client rules, unlink dates and the consent table.
- Decision:
  - `songbird_proto::supervision` (policy and approval formats); `songbird_accounts::supervision`
    (`superv`, locks `sv_*` and `grp_approval` on the account row, `put_supervision`,
    `supervisions_due`, `end_supervision`); locks checked in `set_flag` and `set_requests_off`.
  - `PUT` and `GET /v1/supervision`; `Group-Add-Approval` on commits and joins; the worker's sweep.
  - `songbird_content::supervision`: `PolicyKey` (sign, approve), `ClientRules` (sealed under the
    link secret), `link_code`, `unlink_at`, `CONSENT_AGES`.
- Consequences: the app (M6) runs the parent's and child's flows on these. The consent table must
  be confirmed by counsel before release (THREAT_MODEL §5).

## D-0104 — Prompt 9, milestone 3: sealed presence batches, nudges in an attention class

- Date: 2026-11-18
- Status: accepted for the milestone (agent, under the approved plan D-0101); its threat-model rows
  were approved by the owner on 2026-11-18 with the gate (D-0109)
- Decided by: agent (with owner approval of the plan)
- Context: encrypted presence and last seen to allowed contacts through the ephemeral lane, a
  batched fan-out reusing the send path, debouncing and rate limits, invisible sending nothing; a
  nudge in an attention class rate-limited per pair (D-0101 questions 3 and 4).
- Options considered: identified presence on the WebSocket (the server would see each user's
  contact list at every change) or sealed batches over HTTP (chosen, owner's answer to question
  3); presence budgets per recipient account (one Valkey call per account, 500 per batch) or per IP
  range counted per device (chosen; the access key already limits who can address whom); last seen
  stored on the server or carried only in ephemeral presence (chosen, question 4).
- Evidence: `presence_reaches_only_contacts_and_invisible_sends_nothing` and
  `nudges_are_held_to_their_budgets_and_blocks_drop_them` (`tests/e2e/tests/gate_p09.rs`, a
  delivery node, `api`, the registry and `valkey-hot`); proptests `invisible_never_sends_anything`
  and `sends_are_debounced_and_the_last_state_is_always_sent`; `presence_batch_latency`: a
  1,024-device batch (205 KiB, 16 devices online) p50 3.2 ms, max 10 ms over 19 runs on an Apple
  M5 Pro, compose stack, in-process router.
- Decision:
  - `PUT /v1/ephemeral/sealed` (`songbird_proto::ephemeral::SealedBatch`) in `api`'s profiles,
    behind the sealed-sender flag; the directory snapshot for keys; `Submit::ephemeral` routes
    through the router `api` already has (publish-only Valkey in production).
  - Client frame `0x04 Attention`; `Gate::attention` (budgets `q:n:` per pair, `q:m:` per
    sender; blocks); `Client::attention`; metrics `delivery_attention_routed`/`_dropped`.
  - `Content.presence` (17) and `Content.attention` (18); `songbird_content::presence::Presencer`
    with a 5 s debounce, reply once per contact per minute, last seen to the minute, invisible
    silent; the stored `Nudge` kind superseded.
- Consequences: the app (M6) runs `Presencer` and sends sealed batches with its contacts' keys,
  and nudges over its connection. Revisit the per-range budget with real NAT sizes.

## D-0103 — Prompt 9, milestone 2: discoverability, group adds from contacts, the profile's contents

- Date: 2026-11-18
- Status: accepted for the milestone (agent, under the approved plan D-0101); its threat-model rows
  were approved by the owner on 2026-11-18 with the gate (D-0109)
- Decided by: agent (with owner approval of the plan)
- Context: the prompt's server-enforced settings (username discoverability, who may reach you,
  inactivity, plan), "who can add me to groups" (D-0101 question 6), and statuses and the about
  line in the encrypted profile with a "profile changed" signal.
- Options considered: discoverability off hiding the username from links too (breaks the links
  and QR codes the user handed out) or only from lookups by name (chosen: a lookup through one of
  the account's own link handles still resolves); group adds gated per added account with a key
  each, or one XOR of all gated accounts' keys as broadcasts do (chosen: one answer whichever
  account was missing); a server default of "contacts" for group adds (would refuse adds for every
  existing account and test) or "everyone" with the app setting "contacts" at registration
  (chosen).
- Evidence: `a_hidden_account_is_found_only_through_its_own_link` (usernames live: 404 as for an
  unknown name, a wrong link 404, the own link 200, a linked device 403 when loosening);
  `group_adds_from_contacts_only_need_the_access_key` (groups gate, 13 of 13 pass: no key and a
  wrong key 403 with nothing stored, the right key 200, a join request exempt, "everyone"
  unchanged); `profiles_round_trip_bound_to_their_version`, `a_status_disappears_at_its_expiry`,
  `fields_are_bounded_and_a_full_profile_fits_the_server_limit`.
- Decision:
  - `account.hidden` and `account.grp_contacts`; `GET /v1/account/settings`,
    `PUT /v1/account/discoverable`, `PUT /v1/account/groups`; loosening by the primary only, in
    the transaction; the directory snapshot carries `groups_contacts`.
  - Lookups take an optional `link` handle; one SurrealQL statement of the same work in every case.
  - Commits take `Group-Add-Access-Key`; exempt: the committer's account and accounts with a
    pending join request.
  - `songbird_content::profile`: `ProfilePlain` (name, about, status with emoji and expiry,
    avatar, links), sealed with AES-256-GCM-SIV under a key from the profile key, the version in
    the AAD, padded to 256 bytes; `Content.profile_changed` (tag 16).
  - THREAT_MODEL §4 lists every server-enforced setting; plan is reserved for Prompt 11, parental
    supervision for M4.
- Consequences: the app (M5) sets "contacts" at registration, sends `profile_changed` after
  edits, and adds the header when it adds contacts to groups. M4 makes a supervised account's
  rules unloosenable.

## D-0102 — Prompt 9, milestone 1: the account record store, with a manifest and a hybrid logical clock

- Date: 2026-11-18
- Status: accepted for the milestone (agent, under the approved plan D-0101); its threat-model rows
  were approved by the owner on 2026-11-18 with the gate (D-0109)
- Decided by: agent (with owner approval of the plan)
- Context: settings, chat preferences and contacts must sync across devices without the server
  seeing them, converge whatever order devices sync in, and survive a server that misbehaves
  (D-0101 questions 2 and 10).
- Options considered: per-record last writer wins alone (simpler; the server can withhold or roll
  back records unseen); Signal's storage service (fresh IDs, an encrypted manifest that only moves
  forward); a manifest listing every ID (rewritten whole at each write: 160 KiB at 10,000
  records). Chosen: fresh IDs per write and a **small** manifest holding the version and a digest
  of the ID set (76 bytes), written in the same compare-and-swap; the ID list is read separately
  only when the version moved.
- Evidence:
  - `crates/sync/tests/convergence.rs`: proptest of 2 to 4 devices, ±1 h clock skews, offline
    edits, deletions and random sync orders (512 cases): every device ends with the newest edit
    of each key by stamp; a clock days ahead never pins a key; a delete and an offline edit
    converge; tombstones go after 90 days and stale edits do not come back; withheld, swapped
    and rolled-back records are refused; a key rotation rewrites everything.
  - `services/api/tests/storage_live.rs` (SurrealDB, `valkey-hot`, `valkey-jobs`, `RustFS`):
    replicas over HTTP converge; eight concurrent writers lose no edit; caps, lengths, versions,
    other accounts and pending devices are refused; objects are the account's own; deletion
    leaves nothing; latency below.
  - Latency on an Apple M5 Pro (48 GB, macOS 27.2.1), compose stack, one `api` in the test
    process: first write of 500 records 93 ms; a one-record sync p50 24 ms, p99 113 ms; a sync with
    nothing new p50 5.6 ms, p99 36 ms; a fresh device reading 501 records p50 30 ms, p99 36 ms.
    Development numbers, not the production host class.
  - Fuzz target `storage` (every server decoder; the client's record and manifest openers).
- Decision:
  - `songbird_proto::storage` (wire), `songbird_accounts::storage` (rows `srec`, `sobj`; the
    store's version and manifest on the account row), `services/api/src/storage.rs`
    (`[api.storage]`), the new client crate `crates/sync` (sans-IO: HLC, records, manifest,
    merge, sync loop, key rotation, an in-memory reference server).
  - Records padded with Padmé from **256 bytes** (the plan said 64: at 64 two small settings
    differed in size; at 256 most settings share one size); the server refuses other lengths.
  - AES-256-GCM-SIV; a record key per ID from HKDF; zero nonce for records (one key each, never
    rewritten), random nonce for the manifest.
  - HLC with a 24 h follow bound; a write always beats the key's current stamp; tombstones
    and stale local edits expire after 90 days.
  - Caps per account: 10,000 records, 16 MiB, 16 objects of 2 MiB; a write may carry 20,000
    deletions and 20,000 insertions, so a key rotation is one atomic write.
  - The storage key travels in the provisioning message version 2; rotated keys and "fetch now"
    travel in a new `storage` sync message; `settings` sync is superseded.
  - No write time is stored.
- Consequences: M2 to M6 keep everything they sync in the store; the app's settings (M5) define
  each value's encoding (PROTOCOL §10.2). Rotation on device removal is the app's step (M5).
  Revisit the caps with real usage; revisit the 24 h bound if clocks on real devices drift more.

## D-0101 — Prompt 9: the plan, and the owner's decisions it needs

- Date: 2026-11-18
- Status: accepted (owner, 2026-11-18, "Approve all": every recommendation; a supervised child
  may delete their own account)
- Decided by: owner, on the agent's plan `docs/reports/P09-plan.md`
- Context: Prompt 9 asks for encrypted settings sync, server-enforced settings, presence and
  last seen, statuses, nudges, scenes and sound stickers, contacts sync and parental controls.
  The owner widened the scope on 2026-11-18: every setting on the app's Settings screens made
  real, except those later prompts own (backups, Prompt 10; plans, Prompt 11; WhatsApp, Prompt
  14). What exists: message requests and the inactivity period (server-enforced), profiles
  (one encrypted blob), the ephemeral lane (one device per frame, identified, ungated), a
  stored `Nudge` kind, pack kinds for scenes, sounds and emoticons, and a mocked app whose
  settings are neither saved nor synced. Missing: an account record store, an HLC, username
  discoverability, a profile-changed signal, batched presence, an attention class, contacts,
  parental controls, app-core.
- Options considered and evidence: in the plan ("What Prompt 9 starts from", "Findings", "Every
  setting, and where it lands").
- Decisions asked of the owner (recommendations first):
  1. the scope inventory as written; host-only rows (biometrics, screenshot blocking, app
     switcher, passkeys) after the whole app is built;
  2. fresh record IDs per write and an encrypted manifest in the same compare-and-swap;
  3. presence sealed, batched and gated by the recipients' access keys;
  4. last seen through the ephemeral lane only, no new server state;
  5. app lock by an in-app passcode now;
  6. a server-enforced "contacts only" bit for group adds, proved by the access key;
  7. "My contacts" and "Nobody" for photo, about and status;
  8. parental controls with a per-child policy key unlinked to the parent's account, rule bits,
     a version and an unlink date, never the parent's identity or the birth date; and whether a
     supervised child may delete their own account (recommended yes);
  9. WhatsApp's defaults (contacts see last seen and online; receipts and typing on).
- Agent decisions, unless the owner objects: `crates/sync` shared with app-core; HLC with a
  24-hour clamp and 90-day tombstones; Padmé padding and write counters; AES-256-GCM-SIV under
  per-record keys; the storage key in provisioning v2 and rotated on device removal; caps
  measured in M1; custom wallpapers as storage objects; nudges identified in an attention class
  with pair and sender budgets, the stored `Nudge` kind removed; discoverability off keeps links
  and QR codes working; `SettingsSync` and `starred` sync replaced by records; the pack item
  limit lowered to 16 MiB; proxy dialing in `crates/client`; one branch and one PR.
- Consequences: M1 (the record store) starts first.

## D-0100 — Prompt 8: the security review's fixes

- Date: 2026-11-18
- Status: accepted; part of the Prompt 8 gate (`P08-gate.md`, "Security review")
- Decided by: agent, on an independent read-only review of the whole Prompt 8 diff (a separate
  agent given the code, configuration and documents, not the reports' conclusions).
- Context: no high findings; two medium, five low, three informational.
- Decision:
  1. **The post-rollout check printed other containers' secrets** (medium): it ran `docker
     inspect` whole, so LiveKit's keys and Valkey passwords on the calls hosts could reach
     Ansible's output. Now a `--format` template returns each container's name, mount sources
     and environment variable *names* only (cut at `=`), the task is `no_log`, and the name check
     accepts Docker's and podman's forms. Checked against a local container: names only.
  2. **The token move could revive a deleted device's token** (medium): during a rolling deploy a
     row still in the registry escaped deletion, and the next `api` start moved it back.
     - The worker now deletes and verifies push rows in both the push database and the registry
       (`PushStore::delete_legacy`, `count_legacy`) until every environment has moved.
     - The registry keeps the old table defined, empty once moved, so that works.
     - The move is one idempotent `UPSERT … SET sealed = sealed ?? $s`, so two `api` instances
       moving at once neither fail nor overwrite a newer registration.
     - Test: `push_registrations_live_apart_and_old_rows_move_once` (concurrent moves; a legacy
       row deleted with its device is never moved back).
  3. **Parent-folder mounts escaped the check** (low): a mount of `/`, `/etc`, `/etc/songbird`
     or `/etc/songbird/secrets` now fails it too.
  4. **The push uid was shared** (low): every Songbird container ran as 65532, so a process
     escaping another service's container could read the provider keys. Push now runs as its own
     uid (`push_uid`, 65533), which owns its secrets and TLS files. The xtask scope check stays
     lexical and advisory; the post-rollout check enforces.
  5. **Any value of `SONGBIRD_PREVIEW_BUILD` made a preview build** (low): now exactly `1`, in one
     constant (`PREVIEW_BUILD`, tested).
  6. **Monitoring could be stopped silently, and alerts carried private addresses** (low):
     - the lifecycle API is off; reloads go by `SIGHUP` after `promtool check config` and
       `amtool check-config`, and a bad configuration fails the run;
     - SMTP and webhook values are JSON-escaped;
     - alerts leave without the `instance` label (private IP:port), and summaries name the
       inventory `host`. Rule tests updated and passing.
  7. **Ring deadlines and clock skew** (low/info): the deadline is set on `api` and checked on
     `push`. Every host runs `chrony` (common role). The residual risk: a push host more than
     30 s ahead drops rings, one behind sends them late by the skew. Recorded, not changed.
  8. **The ring stream's timing in `valkey-jobs`** (info): now a threat-model §1 row.
  9. **What `SILENT` reveals** (info): the §1 row now says the server learns "a receipt or sync",
     as with Signal's `urgent=false`.
  10. **Registry credentials of `api` and `worker`** (info): the separation of the push database
     holds against the backup user and the push service. `api` and `worker` must also use
     registry users other than SurrealDB's root before production. Listed in the gate.

## D-0099 — Prompt 8, milestones 4 and 5 (first part): the iOS notification extension and on-device checks; what waits for the owner

- Date: 2026-11-18
- Status: in progress; reports `docs/reports/P08-M4.md`, `docs/reports/P08-M5.md`
- Decided by: agent, with the owner's go-ahead to download the iOS tooling and use the local
  Android SDK (2026-11-18)
- Built:
  - `apps/ios/NotifyRules` (Swift package, 6 tests): what the Notification Service Extension
    shows, from the user's preferences in the App Group, the same rules as
    `apps/ui/src/features/notify.rs`.
  - `apps/ios/NotificationService`: the extension (`chat.songbird.app.notify`), and
    `apps/ios/build-simulator.sh`, which builds the app with `nsh package --target
    ios-simulator`, compiles the extension for the simulator, puts it in the app and signs both
    ad hoc (no Apple account needed).
  - Preview builds (debug, or `SONGBIRD_PREVIEW_BUILD`) show the Calls tab's test rows and honour
    `SONGBIRD_DEMO_ARRIVALS`, to exercise notifications on devices; release builds never do.
- Checked on devices:
  - **iOS Simulator** (iPhone 18 Pro Max, iOS 27.2): `simctl push` of the exact payload our APNs
    client sends is delivered as a banner (authorization granted); the extension is installed
    and registered, but the simulator does not start service extensions for `simctl push`, so
    the extension runs only with a real APNs delivery (needs the registered bundle ID).
  - **Android emulator** (Pixel, Android 36, Google APIs): the app's notifier posts "Ana Popescu ·
    Incoming voice call" (tag `chat:1`, importance high) for a test call, read back with
    `dumpsys notification` and seen in the shade.
  - **macOS**: the notifier fires for a message and a call while the window is closed to the
    tray, and NSH hands both to the system, which refuses them: "Notifications are not allowed
    for this application" (an ad hoc signed build that was never granted permission). Allowing
    Songbird in System Settings › Notifications, or a signed build, shows them.
- Not built yet, and why:
  - The Xcode and Gradle **host projects** that register for remote pushes (APNs token, PushKit
    with CallKit, FCM, UnifiedPush, ConnectionService) and send the token to `PUT /v1/push`: a
    packaged NSH app cannot hold them (D-0094 finding 7). Real APNs needs the bundle IDs and a
    provisioning profile, FCM the Android app in Firebase `songbird-6fab1`
    (`google-services.json`): the owner's steps (D-0094 §2, §5).
  - VoIP and CallKit: PushKit does not deliver to the Simulator; needs a physical iPhone.

## D-0098 — Prompt 8, milestone 6: desktop notifications decided on the device; close to the tray; the real login item

- Date: 2026-11-18
- Status: accepted (agent, under D-0094's approved plan, finding 10); report `docs/reports/P08-M6.md`
- Decided by: agent
- Context: desktops use no push service (D-0094): the app keeps its connection while it runs.
  The app had the tray icon and the settings switches, but nothing raised a notification, closing
  the window ended the app, and "Open at login" was a mock (NSH 0.5 had no API; 0.6 has one).
- Decision (`apps/ui`):
  - **One rule set, pure** (`features::notify::decide`): a message notifies unless its kind is
    switched off (direct, groups), the chat is muted, or the user is reading that chat with the
    window focused; reactions follow their own switches; a call rings even in a muted chat and
    even in the open chat; hidden content shows "Songbird" and "New message" / "Incoming call"
    only; no sound when sounds are off. The mobile notification extensions use the same rules
    (M4, M5).
  - **Decided at arrival.** The store tells the notifier at once (`Store::arrive`,
    `on_arrival`), not through an effect: an effect runs later, with the chat and settings as
    they are then (a headless test caught a muted chat notifying because it was unmuted before
    the effect ran), and sees only the last of two messages in one batch.
  - The notifier asks for permission once, keeps the badge at the unread count, and opens the
    chat (and the window) a pressed notification is about.
  - **Close to the tray:** while the tray icon shows, closing the main window hides it
    (`close_on_press: false`, `on_close_press` → `hide`), and the app stays connected; quit from
    the tray menu or the app menu. Without the icon, closing works as the system's does. The tray
    choice is remembered.
  - **Open at login** through NSH 0.6 (`set_launch_at_login`, `launch_at_login`), read again
    whenever the switch shows; a refusal puts the switch back with a toast.
  - Dev-dependencies `nsh-engine` and `nsh-testing` 0.6.0 (the same release as `nsh-core`, NSH's
    own test harness) for tests on the real engine, headless.
- Evidence: 121 app tests pass; six rule tests (`a_message_notifies_with_its_chat_and_text`,
  `muted_chats_stay_quiet_but_calls_still_ring`, `each_switch_turns_its_kind_off`,
  `the_chat_being_read_is_not_notified_but_a_background_one_is`,
  `hidden_content_shows_no_name_and_no_text`, `sounds_follow_the_setting`) and one on the engine
  (`the_store_s_events_reach_the_notifier`: a message from someone else notifies; the user's own,
  one in a muted chat and one in the chat being read do not; two in one batch both notify; a ring
  does).
- Not verified: a notification on screen from the running app on macOS, Windows and Linux (the
  headless engine does not capture system notifications); the plan's screenshots are still to
  take.

## D-0097 — Prompt 8, milestone 3: push credentials provably in `push` alone; alert rules and monitoring

- Date: 2026-11-18
- Status: accepted (agent, under D-0094's approved plan); report `docs/reports/P08-M3.md`
- Decided by: agent, on the owner-approved plan (D-0094 §6, finding 9; question 4)
- Context: the deployed configuration carried `[common]` only, for every service; push
  credentials had no way onto a host and nothing proved where they were. Every service exports
  metrics, but nothing scraped them or alerted: the "alert on dead letters" of the runbooks had no
  metric behind it either (`QueueStats` was never exported).
- Decision:
  - **Deployment.** `roles/songbird/tasks/push.yml` writes the push service's secrets from SOPS
    (`push_x25519_key`, the push-index and job secrets, its push-database password, and the APNs
    `.p8`, FCM service account and VAPID key for the providers switched on in
    `push_providers`) as files of mode 0400, uid 65532, under `/etc/songbird/secrets/push`, mounted
    read-only into `songbird-push` alone; `push.toml.j2` is the only configuration that names
    them. `deploy/bin/secrets-init` generates the keys and leaves the provider keys `CHANGE_ME`
    (production keys are created straight into SOPS). The data role defines the `push` database
    and its three users (D-0095).
  - **Proof, three ways:** (1) `api`, `delivery`, `worker` and `calls` refuse a provider section
    or a push key in their configuration (tests in each); (2) `cargo xtask secret-scope` (in
    `ci fast`) fails if any file in `deploy/` or `config/` other than the push task and
    `secrets-init` names a push credential, or mounts the secrets directory outside the push
    container's own line (the data host may name the push database user's password, which it must
    define); (3) after every rollout, `verify-push-credentials.yml` inspects every container on the
    host: only `songbird-push` mounts the directory, no environment names a push credential, the
    files are 0400 and the push uid's.
  - **Alerts.** `deploy/monitoring/rules/songbird.yml`: `JobsDeadLettered`, `PushProviderDown`
    (retries and no sends for 10 minutes), `PushPermanentFailures`, `PushRingsTooLate`,
    `PushRegistrationsUnopenable`, `WakeUpsDropped`, `RegistryBusy`, `ServiceDown`; each names its
    runbook; unit-tested with `promtool test rules` in the CI `iac` job (image pinned by digest).
    New process-wide counters `songbird_jobs_dead_lettered_total` and `songbird_jobs_retried_total`
    (the D-0093 pattern), exported by `push` and `worker`.
  - **Monitoring.** A `monitoring` role on the bastion: Prometheus v3.5.0 and Alertmanager v0.28.1
    (pinned by digest), loopback only (reached over the operators' SSH tunnel), 30 days of
    retention, scraping every service's admin port over the private network (already open to
    `private_cidr`). The receiver is the owner's choice (email through an SMTP relay, or a webhook
    for ntfy or Matrix); until set, alerts show in Alertmanager's UI only.
- Evidence: `secret_scope` tests (the repository passes; a credential named elsewhere and the
  directory mounted into another container both fail); four configuration tests;
  `promtool test rules` SUCCESS (8 rules, 4 test groups); `deploy/bin/validate-iac ansible`
  (syntax and lint) passes in a Python 3.12 container with the pinned requirements.
- Consequences: the other services' deployed configuration is still `[common]` only (a gap
  older than Prompt 8, recorded for Prompt 13); the alert receiver waits for the owner.

## D-0096 — Prompt 8, milestone 2: outages and numbers; one reader connection per job stream

- Date: 2026-11-18
- Status: accepted (agent, under D-0094); report `docs/reports/P08-M2.md`
- Decided by: agent
- Context: the prompt asks for pushes per second, p99 from job to provider, and an outage
  simulation proving messages stay safe and pushes resume.
- Finding, and fix: the first measurement gave **~850 pushes/s**. Each part alone was fast (queue
  read and finish ~38,000 jobs/s, direct APNs sends ~38,000/s), the consume loop with no provider
  ~800/s. Cause: `Queue` had **one reader connection** for every stream; Valkey runs a
  connection's commands in order, so the ring consumer's `XREADGROUP BLOCK 250` on an empty
  `j:ring` held every `j:push` read for up to 250 ms (256 jobs a round ≈ 1,000/s). The worker had
  the same flaw since D-0036 (its cleanup reads behind its deletion reads). Now one reader
  connection per stream, opened at its first read: the consume loop went from ~800 to ~34,000
  jobs/s with no provider.
- Evidence (Apple M5 Pro, 18 cores, 48 GiB, macOS 27.2.1; compose SurrealDB v4.0.1, valkey-jobs;
  release build; mock APNs over TLS and HTTP/2 on loopback, 4 connections; consumer batch 256,
  concurrency 256; `services/push/tests/outage_live.rs` `m2_numbers`):
  - a burst of 20,000 wake-ups: **9,800 to 28,600 pushes/s** over 5 runs; the spread follows the
    local SurrealDB's load (one token lookup per job; the server held many test databases);
    enqueue→provider p99 0.51 to 1.5 s, which is the burst queueing behind itself;
  - steady 1,000 wake-ups/s, one job at a time: **enqueue→provider p50 1.3 to 1.9 ms, p99 6.5 to
    11 ms**, max 26 to 37 ms (2 runs).
  - Outages (all pass): FCM 503 for 3 s (67 requests refused thanks to the breaker; all 200
    devices woken 2.0 s after recovery); APNs black-holed (accepts, never answers; 300 ms
    timeout; breaker open; all woken after recovery); `retry-after: 1` honoured (retry ≥ 0.95 s
    later); `push` crashed mid-burst (its runtime dropped with jobs in hand): every one of 300
    devices woken after reclaim, 64 twice, nothing dead-lettered. End to end
    (`gate_m5::an_offline_device_receives_a_push_end_to_end`): APNs down while Bob writes to
    Alice's offline phone, the three messages wait in her mailbox, the wake-up goes out once APNs
    is back, she reads them in order.
- Consequences: production numbers wait for the production host class (with the D-0043 soak at
  the end of the project); the mock records requests with their arrival time
  (`RecordedRequest::at`) and counts them cheaply (`request_count`).

## D-0095 — Prompt 8, milestone 1: rings on their own stream, trailing wake-ups, `SILENT` sends, push tokens in their own database

- Date: 2026-11-18
- Status: accepted (agent, under D-0094); its threat-model §1 rows **approved by the owner on
  2026-11-18** with the Prompt 8 gate; report `docs/reports/P08-M1.md`
- Decided by: agent, on the owner-approved plan (D-0094 questions 1 and 3, findings 1 and 2)
- Decision:
  - **Rings.** `Work::Ring { device, until_ms }` on `j:ring` (sealed like every job; the push kind's
    old call flag retired and refused), a consumer of its own in `push` (`ring_concurrency`), never
    debounced. `api` gives a ring 30 s (`RING_LIFE_MS`; the offer expires after 60 s); `push` drops
    a ring past its deadline, retries one only while it can still ring in time, and never
    dead-letters one (`push_rings_expired`). On FCM and Web Push a ring is the same message as any
    wake-up; on APNs it is the `VoIP` push it was (expiry 0, empty payload).
  - **Coalescing, leading and trailing.** `crates/send` (`Coalescer`, pure) and `push`
    (`Debounce` → `Admit::{Now, Defer, Drop}`): the first wake-up in a window goes at once; any
    later one schedules a single wake-up at the window's end (`push` returns the job to the queue
    for the rest of the window). A burst is at most two identical pushes and the last message
    always has a wake-up after it.
  - **`SILENT` (`0x80`) on `Send`** (WebSocket and `PUT /v1/messages`, one encoding): stored and
    delivered as usual, no wake-up (`routes_silent`). With it, every bit of the flags byte is
    assigned: a future flag needs a new block. Test vector in `crates/proto/tests/vectors.rs`,
    PROTOCOL §1.
  - **Push tokens in their own database** (`songbird_accounts::PushStore`): table `push_token` as
    before, moved out of the registry; `api` (register, remove, GDPR export), `worker` (delete
    with devices and accounts, verify) and `push` (read, delete) each connect with
    `[<service>.push_tokens]`, required in staging and production with TLS, its own user and its
    own database; development uses `PushStore::beside` (`<registry>_push`). `push` no longer
    connects to the registry. `api` moves rows a registry still holds once at start
    (`move_from_registry`: never over a newer registration). `Remnants` lost `push_tokens`; the
    worker checks the store itself.
  - **Mocks:** `Behaviour::Hang` (accepts, never answers), `Behaviour::Reset` (stream reset), and
    `outage`/`recover` (a behaviour for every unscripted request).
- Evidence: unit and property tests (`a_burst_is_one_leading_and_one_trailing_wake_up`,
  `no_request_is_left_without_a_later_push`, `every_wake_up_has_a_job_at_or_after_it`,
  `the_retired_call_flag_no_longer_opens`, the `SILENT` vector and flag rules); live:
  `offline_devices_get_one_constant_wake_up` (leading and trailing pushes byte-identical; rings
  to the VoIP token with expiry 0; rings on FCM and Web Push equal to wake-ups; a late ring
  dropped; **a ring reaches APNs in 52 ms behind 10,000 queued wake-ups**, 9,984 still queued),
  `a_silent_send_is_stored_and_delivered_but_wakes_nobody`,
  `push_registrations_live_apart_and_old_rows_move_once`, the provider tests for hanging and reset
  requests and a black-holed APNs opening and closing its breaker.
- Consequences: a client must set `SILENT` on receipts and sync messages (app-core, A2); three
  threat-model §1 rows for the owner.

## D-0094 — Prompt 8: the plan, and the owner's decisions it needs

- Date: 2026-11-18
- Status: accepted (owner, 2026-11-18, "Approve all": every recommendation)
- Decided by: owner, on the agent's plan `docs/reports/P08-plan.md`
- Context: Prompt 2, milestone 5 (D-0036) already built `services/push` and
  `crates/push-providers`: sealed jobs, sealed and keyed-hashed tokens, APNs (alert and VoIP), FCM
  HTTP v1, Web Push with RFC 8291 and VAPID, collapse IDs, a breaker per provider, pooled HTTP/2,
  dead letters. Prompt 8 closes the gaps the plan lists:
  - calls queue behind message wake-ups on one stream;
  - the leading-edge debounce can leave a stored message without a wake-up;
  - every stored envelope wakes the device, receipts included;
  - mute on iOS needs Apple's filtering entitlement;
  - tokens sit in the registry's database;
  - nothing alerts on dead letters;
  - nothing deploys or proves where the push credentials are;
  - no numbers or outage simulation exist;
  - the NSH app cannot receive remote pushes as packaged (NSH 0.6 has local notifications only);
  - desktop has no notifications while the window is closed.
- Options considered and evidence: in the plan ("What Prompt 8 starts from", "Findings").
- Decisions asked of the owner (recommendations first):
  1. an `urgent` flag on sends, so receipts and sync wake nobody (one bit per send, never stored);
  2. register the bundle IDs `chat.songbird.app`, `chat.songbird.app.notify` and the App Group,
     then request `com.apple.developer.usernotifications.filtering` (the account is an Individual
     enrolment: the owner's name goes on the app);
  3. a SurrealDB database of its own for push tokens, a credential per service, left out of
     backups;
  4. Prometheus and Alertmanager on the bastion with CI-tested rules, and where alerts go;
  5. the app embedded in Songbird's own Xcode and Gradle hosts (`apps/ios`, `apps/android`) for the
     notification extension, PushKit with CallKit, FCM, UnifiedPush and ConnectionService; the
     Android app registered in Firebase `songbird-6fab1`.
- Agent decisions, unless the owner objects:
  - `j:push-call` with a 30 s life;
  - leading and trailing coalescing;
  - call pushes identical to message pushes on FCM and Web Push;
  - the debounce stays per process;
  - two new mock behaviours;
  - three checks that prove the credentials are only in `push`;
  - desktop uses no push service (no APNs on macOS, no WNS on Windows): the app stays connected
    in the tray and notifies locally, through NSH 0.6's tray, hidden window, login item and
    notifications; Linux UnifiedPush is future work;
  - real-provider tests read key paths from the environment and skip without them;
  - one branch and one PR for every milestone.
- Consequences: M1 starts. M4 and M5 also wait for the owner's Apple and Firebase steps; M3
  leaves the alert receiver to configure until the owner chooses it. Production provider keys are created at deployment, straight
  into SOPS.

## D-0093 — Registry write conflicts: jittered retries within a time budget, then `Busy`

- Date: 2026-11-18
- Status: accepted
- Decided by: agent (root-cause fix of a CI flake; the owner asked for no retries hiding faults and
  no weakened assertions)
- Context: `racing_token_requests_never_exceed_the_device_limit` (40 concurrent
  `create_link_token` over eight connections, limit 6) failed in CI runs 43753509213 and
  44192622152 with `surrealdb statement failed (TransactionConflict)`. Every registry transaction
  that loses a SurrealDB write conflict was retried **at most 8 times with a fixed backoff**
  (4, 8, 16, 32, 64, 128, 128, 128 ms: about 0.5 s), copied into nine loops. The failing request
  lost 9 times in a row.
- Why a call can lose more often than its competitors commit (source of SurrealKV 1.2.0, the
  version SurrealDB v4.0.1's `Cargo.lock` pins, crate checksum `bb696686…f8264e` verified):
  - At commit, `ring::pipeline::validate` checks each written key (and locked read) against every
    commit entry with a lower ring sequence that wrote it and that the transaction's snapshot
    does not see (`ring::index::WriteIndex::conflicts`). An entry is ignored only once it has
    **aborted** or is visible to the snapshot (`CommitEntry::is_ignorable_at`): entries **in
    flight** (still validating; they may yet abort) and **accepted but not yet durable** (waiting
    for the group commit's fsync, `sync=every`) count as conflicts.
  - So a transaction can lose to another loser, and a retry that starts while the winner waits
    for its fsync takes a snapshot that still does not see it and loses to it again. Losses per
    call grow with commit latency (disk, CPU), not with the number of winners.
  - The same rule is what keeps limits exact: two racing grants both write the account row
    (`dgen`), so the later to claim a sequence finds the earlier one (in flight, accepted or
    committed after its snapshot) and aborts. It may abort both, never admit both.
- Evidence (Apple M5 Pro, macOS 27.2.1; compose SurrealDB v4.0.1 on SurrealKV `sync=every` under
  podman; a temporary probe, not committed):
  - Refused requests write nothing: after the race the account's `dgen` grew by exactly 5 (the
    grants); the 35 refusals (`THROW` in the transaction) cancel and never reach the commit ring.
  - With the VM's CPUs saturated (32 busy loops), calls lost **6** times against 5 winners;
    unloaded, the old code lost at most 4. CI's slower runner reached 9.
  - Per-call latency to an answer under that load (80 races, 3,200 calls): slowest 337 ms with a
    4 s budget, 506 ms with the final 1.5 s budget; never `Busy`.
- Decision:
  - One `Backoff` for every registry retry loop (`crates/accounts/src/lib.rs`): **full jitter**
    (a uniform wait up to a cap of 2 ms doubling to 128 ms), **at most 20 attempts and 1.5 s**
    (about 4.5 times the slowest call measured under CPU starvation, 3 times what CI ran out of).
    A smaller budget bounds the extra load retries put on SurrealDB under contention.
  - When the budget is spent the call answers **`RegistryError::Busy`** instead of a raw
    database error: nothing was written and the caller may retry. The API already answers every
    registry database failure with 503, so clients see no change.
  - **Degraded mode made explicit** (invariant 8): `songbird_registry_conflict_retries_total`
    and `songbird_registry_busy_total` (process-wide counters, no labels) on `api`, `worker`,
    `push` and `delivery`, through a new `Metrics::register_counter_fn` (names checked against
    the redaction rules like any other); a warning log without identifiers on each `Busy`; the
    runbook "Registry busy" in `docs/RUNBOOKS/jobs-push-api-worker.md`.
  - Rejected: mapping the first conflict to an API error (pushes a normal outcome of optimistic
    concurrency onto clients, who would retry less well); loosening the test (it must keep
    asserting exactly 5 grants and no other error); serialising per account in the process (does
    not hold across `api` instances); reordering the account-row write after the checks (measured
    to change nothing: refused transactions never commit).
- Evidence for the fix: unit tests `backoff_tests` (caps; attempt limit; time limit; ends in
  `Busy` within the budget plus one wait, counted) and the telemetry test for counters read at
  scrape; the `devices_live` suite 60 of 60 runs; the whole `songbird-accounts` crate 10 of 10
  runs with the VM's CPUs saturated; 80 starved races all granted exactly 5.
- Consequences: a call under heavy contention takes at most ~1.6 s before `Busy` (was ~0.5 s
  before an error). The per-account throttles in `api` (for example `tokens_per_hour`,
  `token_burst` for link tokens) keep one account from reaching this in production. The
  mailbox's own retry loops (D-0027) are unchanged. Revisit the budget if
  `songbird_registry_busy_total` moves in production, or when the registry moves to TiKV.

## D-0092 — The app UI (A1–A15) on NSH in Rust, over a mocked app-core, with a first real-backend slice

- Date: 2026-11-17
- Status: proposed (waiting for the owner, with the A1 design gate)
- Decided by: agent (owner's request: "build the whole NSH app in one shot … what is not implemented just mock it")
- Context: the app prompt set (A0–A15) needs a UI before app-core (A2) exists. The owner asked for every
  screen now, mocked where needed, premium in look, and connected to the backend where it already can be.
- Options considered: NSH TSX vs NSH Rust (A0: "NSH for all UI, written as Rust components"); one shared
  checkout for parallel agents vs isolated APFS clones merged by the lead; wiring the backend through a
  copied `songbird-api` client vs the backend's own small crates.
- Evidence: `apps/ui` builds with `cargo check --all-targets` and no warnings; `cargo test --lib` 93 tests;
  `nsh test Screens.test.tsx --target macos` (and the per-area `Polish_*`/`Screens_*` tests) produce the
  screenshots in `apps/ui/shots/`; `docs/design/tools/contrast.py` reports 0 failing colour pairs in light,
  dark and both high-contrast themes. A cross-check (scratch crate linking `songbird-api`) confirmed the
  server's own code parses the app's request signatures and verifies its registration request
  (PoW, challenge tag, both key signatures).
- Decision:
  1. **NSH 0.5 Rust mode**, `apps/ui` as its own Cargo workspace (`nsh-app-songbird`), NSH crates at the CLI's
     version. Design system in `theme.json` (one token source; light, dark; high contrast and forced
     light/dark built from it at run time, `src/theming.rs`) and an app `ui` layer (`src/ui`) every feature
     composes; Lucide icons (ISC) inline; mock profile photos generated per contact (`ui/portrait.rs`).
     Palette: **Dusk violet** (owner's choice over Ink & Gold, Robin and the first green-teal), so the app keeps
     WhatsApp's familiar structure without its look (green, mint/teal bubbles, beige wallpaper, blue ticks).
  2. **IA**: Chats · Calls · Communities · You (WhatsApp's without Status); phones a native stack over a
     tab bar, wide windows a rail + list pane + detail stack (`src/shell.rs`). Pending owner approval (A1).
  3. **app-core is mocked** in `src/store` (seeded data, timers for ticks, typing and calls). Each mock is
     marked `// mock:` with the real app-core call. No UI copy claims a property the mock lacks.
  4. **Real backend now** (`src/backend`, opt-in under You → Server connection): registration (challenge,
     Argon2id PoW on a blocking thread, Ed25519 identity and device keys, invite), username claim (hash and
     proof only), devices, invites, GIF search, account deletion, and the delivery WebSocket
     (`crates/client`) with live status. Envelopes are counted and **never acknowledged**, so nothing is lost
     before app-core can decrypt them. New dependencies: the backend's own `songbird-types/-proto/-pow/-keys/
     -client` by path (wire formats cannot drift), `aws-lc-rs` (as the workspace), `serde_json` (theme
     variants, JSON answers). Not `songbird-api` (a service crate with database clients).
  5. **Invite codes and usernames follow the server**: 24 Crockford symbols; usernames 3–32, case kept as
     typed, unique case-insensitively (PROTOCOL.md §3).
- Consequences: device keys live in NSH app storage in development (marked mock); they move to the Secure
  Enclave / Android Keystore with app-core. Message content, profiles, groups (MLS), attachments, calls
  media, backups and settings sync stay mocked until A2 and Prompts 8–11. `scripts/dev-app-backend.sh`
  runs api and delivery against the compose stores for the app; on the agent's macOS host Podman's port
  forwarding was too slow for SurrealDB (13 s per WebSocket upgrade), so registration was verified against
  the server's code rather than a running server. Revisit: the A1 gate (design, IA, brand, usability
  tests), the native modules NSH lacks (`apps/ui/DESKTOP_GAPS.md`), and libsignal's licence for app-core
  (D-0047).


## D-0091 — Prompt 7: the security review's fixes

- Date: 2026-11-17
- Status: accepted; part of the Prompt 7 PR (report `P07-gate.md`, "Security review"); approved by the owner with the gate on 2026-11-17
- Decided by: agent, on an independent read-only review of the whole Prompt 7 diff (a separate
  agent given the code and documents, not the reports' conclusions)
- Context: the review found one high, four medium and seven low findings, and three for
  information.
- Decision:
  1. **A removed member could rejoin with a token it still held** (high). LiveKit does not revoke
     tokens on `RemoveParticipant`, and refreshes connected participants' tokens itself. The
     reconcile now writes the removed identities to the room's **deny set** (`cl:x:`, pseudonyms
     only, the call's life) before disconnecting them; the `calls` service disconnects any denied
     identity again on its `participant_joined` webhook. Test: the group gate test reconnects Bob
     with his old token and shows him cut off. The MLS keys already denied him the media.
  2. **The group-call claim could split a group across two rooms** (medium). The call's record is
     written before the pointer is claimed; a pointer younger than 15 s is trusted without asking
     LiveKit; pointers are released by compare-and-delete (Lua); each join refreshes a live call's
     records, so a call longer than a day keeps its pointer.
  3. **A stranger's call to an open account sent a `VoIP` push** (medium), which rings on iOS.
     Only a caller presenting the access key causes wake-ups now, and a (caller, callee) budget
     (20 an hour, burst 6) stops one caller spending a callee's whole wake budget. The block check
     for open accounts became unnecessary and was removed.
  4. **One device could fill a link's lobby, and declines kept their places** (medium). One place
     per device (asking again returns it), declines delete at once, and the add and decide steps
     are single Lua scripts (no lobby recreated without a TTL after a revoke).
  5. **Missing budgets** (medium): group joins and every link ticket now spend the ticket budget;
     every link request spends the poll budget first, whatever it claims to be.
  6. Low: read-only listeners no longer get `can_publish_data`; ringing is capped at 2 minutes
     from now even if the sender's clock claims the future; a webhook whose cleanup failed is
     unmarked so LiveKit's retry runs it; a link revoked while its room was being opened gets the
     room closed again; the reconcile tries three times and counts failures
     (`api_call_reconcile_failed`, runbook).
  7. Deployment: the room-service tunnel admits only the edge hosts' `api.<fqdn>` certificates;
     the firewall opens 7880 to edge hosts and LiveKit's Valkey to calls hosts only; LiveKit runs
     with every capability dropped but `NET_BIND_SERVICE`; TURN refuses to relay to private,
     loopback, link-local and CGNAT ranges (`deny_peer_cidrs`).
- Not changed, recorded:
  - The public signalling front also forwards LiveKit's room-service API. Every call to it needs
    a token signed with the LiveKit secret (LiveKit's design; its cloud exposes the same); the
    private tunnel is defence in depth for `api`'s path, not the only guard.
  - Valkey's password is on its command line, as for `valkey-hot` and `valkey-jobs` (D-0034);
    containers on the host see it, nobody else. Revisit for every Valkey together.
  - Lobby cards are not signed: apps show a joiner as unverified until the Signal session and,
    for strangers, the safety numbers (PROTOCOL §8.7).
  - 1:1 audio calls may turn the camera on (WhatsApp does); only group audio calls are
    microphone-only, for their larger cap.
- Consequences: one more threat-model §1 row (the deny set); PROTOCOL §8.6–§8.8 updated.

## D-0090 — Prompt 7, milestone 5: per-host capacity, bandwidth per mode, and the caps kept

- Date: 2026-11-17
- Status: accepted by the agent; part of the Prompt 7 PR, approved by the owner with the gate on 2026-11-17
- Decided by: agent, within the plan the owner approved (D-0085 §3: the caps may move with the
  numbers)
- Context: the prompt asks for LiveKit's load tester over rooms × participants, CPU and bandwidth
  per participant, behaviour under loss and latency, bandwidth per low-data mode, and a sizing
  table.
- Evidence: P07-M5 (eight runs on a LiveKit container capped at 4 CPUs; `lk` v2.19.2).
- Decision:
  1. **The caps stay at 32 (video) and 128 (audio-only).** A full 32-person video room used 0.29
     cores and 71 Mbit/s; a 128-person audio room 1.07 cores and 356 MiB: both fit a 4-vCPU host
     several times over. Larger caps would be bandwidth for people, not for the SFU; the owner's
     WhatsApp-parity choice holds.
  2. **Low-data mode** (PROTOCOL §8.9) costs a receiver about 0.15 Mbit/s per camera instead of
     0.6–0.9 Mbit/s, about 4× less, and halves the SFU's CPU.
  3. **Sizing:** about 170 concurrent 1:1 video calls, 350 low-data, 900 audio, 9 full video rooms
     or 2 full audio rooms per 4-vCPU host, at 70 % CPU and 700 Mbit/s; two calls hosts for the
     beta.
- Consequences: re-run the load tests from a separate host against the production `calls` class
  before launch, and replace the table with those numbers.

## D-0089 — Prompt 7, milestone 4: LiveKit on its own Valkey, TURN/TLS on a second address, an E2EE harness outside the workspace

- Date: 2026-11-17
- Status: accepted by the agent; part of the Prompt 7 PR, approved by the owner with the gate on 2026-11-17
- Decided by: agent, within the plan the owner approved (D-0085 §4)
- Context: the prompt asks for a multi-node LiveKit through its Redis-protocol store (Valkey
  compatibility verified), TURN over TLS on 443, recording and egress disabled, and a harness on
  LiveKit's Rust SDK with E2EE that proves calls connect, wrong keys fail and keys rotate.
- Evidence: P07-M4 (two nodes on Valkey 9.1.3; the harness output); LiveKit `v1.13.11` source
  for `turn.bind_addresses`, `redis.tls` and `webhook.filter_params`.
- Decision:
  1. LiveKit listens on loopback; apps reach signalling through a TLS front on the public address
     (443, or 8443 beside the edge role), `api` reaches the room service through a mutual-TLS
     tunnel on the private address.
  2. A dedicated Valkey on the first calls host, mutual TLS, no persistence (routing only).
  3. TURN/TLS on 443 and TURN/UDP on 3478 of a floating IPv4 per calls host; 5349 on the public
     address where there is no second one.
  4. `auto_create: false`; webhooks to the local `calls` for six events only.
  5. `tools/calls-harness` outside the workspace (`+crt-static`, its own lock file), run before
     releases.
- Consequences: two new secrets (`livekit_valkey_password`, `calls_secret`), one new certificate
  component (`valkey-livekit`), a floating IP per calls host; public certificates and DNS for
  `calls_domain` and `turn_domain` wait for the owner's domain choice.

## D-0088 — Prompt 7, milestone 3: call links with no account, lobbies bound to a device, webhooks once

- Date: 2026-11-17
- Status: accepted by the agent; part of the Prompt 7 PR, approved by the owner with the gate on 2026-11-17
- Decided by: agent, within the plan the owner approved (D-0085)
- Context: the prompt asks for call links as random secrets stored as hashes, with expiry and an
  optional lobby where an admin admits people, and for signature-verified LiveKit webhooks used
  only for cleanup and capacity metrics.
- Options considered:
  - **Link storage**: `valkey-hot` (no persistence: links would vanish on restart) or SurrealDB.
    Chosen: SurrealDB for links (days), `valkey-hot` for lobbies (minutes).
  - **Who is admin**: the creating account (the server learns who made which link) or a token
    (anonymous, like packs, D-0079 §3). Chosen: a token; account deletion cannot find links, which
    expire within 30 days.
  - **Lobby redemption**: by handle alone (an admin who sees handles could redeem one for another
    device) or bound to the waiting device's keyed tag. Chosen: bound.
  - **Webhook metrics**: gauges (go negative after a restart) or counters. Chosen: counters.
  - **Room creation on join**: LiveKit's default `auto_create: true` lets a token for a finished
    room recreate it without our caps. Chosen: `auto_create: false`; liveness by `ListRooms`.
- Evidence: P07-M3 (live tests, a real LiveKit webhook replayed through the receiver).
- Decision: PROTOCOL §8.7, §8.8; links default to a day, at most 30 days; lobbies 64 people, 10
  minutes; webhooks within 5 minutes, once per event ID (an hour).
- Consequences: three threat-model §1 rows for approval (call links, lobbies, webhook marks);
  every LiveKit configuration gets `auto_create: false` and a webhook to its local `calls` (M4).

## D-0087 — Prompt 7, milestone 2: tickets with the least grants, pseudonyms per room, removal by reconcile

- Date: 2026-11-17
- Status: accepted by the agent; part of the Prompt 7 PR, approved by the owner with the gate on 2026-11-17
- Decided by: agent, within the plan the owner approved (D-0085)
- Context: `api` must admit only the caller and the callee's devices to a 1:1 call, only the
  current roster to a group call, and drop a removed member mid-call; LiveKit's state should
  reveal as little as possible.
- Options considered:
  - **Where calls are authorized**: in `api` (it holds the registry, the directory snapshot and
    device authentication) through the `songbird-calls` library, or in the `calls` service
    behind an internal RPC. Chosen: `api`, in process; the LiveKit secret lives in both. A second
    hop would add latency to ringing and a new internal protocol for no separation that matters:
    `api` already holds every other user-facing secret.
  - **Participant identity**: the device ID (links everything), a random per-join ID (needs a
    table to remove a device later), or `HMAC(k, room ‖ device)` (stable in a room, unlinkable
    across rooms, recomputable). Chosen: the HMAC, 16 bytes in hex.
  - **Removal mid-call**: a table of who joined (more metadata), or reconcile the room against the
    roster after each commit. Chosen: reconcile.
  - **Dependencies**: `livekit-api` with `access-token` and `webhooks` only (`livekit-token`,
    `jsonwebtoken` with an HMAC-only provider, `livekit-protocol`, `pbjson-types`); the
    `services` feature would bring `reqwest` and rustls on `ring`. About 110 crates in its tree,
    almost all already in the workspace; recorded for `cargo vet`.
- Evidence: P07-M2 (live tests against LiveKit `v1.13.11`; unit tests).
- Decision: PROTOCOL §8.6; `[api.calls]` required when deployed; tokens 2 minutes; budgets 60
  calls an hour per account, 60 wake-ups an hour per receiving account, 600 tickets an hour per
  device.
- Consequences: five threat-model §1 rows for approval (live 1:1 call, live group call,
  LiveKit's state, call budgets, the LiveKit secret). M3 deletes records when LiveKit reports a
  room finished.

## D-0086 — Prompt 7, milestone 1: per-sender call keys from Signal and MLS; Signal-style link keys

- Date: 2026-11-17
- Status: accepted by the agent; part of the Prompt 7 PR, approved by the owner with the gate on 2026-11-17
- Decided by: agent, within the plan the owner approved (D-0085)
- Context: LiveKit's E2EE takes a key per participant identity and key index (a ring of 16) and
  derives its frame keys from it; the prompt asks for 1:1 keys from the Signal session and group
  keys from the MLS exporter per epoch, rotated on every membership change, with test vectors.
- Options considered:
  - **one shared key per epoch** (LiveKit's "shared key" mode) versus **a key per sender derived
    from the epoch secret and the identity**. The frame cryptor's IV is SSRC, timestamp and a
    counter; with one key for everyone, two senders whose SSRCs collide reuse IVs under AES-GCM.
    Per-sender keys cost one HKDF per participant per epoch and remove the risk. Chosen.
  - **when to switch sending after a commit**: at once (receivers may not have the key yet, so
    frames drop) or after a delay (a removed device decrypts a little longer). Chosen: install at
    once, send with the new index 1 second later; the server's removal from the SFU (M2) closes
    that second.
  - **link calls**: Signal's design (the owner's choice); cards with account, device, identity
    and an optional name, sealed under a key from the link's root with ChaCha20-Poly1305 (the
    cipher already used for content-adjacent sealing in this crate's attachment suite 1), 512 bytes
    at most.
- Evidence: P07-M1 (tests, vectors, independent Node.js check, protox check, OpenMLS test).
- Decision: PROTOCOL §8.1 to §8.5 as written; `songbird_content::call` is the reference.
- Consequences: apps hand LiveKit sender keys, never a shared key; a group call's keys follow the
  MLS epoch; the server stores only `SHA-256` of a link ID (M3). Revisit if LiveKit adds SFrame
  with its own key schedule.

## D-0085 — Prompt 7: the plan, and the owner's decisions it needs

- Date: 2026-11-17
- Status: accepted (owner, 2026-11-17, every recommendation)
- Decided by: owner, on the agent's plan `docs/reports/P07-plan.md`
- Context: `services/calls` is empty; LiveKit `v1.13.11` runs in compose and Ansible with no
  store, TURN/TLS 443 or webhooks. Prompt 5's roster and `call` role exist; Prompt 3's access key
  proves contacts; the `VoIP` push path exists but nothing uses it.
- Options considered and evidence: in the plan ("Findings").
- Decisions (the agent's recommendations, all accepted by the owner):
  1. A 1:1 call names the callee and carries the contact proof (or the callee is open); the two
     accounts stay in `valkey-hot` for the call's life only (at most 24 h), never in SurrealDB or
     logs; the server may queue `VoIP` and high-priority wake-ups for it.
  2. Call links use Signal's design: identities exchanged over the link's encrypted channel,
     per-sender keys over Signal sessions, rotated on every join and leave; joining needs an
     account.
  3. Caps: 32 participants in a video call, 128 audio-only; configurable.
  4. LiveKit gets a dedicated Valkey on the calls hosts, and each calls host a second public IPv4
     for TURN/TLS on 443.
  5. Agent decisions, not objected to: `livekit-api` for tokens and webhooks only, with Songbird's
     own Twirp client on aws-lc-rs; the E2EE harness outside the workspace; LiveKit's `lk` CLI as
     the load tester; tokens valid 2 minutes.
  6. One branch and one PR for every milestone (the owner, 2026-11-17).
- Consequences: who calls whom becomes server-visible for the call's life (a threat-model row for
  approval with M2); the `turn.` name under `songbird.chat` and the second IPv4 are the owner's
  before deployment; M1 starts.

## D-0084 — Prompt 6: the security review's fixes

- Date: 2026-11-17
- Status: accepted; part of the Prompt 6 PR (report `P06-gate.md`, "Security review")
- Decided by: agent, on an independent read-only review of the whole Prompt 6 diff. The
  reviewer was a separate agent, given the code and documents, not the reports' conclusions.
- Context: the review found no high-severity issue. It found three medium findings, four low
  and two for information.
- Decision:
  1. **One recipient could confirm every fetch and delete a file early** (medium).
     Confirmations need no account, so a recipient holding the object ID could send N of them.
     - Each source now counts once per object: an IPv4 address, or an IPv6 /64. The record is
       keyed in `valkey-hot` for the attachment's lifetime.
     - Recipients sharing an address count as one. Their file is then kept until 30 days, which
       is the safe direction.
     - Per-device receipts were rejected: a group message is one MLS ciphertext for every
       member, so it cannot carry a receipt per device.
     - Test: `fetches_are_confirmed_and_the_last_one_deletes` (sources 127.0.0.1 and
       127.0.0.2; three confirmations from one source count once).
  2. **A retried completion could delete a completed attachment's object** (medium).
     - `Registry::finish_upload` returns completed, already an attachment, or no live grant.
       Only the last deletes what was stored.
     - A grant must also be unexpired to complete, so the worker's sweep of expired grants
       cannot race a late completion.
     - Test: `resumable_grants_record_one_upload_and_slide_to_a_deadline`.
  3. **The abandoned-upload sweep saw only the first 1,000 uploads** (medium).
     `list_multipart_uploads` now pages with `NextKeyMarker` and `NextUploadIdMarker`, up to
     100 pages. Tested in `multipart_documents`.
  4. **Unauthenticated GIF file requests could open the search breaker** (low/medium).
     - File fetches no longer move the breaker, have their own provider budget (`m:gm`), and
       run at most 32 at a time per node.
     - Searches count only transport errors, timeouts, 5xx and 429.
     - Test: the live GIF test fetches a removed GIF eight times, then searches successfully.
  5. **Part rewrites were not bounded** (low). Each grant allows at most 2 × parts + 8 part
     writes, and each pack 2 × objects + 8, counted with compare-and-set before writing.
     Retries pass; endless rewrites get 409. Tests: `parts_can_be_retried_but_not_rewritten_forever`
     and the pack test.
  6. **A pack upload racing a deletion or expiry left an orphan** (low). After writing, the
     upload checks that the pack still exists and deletes the object if it does not. A pack
     completed meanwhile keeps the object: that race is the creator's own, and deleting would
     break a finished pack.
  7. **A signed URL could be replayed without limit for 10 minutes** (low). The media path now
     has a per-IP-range budget: 36,000 an hour, burst 3,600, enough for a player's range
     requests.
  8. **`[api.media]` was not required when deployed** (low). It is now required in staging and
     production. Without it there are no budgets, downloads, packs or GIFs.
  9. **For information:**
     - the GIF base URL's host is now parsed and compared exactly;
     - `part_size` must keep uploads within 1,000 parts, the list the server reads.
- Not changed, recorded: the attachment `digest` commits to the key, not to the ciphertext.
  - Objects are immutable once complete: single-use grants, and pack objects writable only by
    their creator before completion. So no one but the uploader can change content under a
    pointer, and only before anyone could fetch it.
  - A ciphertext hash would cost every app a second pass over every file.
- Consequences: the threat model's §1 download row and §3.23 now say how confirmations count;
  PROTOCOL §7.3 and §7.5 likewise.
- CI (2026-11-17): `cargo audit` began failing on two "unmaintained" notices published that day,
  RUSTSEC-2026-0542 (`humantime`) and RUSTSEC-2026-0543 (`chrono`), both pulled in by the Iggy
  SDK (`iggy_common` 0.12.0, D-0030) and present on `main` before Prompt 6. They are not
  vulnerabilities, and no stable Iggy release drops them. They are ignored in `.cargo/audit.toml`,
  matching cargo-deny's existing `unmaintained = "workspace"`. Revisit with the next Iggy SDK
  release or by 2027-02-16. Replacing the Iggy SDK stays the owner's decision.

## D-0083 — Prompt 6, milestone 4: one anonymous pack service; a GIF proxy that hides who searches

- Date: 2026-11-17
- Status: accepted by the agent under the plan (D-0079 §3 and §4); report `P06-M4.md`. Two
  threat-model §1 rows (pack records; GIF queries): **approved by the owner on 2026-11-17** (the gate).
- Decided by:
  - owner: packs without an account link, deleted by token or after a year unused; GIPHY behind
    `api`;
  - agent: the formats and the mechanisms.
- Context: the prompt asks for:
  - one generic service for sticker packs, chat scenes, sound stickers and emoticon packs, with
    an encrypted manifest and encrypted assets shared by a link carrying the pack key;
  - a GIF search proxy with caching and rate limits that strips addresses and identifiers;
  - no server-side link previews.
- Options considered:
  - **Pack keys.** A random key per asset listed in the manifest, versus keys derived from the
    pack key. Chosen: **derived** (HKDF, info `songbird.v1/pack-object\0` ‖ be16(n)). The
    manifest still lists each asset's commitment, so an asset cannot be swapped.
  - **The manifest's length.** In the link, or a length prefix inside the object. Chosen: **a
    be32 prefix**, with the padding after it checked as zeros. The link stays short.
  - **Upload tracking.** Per-object rows, versus `complete` checking every object in the store.
    Chosen: **the check at `complete`**: creating a pack is rare, and the check needs no state.
  - **GIF requests.** A `GET` with the query in the URL, versus a signed `POST` body. Chosen:
    **`POST`**, so no access log or URL carries a query.
  - **GIF files.** Straight from the provider's CDN, which shows it the app's address, versus
    proxied. Chosen: **proxied**, with MAC-protected tokens and a host allowlist, so the proxy
    never becomes an open proxy.
  - **Outbound HTTP.** A new client dependency, or the pinned hyper, rustls and tokio-rustls,
    plus `webpki-roots` (already in the tree for push). Chosen: the latter, so no new crate.
- Evidence:
  - `songbird-content`:
    - `pack` unit tests: bounds, the length prefix and padding, pinned object keys reproduced
      by Node.js;
    - `a_pack_opens_from_its_link_alone`: a foreign key or slot is refused before any chunk.
  - Live (`media_live`):
    - `packs_are_created_fetched_anonymously_and_deleted_by_token`: creator-only uploads, exact
      sizes, 409 before every object is there, the creator forgotten, anonymous fetches,
      token-only deletion;
    - `the_gif_proxy_hides_who_searches_and_survives_the_provider`: the provider gets exactly
      six query parameters and three headers; files from other hosts are dropped and HTML is
      refused; answer and file caches; a tampered token gets 404; 401 without an account; the
      breaker opens within five failures; the account budget is enforced.
  - Gate (`gate_p06::a_pack_opens_from_its_link_and_expires_when_unused`): a real encrypted
    pack, opened from its link alone; gone a year after its last fetch; an unfinished one gone
    after a day.
  - Fuzz targets `pack_manifest` and `gif_proxy` (built; they run in the gate's batch).
- Decision: as PROTOCOL §7.4 and §7.5. GIF defaults:
  - 300 searches an hour per account, burst 60;
  - 50 provider requests a second across the cluster;
  - answers cached 10 minutes, files a day (256 MiB);
  - files at most 8 MiB;
  - a 5 s timeout, and a breaker that opens for 30 s after 5 failures.
- Consequences:
  - Packs need no account at all once complete. A deleted account's packs live until their year
    ends, as the owner chose.
  - GIPHY's key and terms are the owner's before production. `[api.media.gifs]` refuses to start
    without a key.
  - A switch to Klipy is a `Provider` adapter and a configuration change (runbook
    `media.md`).
  - Revisit: a shared cache across nodes, if GIF traffic grows. Each node's in-memory cache is
    enough at beta scale.

## D-0082 — Prompt 6, milestone 3: downloads by capability through short-lived signed URLs, explicit fetch confirmation, no caching proxy

- Date: 2026-11-17
- Status: accepted by the agent under the plan (D-0079 §1 and §5); report `P06-M3.md`. The
  threat-model §1 row (downloads by capability, IP-range throttle records): **approved by the owner
  on 2026-11-17** (the gate).
- Decided by: owner (no device authentication on downloads, D-0079 §1); agent (the URL design,
  the confirmation, and the proxy by measurement)
- Context: the prompt asks for:
  - unguessable object IDs with short-lived signed URLs;
  - HTTP range requests;
  - deletion when every recipient device has fetched an attachment, or after 30 days;
  - a caching proxy if benchmarks justify one.

  Prompt 2 served whole objects to signed device requests, and counted a fetch when the last
  byte went out.
- Options considered:
  - **URLs.** S3 presigned URLs, which would expose RustFS (loopback-only, D-0035), versus
    Songbird's own HMAC URLs served by `api`. Chosen: Songbird's own. The URL carries the
    expiry and the size, so a range request costs one MAC check and no database read.
  - **Counting.** At the last byte (Prompt 2) versus an explicit confirmation. Chosen:
    explicit. With ranges, the last byte says nothing (players read the end of an MP4 first),
    and a device should confirm only once it has verified the whole file.
  - **Proxy.** A caching proxy in front of RustFS, or not.
- Evidence:
  - `gate_p06::media_load`: release build, same machine and stack as D-0081. p50 / p99:

    | Measurement | Result |
    | --- | --- |
    | 1 MiB, one at a time | grant 4.6 ms; PUT through `api` 58 ms; PUT straight to RustFS 53 ms. Uploads are RustFS's write latency in this VM |
    | 1 MiB at concurrency 1 | upload (grant + PUT) 64 / 75 ms; download (redirect + GET) 5.0 / 9.1 ms; 36 MB/s each way |
    | 1 MiB at concurrency 16 | upload 44 / 125 ms; download 9.2 / 13.9 ms; 486 MB/s each way |
    | 1 MiB at concurrency 64 | upload 136 / 187 ms; download 44 / 56 ms; 684 MB/s each way |
    | 1 MiB GET at 32 in flight | straight from RustFS 9.1 / 12.1 ms; through `api`'s signed URL 21.8 / 31.3 ms |
    | 64 MiB | upload in parallel parts 277 MB/s; download 775 MB/s |
    | 128 KiB ranges at random offsets, 8 in flight | 4.0 / 5.6 ms |

  - Tests:
    - `media_live::downloads_need_no_account_and_serve_ranges`: unsigned requests; 303; whole
      file and five kinds of range; 416; ignored ranges; tampered expiry, size, MAC and object,
      an expired URL and one dated too far ahead all refused; unknown IDs.
    - `media_live::fetches_are_confirmed_and_the_last_one_deletes`, including the IP-range
      budget.
    - `gate_p06`: a real encrypted file through parallel parts; a seek decrypted chunk by
      chunk; two devices decrypt and confirm, and the last deletes (worker job); 30 days; the
      abort of an abandoned upload.
    - Unit tests for ranges and URLs; fuzz target `media_url`.
- Decision:
  - **Downloads.**
    - `GET /v1/attachments/<id>` takes no account and answers 303 to
      `/v1/media/<id>?e&z&s`: HMAC-SHA256, truncated to 16 bytes, under a key derived from the
      media secret, valid 10 minutes.
    - The media path serves one range or the whole object, and the device's cache may keep it
      until expiry (`private, immutable`).
    - `POST …/fetched` counts a fetch, and the last one deletes. Redirects and confirmations
      are budgeted per IP range.
    - Without `[api.media]`, downloads answer 503. That configuration is development only.
  - **No caching proxy.** RustFS accounts for about 9 of the 22 ms of a loaded download. Each
    attachment is read by its recipients only, a few times, so a cache in front of RustFS would
    rarely hit.
    - Packs (M4) and GIFs are the only objects read by many. GIFs are cached in `api`'s memory
      (M4).
    - Packs get revisited with Prompt 13's numbers on the production hardware.
- Consequences:
  - The approved attachment row's "device-signed download" no longer holds. The new §1 row
    describes what replaces it, for approval.
  - Prompt 2's tests (`api_live`, `gate_m5`) moved to the redirect and the confirmation, and to
    legal sizes.
  - `api`'s avatar serving reuses the same frame adapter (`media::Frames`).
  - The test client now sets `TCP_NODELAY` like every other client in the tree.

## D-0081 — Prompt 6, milestone 2: grants held to the format and to per-account budgets; resumable uploads in parallel S3 parts (tus measured and removed)

- Date: 2026-11-17
- Status: accepted by the agent under the plan (D-0079); report `P06-M2.md`. The two
  threat-model §1 rows (resumable grants up to 24 h; budget records): **approved by the owner
  on 2026-11-17** (the gate).
- Decided by: agent (the protocol and the part size by measurement, as D-0079 §5 delegates)
- Context:
  - The prompt asks for upload grants bound to an exact padded size and a per-plan limit,
    resumable chunked uploads to RustFS (S3 multipart or tus, benchmark both), and per-account
    upload rate and storage quotas.
  - RustFS is reachable only from Songbird services (D-0035), so every byte passes through
    `api`.
- Options considered:
  - **Quotas.** A storage quota on the bytes held would need a link from objects to accounts,
    which the approved attachment row rules out. A **rolling volume budget** bounds storage
    just as well: at most budget × 30 days per account. Chosen, together with a grant-rate
    budget. Both are GCRA records, keyed, in `valkey-hot`.
  - **Resumable protocol.** Two options behind the same S3 multipart machinery:
    - fixed parts: `PUT …/parts/<n>`, `GET …/parts`, `POST …/complete`;
    - tus 1.0 core: `HEAD` and `PATCH`, the body split into consecutive parts, with
      `Upload-Offset` moving in whole parts.

    Neither buffers: each part streams straight into an `UploadPart` with its exact length.
  - **Part size:** 5, 16 or 64 MiB.
- Evidence:
  - `media_live::upload_protocols`: release build, Ryzen 9 7950X, Windows 11 Pro 10.0.26300.
    RustFS 1.0.5, SurrealDB v4.0.1 and Valkey 9.1.3 ran under podman 5 (WSL 2 VM: 4 vCPU,
    6 GiB), over loopback port forwarding.
  - One 256 MiB attachment per run; MB/s, median of 5:

    | Part size | Single `PUT` | Parts, 1 at a time | Parts, 4 at a time | tus, one `PATCH` | tus, one `PATCH` per part |
    | --- | --- | --- | --- | --- | --- |
    | 5 MiB | 255 | 79 | 288 | 83 | 58 |
    | 16 MiB | 376 | 157 | 508 | 144 | 156 |
    | 64 MiB | 297 | 228 | 731 | 223 | 192 |

  - tus is never faster than parts one at a time: it is the same sequence of `UploadPart`s.
    Parallel parts are 2 to 3.5 times faster, and only the parts protocol can run them.
  - The cost per part (RustFS commits each part) dominates small parts.
  - Live tests (`services/api/tests/media_live.rs`, `crates/s3/tests/rustfs_live.rs`,
    `crates/accounts/tests/registry_live.rs`):
    - interruptions mid-part and parts out of order;
    - racing first parts (one upload kept, the other aborted);
    - early completion refused (409);
    - another device, a wrong length or a part past the last refused;
    - reuse after completion refused;
    - the byte-exact result;
    - the budgets;
    - lengths outside the format refused.
- Decision:
  - Grants accept only lengths the attachment format produces, up to `max_bytes`, which now
    defaults to 2 GiB (D-0079).
  - Budgets per account: 300 grants per hour (burst 60) and 10 GiB per 24 hours, under
    `[api.media]`. Without that section (development only), there are no budgets.
  - **Resumable uploads use the parts protocol**, with **16 MiB parts** by default
    (configurable from 5 to 512 MiB, at most 10,000 parts). This is the best balance between
    throughput and how much a dropped mobile connection loses.
  - tus is removed: it never beats parts, adds a second protocol to audit, and needs a
    body-splitting stream with a lock.
  - A resumable grant renews for 1 h with each part and lasts at most 24 h
    (`GRANT_MAX_AGE`). Clients get no longer than that.
  - The registry records the multipart upload ID on the grant with compare-and-set.
  - The worker aborts every multipart upload whose grant is gone (`abort_abandoned_uploads`).
    This covers expiry, completion, device and account deletion, and crashes in one rule, so
    deletion paths need not track upload IDs.
- Consequences:
  - `crates/s3` gains create, upload part, list parts, complete, abort and list uploads, with a
    bounded parser and a fuzz target (`s3_multipart_xml`).
  - The grant decoder becomes `songbird_api::media::GrantRequest`, fuzzed as `upload_grant`.
  - Prompt 2's attachment test now uses a legal length: 4,096 bytes is not one.
  - Clients that want tus libraries map their chunks to parts.
  - To revisit with the apps: the part size on real mobile links, and whether more than four
    parallel parts helps.

## D-0080 — Prompt 6, milestone 1: the attachment format (STREAM, key-committing, Padmé), AES-256-GCM by default

- Date: 2026-11-17
- Status: accepted by the agent under the plan (D-0079); report `P06-M1.md`
- Decided by: agent (the AEAD by measurement, as D-0079 §5 delegates)
- Context: the prompt asks for files encrypted in fixed-size chunks with a streaming AEAD (stream,
  resume, verify chunk by chunk) and sizes padded into buckets. Prompt 4 left
  `AttachmentPointer.key` (64 bytes) and `digest` (32 bytes) opaque.
- Options considered:
  - Signal's format: AES-CBC with HMAC over the whole file. Nothing can be verified before the
    end, and it needs a separate incremental MAC to stream.
  - Per-chunk AEAD with random nonces: reordering and truncation go undetected without extra
    framing.
  - **STREAM** (as `age` and Tink's streaming AEAD): positional nonces and a last-chunk flag.
    Chosen.
  - Padding: power-of-two buckets (up to 100 % overhead) versus **Padmé** (at most 12 %,
    O(log log n) bits leaked). Padmé chosen, with a 1 KiB floor so thumbnails all look alike.
  - AEAD: AES-256-GCM versus ChaCha20-Poly1305, both from aws-lc-rs.
- Evidence:
  - `crates/content/tests/attachment_bench.rs`, release build, Ryzen 9 7950X, Windows 11 Pro
    10.0.26300, aws-lc-sys 0.47 with its prebuilt NASM objects; medians of 9 runs, encrypt and
    decrypt in MB/s. Both suites are equal at 64 KiB, where key derivation dominates:

    | Size | ChaCha20-Poly1305 | AES-256-GCM |
    | --- | --- | --- |
    | 1 MiB | 2,383 / 2,400 | 3,087 / 3,185 |
    | 64 MiB | 2,246 / 2,501 | 2,950 / 3,413 |

  - Every ARMv8-A phone core has the AES instructions, so the order should hold on phones. The
    ARM numbers come with the apps.
  - Tests in `crates/content/tests/attachment.rs` (10, including a proptest):
    - round trips at every chunk and padding boundary in both suites;
    - a flipped bit fails at its chunk; swapped chunks, truncation, a lying size and trailing
      bytes are refused;
    - a wrong key, salt, digest or version byte is refused before any chunk (no downgrade
      between suites);
    - non-zero padding is refused;
    - chunks open in any order.
  - `crates/proto` checks the size arithmetic exhaustively up to 4 MiB (every legal length
    inverts, every other length is refused) and with proptests to 1 TiB.
  - Vectors reproduced independently by `tools/attachment-vectors/check.mjs` (Node.js 26,
    OpenSSL 3.5).
  - Fuzz target `attachment_decrypt` (built; run with the gate's fuzz batch).
- Decision:
  - The format in PROTOCOL §7.1: HKDF-SHA256 from the 64-byte pointer key and a 32-byte header
    salt to a chunk key and a 32-byte key commitment (the pointer's `digest`); STREAM over
    64 KiB chunks; Padmé padding of zeros; version byte `1` (ChaCha20-Poly1305) or `2`
    (AES-256-GCM).
  - Apps write `2`, and readers accept both.
  - `digest` changes meaning from "SHA-256 of the object" (Prompt 4's comment) to the key
    commitment. No app has shipped, and the published content vectors keep their bytes.
  - The sizes live in `songbird_proto::media`, so `api` can refuse grants for lengths the
    format never produces without linking `songbird-content`.
- Consequences:
  - `songbird-content` now depends on `songbird-proto` and `zeroize` (both already in the tree).
  - `crates/client` cannot link `songbird-content` (`no_service_links_content` covers every
    crate), so the plan's "client streaming encrypt" lives in `songbird-content`. The client
    gets the transport side (resumable upload, ranged download) in M2 and M3.
  - On this machine, aws-lc-sys builds with `AWS_LC_SYS_PREBUILT_NASM=1`, because NASM is not
    installed. CI is unaffected.

## D-0079 — Prompt 6: the plan, and the owner's decisions it needs

- Date: 2026-11-17
- Status: accepted (owner, 2026-11-17, every recommendation)
- Decided by: owner, on the agent's plan `docs/reports/P06-plan.md`
- Context: Prompt 2 (D-0036) built single-use, exact-size grants, uploads streamed to RustFS and
  deletion after the last fetch or 30 days. Prompt 6 adds a chunked encryption format, resumable
  uploads, range downloads, packs and a GIF proxy. RustFS is reachable only from Songbird
  services (D-0035), so signed URLs are Songbird's own. Downloads are signed by a device today.
  Tenor's API closed on 2026-06-30.
- Options considered and evidence: in the plan ("Findings").
- Decisions (the agent's recommendations, all accepted by the owner):
  1. Downloads without device authentication: the object ID is the capability; a 10-minute
     signed URL serves range requests; fetches confirmed explicitly.
  2. 2 GiB per attachment; 10 GiB uploaded per account per 24 hours; 300 grants per hour.
  3. Packs with no account link, deleted by a management token or a year after the last fetch;
     200 assets and 300 MiB each.
  4. GIPHY behind a provider trait; `api` proxies search and images and never stores queries.
  5. Benchmarks decide AES-256-GCM versus ChaCha20-Poly1305, resumable parts versus tus, and a
     caching proxy.
  6. One branch and one PR for every milestone (the owner, 2026-11-17).
- Consequences: downloads stop naming devices (the attachment row in threat model §1 changes, for
  approval with M3); the attachment limit rises to 2 GiB; packs are anonymous; GIPHY's API key
  and terms are the owner's before deployment; M1 starts.

## D-0078 — Prompt 5 follow-ups: the security review's fixes

- Date: 2026-11-17
- Status: accepted; part of the follow-ups PR (report `P05-followups.md`, "Security review")
- Decided by: agent, on an independent read-only review of the follow-ups diff (a separate
  agent, given the code and documents, not the report's conclusions)
- Context: the review found two medium and four low findings, and four for information. It
  found no way for a server, member or outsider to forge a link proof or get a listed member
  removed past an honest server. Most findings were gaps in the apps' rule (the harness)
  against a malicious server.
- Decision:
  - **One external-commit rule** (`ds::check_external_commit`, medium). Apps checked only the
    link proof, so a malicious server could have them follow an external commit that also
    removed another member's leaf. The structural rule now lives in `crates/groups`, and
    the server and apps both run it:
    - only the joiner's own `ExternalInit` and `Remove`, inline;
    - the joiner's leaf is the authenticated device's;
    - the only leaf removed is the joiner's own old one;
    - no device twice.

    Apps also check that a commit's context names exactly the service key they pin
    (`extensions_allowed`), as the server does (low).
  - **Refused server proposals are kept aside** (medium). A member whose view of a device list
    is stale refused a valid server removal and dropped it. A later commit referencing it
    then could not be processed. Apps now keep it outside the OpenMLS pending store (their own
    commits never include it), and judge it again after refreshing the device lists, before
    processing a commit that references one they lack (`held`, `recheck_held`).
  - **A server removal of an empty leaf** is refused by apps (low).
  - **Commits no longer bring deleted IDs back into prune marks** (low–medium, GDPR). A commit
    names in a mark only devices whose rows still exist; deleted ones are found through the
    directory. Before, a commit after the final deletion could re-add the IDs and hold the
    deletion job's `remnants` check.
  - **A failing mark goes to the back of the queue** (`group_prune_later`, low), so one group
    that keeps failing cannot hold up the others.
  - **Link keys must be honest points** (D-0048's check, on decode; info). Implementations
    then cannot disagree about a proof under a small-order or non-canonical key. PROTOCOL
    asks apps for strict RFC 8032 verification.
  - **A commit that leaves the service's proposal out** (found while fixing): its devices are
    marked again from the queued proposals (`pending_server_removals`), whatever this node's
    snapshot says.
- Not changed, recorded (threat model §6):
  - **Channel accounting.** The class is spent before later failures (a mailbox outage, an
    idempotent replay). A refused post still costs its per-node token. This is stricter than
    configured, never looser.
  - **Prune passes.** No per-mark lease: every `api` node may handle the same mark. The work
    is idempotent, and candidates are looked up one by one.
  - **Groups made before D-0075** would need a context change naming the service. None exist
    in production.
  - **Key material.** The external-sender seed shares the cluster secret with the fan-out
    keys. HKDF outputs are not zeroized.
- Evidence (Windows 11, `rustc 1.99.1`; live: SurrealDB v4.0.1 and Valkey 9.1.3 under podman):
  - harness:
    - `a_join_may_remove_no_one_else_on_the_server_or_in_apps`: a link holder's and a
      member's second device's external commits removing Alice are refused by the server and
      by Bob's app;
    - `a_member_with_a_stale_view_catches_up_on_a_held_proposal`;
    - `apps_refuse_a_server_removal_of_an_empty_leaf_and_a_context_without_the_service`;
    - `a_commit_that_leaves_a_proposal_out_still_names_the_departed_device`;
  - `roles` unit test: small-order link keys refused;
  - live `devices_live::departing_devices_mark_the_groups_that_hold_them`: a commit naming a
    deleted device marks the group without its ID.
- Consequences: apps implement the external-commit rule, the pinned-key check and the held
  proposals as the harness does (PROTOCOL §6.8, §6.11).

## D-0077 — Prompt 5 follow-up 3: the channel rate class across the cluster, in `valkey-hot`, with the per-node budget in front and as the fallback

- Date: 2026-11-17
- Status: accepted; built and passing live (report `P05-followups.md`); the threat-model row
  and the fallback **approved by the owner on 2026-11-17** ("approve all")
- Decided by: agent, on the owner's approval of the follow-up at the Prompt 5 gate (2026-11-17)
- Context: D-0072's channel rate class (1 post a second per channel, burst 10) was per node. A
  channel's admins posting through several delivery nodes and `api` got each one's budget,
  and a post is stored for up to 2,048 devices.
- Options considered:
  - **A new store or service for the budget** was rejected: `valkey-hot` already holds every
    cluster-wide budget as a GCRA record on Valkey's clock (`messages::GCRA` for HTTP sends,
    the gate's request and broadcast budgets).
  - **A separate Valkey connection and key for the channel class** was rejected: the gate
    (`songbird_send::Gate`) is already on both transports' send path, with a `valkey-hot`
    connection and keys every node pins in the registry. The class is one more gate method
    (`Gate::channel`), and the record `q:c:<HMAC(group)>` comes from the same keys, so Valkey
    never sees a group ID.
  - **Replacing the per-node budget** was rejected. It stays in front of the cluster record,
    as a fast path: a node already over the class refuses without a round trip.
  - **When `valkey-hot` is down, failing closed** (refusing every channel post) was weighed
    against **falling back to the per-node budget**:
    - the gate fails closed for message requests, because they protect a person from
      strangers, and a block must not be skipped;
    - channel posts come from the channel's own admins (authorized posters, each also under
      its device's budget), and the class bounds storage amplification, not who may post;
    - failing closed would turn a `valkey-hot` outage into a channel outage.

    **Decided: fall back to the per-node budget**, which is D-0072's guarantee, counted
    (`channel_rate_local_only`) and covered by a runbook. Each post waits at most 250 ms
    (`CHANNEL_GATE_WAIT`) for Valkey, since a connection to an unreachable host can hang.
- Decision:
  - `RequestLimits` gains `channel_per_sec` (1) and `channel_burst` (10), used by both parts
    of the class (`[api.requests]`, `[delivery.requests]`).
  - The send path (`SendPath::channel_class`, both transports) runs:
    1. the per-node budget;
    2. then `Gate::channel` (`RegistryGate`: cost-weighted GCRA on Valkey's clock), within
       250 ms;
    3. refused: reason 1 `RateLimited` with the wait;
    4. unreachable or slow: the per-node decision stands, and the post is counted.
  - Without a gate (development), the per-node budget alone, as before.
- Evidence (Windows 11, `rustc 1.99.1`; live: SurrealDB v4.0.1 and Valkey 9.1.3 under podman):
  - `crates/send`:
    - `the_channel_class_is_shared_by_every_send_path`: two send paths with per-node budgets
      of 5 and one shared gate of 3 admit 3 in all;
    - `without_valkey_the_per_node_budget_decides_alone`: a store that is down, and one that
      never answers (each post bounded by the 250 ms wait), admit at the per-node rate and
      count it;
    - the existing `channels_have_their_own_rate_class` and `the_channel_budget_refills`.
  - Live `gate_p05::a_channels_rate_class_is_shared_by_the_websocket_and_http`: Alice posts 24
    times, alternating a delivery node's WebSocket path and `api`'s HTTP path, each with its
    own `valkey-hot` connection. 10 to 12 are admitted in all (each node alone would have
    admitted 10), nothing falls back, and the `q:c:` records hold no group ID.
- Consequences:
  - One threat-model §1 row (the keyed channel record in `valkey-hot`), approved by the owner
    on 2026-11-17. The §6 risk "the channel rate class is per node" holds only while `valkey-hot`
    is down.
  - Runbook: `docs/RUNBOOKS/delivery.md`, "`valkey-hot` unreachable".

## D-0076 — Prompt 5 follow-up 2: invite-link proofs members check, under a key the link's creator publishes in the roles extension

- Date: 2026-11-17
- Status: accepted; built and passing live (report `P05-followups.md`); the threat-model row
  **approved by the owner on 2026-11-17** ("approve all")
- Decided by: agent, on the owner's approval of the follow-up at the Prompt 5 gate (2026-11-17)
- Context: D-0071 left whether a joiner held a usable invite link to the server's word. A
  malicious server could admit any account to a group by external commit. Members would see
  the join, but could not tell it from an honest one.
- Options considered:
  - **A MAC key shared in the encrypted group info** (the owner's sketch) was weighed and not
    taken, for three reasons:
    - the encrypted info is served to anyone who opens any of the group's links (they need
      the name and picture), so every link holder would learn every link's MAC key and could
      prove for links they never held;
    - the server cannot check a MAC whose key it never sees, so it would accept joins its
      honest members then refuse (a fork);
    - the info is replaced by any role with `edit_info`, not only the links role.
  - **A signature instead (the "equivalent" the owner allowed).** The link's secret also yields
    an Ed25519 key pair. Its public key goes in the roles extension, which:
    - every member verifies in the MLS context;
    - the server can read, so it applies the same rule and an honest server never accepts a
      join members refuse;
    - only the `links` role changes.

    Knowing the public key forges nothing. The server keeps only `SHA-256(auth)` from the
    same secret, so it cannot sign.
  - **A separate context extension for the keys** was rejected: OpenMLS accepts a change to a
    custom extension only if `required_capabilities` names it, and every member's leaf would
    need a new capability. The keys are a section of the roles extension (version 2), which is
    already required. A group without link keys keeps version 1 byte for byte.
  - **The proof in a new bundle part** was rejected: members receive the commit message alone,
    not the bundle. It travels in the external commit's authenticated data, which the joiner
    signs as part of the commit and every member receives.
- Decision:
  - **Keys.** `proof` = HKDF-SHA-256(salt `songbird.v1/group-link`, the link's secret, info
    `proof`), an Ed25519 seed. The link's creator commits `handle → public key` into the roles
    extension before sharing the link: at most 16, ascending by handle. Publishing or
    withdrawing a key needs the `links` role (`roles::authorize`).
  - **The proof:**
    - the external commit's authenticated data, `1 | handle [16] | signature [64]`;
    - signed by the link's key over `"songbird.v1/link-proof\0" | group | epoch | handle |
      device | len u16 | leaf signature key`, so it cannot be moved to another group, epoch,
      device or leaf.
  - **One rule** (`links::verify`):
    - the server runs it on every join by link, requiring the proof to name the link the join
      is counted against (403 `proof`, nothing spent);
    - apps run it on every external commit from an account not yet in the group
      (`receive_join_checked` in the harness), and refuse to follow one without it;
    - rejoins of an account's own devices need no proof.
  - **The server keeps working as before:** handles, `SHA-256(auth)`, uses, expiry and
    revocation. Revoking a link is now also a commit withdrawing its key, so members refuse
    later joins by it even from a server that ignores the revocation.
- Evidence (Windows 11, `rustc 1.99.1`; live: SurrealDB v4.0.1 and Valkey 9.1.3 under podman):
  - `crates/groups`:
    - `links` unit test: a proof moved to another group, epoch, device or leaf fails, as do
      another handle, an unpublished link, another key, a truncated proof and another version;
    - `roles` unit test `link_keys_have_one_encoding_and_are_the_links_roles`.
  - `tests/groups/tests/link_proofs.rs`, 4 tests:
    - a join by link with its proof is accepted by the server and by members;
    - refused by the server (`LinkProof`) and by members: no proof, an unpublished link,
      the published handle signed with another secret, a valid proof made out to another
      device;
    - a rejoin needs no proof;
    - only the links role publishes or withdraws keys, and a withdrawn key stops joins by
      its link.
  - Live `gate_p05`:
    - in `invite_links_and_join_requests`, a join without a proof and one under an
      unpublished link get 403 `proof` and spend nothing, then the valid join succeeds and
      both members check Carol's proof themselves;
    - `parallel_joins_never_exceed_a_links_uses` joins with proofs.
- Consequences:
  - Apps derive three keys from a link's secret (`auth`, `info`, `proof`). They commit the
    proof key before sharing a link, and withdraw it when revoking.
  - One threat-model §1 row: link handles and proof public keys in the group context. The §6
    risk "whether a joiner held a link is the server's word" closes for apps that check.
  - A joiner whose link leaked is still admitted: the proof shows the link was held, not by
    whom (as before).

## D-0075 — Prompt 5 follow-up 1: the delivery service as every group's MLS external sender, proposing to remove devices that left the directory

- Date: 2026-11-17
- Status: accepted; built and passing live (report `P05-followups.md`); the threat-model rows
  **approved by the owner on 2026-11-17** ("approve all")
- Decided by: agent, on the owner's approval of the follow-up at the Prompt 5 gate (2026-11-17)
- Context: D-0073 left a gap. When an account is deleted or a device removed, its leaves (device
  ID, public keys, and for an account that held a role, its account ID in the roles extension)
  stay in every group's MLS state until a member happens to commit their removal. The server
  cannot edit a group's state without forking it.
- Options considered:
  - **The server rewriting group state** stays rejected (D-0073): it forks every member.
  - **Apps noticing on their own** (a member removes leaves whose device left the directory)
    was already allowed (D-0070's "dead devices"), but nothing makes it happen, and every
    member would race to do it.
  - **A server signing key inside the verify-only provider** was rejected: that provider's
    property is that it signs nothing (D-0066). The key lives in
    `crates/groups/src/external.rs`, used for one message type.
  - **OpenMLS's `ExternalProposal::new_remove`** needs a full client provider type (storage,
    randomness), which the server crates do not link. The proposal is encoded by hand (RFC 9420
    §6: 42 bytes, then the signature), then run through the service's own `PublicGroup` before it
    is queued, so an encoding mistake is refused, never stored.
  - **Trusting the server's word on who left** was rejected. The trust rule is the apps':
    commit a server removal only for a device no longer in its account's signed device list.
  - **Making commits include every queued server proposal** was rejected: a member who has
    not fetched the proposal yet would be refused. Instead the service proposes again after
    every commit that leaves a departed device in the group.
  - **Failing closed on a stale directory** (403) was rejected for the server's own check. The
    service proposes only removals the registry confirmed, and devices never come back, so a
    commit removing a device this node still lists means the node's snapshot is behind: the
    answer is 503 (retry).
- Decision:
  - **The key.** Ed25519, from the cluster secret (HKDF-SHA-256, salt `songbird.v1/groups`, info
    `external-sender`): every `api` node signs with the key every group names.
    `POST /v1/groups/ds` serves the public half; apps should also pin it.
  - **Every group names it.** Its context carries `external_senders` with exactly one entry: the
    key, with the basic credential `songbird.v1/ds`. Checked at creation and after every commit
    (`Limits::external_sender`): a member can neither drop the service nor add a second external
    sender.
  - **What the server may propose:** `Remove` only. External `Add`, context changes and every
    other kind are refused by the server and by apps. No other outsider proposes
    (`NewMemberProposal` refused).
  - **The trust rule** (`roles::Change::external`, `roles::Denied::Listed`): a commit that
    includes a server removal of a still-listed device is refused, whoever commits it, even an
    owner who could remove that person by role. Apps check a server proposal before storing it
    (`receive_server_proposal` in the harness), and check commits with the same rule.
  - **When.** Every registry transaction that disables devices (a primary's new device list,
    a linked device removing itself, both ways an account deletion starts) marks the groups
    holding them (`grp_prune:[group]`, with that group's departed devices). So does a commit
    that leaves a departed device in its group. `api`'s re-drive loop (every 5 s):
    - confirms each candidate in the registry, never on its cached snapshot alone;
    - queues one proposal per departed leaf (compare-and-set on epoch and revision, as
      leaving does);
    - fans them out to the listed members;
    - forgets the mark, unless a later mark came.

    Signing is deterministic, so a retried proposal is the same bytes under the same fan-out
    key.
  - **Apps** commit pending server removals soon after receiving them (with a random delay,
    so members do not race), or with their next commit. The removal needs no role.
  - **GDPR:** the marks hold group IDs and the departed devices' IDs for seconds. The final
    deletion strips the IDs from any mark left (`remnants` counts them).
- Evidence (Windows 11, `rustc 1.99.1`; live: SurrealDB v4.0.1 and Valkey 9.1.3 under podman):
  - `tests/groups/tests/server_removals.rs`, 4 tests:
    - a departed device is proposed once (again: no new proposal, the same bytes), members
      accept it, and the next commit removes it on the server and every client;
    - a server removal of a listed device is refused by a member checking it, and when
      committed anyway, by the server (`Forbidden(Listed)`) and by clients, even when the
      owner commits it;
    - an external `Add` signed with the service's key is refused by apps, by `/propose`
      and when committed;
    - groups must name exactly the service: none, another key, the key on a server without
      one, dropping it and adding a second are all refused.
  - `roles` unit test `server_removals_are_only_for_devices_that_left`; `external` unit tests
    (the RFC layout, the signature, determinism).
  - Live:
    - `devices_live::departing_devices_mark_the_groups_that_hold_them`: each disabling path
      marks exactly the right groups; a later mark survives; deletion strips the IDs;
    - `gate_p05::departed_devices_are_removed_through_server_proposals`, with the HTTP key
      endpoint: Carol's account deletion, the proposal confirmed in the registry while the
      node's snapshot is stale, members accepting it, Bob's commit answered 503 until the
      snapshot follows, then 200 with Carol's leaf gone, and nothing naming her after deletion.
- Consequences:
  - Threat-model §1 gains two rows (the service key in every group's context and its queued
    removals; the prune marks). Approved by the owner on 2026-11-17.
  - The GDPR gap of D-0073 closes once a member is online: a departed device's leaf goes with
    the next commit. A group none of whose members comes back keeps the leaves (threat model
    §6).
  - Rotating the key means a context change in every group; not built (§6).
  - Review finding 3's edge (D-0074): an owner whose devices are gone is now removed by the
    next commit, so succession happens then, not whenever someone happens to remove the leaf.

## D-0074 — Prompt 5: the security review's fixes

- Date: 2026-11-17
- Status: accepted; part of the Prompt 5 PR (report `P05-gate.md`, "Security review")
- Decided by: agent, on an independent read-only review of the whole Prompt 5 diff
- Context: the review found:
  - two high findings: a default-role change bypassed the role checks; an added KeyPackage
    was not bound to its device;
  - four medium: a group with no listed owner froze; reads had no budget; a fan-out could stall
    when the directory changed; outsiders' commits were processed before the membership check;
  - five low.
- Decision:
  - **Default role** (`roles::authorize`): a change to `policy.default_role` is a role change
    for every member it applies to, checked like any other. An admin can no longer make
    everyone an owner, or demote unnamed owners.
  - **Published KeyPackages only** (`ds::Applied::key_packages`,
    `Registry::key_packages_published`):
    - a commit may add a KeyPackage only if its device published it, as a single-use package
      claimed from the key directory (its reference kept in `mlskp_used` until it expires) or as
      its last-resort package (`device.mlslr_ref`);
    - otherwise 403 `key_package`, and nothing is stored;
    - without this, a member could add a leaf under another device's ID with keys of its own
      and speak as that device.
  - **Succession** (`roles::authorize`): when no owner is left in the group, whether every
    owner left or their devices are gone, the committer names new owners, from the highest
    role left. A vanished owner no longer freezes the group, and a read-only subscriber cannot
    take a channel while admins remain.
  - **`required_capabilities`** must be exactly Songbird's (roles extension, `SelfRemove`,
    basic credentials), at creation and after every commit.
  - **Outsiders first** (`api`): a commit or proposal from a device that may not read the group
    gets 404 before the state is decoded. A join by link is the one way in.
  - **Reads have a budget**: state, change log and community state, 5 a second per device,
    bursting to 20. Role checks read the roster only, never the state.
  - **Fan-outs finish** when a recipient left the directory between attempts: an idempotency
    conflict means a store under that key already completed.
  - **Docs:** join requests are the `add` role's (PROTOCOL §6.6).
- Not changed, recorded as risks (threat model §6):
  - role checks for metadata, links, requests and communities read the roster before the
    write, so one in-flight action by a just-demoted admin can land;
  - no per-account cap on creating groups, and groups are not deleted yet;
  - a client whose directory view differs from the server's may judge a commit differently;
  - one member can fill the 64-proposal queue until the next commit (which removes them).
- Evidence:
  - `roles` unit tests `the_default_role_moves_everyone_it_names`,
    `a_vanished_owner_has_successors_too`, and the heir in
    `leaving_is_the_leavers_and_owners_have_successors`;
  - `gate_p05` live, 10 of 10, with `a_member_cannot_add_a_leaf_it_made_up_for_another_device`
    and `outsiders_get_the_answer_for_no_group`. Every live add now goes through the key
    directory, as apps do;
  - groups, harness and send: 76 tests.
- Consequences:
  - Apps add people only with KeyPackages fetched from the key directory, which they did
    anyway.
  - One more registry table (`mlskp_used`), swept with expired packages, deleted with the
    device, counted in remnants.

## D-0073 — Prompt 5, milestone 5: groups in the GDPR export and deletion

- Date: 2026-11-16
- Status: accepted; built and passing live (report `P05-gate.md`); part of the Prompt 5 PR
- Decided by: agent, under D-0056 (GDPR, Prompt 3) and the standing instruction
- Context: every new store must be in the account export and go with the account (constitution;
  D-0056).
  - The stores are `grp`, `grp_fanout`, `grp_log`, `grp_link`, `grp_req`, `cmty` and
    `cmty_grp`.
  - Unlike the others, a group's MLS state is shared: the members' apps hold the same tree, and
    a server that edited it would fork the group (every later commit would fail for them).
- Options considered:
  - **The server rewriting the group's state** on deletion (dropping the leaves) was rejected:
    it forks the group for every member.
  - **The server as an MLS external sender** (RFC 9420 `external_senders`) proposing the
    removal, for the next committer to include, would remove the leaves within a commit. It was
    not built now:
    - it adds a server signing key to every group;
    - apps must decide which server proposals to trust (only for devices absent from the
      directory), which is a trust decision for the owner.

    It is offered as a follow-up.
  - **Scanning every group** for an account's devices on export was rejected (cost grows with
    the service): `grp` gains an indexed `devs` array, written with every commit.
- Decision:
  - **Export** (`Registry::export`):
    - `groups`: each group the account's devices are in, its epoch, the devices with their
      roles, and its community;
    - `group_requests`: pending requests to join, with their time;
    - the "not attributable" list explains what stays in groups after deletion.
  - **Deletion** (`delete_account_rows`, checked by `remnants`):
    - the account's join requests go (`grp_req.acct`, indexed);
    - group messages are no longer stored for devices the directory does not know (deleted
      accounts, removed devices). Commit fan-outs already skipped them.
    - **What stays** until a member commits the removal: the devices' leaves (device ID,
      public keys) and, for an account that held a role, its account ID in the roles extension.
      Apps commit the removal of devices that left the directory; anyone may drop a departed
      account's role entry (D-0070). Once the account's rows are gone, these no longer lead to
      the person on the server.
  - **Not attributable:** change-log entries (members' ciphertext, signed by a leaf key) and
    invite links (hashes; no creator recorded). Communities hold group IDs only.
  - **Fuzzing:** a `group_records` target for the decoders added since M1 (roles extension,
    registry roster, change-log entry, stored record), in the nightly run with `mls_wire` and
    `mls_commit`. The M3 and M4 request bodies are fixed-size splits with no indexing (clippy
    denies it), checked by their tests.
- Evidence:
  - `gate_p05` live: `groups_are_in_the_export_and_requests_go_with_the_account`. The export
    names the group, the device and its role (owner, member) and the pending request. After
    deletion, `remnants` is empty and the request list is empty.
  - Send: `devices_gone_from_the_directory_get_no_group_messages`.
  - Fuzz: `group_records` (numbers in `P05-gate.md`).
- Consequences:
  - A threat-model §6 note: what stays in group state after deletion, and why.
  - Follow-up for the owner: server-proposed removals (`external_senders`).

## D-0072 — Prompt 5, milestone 4: communities as opaque links run by the announcement group's admins; private channels capped at 2,048 devices with a rate class; receipt tiers

- Date: 2026-11-16
- Status: accepted; the benchmarks and caps **accepted by the owner on 2026-11-17** (report `P05-M4.md`);
  part of the Prompt 5 PR
- Decided by: agent, under D-0065 (owner: tiers §5, public channels §4, X-Wing §3) and the
  standing instruction (Proton-level bar)
- Context: Prompt 5 asks for:
  - communities (shared admins, an announcement group, opaque links, encrypted names);
  - private channels up to a benchmarked cap;
  - commit and Welcome costs at 1,024 and 5,000 members;
  - D-0065 §3 keeps X-Wing unless those numbers say otherwise.
- Options considered:
  - **Community admins as their own list on the server** was rejected: it would be a second
    source of truth that no client verifies. The announcement group's roles extension is
    already verified by every member (D-0070), so the server reads the community's admins from
    it.
  - **A community admin linking any group** was rejected: linking needs an admin of the group
    too, so nobody can pull a group into a community it did not choose. Unlinking needs either.
  - **Community state for anyone with the ID** was rejected: only the announcement group's
    members (as for group state, D-0067).
  - **The channel cap at 5,000** was rejected on the numbers:
    - a full-path X-Wing commit of a fresh 5,000-leaf tree is 5.6 MiB, over the 4 MiB bundle
      limit, and every subscriber would download it;
    - at 2,048 it is 2.3 MiB.
  - **Classical ciphersuites for large groups** were not adopted. Welcomes and add commits stay
    small with X-Wing (1.4 and 2.8 KiB). Its cost is full-path commits on fresh trees, 14 times
    classical, falling to about 24 KiB as the tree fills. The owner may still choose otherwise
    at the gate.
  - **A cluster-wide channel budget in `valkey-hot`** was not built yet: the per-node budget
    matches how WebSocket budgets already work (threat model §6).
  - **Server-enforced receipts** are impossible (receipts are encrypted). The rule is one
    function the apps share.
- Decision:
  - **Communities** (`PROTOCOL.md` §6.9):
    - `cmty:[id]` holds the announcement group and the encrypted info with its version;
      `cmty_grp:[group]` holds the community, so a group is in at most one;
    - at most 100 groups;
    - created by an admin of an announcement-only group (its `send` role at least `Admin`);
    - linking needs an admin of both; info by the announcement group's admins; deleting by
      its owners.
  - **Private channels** (§6.10):
    - `Limits::max_channel_members` is 2,048 devices; a group stays at 1,024. The limit
      follows the policy the commit leaves.
    - The send path's channel rate class is 1 post a second per channel, bursting to 10, per
      node (`send::groups::ChannelBudget`). Over it, reason 1 with the wait.
  - **Tiers** (`songbird_groups::tiers`):
    - up to 32 people: every device sends delivered receipts, with a 1 s window;
    - larger groups: primaries only, with a 10 s window;
    - channels: no receipts.
  - **rayon:** the delivery service never builds an update path, the only place OpenMLS uses
    rayon, so `api` runs no rayon threads; nothing is configured.
- Evidence (Windows 11, `rustc 1.99.1`; Ryzen 9 7950X; live: SurrealDB v4.0.1 and Valkey 9.1.3
  under podman):
  - `tests/groups/tests/scale.rs`, release: the tables in `P05-M4.md`. At 1,024 X-Wing leaves
    the server takes 8 to 13 ms per commit, and joining from a Welcome takes 45 ms.
  - Harness `a_channel_has_its_own_size_limit`.
  - Send: `channels_have_their_own_rate_class`, `the_channel_budget_refills`. Groups:
    `tiers::tests`.
  - `gate_p05` live, 7 of 7, with `communities_are_run_by_the_announcement_groups_admins`
    (every community cell refused for those who may not, accepted for those who may).
- Consequences:
  - One threat-model §1 row (communities) and one §6 risk (the community graph, channel
    subscribers seeing each other, large commits on fresh trees, the per-node rate class).
  - Apps should commit a key update soon after joining a large group or channel.
  - The caps are configuration (`Limits`); raising the channel cap needs a larger bundle
    limit and a fresh look at what each subscriber downloads.

## D-0071 — Prompt 5, milestone 3: invite links the server cannot read, joins by external commit, and join requests

- Date: 2026-11-16
- Status: accepted; built and passing live (report `P05-M3.md`); part of the Prompt 5 PR
- Decided by: agent, under D-0065 and the standing instruction (Proton-level bar)
- Context: Prompt 5 asks for invite links (with expiry, a use limit and revocation), "admin
  approval for new members", and joining without the server learning more than it must.
- Options considered:
  - **The server holding the link's secret** was rejected. The link carries a secret the app
    splits with HKDF: `auth` for the server, and a key for the group's encrypted info. The
    server stores `SHA-256(auth)`, so its database alone opens no link, and it never sees the
    key to the group's name and picture.
  - **Joining by a Welcome from an admin's device** (the admin's app is online and adds the
    joiner) was rejected for open links: the person would wait for an admin to be online. A
    join is an external commit (RFC 9420 §12.4.3.2) that the server accepts once the link is
    checked. It joins with the default role: external commits cannot change extensions.
  - **Counting a link's use before validating the commit** was rejected: a failed commit would
    spend the use. The use is counted in the transaction that stores the commit, so the last
    use goes to exactly one join, and a retried join that already won is answered again.
  - **Approval as an admin's signature over the request** was rejected as more machinery for
    the same effect: approving someone is adding them in an ordinary commit, which the roles
    rule already checks (`add` role), and the commit answers the request.
  - **Distinct answers for wrong, used-up, expired and revoked links** were rejected: one 404
    for all, and the secret compared in constant time.
- Decision:
  - **Endpoints** (`PROTOCOL.md` §6.8): `POST /v1/groups/links` and `/links/revoke` for the
    `links` role; `/links/open`; `/join`; `/requests`; `/requests/list` and `/requests/decline`
    for the `add` role.
  - **Opening** gives the group's state if the device may join now. If a join needs approval
    (the link's flag, or the group policy's), it gives only the encrypted info, no members.
  - **Limits:** 16 links per group; 256 pending requests per group, lapsing after 7 days;
    links live at most a year.
  - **Budgets:** opening, joining and asking have their own budget per account (1 a second,
    burst 5); managing links and requests uses the commit budget.
  - **Retention:** an hourly sweep forgets lapsed requests and expired links. Used-up links go
    when the group's next link is made.
  - **Registry:** `grp_link` (by handle, indexed by group and expiry) and `grp_req` (by group
    and account, indexed by group and time). `grp_log` gains an indexed `grp` field: reading a
    group's change log no longer scans every group's.
- Evidence (Windows 11, `rustc 1.99.1`; live: SurrealDB v4.0.1 and Valkey 9.1.3 under podman):
  - Harness: a stranger's external commit is refused by `apply_commit`, accepted by
    `apply_join` with the default role, and read by the members afterwards. A member's
    ordinary commit is refused by `apply_join`.
  - `gate_p05` live, 6 of 6. `parallel_joins_never_exceed_a_links_uses`: five people race a
    two-use link, one join per epoch, exactly two get in. `invite_links_and_join_requests`
    checks:
    - the `links` role (403 for a member);
    - a taken handle (409);
    - wrong and unknown links get one answer (404), also when they carry a join;
    - opening and joining by external commit (200, epoch 2), and the members reading the joiner;
    - a lost answer to a join recognised after the link is spent;
    - the use limit, expiry and the sweep, and revocation (403 for a member, then 404);
    - approval: info only on opening, 403 `approval` on joining, asking twice, 409 `member` and
      `open`, the list for members and 404 for outsiders, oldest first, approving by adding
      (the request is gone), declining;
    - opening links at speed: 429.
  - Registry, `api`, send, proto and groups suites: 205; e2e: 45.
- Consequences:
  - Two threat-model §1 rows (links; pending requests naming the account that asks) and one §6
    risk: whether a joiner held a link is the server's word. A malicious server can admit an
    account, but not hide it: every member sees the join, its account and its default role.
  - Apps must derive `auth` and the info key from the link's secret as §6.8 says.
  - A client-checkable link proof (members verifying that a joiner held a link they made) is
    possible later, if the owner wants it: the link's admins would share a MAC key in the
    encrypted info.

## D-0070 — Prompt 5, milestone 2: roles in an MLS extension every client verifies, mirrored on the server; leaving by proposal; encrypted metadata and change log

- Date: 2026-11-16
- Status: accepted; built and passing live (report `P05-M2.md`); part of the Prompt 5 PR
- Decided by: agent, under D-0065 (owner: the matrix, client-only cells) and the standing
  instruction (Proton-level bar)
- Context: Prompt 5 asks for:
  - roles owner, admin, moderator, member and read-only, in an MLS group-context extension that
    every client verifies, mirrored on the server;
  - a permission matrix with a negative test for every cell, on the client harness and on the
    server;
  - an encrypted, versioned metadata blob that only permitted roles may replace;
  - a signed, encrypted change log for newly linked devices.
- Options considered:
  - **Roles per device** were rejected: a person is an account, and their devices share a role.
  - **A role entry for every member** was rejected. Accounts not listed have the policy's
    default role (`Member` in a group, `ReadOnly` in a channel). That keeps adding someone
    a plain Add, and lets external commits (which RFC 9420 forbids from changing extensions)
    keep working.
  - **The server deciding roles from its own records** was rejected: the extension is in the
    MLS group context, so a server that lied about roles would be detected by every client. The
    server only mirrors it (in the roster) for the send path and `api`.
  - **Leaving by a commit of one's own removal** is impossible in MLS (OpenMLS refuses
    `CannotRemoveSelf`). A leaver sends a `SelfRemove` proposal; the server queues it and fans
    it out; the next committer includes it and needs no role to.
  - **The change log in clear for the server** was rejected: entries are ciphertext under a key
    the members share. The server checks only the committer's signature, so it cannot forge one.
- Decision:
  - **The extension** `0xF5B0` (MLS private use; `crates/groups/src/roles.rs`). It holds the
    policy, the least role for each of send, add, edit info, links and call, the default role
    and flags (channel, approval), plus the explicit roles. It has one encoding. A
    `required_capabilities` extension names it and `SelfRemove`, so every member must support
    both (OpenMLS enforces it).
  - **The matrix** is `roles::authorize`, run by the server before merging and by clients
    before accepting:
    - add a person: the `add` role;
    - add a device to an account already present: `Member`;
    - remove someone else: an admin who outranks them;
    - one's own devices and dead devices: anyone;
    - change the policy: `Admin`;
    - change a role: an admin outranking both the old and the new role, or an owner;
    - lowering one's own role: always;
    - at least one owner; when every owner leaves, the committer names a successor;
    - entries only for accounts still present.

    The cells no commit carries:
    - send: the send path, reason 14 `NotAllowed`;
    - edit info: `POST /v1/groups/meta`;
    - links and approval: M3;
    - calls: Prompt 7;
    - pin and delete others' messages: clients only (D-0065 §6).
  - **The roster** in the registry becomes format 2: `version 2 | policy [7] | n × (device,
    role)`. The send path reads it.
  - **Leaving:** `POST /v1/groups/propose` takes only a member's own `SelfRemove`. The group's
    state is compared and set on epoch and a new revision counter.
  - **Metadata:** `POST /v1/groups/meta`, up to 16 KiB, compared and set on its version, for
    roles at least `edit_info`. Returned with the state.
  - **Change log:** bundle version 2 carries one entry (`ciphertext ≤ 2 KiB | Ed25519
    signature` over a label, the group, the commit's epoch and the ciphertext). It is verified
    against the committer's leaf key and stored in `grp_log`, the newest 1,000 per group.
    `POST /v1/groups/log` serves it.
- Evidence (Windows 11, `rustc 1.99.1`; live: SurrealDB v4.0.1 and Valkey 9.1.3 under podman):
  - `crates/groups` `roles` unit tests (8): one encoding; every rule, with leaving and
    succession.
  - `tests/groups/tests/matrix.rs` (8): each commit-borne cell refused for the roles that may
    not, and accepted for one that may, **on the server and on a client**. Also leaving and
    succession, and that only self-removal is a standalone proposal.
  - Change-log tests: an entry signed by another member, or replayed from another epoch, is
    refused.
  - `crates/send`: `only_roles_that_may_send_send`, and rosters with one encoding.
  - `gate_p05` live, 4 of 4:
    - the M1 tests;
    - a member's removal of another refused (403);
    - "only admins can send" (reason 14), metadata by role (403, 200, 409), the change log for
      members, and leaving by proposal.
  - Everything else in the registry, `api`, send, proto and e2e live suites passes (188).
- Consequences:
  - Three threat-model §1 changes: the group row names roles; the metadata ciphertext; the
    change-log entries.
  - Apps must support the roles extension and `SelfRemove` (or OpenMLS refuses to add them).

## D-0069 — The test suite and clippy on Windows (development only)

- Date: 2026-11-16
- Status: accepted
- Decided by: agent
- Context: the owner develops on Windows 11, where `cargo nextest run --workspace --profile ci`
  failed 29 of 600 tests (the same 29 on `main` at `c5a14d1`) and clippy `-D warnings` failed on
  `unused_mut` in code whose only mutation is `#[cfg(unix)]`. CI and the servers run Linux, where
  nothing changes.
- Root causes and fixes:
  - **A refused loopback connection costs about 2 s on Windows** (it retries a refused SYN twice;
    Linux answers at once). The push mocks listened on `127.0.0.1` only behind a
    `https://localhost` URL; Windows resolves `localhost` to `::1` first, so every connection
    spent the whole 2-second connect timeout on `::1` (20 push-provider tests, and the M5 gate's
    "no push reached APNs"). The mock now listens on one port of both loopbacks (`127.0.0.1`
    alone where there is no IPv6 loopback). The dead-first-address fallback stays proved by
    `net::tests::a_dead_first_address_falls_through_to_the_next`. The link test
    `a_stopped_server_serves_nothing_more_even_on_open_connections` ends on that same refused
    reconnection: its bound is 1 s plus `REFUSAL` (2.5 s on Windows, zero elsewhere), still
    inside the 5-second timeout it proves is not waited for.
  - **Windows cannot open a directory as a `File`,** so the mailbox key store's directory fsync
    failed every write (7 `iggy::keys` tests). `sync_dir` is now Unix-only; elsewhere it does
    nothing (NTFS journals the rename; durability across a power cut is not promised on a
    development machine). Production (Linux) is unchanged.
  - **`/proc/no/such/dir` is an ordinary, writable path on Windows** (`C:\proc\...`), so
    `an_unwritable_log_stops_the_action` found its log writable. The test now places the log
    under a regular file, which no platform can create a directory in.
  - **`unused_mut`:** the five `DirBuilder`s that only Unix gives a mode carry
    `#[cfg_attr(not(unix), expect(unused_mut, reason = ...))]`.
- Evidence (Windows 11, SurrealDB, valkey-hot, valkey-jobs and RustFS under Podman, CI's
  environment variables without Iggy and age): clippy `--workspace --all-targets -D warnings`
  clean; the workspace without `songbird-api`, `songbird-jobs` and `songbird-push` 497/497, then
  those three one at a time with `-j 1` as CI runs them: 52/52, 12/12, 7/7. In one
  workspace-wide run, those three crates fight the e2e tests over the shared job streams (on any
  platform), and the jobs left in persistent valkey-jobs then fail the next run's deletion tests.
- **`cargo xtask dev-env`** read `/dev/urandom`, which Windows lacks: its secrets now come from
  `getrandom::fill` (the `getrandom(2)` system call on Linux, `ProcessPrng` on Windows).
  `getrandom` 0.4.3 was already a workspace dependency, so xtask adds an edge, not a crate. On
  Windows `.env` takes its directory's ACL (the user's profile) instead of mode 0600.
- **`cargo xtask compose`** ran `docker compose`, and Podman Desktop on Windows installs no
  `docker` command: with none, it now runs `podman compose` (which drives Docker Compose through
  the Podman machine's API) and applies the Podman override. On a failure there, it says how to
  restore the machine's API (`podman machine stop`, then `start`). On the owner's machine that
  restart alone did not help: `win-sshproxy` refused to start because a stale
  `%TEMP%\podman\podman-machine-default-api.sock` from an earlier run could not be bound again.
  Deleting it, then restarting the machine, restored the API.
- Verified on Windows: `cargo xtask dev-env --force`, then `cargo xtask compose up` (all seven
  services healthy, Iggy and LiveKit included), `ps` and `down`. With the whole stack and CI's
  variables, Iggy's included (`SONGBIRD_REQUIRE_IGGY=1`), the same partitioned run passed again:
  497/497, 52/52, 12/12 and 7/7.
- Consequences: no test is skipped or weakened on Linux, and Linux with `docker` takes the same
  path as before.
## D-0068 — Prompt 5, milestone 1: the supply chain of OpenMLS and the client harness

- Date: 2026-11-16
- Status: accepted; cargo-deny, cargo-audit and cargo-vet pass (CI's versions); the licence
  exceptions **approved by the owner on 2026-11-16** with the M1 gate ("Approve all three")
- Decided by: agent, under D-0065 (owner)
- Context: D-0065 §1 brought OpenMLS into the workspace, and D-0066 the test harness on OpenMLS's
  RustCrypto provider. cargo-deny refused three crates' licence and one advisory; cargo-vet
  wanted 116 crates vetted.
- Evidence and decision:
  - **Licences:** `hpke-rs`, `hpke-rs-crypto` and `hpke-rs-rust-crypto` are MPL-2.0. They come
    only through `openmls_rust_crypto` in `tests/groups`, which no service links (the
    `crates/groups` manifest guard). MPL-2.0 is file-level copyleft; it binds code that is
    distributed, which test crates are not. Per-crate exceptions, these three only.
  - **Advisory RUSTSEC-2026-0387** (`crypto-bigint` 0.5.5: secret-dependent branches in
    constant-time arithmetic). Ignored with a review date of 2027-02-16:
    - it reaches the tree only through the harness's RustCrypto provider (its P-256/P-384 code);
    - Songbird groups use X-Wing and Ed25519, never those curves, and a test run has no
      observer for a timing side channel;
    - the fix (0.7.6) needs `elliptic-curve` 0.14, which OpenMLS's providers have not adopted.
  - **cargo-vet: 116 exemptions,** each with a note. cargo-vet resolves every optional feature,
    so OpenMLS's optional backends (libcrux, RustCrypto, SQLite storage) count as dependencies
    of `crates/groups` even though `default-features = false` never builds them. A
    `safe-to-run` policy for the harness changed nothing for that reason, and was not kept.
    - **11 are in the server build** (`api` only; delivery, worker and push do not link OpenMLS,
      shown by `cargo tree -i openmls`): `openmls`, `openmls_traits`,
      `openmls_serialization_helpers`, `tls_codec`, `tls_codec_derive`, `rayon`, `rayon-core`,
      `crossbeam-deque`, `crossbeam-epoch`, `postcard`, `cobs`. Noted "audit backlog".
    - **105 are never built for a server.** Noted as such.
    - Totals now: 167 fully audited, 26 partially, 374 exempted.
  - **cargo-audit** (0.22.2) reads every lockfile entry, so it also reports OpenMLS's optional
    backends, which cargo-deny's feature graph leaves out. Six findings, none on `main`
    (479 crates, clean), all ignored in a new `.cargo/audit.toml` with their reasons:
    - RUSTSEC-2026-0387 (`crypto-bigint`, above): built in the test harness only;
    - RUSTSEC-2026-0329, -0330 and -0331 (`libcrux-hmac-drbg`, `libcrux-kem`: panics on some
      lengths and short keys), RUSTSEC-2026-0459 (`libsqlite3-sys`: an SQLite CVE) and
      RUSTSEC-2026-0173 (`proc-macro-error2` unmaintained): never built at all (`cargo tree
      -i` finds no path, tests included).
- Consequences: the audit backlog grows by 11 crates that `api` runs, and by 105 that ship
  nowhere. On Windows, `cargo vet` calls the store "not correctly formatted" until `cargo vet fmt`
  rewrites its line endings; the content does not change (a cargo-vet quirk; CI runs on Linux).

## D-0067 — Prompt 5, milestone 1 (integration): groups in the registry, commits over `api`, group messages fanned out from the roster

- Date: 2026-11-16
- Status: accepted; built and passing live; the M1 gate (`P05-M1.md`) and the two
  threat-model §1 rows **approved by the owner on 2026-11-16**
- Decided by: agent, under D-0065 (owner) and the standing instruction (Proton-level bar)
- Context: D-0066's core needed a home for each group's state, an authority for its epoch, a
  way for commits and Welcomes to reach devices, and a send path for group messages.
- Options considered:
  - **Keep the epoch in the mailbox** (D-0026, flag `0x01`). Rejected: a commit must be
    validated against the group's state before anyone stores it, and that state lives with the
    registry, not in each mailbox partition. The flag is retired before the gate.
  - **Fan out from a worker job.** Rejected: the worker has no send path, `api` does (D-0044).
    `api` fans out itself and re-drives what a crash interrupted.
  - **Let clients list a group message's recipients,** as a shared body does today. Rejected:
    the server knows the roster. Fanning out from it means a removed device never receives a
    byte, and membership replaces D-0064's interim contact-key rule.
  - **Fan-out keys from the group and epoch alone.** Rejected: a member could send something
    under the same key to the same devices first, and the real commit would be dropped as a
    replay. The keys are an HMAC under a key derived from the cluster secret.
  - **Delivery reads `PrivateMessage`s through `crates/groups`.** Rejected: delivery would link
    OpenMLS. The allocation-free prefix parser moved to `songbird-proto` (`mls.rs`).
- Decision:
  - **Registry** (`crates/accounts/src/groups.rs`): `grp:[group]` holds the epoch, the
    D-0066 record, the roster (16 bytes per device, sorted) and the hash of the last commit.
    `commit_group` compares and sets the epoch and writes `grp_fanout:[group, epoch]` in one
    transaction (the `put_device_list` pattern).
  - **`api`** (`services/api/src/groups.rs`, PROTOCOL §6.2–6.4): `POST /v1/groups`,
    `/v1/groups/commit` and `/v1/groups/state`.
    - A per-device commit budget (2 a second, burst 10, keyed records).
    - CPU work in `spawn_blocking`.
    - The answer only after every recipient's mailbox stored the fan-out.
    - A retried winning commit answered 200 again.
    - `redrive` every 5 s for fan-outs older than 10 s.
  - **Send path** (`crates/send/src/groups.rs`, PROTOCOL §6.5): the `GROUP` flag `0x40` and
    the `Rosters` trait, with a per-node cache (papaya, at most 1 s, refreshed at once for a
    newer epoch). The message's epoch may be the roster's or the one before. New reasons:
    12 not a member, 13 invalid group message.
  - Pending devices count as unknown to groups (`Directory::listed_account`).
- Evidence (Windows 11, `rustc 1.99.1`):
  - `crates/send`: `group_messages_go_to_the_roster_and_only_from_members` (roster only, not
    the sender, previous epoch accepted, seven refusals with nothing stored) and
    `group_messages_without_rosters_are_unavailable`.
  - `crates/proto`: `group_messages_name_the_group_and_list_no_devices` (round trip, every flag
    rule) and the prefix parser's tests.
  - 219 tests across the changed crates pass; clippy `-D warnings` is clean on them.
  - `tests/e2e/tests/gate_p05.rs`, live (SurrealDB v4.0.1 and Valkey 9.1.3, the pinned images,
    under podman; memory mailbox), **3 of 3, ten runs in a row (30 of 30)**:
    - create, add, Welcome, state, a group message, removal, and a removed device refused
      after the roster cache's second;
    - five members committing concurrently over HTTP, one winner per epoch, no fork;
    - a retried commit answered once; an interrupted fan-out re-driven exactly once.
  - The existing live suites with the new schema and send path: `crates/accounts`,
    `services/api`, `crates/send`, and in `tests/e2e`: `invariants`, `send_api`,
    `registration`, `restore`, `allocations`, `receipts_load`. All pass.
  - `gate_m5`, `gate_p03` and `gate_p04` need RustFS and `valkey-jobs` and **did not run here**.
- Consequences:
  - Threat model §1 gains two rows (below in the M1 report), for the owner.
  - The second-long window for removed devices is an open risk (§6).
  - Groups are not yet in the GDPR export and deletion; they get there with M2's roles, since
    an account's deletion must also remove its leaves.

## D-0066 — Prompt 5, milestone 1 (core): OpenMLS `PublicGroup` behind a verify-only aws-lc-rs provider; X-Wing `0x004F`; group state as postcard; commits over HTTP

- Date: 2026-11-16
- Status: accepted; built (`crates/groups`, `tests/groups`); integrated by D-0067; the M1 gate
  **approved by the owner on 2026-11-16**
- Decided by: agent, under D-0065 (owner) and the standing instruction (Proton-level bar)
- Context: D-0065 §1 and §3 needed a concrete provider, ciphersuite, state format and transport.
- Options considered and evidence (Windows 11, `rustc 1.99.1`, release build):
  - **The server's provider: verify only.** Hashing, Ed25519 verification (with D-0048's
    honest-key check), HMAC and HKDF on aws-lc-rs; AEAD, HPKE, signing and key generation return
    errors. The delivery service never holds a group secret, so it cannot be made to use one.
    - Known answers: RFC 5869 case 1, RFC 8032 test 1.
    - OpenMLS's RustCrypto provider plays the clients and interoperates byte for byte (every
      harness test).
  - **The ciphersuite: X-Wing `0x004F`**
    (`MLS_128_MLKEM768X25519_AES128GCM_SHA256_Ed25519`, draft-ietf-mls-pq-ciphersuites,
    provisional code point).
    - Chosen over `0x004D` (an older X-Wing draft suite that only libcrux implements); RustCrypto
      implements `0x004F`.
    - Groups accept only `GROUP_CIPHERSUITES` (one constant). The KeyPackage upload check (D-0051)
      still allows `0x0001`/`0x0003` until M1's integration extends it.
    - A provisional code point that changes later means new groups under the new value, since
      every client is ours.
  - **Stored state: OpenMLS's public storage format, as postcard, tagged with its storage
    version.** This replaces the plan's "RFC 9420 wire bytes", because OpenMLS rebuilds a
    `PublicGroup` from them only through `from_external`. That call checks the lifetimes of
    never-updated leaves, so a member idle beyond 100 days (D-0051) would make the group
    unloadable.
    - postcard 1.1.3 (`cobs`, `serde`; a specified, stable format) instead of JSON: the stored
      tree is 1.67 KB per member against 6.26 KB (21 X-Wing members: 35,121 bytes against
      131,461).
    - Records failing to decode are refused (fuzz target `mls_commit`).
  - **Commits travel over HTTP to `api`, not in the WebSocket `Send` frame.** X-Wing commits grow
    about 1.2 KB per unmerged leaf (adding 20 members one at a time: 5,479 to 32,611 bytes), past
    the 64 KiB envelope and 1 MiB frame limits long before 1,024 members.
    - Group application messages stay on the WebSocket. There the send path reads only the
      `PrivateMessage` prefix (group, epoch, content type), allocation-free, and checks the
      roster.
    - M4 measures commits and Welcomes at 1,024 and 5,000 members; D-0065 §3 comes back to the
      owner if they are too large.
- Decision (Songbird's checks, on top of OpenMLS's RFC 9420 validation, before anything is
  merged):
  - a member commit is signed by the leaf whose credential is the authenticated device;
  - credentials are basic, naming a 16-byte device the directory knows, and are never changed by
    an update;
  - only add, remove, update, self-remove, and external init (external commits only) proposals;
  - an external commit only for an account already in the group, removing at most the joining
    device's own old leaf (RFC 9420 §12.4.3.2). A reinstalled device is a new device: it joins,
    then a member commit removes the dead leaf;
  - at most 1,024 leaves and a 4 MiB bundle;
  - the Welcome has exactly one secret per added KeyPackage;
  - the uploaded `GroupInfo` is the new epoch's, signed by the committer;
  - the group is consumed by `apply_commit` and returned only on success, so half-checked state
    cannot be used.
- Evidence:
  - `tests/groups`:
    - `lifecycle`, 11 tests: every refusal above, adds and removes and updates, reloads from
      stored bytes, an application message readable by the others;
    - `convergence`: six concurrent commits, one winner per epoch, no fork; and a property test,
      24 random sequences of adds, removals and races, in which every honest client always has
      the server's epoch and roster and can read every other member.
  - `crates/groups`: 9 unit tests, among them the wire parsers' malformation and minimality
    tests and the manifest guard (no service links OpenMLS's client providers or the harness).
  - Fuzz targets `mls_wire` and `mls_commit`, written and type-checked. **Not yet run:** this
    machine has no nightly toolchain; they run before the M1 gate.
- Consequences:
  - New crates in the server build: `openmls`, `openmls_traits`,
    `openmls_serialization_helpers`, `tls_codec`, `tls_codec_derive`, `rayon`, `rayon-core`,
    `crossbeam-deque`, `crossbeam-epoch`, `postcard` and `cobs`. Each needs a cargo-vet entry.
  - `Cargo.lock` grows from 479 to 600 entries. Most are OpenMLS's optional backends, which
    Cargo resolves but never builds, plus the harness's test-only RustCrypto.
  - OpenMLS upgrades must keep stored records loadable (a fixture test with the integration).

## D-0065 — Prompt 5: the plan, and the owner's decisions it needed

- Date: 2026-11-16
- Status: accepted (owner, 2026-11-16: "Approve all")
- Decided by: owner (every recommendation below), on the agent's plan `docs/reports/P05-plan.md`
- Context:
  - Prompt 5 asks for "OpenMLS for validation", but D-0051 kept OpenMLS off the server
    (194 crates with its crypto providers).
  - Server-side authorization needs a roster, which is group metadata.
  - Issue #24 asks for post-quantum groups.
  - Public channels and delivered receipts in large groups are open owner decisions.
- Options considered and evidence: in the plan ("Findings"). In short:
  - OpenMLS's core needs no crypto backend; aws-lc-rs 1.19.0 has ML-KEM-768 and SHA3-256 for
    X-Wing.
  - Two permission cells (pin, delete others' messages) live inside ciphertext the server never
    sees.
- Decisions (the agent's recommendations, all accepted by the owner):
  1. OpenMLS `PublicGroup` on the server with Songbird's own aws-lc-rs provider; D-0051's parser
     stays the upload check.
  2. The server keeps the roster (devices, accounts, roles, epoch) and nothing else in the
     clear; a new threat-model row.
  3. X-Wing by default for every new group, unless the 1,024-member benchmark says otherwise.
  4. No public channels in Prompt 5.
  5. Delivered receipts from every device up to 32 members, primaries only from 33 to 1,024,
     none in channels.
  6. Pinning and deleting others' messages are enforced by clients only.
  7. One branch, one PR, a commit per milestone (M1 to M5).
- Consequences: OpenMLS (core only) joins the workspace behind a Songbird provider on aws-lc-rs;
  new groups are post-quantum; a roster row goes to threat model §1 for approval with M1; public
  channels stay an open owner decision for a later prompt.

## D-0064 — Prompt 4: the independent security review's findings, fixed

- Date: 2026-11-16
- Status: accepted; built (report `P04-gate.md`, "Security review"); amends D-0059, D-0061,
  D-0062 and D-0063; **approved by the owner on 2026-11-16** with the Prompt 4 gate
- Decided by: agent, at the owner's bar
- Context: the review of `120c7a5..HEAD`, by an independent read-only reviewer, found one high,
  two medium, one low–medium and two low issues, all confirmed against the code.
- Decision (the fixes):
  1. **High: a shared body skipped the contact gate and blocks.** A send with the shared-body
     flag reached any closed account with no key, no request budget and no block check.
     - Fix: a shared body is classified like a broadcast. Every closed account it reaches needs
       its key in the combined `ACCESS` key, which the frame now allows with a shared body; a
       request is never a shared body.
     - There is no allocation when every account is open (the development directories and the
       allocation gate). Prompt 5 replaces this with group membership.
  2. **Medium: a block could be detected through idempotency.** A dropped request wrote no
     idempotency record, so reusing its key (for other devices, or a subset) answered
     differently.
     - Fix: a dropped send now stores the whole send, under its own key, already expired: never
       fetched or routed, swept. The record and every answer match a delivered send's.
     - The sender's own envelopes go on live under a derived key. The expired copy waits for the
       next sweep (minutes); the risk is listed in threat model §6.
  3. **Medium: a linked device could open the account,** bypassing the primary-only loosening
     rule, and blocks never applied to open accounts.
     - Fix: opening the account (false to true) is refused unless the primary asks, checked in
       the profile's transaction (`put_profile_as`).
     - Sends to an open account go through a new `Open` lane, where the gate drops those from
       senders it blocked. That is one registry lookup, for open accounts only.
  4. **Low–medium: the profile key was inside the committed content,** so a report would hand
     moderators the sender's profile key, and with it its access key.
     - Fix: it moves to `Plaintext.profile_key`, beside the franking key, outside what a report
       reveals. `Content` field 5 is reserved.
     - There are now 33 vectors (new: `plaintext_accept`), and the independent compiler agrees.
     - `songbird-admin reports open` prints the rendered message, not raw bytes.
  5. **Low: a sender missing from the snapshot was stamped as sealed.** It is now refused as
     unavailable (retry); only the sealed endpoint's sentinel is sealed.
  6. **Low: configuration gaps.**
     - `max_broadcast_accounts` must not exceed `broadcast_burst`.
     - Every node pins a keyed fingerprint of the requests secret in the registry, and a node
       with another secret refuses to start. A worker with the wrong secret could otherwise "verify"
       a deletion that missed the block rows.
     - A franking key that differs between delivery and `api` stays a deployment check (runbook).
  7. **Notes:**
     - keyed tags are now length-framed;
     - the directory loader's comment is corrected;
     - group-report dedupe (the first reporter takes a shared body's slot), re-reporting after
       deletion and replays past a deadline are recorded in the gate report.
- Evidence:
  - new unit tests:
    - shared bodies need every closed member's key;
    - an unknown sender is refused;
    - open accounts take anyone, but their blocks apply;
    - a dropped request's key, reused for other devices or a subset, gets reason 7 as an
      undropped one does, and a retry is answered as a retry with the sync transcript once;
    - the profile key never appears in the committed bytes;
  - `blocks_live`: only the primary opens an account; a node with another secret is detected;
  - `gate_p04`: a shared body refused without the keys and delivered with them;
  - `tools/content-vectors` 33 of 33; all suites re-run (`P04-gate.md`).
- Consequences: groups need their members' profile keys (shared inside the group) until
  Prompt 5. A dropped send briefly occupies the recipient's mailbox, unreadable, until the next
  sweep.

## D-0063 — Prompt 4, milestone 5: reporting with message franking; reports sealed to an offline moderation key

- Date: 2026-11-16
- Status: accepted; built (report `P04-M5.md`, and the Prompt 4 gate `P04-gate.md`); the gate
  and the two threat-model §1 rows **approved by the owner on 2026-11-16**
- Decided by: agent, under the owner's plan (D-0058) and standing instruction (Proton-level bar)
- Context: Prompt 4: "The reporter submits the plaintext and the franking opening; the server
  verifies the commitment it stamped at send time, then queues a minimal report for moderation
  with short retention. Nothing else about the conversation is revealed."
- Options considered:
  - **Storing reports in the clear** for moderators was rejected: the registry and its backups
    would hold message plaintext. Reports are sealed to a moderation key whose private half stays
    offline, so the servers can verify and queue but never read them back.
  - **Naming the reporter in the report** was rejected as more than moderation needs; repeat
    reports are stopped by the keyed stamp ID and the budget.
  - **Several messages (context) per report** was rejected: the prompt says nothing else about
    the conversation, and each message carries its own stamp, so an app may send several reports.
  - **Decoding the content on the server** to check it is a message was rejected (the server
    never links `songbird-content`); moderators decode it in `songbird-admin`.
- Decision:
  - `POST /v1/reports` with the binary body of PROTOCOL §5.14 (`songbird_proto::report`, fuzzed).
  - **Checks, in order:** the reporter's budget (spent first), the franking key, the 30-day
    window, the stamp (sender attested or sealed; this recipient or a group), then the commitment
    (constant time).
  - **The minimal report:** eight fields, sealed with `seal_with(Domain::Report, …)` (salt
    `songbird.v1/report`). The row is named by a keyed hash of the stamp's tag (one per
    message), kept 30 days (at most 90), and swept by the worker.
  - **`songbird-admin reports`:** `keygen`, `list`, `open` (decrypt and decode) and `delete`
    (audit-logged).
  - `[api.reports]` is required in staging and production, and needs `[api.requests]` and
    `[api.send] franking_key`.
- Evidence (Apple M5 Pro, macOS 27.2.1):
  - verification known answers (identified, sealed, group, other reporter, other sender, other
    words or key or commitment or time, forged tag, unknown or previous key, too old, future);
  - the commitment's known answer equals `songbird_content`'s, and the live test checks both
    implementations agree on random keys;
  - `gate_p04` live:
    - a request reported by its recipient, queued sealed;
    - duplicate 409; another reporter, other words or another sender 403; garbage 400;
    - only the moderation key opens it, the content is not in the clear, and no reporter is in
      it;
    - floods give 204, 409 ×4, 429 ×2;
    - export and deletion of Prompt 4's stores;
  - admin seal and open tests; fuzz `franking` (with reports), 60 s: 27,774,418 runs.
- Consequences: a moderation key for the owner to generate and hold offline; two threat-model §1
  rows. Reports of sealed sends name a claimed sender only.

## D-0062 — Prompt 4, milestone 4: the contact gate, message requests, blocking, rate limits per conversation type, broadcast lists

- Date: 2026-11-16
- Status: accepted; built (report `P04-M4.md`); the gate and the three threat-model §1
  rows **approved by the owner on 2026-11-16**
- Decided by:
  - owner: access key or request (D-0058 §3); tighten from any listed device, loosen from the
    primary (D-0058 §4);
  - agent: the design below.
- Context: Prompt 4's server part: message requests the recipient controls, rate-limited; a
  server-side drop of blocked identified senders with block lists stored as little as possible;
  rate limits per conversation type; broadcast lists rate-limited as bulk.
- Options considered:
  - **A contact list on the server** was rejected: it is a social graph. The sender proves
    contact by holding the recipient's access key, which only contacts hold (D-0054).
  - **Checking access keys with a store query per send** was rejected (hot path). The directory
    snapshot every node already holds (D-0036) gains each account's key, open flag and request
    policy; profile and policy changes bump its version.
  - **Blocks as account IDs** were rejected. They are keyed hashes of the pair, with a keyed
    owner tag and a per-owner counter, and the full list syncs between the user's devices in
    encrypted content.
  - **Telling a blocked sender** was rejected (WhatsApp does not either). To keep the block
    invisible:
    - its requests are accepted and dropped;
    - the budgets are spent before the block check;
    - the timing difference is stated as an open risk.
  - **Requests over HTTP only** were rejected: one gate behind a trait (`songbird_send::Gate`),
    used by the WebSocket and `PUT /v1/messages` alike.
- Decision:
  - **Lanes** (PROTOCOL §5.10): sync, direct, broadcast, request, group, sealed, decided from the
    snapshot. No allocation for 1:1 sends; access keys compared in constant time. Reason 10
    ("access required") and 11 ("invalid request").
  - **The gate** (`RegistryGate`):
    - budgets per sender, per pair and per recipient for requests, and accounts per hour for
      broadcasts. These use a cost-weighted GCRA in `valkey-hot` under keyed names and fail
      closed;
    - then the request policy and the block row in SurrealDB.
    The keys derive from `[api.requests]` and `[delivery.requests] secret` (and the worker's
    `requests_secret`).
  - **Endpoints:** `PUT`/`DELETE /v1/blocks`, `GET`/`PUT /v1/account/requests`, with D-0058's
    tighten/loosen rule.
  - **Per-class rate limits:** group sends get their own budget within the per-device one (10 a
    second, burst 40, on both transports). Requests and broadcasts are budgeted per account
    across the cluster. Channels wait for Prompt 5.
  - **GDPR:**
    - the worker deletes an account's block rows by owner tag and verifies none are left;
    - the export reports the policy and the block count;
    - the policy flag goes with the account row.
  - **Static development directories** treat every account as open, so the delivery invariants,
    load and allocation tests are unchanged; a registry directory is closed by default.
  - `SEALED_SENDER` moves into `songbird-send`; the sealed endpoint refuses `ACCESS`/`REQUEST`.
- Evidence (Apple M5 Pro, macOS 27.2.1; compose stack under podman without Iggy):
  - `crates/send`, 22 tests, among them every lane's rules, silent drops, floods (one to many,
    many to one, one pair), a blocked sender seeing the same budgets, broadcasts (keys, size,
    budget), groups and sealed sends unaffected, and no gate meaning unavailable;
  - `blocks_live` (2): keyed rows; 1,020 racing adds never exceed 1,000; deletion by owner; the
    policy and profiles bump the directory;
  - `gate_p04` (2, live): every rule end to end over the WebSocket and HTTP, with
    stamps verified, the linked device refused to loosen, and floods held cluster-wide;
  - the earlier live tests updated to the contact flow (`registration`, `gate_m5`, `send_api`)
    pass, and the non-live suite passes (555).
- Consequences:
  - Apps must publish a profile at registration and handle reasons 10 and 11.
  - Blocking rotates the profile key (a client rule).
  - Three threat-model §1 rows.
  - `[api.requests]`, `[delivery.requests]` and the worker's `requests_secret` are required in
    staging and production.
  - The open risks (timing, sybil recipient budget, snapshot reloads, ephemeral frames) are in
    threat model §6; shared bodies, idempotency and open accounts were fixed by D-0064.

## D-0061 — Prompt 4, milestone 3: franking stamps and disappearing envelopes; the `Send` frame's hints

- Date: 2026-11-16
- Status: accepted; built (report `P04-M3.md`); the gate and the two threat-model §1 rows
  **approved by the owner on 2026-11-16**
- Decided by: agent, under the owner's plan (D-0058) and standing instruction (Proton-level bar)
- Context: Prompt 4 asks for reporting with message franking (the server verifies the commitment
  it stamped at send time) and for an envelope flag that expires undelivered envelopes early.
  Both need something the server can see; the content stays opaque.
- Options considered:
  - **Hints inside the ciphertext** were rejected: the server would have to parse client content.
  - **A new protocol version** was rejected while no app exists. Instead, v1's `Send` frame gains
    four optional blocks behind new flag bits. Unknown bits stay refused, and each block appears
    only with its flag, in a fixed order, so there is still one encoding per frame.
  - **A stamp that names the sender** was rejected: it would store the sender with the envelope in
    the clear (threat model §1, "never stored with the envelope"). It is a MAC over the accounts
    instead.
  - **A deadline in a server-written envelope header** was rejected: Iggy rebuilds its index from
    record metadata, without opening envelopes. The deadline goes into the records instead (sealed
    format 5).
  - **A new Iggy record kind for dropped entries** was rejected as unnecessary: an expired
    entry's key is punctured, so replay leaves it out (punctured slots are absent), and the
    deadline in the metadata re-drops anything a crash left unpunctured.
- Decision:
  - **Frame:** `ACCESS 0x04` (16-byte key), `REQUEST 0x08`, `FRANKED 0x10` (32-byte commitment),
    `EXPIRES 0x20` (`u32` seconds, 1 to 30 days). Access or request, never both, and neither on a
    shared body; a request is never an MLS commit. Reasons 10 (access required) and 11 (invalid
    request) are defined here and enforced in M4.
  - **Stamps:** envelopes for another account are stored as `stamp ‖ envelope`, with a MAC by the
    franking key over the key ID, commitment, time, sender and recipient accounts (zeros where
    unknown or shared). The sender's own devices are not stamped. One allocation per stamped
    envelope; unfranked sends are unchanged.
  - **Reserved prefix:** `F5 53 42` covers system frames, stamps and any later server format.
  - **Deadlines:** `StoreRequest::expires_after`, kept by every mailbox:
    - memory: per entry;
    - SurrealDB: `exp` with an index;
    - Iggy: in the store metadata and the relocated slots (format 5; 3 and 4 still read), carried
      over node links (wire 2) and by the re-driver.
    Fetch skips expired entries, and the sweep deletes them (Iggy punctures their keys).
  - **Keys:** `[delivery] franking_key` and `[api.send] franking_key` (+ `previous_franking_keys`
    for reports), required in staging and production. Without a key a franked send is refused as
    unavailable.
- Evidence (Apple M5 Pro, macOS 27.2.1):
  - the stamp's known answer, recomputed with Python's `hmac` and `hashlib`;
  - stamps bound to key, sender, recipient, time and commitment; own devices unstamped; shared
    bodies once; sealed without a sender; forged prefixes refused;
  - frame vectors and flag rules, with proptest round trips including hints;
  - deadlines in the memory mailbox, SurrealDB (live) and the Iggy state (unit; the Iggy live
    test runs in CI);
  - fuzz: `franking` 14,180,569 runs, `proto_client_frame` 7,624,430 and `iggy_record` 147,775 in
    30 s each, nothing found.
- Consequences: no new dependency. Iggy's sealed format is 5 and the node-link wire format is 2;
  every node of a cluster runs one build. Two threat-model §1 rows for the owner.

## D-0060 — Prompt 4, milestone 2: order-independent chat state and message status; receipt batching sized by measurement

- Date: 2026-11-16
- Status: accepted; built (report `P04-M2.md`); **gate approved by the owner on 2026-11-16**
- Decided by:
  - owner: group read receipts follow the setting; 24-hour edit and delete windows (D-0058);
  - agent: the semantics below.
- Context: Prompt 4 asks that polls tally the same on every device whatever the arrival order
  ("last vote per voter wins, by a per-voter counter"), proven by property tests; statuses that
  only move forward; batched and coalesced receipts; group aggregation as members come and go;
  and a measured share of receipt traffic in a 1,024-member group.
- Decision:
  - **Convergence by construction:**
    - per-author slots keep the greatest `(counter, value)`: reactions, votes, RSVPs, edits;
    - shared chat state takes the greatest `(time, author, message ID)`: pins, timer, policy;
    - every state is a function of the set of operations.
  - **Closing a poll fixes its result:**
    - the author's close lists each voter's counter as the author saw it;
    - only those votes count afterwards, so a late vote cannot make devices disagree;
    - closes by others, or naming another poll, are dropped.
  - **Edit and delete windows** are judged from the messages' own times, never from arrival.
  - **Status:** each recipient's greatest receipt. A group's audience is fixed at send time;
    members who leave are dropped, and joiners are never counted.
  - **Batching window:** 1 s, and **10 s in groups above 32 members**, chosen from the load test
    below. Every device that saves a message sends a delivered receipt, which keeps "at least
    one device" exact.
- Evidence (Apple M5 Pro, macOS 27.2.1, release build):
  - property tests, 512 cases each, two random orders with random repeats: polls (with closes,
    forged closes and invalid votes), reactions and RSVPs, edits and deletes, pins, timers and
    policies, and statuses (forward only);
  - a broken merge ("the last arrival wins") fails at once;
  - known cases: a closed poll, group ticks as members leave and join, a receipt lifting a failed
    message, 200 unread messages giving one read receipt, splitting above 512, the setting off.
  - **Load test** (`tests/e2e/tests/receipts_load.rs`): the real coalescer drives the real send
    path. Setup: 1,024 members with 2 devices each; 60 messages in an hour; 20 % of devices offline
    and catching up; 3 chat openings per member. Receipt share of all envelopes:

    | Traffic | Window | Delivered from | Receipts |
    | --- | --- | --- | --- |
    | Steady, random authors | 1 s or 10 s | every device | 72.6 % |
    | Steady, random authors | 1 s or 10 s | primaries only | 62.6 % |
    | Bursts of two people chatting | 1 s | every device | 65.9 % |
    | Bursts of two people chatting | 1 s | primaries only | 51.0 % |
    | Bursts of two people chatting | **10 s** | **every device** | **47.2 %** (chosen) |
    | Bursts of two people chatting | 10 s | primaries only | 34.3 % |

    Receipts go to each author only (the prompt's rule), so they batch only across one author's
    messages, and "read up to here" saves little in a big group whose messages have many
    authors (9,624 read receipts for 47,360 messages read in the chosen case).
- Consequences: no new dependency. Large groups see receipt envelopes close to half of all
  traffic. Delivered receipts from primaries only would save another 13 points, at the cost of
  "delivered" waiting for the primary; this is left to the owner (P04 gate) and to Prompt 5's
  group tiers.

## D-0059 — Prompt 4, milestone 1: the encrypted content schema (`crates/content`), never linked by a service

- Date: 2026-11-16
- Status: accepted; built (report `P04-M1.md`); **gate approved by the owner on 2026-11-16**
- Decided by: agent, under the owner's plan (D-0058) and standing instruction (Proton-level bar)
- Context: Prompt 4 asks for the content inside the encrypted payload, "specified so future apps
  interoperate exactly": protobuf, versioned, tolerant of unknown fields, never decoded by the
  server, with byte-exact vectors and a compatibility suite.
- Options considered:
  - **Types generated at build time** (`prost-build` with `protoc`) was rejected. It needs
    `protoc` on every build machine and puts generated code out of review. Instead, `prost`
    types are written by hand, field for field, as `crates/proto` already does. The `.proto` file
    is the specification, and an independent compiler proves the two agree.
  - **Signal's single `DataMessage` with every part optional** was rejected in favour of `oneof`
    kinds: a message is exactly one thing (a message, a reaction, an edit…), which makes
    validation and app code unambiguous.
  - **UTF-16 offsets for spans** (Signal) were rejected in favour of UTF-8 bytes:
    one unambiguous unit, checkable on character boundaries.
- Decision:
  - **Layers:**
    - the session layer's ciphertext;
    - a padded `Plaintext` (`0x80 0x00*` to a multiple of 160 bytes);
    - `Plaintext { content bytes, franking key }`;
    - `Content` (version, required version, message ID, time, profile key, and one body: data,
      receipt, typing, sync or request response).
  - **Franking commitment:** `HMAC-SHA256(franking_key, "songbird.v1/franking\0" || content)`
    over the exact content bytes. The server stamps it (M3); a report submits the bytes and the
    key (M5).
  - **Validation after decoding:** every list, string and ID bound written in the `.proto`, with
    sizes checked before decoding. Unknown fields, kinds and enum values are accepted, so a newer
    writer is never refused. `required_version` drives "update to see this".
  - **Message IDs:** 16 random bytes per message; (author, ID) deduplicates resends.
  - **The server never decodes content:** no service or server crate may depend on
    `songbird-content`, enforced by a test over every manifest.
  - **`Debug` prints type names only** (invariant 6 in apps too).
- Evidence (Apple M5 Pro, macOS 27.2.1):
  - 16 tests in `crates/content`: padding, franking known answer (recomputed with Python's
    `hmac`), bounds, a newer writer's fields at three depths, unknown kinds and enums, the
    manifest check, and property tests;
  - **33 vectors** in `vectors/v1/vectors.json`. Their protobuf JSON is written by hand
    (`make_json.py`) and their hex comes from the Rust types. `tools/content-vectors` (protox
    0.10.0 and prost-reflect 0.16.5, outside the workspace) compiles `content.proto` and
    encodes each JSON to exactly that hex, with no unknown field on decoding: **33 of 33**. With
    one field retyped (`sint32` to `int32`) it fails at once;
  - fuzz `content`, 60 s: 7,892,926 runs, nothing found.
- Consequences: no new dependency in the workspace (`prost`, `aws-lc-rs`, `thiserror`); the tool
  brings `protox` and `prost-reflect` outside it. Apps generate their types from
  `content.proto`, and attachment pointers are extended by Prompt 6.

## D-0058 — Prompt 4: the plan, and the owner's decisions it needed

- Date: 2026-11-16
- Status: accepted (owner, 2026-11-16)
- Decided by: owner (each answer below), on the agent's plan `docs/reports/P04-plan.md`
- Context: Prompt 4 leaves group read receipts to the owner (the open decision above); its
  message-request rule changes how identified sends are admitted (PROTOCOL §3.1); blocks and the
  request policy are account-wide changes (D-0052); edit and delete limits are product choices.
- Decisions:
  1. **Plan agreed:** five milestones on one branch and one PR (the owner's request for speed):
     M1 the content schema (`crates/content`); M2 convergence and message status; M3 franking
     and disappearing envelopes; M4 message requests, blocking, rate limits per conversation
     type, broadcast lists; M5 reporting and the Prompt 4 gate.
  2. **Group read receipts follow the read-receipt setting**, reciprocal, as in Signal.
  3. **Contact gate:** every send to another account carries that account's access key (the
     account is open, or the key is in the send) or goes as a message request: rate-limited,
     franked, blockable. Blocking rotates the profile key (a client rule).
  4. **Blocks and the request policy:** any listed device may tighten them (block, refuse
     requests); only the primary may loosen them (unblock, accept requests again). Pending
     devices change nothing.
  5. **Edit window and delete for everyone: 24 hours each**, as in Signal.
- Consequences: identified sends between accounts need the access key from Prompt 4 on, so every
  account should publish a profile (with its access key) at registration; apps must handle the new
  refusal reason by sending a request or fetching the new profile key.

## D-0057 — Key transparency on Meta's akd (supersedes D-0055's own log)

- Date: 2026-11-16
- Status: accepted; built (report `P03-akd.md`, PR #55); **approved by the owner on 2026-11-16**
  ("Approve D-0057"), with the corrected supply-chain cost; supersedes D-0055's own log
- Decided by:
  - owner: adopt `akd` now ("we can use akd now", 2026-11-16);
  - agent: the integration below.
- Correction to D-0055: `akd` 0.13 with the features it needs adds **27 crates to the audit
  backlog** (cargo-vet exemptions), not the 12 measured with its defaults. Two things were
  missed:
  - its `public_auditing` feature is required to compile (it imports `akd_core::proto`
    unconditionally) and brings protobuf code generation at build time (`protobuf-codegen`,
    `regex`, `which`, `tempfile`);
  - it brings a second curve25519/Ed25519 stack (`curve25519-dalek` 4.1.3, `ed25519-dalek`).
  Imported audits cover seven of the new crates. The licences are permissive. The `public_tests`
  helpers (MPL-2.0 `colored`) are not used, so the licence policy is unchanged.
- Decision:
  - **Storage**: akd's `Database` on SurrealDB (`crates/akd`). Records are in `akdrec` by binary
    ID; value states are in `akdval` by (label, epoch) with the label indexed. Each `batch_set`
    is one transaction with the Azks record last. The retrieval rules match akd's in-memory store.
  - **Configuration**: WhatsApp v1. The VRF key comes from the secrets store (worker
    `kt_vrf_key`, api `[api.kt] vrf_key`).
  - **Labels**: `kind || account`, deterministic, so each account has one label per kind and
    everyone is shown the same value.
  - **Values**: `HMAC(salt, context || kind || raw)`, with a random per-account salt given to
    whoever asks and deleted with the account. This keeps deletion unlinkable, because `akd`
    0.13 cannot tombstone. A different salt cannot match the published value.
  - From the security review: the first build put the salt in the label. A server could then
    publish a second label for an account and show contacts a different key than the owner saw.
  - **Writer**: the worker publishes one epoch per sweep from the generation-driven pending list
    (D-0055's mechanism kept). **One writer is enforced by the store**: an epoch commits only from
    the one before it (a compare and swap on `akdhead` in the epoch's transaction). Rollouts run
    several workers.
  - **Readers**: `api` uses akd's read-only directory, opened lazily. It serves keys, signed
    heads (log key) and lookup, history (newest 64) and audit (at most 1,000 epochs) proofs in
    akd's protobuf.
  - **Removed**: the own RFC 9162 log (`crates/kt`) and its tables. The fuzz target `kt` now
    covers akd's proof decoding.
- Evidence:
  - `akd_live`: epochs published and read by a separate reader; lookup, history and audit
    verified with akd's client checks; refused under another label, root or VRF key and for a
    rewritten history; the store's semantics;
  - `kt_live` (pending and published);
  - `clients_verify_the_transparency_log` over HTTP;
  - `gate_p03`;
  - fuzz `kt`.
- Consequences: 27 new exemptions; protobuf generated at build time; a VRF key and a log key for
  the secrets store. No external auditor yet.

## D-0056 — Prompt 3, milestone 7: GDPR export, deletion across every Prompt 3 store, inactivity deletion; the Prompt 3 gate

- Date: 2026-11-15
- Status: accepted; built (report `P03-gate.md`, PR #54); **the Prompt 3 gate approved by the owner
  on 2026-11-16**
- Decided by:
  - owner: the inactivity period, user-chosen from 6, 12 or 24 months with a default of 12 and
    a warning first (D-0047);
  - agent: the design below.
- Decision:
  - **Export** by the primary only (as deletion, D-0052): one JSON document of every registry
    store. Derived secrets (the PIN and username-key verifiers) are reported as present, not
    given out. Sealed push registrations are reported as present. What cannot be attributed to
    the account is listed with its retention.
  - **Deletion**: the existing outbox job extended to avatar objects; the registry deletion
    extended to passkeys, avatars and link tokens. Profile, PIN, device list and transparency
    salt go with the account row; the job verifies all of it.
  - **Inactivity**:
    - "use" is any signed `api` request, recorded at most daily through a keyed mark in
      `valkey-hot`;
    - warnings are wake-up pushes a month and a week before the end, which make the app
      reconnect;
    - at the end, the same deletion as a user's own.
- Evidence: `an_account_is_exported_then_deleted_from_every_prompt_3_store` and
  `inactivity_warns_a_month_and_a_week_before_then_deletes` (`tests/e2e/tests/gate_p03.rs`,
  against SurrealDB, `RustFS` and both Valkeys). The Prompt 3 gate is in `P03-gate.md`.
- Consequences: none new beyond the fields listed in the threat model row. Inactivity is
  measured from `api` use, so apps must call `api` at least daily while in use.

## D-0055 — Prompt 3, milestone 6: key transparency as Songbird's own RFC 9162 log; akd recorded as the upgrade path

- Date: 2026-11-15
- Status: accepted; built (report P03-M6, PR #54); **gate approved by the owner on 2026-11-16**. The
  owner's answer on the choice of log: **"we can use akd now"**, so `akd` replaces the own log
  (D-0057, next); this log stays the interim and the test reference until then.
- Decided by: agent (rest of Prompt 3 in one pass), for the owner to confirm
- Options and evidence:
  - **Meta's `akd` 0.13.0** (MIT/Apache-2.0, used by WhatsApp): a verifiable map with VRF-private
    labels, non-membership and history proofs. It adds **12 crates** to the tree (`akd`,
    `akd_core`, `async-recursion`, `ed25519-dalek`, `ed25519`, `protobuf`, `sha2`, …), so a
    second Ed25519 and SHA-2 stack. It needs its async `Database` trait implemented on
    SurrealDB, a VRF key with its own lifecycle, and epoch publishing. Weeks of integration.
  - **An own RFC 9162 log**: about 500 lines, aws-lc-rs only, proofs checkable by any CT
    library. It is a log, not a map: no non-membership proofs, and a server could withhold an
    account's newest entry from one reader (but not rewrite history without a detectable fork).
- Decision (recommended): **the own log now, behind the same endpoints, with `akd` as the
  upgrade** when non-membership and label privacy are worth its integration.
  - The log covers identity keys, device lists and usernames.
  - Subjects are salted HMACs, so deleting an account's salt leaves its entries unlinkable
    (GDPR), while the log stays append-only.
  - Logging is driven by a generation bumped in each change's transaction and reconciled by the
    worker.
  - Appends are serialized on the head row; only perfect-subtree hashes are stored.
  - Heads are signed by a dedicated log key at serving time.
- Evidence:
  - the CT reference roots (independently recomputed; the size-8 root is the published value);
  - every inclusion and consistency proof up to 70 leaves, with tampering refused;
  - a forked log detected;
  - 160 concurrent appends over 8 connections yield one consistent tree;
  - logging once per change; deletion unlinks;
  - a client's checks over HTTP;
  - fuzz `kt`.
- Consequences: new crate `songbird-kt`; `[api.kt]` with a log key for the secrets store. No
  external auditor yet (a later operational decision).

## D-0054 — Prompt 3, milestone 5: encrypted profiles, avatars and sealed sender behind a flag

- Date: 2026-11-15
- Status: accepted; built (report P03-M5, PR #54); **gate approved by the owner on 2026-11-16**
- Decided by: agent (rest of Prompt 3 in one pass, at the owner's request)
- Decision:
  - **Profiles.** One opaque blob encrypted by the client under its profile key, at most 8 KiB,
    whose version must grow. Avatars go to RustFS (encrypted, at most 512 KiB, random ID); only
    an account's current avatar is served, and replaced ones are swept after an hour.
  - **Unidentified-access key.** `HKDF-SHA256(salt "songbird.v1", profile key, info
    "unidentified-access")[..16]`, stored with the profile, with an "anyone may send" flag.
  - **Sealed sends** (`PUT /v1/messages/sealed`):
    - unsigned, authorized by the XOR of the closed recipient accounts' keys, compared in
      constant time;
    - no MLS commits;
    - throttled per IP range and per recipient account;
    - the send path is entered with a sentinel sender that no device has.
  - **Sender certificates.** Ed25519 by a dedicated certificate key (not the system key, so
    rotating either does not touch the other), carrying the account, device, device identity key
    and expiry; lifetime 24 hours.
  - **Off by default** (`[api.profiles] sealed_sender`). Prompt 12 verifies it before it is
    switched on.
- Evidence:
  - the certificate vector (`certificate_vector`), cross-checked with OpenSSL 4.1;
  - live tests (`profile_live`; `profiles_certificates_and_sealed_sends`: versions, fetch,
    certificates, sealed with the right, wrong, missing or combined keys, open accounts, the
    device check, the flag off);
  - fuzz `profile`.
- Consequences: new settings `[api.profiles]` and a certificate key for the secrets store. No new
  dependency.

## D-0053 — Prompt 3, milestone 4b: two-step PIN and passkeys as second factors for linking; multiple accounts per installation

- Date: 2026-11-15
- Status: accepted; built (report P03-M4b, PR #54); **gate approved by the owner on 2026-11-16**
- Decided by:
  - owner: the PIN as Argon2id + pepper + lockout, at least 6 digits (D-0047); Songbird's own
    minimal WebAuthn verifier (2026-11-15);
  - agent: the design below.
- Decision:
  - **Where the factor applies.** Once an account has a PIN or a passkey, a link request must
    carry one. This is the step a QR-scan phisher cannot complete. Recovery (Prompt 10) will
    require the same.
  - **PIN.** 6 to 12 digits, Argon2id under a pepper derived from the key directory's secret.
    - Every attempt is spent in the account row's transaction before it is checked, so parallel
      guesses cannot exceed the limit.
    - 5 free attempts, then locks of `60 s · 2^(k-1)` up to a week; unusable after 20 until the
      primary sets a new PIN.
    - The comparison is constant-time.
  - **Passkeys.** Songbird's own verifier (about 400 lines, fuzzed): ES256 and EdDSA; no
    attestation trusted; user verification required by default; signature counters
    compare-and-swapped. Registration takes a single-use challenge. The link assertion's
    challenge is derived from the link transcript, so it needs no server state and cannot be
    replayed onto another link.
  - **Only the primary** manages factors.
  - **Multiple accounts.** Each account on an installation is its own account on the server, with
    nothing shared. A shared APNs token links them at the push service; this is documented, and
    per-account registrations are used where the platform allows.
- Evidence:
  - the W3C WebAuthn Level 3 test vectors (ES256 with no attestation, Ed25519 with packed
    attestation) pass, with their refusals;
  - `racing_pin_guesses_never_exceed_the_free_attempts` (40 parallel guesses: 4 checked);
  - live HTTP tests for PIN lockout, passkey linking and account isolation;
  - fuzz `devices` (now including WebAuthn).
- Consequences: no new dependency (`serde_json` is added to `songbird-keys` from the workspace).
  `[api.keys.devices]` gains PIN and WebAuthn settings, and passkeys stay off (503) until
  `rp_id` is set.

## D-0052 — Prompt 3, milestone 4: QR provisioning, linking with a confirmation the server cannot forge, and a signed, versioned device list

- Date: 2026-11-15
- Status: accepted; built (report P03-M4, PR #52); **gate approved by the owner on 2026-11-15**
  ("Approve both"), with its four threat-model rows
- Decided by: owner for scope (M4 split: PIN, passkeys and multiple accounts in M4b, 2026-11-15)
  and the limit (primary + 5, D-0047); design by agent under the owner's standing instruction
  (Proton-level bar)
- Context: Prompt 3, "Devices": QR provisioning through a short-lived channel that sees only
  ciphertext; a signed, versioned device list the server refuses to roll back; device limits;
  removal with "device list changed" frames and a hook for Prompt 5's MLS removal.
- Options considered:
  - Signal's model (the primary sends the account identity private key to the new device) was
    rejected: every linked device could then sign device lists. Here only the primary holds the
    account identity key, and every device has its own identity key (D-0048).
  - Linking authenticated by the token alone was rejected. The server sees the token in the link
    request, so it could link keys of its own. The new device also MACs its keys under a
    confirmation key that only the sealed provisioning message carries. The primary checks the
    tag before vouching for the device.
  - An unsigned list kept by the server was rejected: the server could add devices.
  - The server notifying contacts was rejected: it keeps no social graph (sealed sender comes in
    M5). Only the account's own devices are notified; contacts learn through the send path's
    device check and list fetches.
- Decision:
  - **Provisioning channel** in `valkey-hot`: a 128-bit bearer ID under a keyed hash; one message
    of at most 4 KiB, written once, read once (long poll up to 25 s), gone after ten minutes.
    Sealing is client-side (`songbird_keys::provisioning`: X25519, HKDF-SHA256, AES-256-GCM); the
    server never holds a provisioning key.
  - **Link tokens**: 256-bit, hash stored, single use, ten minutes, primary only. A token
    reserves a server-chosen device ID and counts toward the limit while outstanding.
  - **Link request**: both new keys proven (Ed25519 and XEdDSA) over a transcript binding the
    token, the device ID and the server, plus the confirmation tag. The identity key is fixed at
    link time.
  - **Pending devices**: a linked device authenticates at once, but is outside the send check
    and its keys are not served until a list signed by the account identity key names it
    (`Directory::with_pending`; key fetches skip it).
  - **The device list**: binary, at most 16 entries, Ed25519 by the account identity key over a
    domain-separated body, versions strictly increasing. The server checks the signature, the
    account, the limit, the version, and that every entry matches an enabled device's stored keys
    with the right primary flag. Devices left out are disabled in the same transaction, and a
    `DeleteDevices` outbox job deletes them from every store as account deletion does.
  - **Removal**: by a list, or a linked device removing itself; never the primary (nor a legacy
    first device without the flag).
  - **Account-wide authority** (found by the security review): before M4 every account had one
    device, so `DELETE /v1/account`, usernames and invites never asked which device called. Now
    **only the primary deletes the account** (checked in the deletion's transaction), and **a
    pending device changes nothing account-wide** (usernames, the username key and link,
    invites). Otherwise a device linked by phishing a QR scan could delete the account
    irreversibly before the primary removes it. Listed linked devices keep the account-wide
    changes they had (M4b's second factors may narrow this).
  - **Device limit** under races: every change to an account's devices writes the account row
    in its transaction, so concurrent changes conflict and retry.
  - **Notices**: `DeviceListChanged` (system notice kind 1, signed by the system key) to the
    account's other devices on link, list and removal.
  - The MLS removal hook for Prompt 5 is the `DeleteDevices` job and the list change: removed
    devices are named by the list diff that clients see.
- Evidence (Apple M5 Pro, macOS 27.2.1; compose stack under podman; report P03-M4):
  - `crates/accounts` `devices_live` (6). Among them, **40 concurrent token requests over eight
    connections for a limit of 6 grant exactly 5; without the account-row write all 40
    succeeded**.
  - `services/api` `devices_live` (4): the QR flow over HTTP, every refusal, throttles.
  - `tests/e2e` (2): the notice over a WebSocket and the send check before and after listing;
    removal verified across every store.
  - Unit tests of the formats, proofs, provisioning seal and directory.
  - Fuzz `devices` (60 s, 1,611,017 runs).
- Consequences:
  - Losing the primary means deleting the account until M4b's recovery and a primary transfer
    exist.
  - Until key transparency (M6), a first-time contact trusts the server for the account identity
    key.
  - A registry restore brings back devices removed after the backup; clients refuse the old list
    as a rollback, and the primary must sign again.
  - New settings `[api.keys.devices]` with defaults. No new dependency.

## D-0051 — Prompt 3, milestone 3b: MLS KeyPackages checked by Songbird's own parser, bound to the uploading device, served once

- Date: 2026-11-15
- Status: accepted; built (report P03-M3b); **gate approved by the owner on 2026-11-15** ("Approve
  both"), with its threat-model row
- Decided by: owner (own focused parser over OpenMLS on the server, 2026-11-15); design by agent
- Context: Prompt 3: "Same pattern for MLS KeyPackages (single use, plus a last-resort package)";
  the plan said "parsed and signature-checked with OpenMLS", and the owner split this out of M3
  to evaluate that first.
- Options and evidence: OpenMLS 0.9.2 with `openmls_rust_crypto` resolves to **194 crates, about
  109 new to the tree**, including RustCrypto and libcrux (two more cryptography stacks),
  `bindgen` and `clang-sys` at build time, and `rayon`; a focused parser needs none (Ed25519 and
  X25519 checks already in `songbird-keys`, aws-lc-rs for signatures and SHA-256). Opaque storage
  (size only) would let a device publish packages naming another device.
- Decision (the owner's choice): **Songbird's own parser** (`crates/keys/src/mls.rs`, about 400
  lines, fuzzed) checking RFC 9420's encoding strictly, mls10, ciphersuites `0x0001`/`0x0003`,
  honest keys (init ≠ encryption key), a **basic credential naming the uploading device**,
  capabilities, a current and bounded lifetime (≤ 100 days), and both `SignWithLabel` signatures;
  `last_resort` (extension type 10, as OpenMLS writes it) decides which packages are last resort,
  so no client flag can disagree with the package; storage under `KeyPackageRef`. The credential
  identity is the 16-byte device ID (Prompt 5 maps devices to accounts through the signed device
  list, M4).
  - Proven against OpenMLS: `tools/openmls-vectors` (outside the workspace) makes packages with
    OpenMLS; Songbird accepts its X25519/Ed25519 ones with OpenMLS's own `KeyPackageRef`,
    recognises its last-resort one, refuses its P-256 one, and refuses every single flipped byte.
  - An encoder (`mls::encode`) for clients and tests; the checker accepts what it builds.
  - Storage and claims as M3's prekeys (D-0050): single-use packages claimed by deleting one
    record; expired ones never served and swept; the last-resort one beyond them; the same
    throttles (separate records) and per-target budget; `KeyPackagesLow` (system notice kind 3).
  - No new dependency.
- Evidence (Apple M5 Pro, macOS 27.2.1; compose stack under podman; report P03-M3b):
  `mls_vectors.rs` (OpenMLS packages; every flipped byte); `mls` unit tests (minimal varints,
  `SignContent`, encode/check); `crates/accounts` live (single use, expiry, last resort, bounds,
  deletion; **400 concurrent claims over eight connections, two rounds: each package served
  once, none twice, none lost**); `services/api` `keys_live` (bound to the device, served once,
  every refusal); fuzz `keypackages` (60 s, 2,041,036 runs).
- Consequences: the server accepts only X25519/Ed25519 ciphersuites until Prompt 5 decides on a
  post-quantum one (issue #24); the allow-list is one constant to extend. Clients must still
  validate KeyPackages fully.

## D-0050 — Prompt 3, milestone 3: the PQXDH key directory; XEdDSA and ML-KEM-1024 checked on upload; one-time prekeys claimed exactly once

- Date: 2026-11-15
- Status: accepted; built (report P03-M3); **gate approved by the owner on 2026-11-15** ("Approve
  both"), with its two threat-model rows
- Decided by: owner for scope (M3 split: MLS KeyPackages in M3b) and the post-quantum format
  (ML-KEM-1024 only); design by agent under the standing instruction (D-0041)
- Context: Prompt 3's key directory: per device an identity key, a signed prekey, a last-resort
  post-quantum prekey, one-time EC and post-quantum prekeys as libsignal's PQXDH needs; one-time
  prekeys claimed atomically and never handed out twice; refill signals; signature validation;
  exhaustion protection (rate limits per requester and per target). Its gate: concurrency tests
  that prove no one-time prekey is ever served twice under parallel claims.
- Owner's decisions (2026-11-15): **split M3**, MLS KeyPackages (and the OpenMLS question) in
  M3b; **ML-KEM-1024 only** (type `0x0A`, the NIST standard), not libsignal's default
  pre-standard Kyber1024 (`0x08`).
- Decision:
  - **Formats are libsignal's** (X25519 `0x05 || u`, ML-KEM-1024 `0x0A || ek`, XEdDSA over the
    serialization), so a PQXDH implementation reads them unchanged.
  - **XEdDSA implemented from libsignal's code and specification** (D-0047), in `crates/keys`:
    libsignal and Songbird agree byte for byte (same public key from a private key, the same
    signature for the same randomness) and each verifies the other's signature. Stricter than
    libsignal in one place: `s` must be canonical (libsignal reduces any `s` below 2^253), so a
    signature has a single encoding; honest signers are unaffected.
  - **Every key checked before storing**: X25519 keys honest (canonical, on the curve, prime
    order); ML-KEM keys pass FIPS 203's modulus check. Found while building: **aws-lc-rs 1.19
    accepts an encapsulation key with a coefficient ≥ q**, so the check is ours, with a test
    that asserts aws-lc-rs's behaviour (as M1's for its Ed25519 identity-point forgery).
  - **The identity key is set once per device** and never replaced; a new identity is a new
    device (M4's signed device list, M6's key transparency).
  - **Claims**: one transaction-free delete per attempt, `DELETE ONLY otpk:[device, id] RETURN
    BEFORE`, among up to 16 random candidates: whoever deletes the record gets it; a racing claim
    gets nothing or loses the write conflict and tries another. Proven by the gate test (400
    concurrent claims over eight connections, three rounds: every one-time key served once,
    none twice, none lost), whose power was checked by a deliberately non-atomic claim (read,
    then delete): it served a key twice and the test failed at once.
  - **Exhaustion**: fetches 240 an hour per requesting account (burst 60) and 1,200 per IP range;
    one-time claims 60 an hour per target account across all requesters (burst 30), beyond
    which fetches answer with the last-resort key only, leaving the one-time keys for later.
  - **Refill**: a device left below 10 one-time keys of either kind gets a system notice, kind 2
    `PrekeysLow` (`ec u16 | kem u16`), signed by the system key (D-0044) and sent through the
    send path `api` already links, at most once an hour (a keyed one-hour mark in
    `valkey-hot`). `Submit` gains `system` for it. The system key is optional configuration:
    without it (or without `[api.send]`) clients rely on `GET /v1/keys`.
  - **Deletion**: an account's devices' one-time prekeys are deleted with it and counted by the
    worker's verification.
  - Moved from the plan's M3 row: the send path's `stale` list (D-0026) needs device-list
    versions, which M4's signed device list brings.
  - No new dependency (`curve25519-dalek` and aws-lc-rs's ML-KEM, both in the tree).
- Evidence (Apple M5 Pro, macOS 27.2.1; compose stack under podman; report P03-M3): the gate test
  above; `crates/keys` XEdDSA (tampering, a non-canonical `s`, dishonest keys) and ML-KEM tests;
  `xeddsa_vectors.rs`; `crates/proto` key formats (strict decoding, the longest upload);
  `crates/accounts` live (identity once, bounds, duplicates, last-resort fall-back, deletion);
  `services/api` `keys_live` (4) and `keys` unit test (every kind of tampering refused);
  `tests/e2e` `a_device_low_on_prekeys_is_told_once`; fuzz `prekeys` (60 s, 641,996 runs).
- Consequences: a client stack must speak ML-KEM-1024 (libsignal's `mlkem1024` feature, if a
  client uses libsignal). Sessions started after a target's one-time budget is spent use the
  last-resort key (no one-time X25519 key): the PQXDH spec's degraded but defined mode.

## D-0049 — Prompt 3, milestone 2: unique usernames as Ristretto hashes with poksho proofs, a username key that never confirms existence, resettable links

- Date: 2026-11-15
- Status: accepted; built (report P03-M2); **gate approved by the owner on 2026-11-15** ("Approve
  both"), with its four threat-model rows
- Decided by: owner for the product choices (below); design by agent under the standing
  instruction (D-0041)
- Context: Prompt 3: "Usernames are changeable and found only by exact match. Evaluate
  libsignal's username scheme (the server stores a hash plus a proof, never the username
  itself) and adopt it unless a recorded reason says otherwise. Username lookups are
  rate-limited per account and per IP range", plus WhatsApp's username key. D-0047: implement
  Signal's protocols, do not link libsignal.
- Owner's decisions (2026-11-15):
  1. **Unique handles**, one person per name, as WhatsApp's. The owner first chose Signal's
     `nickname.NN`, then asked how other companies and Proton would do it, and chose unique
     handles "built Signal-grade underneath". The reasons, as put to the owner: the
     discriminator buys little privacy (the hash cannot be salted, and two digits multiply a
     guess by about 99), invites look-alike impersonation (`radu.42` and `radu.24`), and Discord
     dropped discriminators in 2023 because people could not remember them; the username key
     (WhatsApp's answer to enumeration) presumes unique names; WhatsApp switchers expect them.
  2. **A missing or wrong username key answers exactly like "not found".**
  3. **Resettable username links** are built.
- Recorded reason for departing from libsignal's scheme: with unique handles there is no
  discriminator, so libsignal's hash (three generators over nickname, discriminator and a
  SHA-512 scalar) and its statement do not apply. Songbird keeps the construction and changes
  only what the format forces:
  - `hash = s·G`, `s = SHA-512("songbird.v1/username\0" || lower-cased name) mod l`, `G` squeezed
    from libsignal's SHO under a Songbird label (nobody knows its discrete logarithm);
  - the claim proof is a standard **poksho** proof (poksho's label, statement encoding,
    transcript and nonce derivation) of `hash = s·G`.
- Evidence of compatibility (libsignal at commit `8be00c2`, AGPL, built only in a scratch tool
  outside the workspace, `tools/libsignal-vectors`):
  - Before the format changed, this implementation with libsignal's own constants reproduced
    libsignal's username hashes and proofs exactly: 12 of 12 (the SHO sequence, six names with
    hash and proof at fixed randomness, five more hashes), proof nonces included.
  - After: `Sho` matches libsignal's published vector and a longer sequence (tests); libsignal's
    poksho produces, for Songbird's statement and randomness, the same generator, hash and proof
    bytes as Songbird, and verifies Songbird's proof.
- Design:
  - **Format:** 3 to 32 ASCII letters, digits, `_`, `.`; starts with a letter; no `..`, no final
    `.`; unique in lower case. ASCII only (no look-alike letters from other scripts).
  - **Claims** (`PUT /v1/username`): proof checked, reserved names refused by hash, one
    transaction (racing claims conflict and retry); the previous name is **held 30 days**,
    naming no account, so nobody can grab it to pose as its owner; the link goes. Every attempt
    spends a per-account budget (2 an hour, burst 5), so probing which names are taken is slow.
  - **Lookups** (`POST /v1/username/lookup`, signed; the hash in the body, never a URL): exact
    match; 120 an hour per account, 600 per IP range; one answer, 404, for unknown, held, key
    missing, key wrong and budget spent; the registry reads the same rows whether or not the
    name exists; a keyed lookup always runs one Argon2id check (against a dummy verifier when
    nothing is to be checked) and always makes one atomic check-and-spend of the name's guessing
    budget (a single Lua step, so concurrent guesses cannot share a token; found by the review).
    An account being deleted is found by nobody.
  - **Username key:** 4 to 10 digits; `salt || Argon2id(19 MiB, t = 2, secret = pepper)`; the
    pepper and the throttle key by HKDF from `[api.usernames] secret` (secrets store). Key
    guesses that find nothing: 1 an hour per username across all requesters, burst 10.
  - **Links:** the server stores up to 256 bytes of ciphertext under a new random 16-byte handle
    per upload (a reset), deleted with a username change or the account; it never sees the key.
  - **Deletion:** account deletion holds the username (no account named) and deletes the link;
    the worker's verification counts both; the worker sweeps holds that ended.
  - No new dependency: `curve25519-dalek` (D-0048) with its default features still off; `argon2`
    (D-0048) now also in `api`.
- Evidence (Apple M5 Pro, macOS 27.2.1; compose stack under podman; report P03-M2):
  `crates/keys` username tests (rules, tampering, a non-canonical scalar, case) and vectors;
  `crates/accounts` live: unique, held, swept, 16 racing claims with one winner, keys, links,
  deletion (3 of 3 runs); `services/api` live `usernames_live` (6), including keyed lookups at
  the production Argon2id cost taking the same time whether or not the name exists (means
  0.1916 s against 0.1926 s in a debug build, ratio 0.995); fuzz `username` (60 s, 653,381 runs,
  nothing found).
- Consequences: the server holds username hashes, which anyone can test guesses against; a
  common name is found by trying it (recorded in the threat model). Name disputes and
  impersonation by people are an abuse process (Prompt 11). Key transparency (M6) will publish
  username hash to identity key, so a recycled name shows a new key.

## D-0048 — Prompt 3, milestone 1: registration with Argon2id proof of work, honest keys only, invites that record no social graph

- Date: 2026-11-15
- Status: accepted; built (report P03-M1); **gate approved by the owner on 2026-11-15**, with
  its four threat-model rows
- Decided by: agent, under the owner's standing instruction (D-0041) and the Prompt 3 decisions
  (D-0047)
- Context: Prompt 3 asks for accounts without phone numbers, an Argon2id proof of work "tuned so
  a phone solves it in a few seconds", per-IP-range throttles, and (D-0038) an invite-only launch.
  Until now accounts were created only through `api`'s admin link (D-0036); D-0018 left open
  whether the device authentication key stays separate from the identity key.
- Decision:
  - **Keys.** An account has an **account identity key** (Ed25519, aws-lc-rs; it will sign the
    versioned device list in M4 and is what users verify and key transparency publishes, as
    WhatsApp's account signature key is). Each device keeps a separate **authentication key**
    (answers D-0018: a transport credential, rotatable without touching identity, never used end
    to end) and, from M3, its own device identity key for PQXDH. The server draws account and
    device IDs.
  - **Only honest public keys.** Every Ed25519 key the server stores must be the canonical
    encoding of a prime-order point (`crates/keys`, on `curve25519-dalek`). Found while building:
    aws-lc-rs 1.19 accepts a signature under the identity point for any message (`R = sB`), and
    with it anyone could authenticate as a device registered with that key. The admin link now
    refuses such keys too.
  - **Proof of work** (`crates/pow`): stateless challenges (51 bytes, HMAC tag from the
    registration secret, 10-minute life, single use by burning the nonce in `valkey-hot`);
    `solutions` strictly increasing counters, each an Argon2id(v1.3, `m`, `t`, p = 1) output with
    `bits` leading zeros. Several easy solutions instead of one hard one keep a phone's time
    predictable. Defaults 16 MiB, t = 1, 4 bits, 8 solutions (128 evaluations). The server checks
    everything cheap first (length, throttles, tag, age, keys, signatures), burns the nonce, then
    verifies off the runtime with bounded concurrency, stopping at the first failure.
  - **Throttles:** GCRA in `valkey-hot` on Valkey's clock, under HMAC keys of the IP range
    (IPv4 `/24`, IPv6 `/48`, IPv4-mapped folded), per range and deployment-wide, for challenges,
    registrations and refused invite codes. Fail closed.
  - **Invites** (D-0038, D-0047): 120-bit codes (24 Crockford symbols), stored as
    `SHA-256("songbird.v1/invite\0" || code)` with uses and expiry only; one transaction consumes a
    use and creates the account (SurrealDB's write conflict lets one of several racing
    registrations commit; the others retry). Users create single-use codes from an allowance (5,
    configurable); operators create batches with `songbird-admin invites create`. One answer for
    every refused code; revocation by whoever holds the code. Invite-only mode is a setting.
  - **New devices and the directory:** delivery reloads the registry within a second, so a client
    connecting right after registering can be refused (4000). Rather than let unauthenticated
    connections trigger registry lookups on delivery, clients retry 4000 for 10 s after
    registering (`ClientConfig::new_device_grace`; PROTOCOL.md §4.1). Found by the end-to-end
    test.
  - **Privacy Pass, evaluated as a later option:** blind-signed tokens (RFC 9576–9578) would let
    a trusted issuer vouch for "a human passed a check" without linking the check to the account.
    Today there is no issuer to trust and no check other than the proof of work and invites, so
    tokens would add a protocol and a key to rotate for nothing; revisit when the launch opens
    (a token per verified device attestation, App Attest / Play Integrity, is the likely issuer)
    or when proof of work stops deterring bots.
- Dependencies (new to the tree, each with its reason):
  - `argon2` 0.6.0 (RustCrypto, MIT OR Apache-2.0), the prompt's Argon2id; aws-lc-rs has none.
    Default features off (`alloc` only). Brings `blake2` 0.11.0 and `base64ct` 1.8.3 (compiled);
    `password-hash` and `phc` appear in `Cargo.lock` only (optional, not enabled). Contains
    `unsafe` (aligned allocation of the memory blocks, segment views for lanes); read in the
    pinned source. Output checked against the reference `argon2` command (vector in PROTOCOL.md §13).
  - `curve25519-dalek` 5.0.0 (BSD-3-Clause), default features off: point decompression and
    order checks aws-lc-rs does not expose; the library the owner's D-0047 names for XEdDSA and
    the username hash. `curve25519-dalek-derive` (x86-64 backend) and `fiat-crypto` (an optional
    backend, lock only) come with it.
  - All eight are **exempted in `cargo vet`** pending audits (no imported audit covers these
    versions), like the other client libraries (D-0016); audit backlog: argon2 4,022 lines,
    blake2 1,561, base64ct 3,651, curve25519-dalek 38,745.
- Evidence (Apple M5 Pro, 18 cores, 48 GiB, macOS 27.2.1; compose images as pinned, under
  podman; report P03-M1):
  - Argon2id per evaluation (release, one core, median of 41): 4 MiB 1.39 ms, 8 MiB 2.25 ms,
    16 MiB 4.23 ms, 32 MiB 9.08 ms; t = 2 at 16 MiB 8.17 ms. Default parameters: solve over 30
    challenges p10 0.29 s, median 0.48 s, p90 0.76 s, max 0.82 s; verification of all 8 solutions
    31.4 ms. A phone core is several times slower than this Mac's; "a few seconds" there is an
    estimate until the apps exist, and challenges carry their parameters, so retuning needs no
    client release (`cargo run --release -p songbird-pow --example timing`).
  - Tests: `crates/pow` (4, vector), `crates/keys` (4), `crates/proto` registration (5 plus the
    §13 vector test), `crates/accounts` live (4 new: registration, invites valid/expired/used
    up/unknown/revoked, 24 racing registrations for 1 and 5 uses, allowance and no issuer; 3 of 3
    runs), `services/api` live `registration_live` (8: the D-0038 invite cases, guessing
    throttled before any work, quota and revocation, mode off, replay/forgery/wrong work, keys,
    throttles, no configuration), `tests/e2e` `registration` (registered accounts connect and
    talk; a stranger still stops); fuzz targets `registration_request`, `pow_challenge`,
    `ed25519_key` (60 s each: 32.5 M, 14.6 M, 3.4 M runs, nothing found).
- Consequences: delivery is deployable without operator steps for accounts. Four threat-model §1
  rows, approved by the owner on 2026-11-15. Phone timing to measure when the apps exist. A registry restore revives
  invite codes used after the backup until they expire. Per-range throttles count everyone
  behind one carrier NAT as one client (generous numbers, deployment-wide caps behind them).

## D-0047 — Prompt 3: the plan, and the owner's decisions it needed

- Date: 2026-11-15
- Status: accepted (owner, 2026-11-15)
- Decided by: owner (each answer below), on the agent's plan `docs/reports/P03-plan.md`
- Context: Prompt 3 leaves the inactivity period and the inviter link (D-0038) to the owner;
  issue #23 asks the owner how people find each other; and the prompt asks for libsignal's own
  crates, which are AGPL-3.0-only (root `Cargo.toml`, checked 2026-11-15) and not published on
  crates.io by Signal (the crates.io `libsignal-protocol` is an unrelated 2019 crate), while
  `deny.toml` admits permissive licences and crates.io sources only.
- Decisions:
  1. **Plan agreed:** seven milestones, each with its own branch, pull request, report and
     decision: M1 registration and invites; M2 usernames; M3 key directory (the concurrency
     gate); M4 devices, PIN, passkeys; M5 profiles and sealed sender; M6 key transparency; M7
     GDPR and the Prompt 3 gate.
  2. **libsignal: implement the protocols, do not link the library.** The server verifies
     XEdDSA, the Ristretto username hash and proof, and key formats with permissive crates
     (`curve25519-dalek`, aws-lc-rs's ML-KEM), byte-compatible with libsignal, proven by vectors
     generated from libsignal by a tool outside the workspace. The repository licence stays open.
     (The same question returns for the apps.)
  3. **Inactivity deletion:** the user chooses 6, 12 or 24 months; default 12; a warning notice
     before deletion (built in M7).
  4. **Who invited whom: not stored** (D-0038's default); only each account's remaining
     allowance. Abuse is handled on the abusive account.
  5. **Contact discovery (issue #23): none from address books.** People find each other by exact
     username, invite link or QR code; the server holds no address books.
  6. **Two-step PIN:** Argon2id with a per-account salt plus an HMAC pepper held in the secrets
     store (not in the database or backups), at least 6 digits, attempt limits with lockout.
     It stops a database or backup thief, not a fully compromised `api`; hardware guess limits
     are issue #26 (Prompt 10). A server that learns a PIN still cannot add a device unnoticed:
     the device list is signed by the account identity key.
  7. **Device limit until Prompt 11's plans:** one primary and 5 linked devices, configurable.
  8. **Prompt 2's final gate:** the owner asked for a report against the master prompt's
     definition of done; it was written and approved the same day in another session
     (`docs/reports/P02-final.md`, with the two M5 threat-model rows).
- Consequences: Prompt 3 departs from the prompt's "libsignal's own crates" with this recorded
  reason; the vectors from libsignal carry the compatibility proof. No address-book upload
  endpoint will exist.

## D-0046 — The repository moves to the `songbirdchat` organization and becomes public; domain `songbird.chat`

- Date: 2026-11-14
- Status: accepted (owner); done on 2026-11-14: the repository is `songbirdchat/songbird` (GitHub
  Free organization) and **public**.
- Decided by: owner (created the organization `songbirdchat` and registered `songbird.chat`;
  `songbird` and `getsongbird` were taken on GitHub). Organization name matches the domain.
- Decision: the repository's home is `github.com/songbirdchat/songbird`; images are published to
  `ghcr.io/songbirdchat/<service>`. Every reference in the tree points there (`Cargo.toml`
  `repository`, `deploy/ansible/group_vars/all.yml` registry, `deploy/bin/verify-release` default,
  `deploy/proxy` image and its cosign identity, `deploy/ci-runner` registration,
  `scripts/github-setup`, the security-advisory link). `CODEOWNERS` keeps `@countradooku`: it names
  the owner as a person. Reports under `docs/reports` keep the old name as history.
- Consequences:
  - Release signatures are keyless (D-0013): the certificate identity is the workflow path in the
    repository, so `cosign verify` checks `https://github.com/songbirdchat/songbird/...`. No release
    has been made yet, so no signature carries the old identity.
  - Public: anyone can read the code and open pull requests, so CI runs only on GitHub-hosted
    runners (D-0045). Checked on 2026-11-14 before and after: no secret in any of the 136 commits
    (pattern scan for age, PEM, GitHub, AWS, Google and Slack keys; the hits are AWS's published
    example key and PEM templates in tests), and none of the APNs or FCM identifiers. Public with
    it: the runner host's name in D-0039, commit author e-mail addresses, the full design and
    threat model (by intent: security rests on keys, not on secrecy of the design).
  - Write access: the owner (admin) and one collaborator (write) on 2026-11-14.
  - `scripts/github-setup/setup.py` finds the project board through the `ProjectV2Owner` interface,
    which matches an organization as well as a user; the board on the personal account is not
    moved by it (a new one is created under the organization when the script runs).
  - Which names under `songbird.chat` serve clients and proxy links is still open (D-0011).

## D-0045 — CI on GitHub-hosted runners only, for a public repository; self-hosted runners retired

- Date: 2026-11-14
- Status: proposed: implemented on branch `ci/hosted-public` (which also merges D-0039's branch,
  PR 37, for its scripts and the rootless Autobahn option); accepted when the owner merges it.
- Decided by: owner ("i dont have money for that, we must continue locally"; then moved the
  repository to `songbirdchat` and made it public, D-0046; "Yes, rework it" for hosted runners);
  layout by agent.
- Context: D-0039's runners were never merged into `main`, so Prompt 2's pull requests (M2 to M7
  and the send library) ran every `ci` and `codeql` job on GitHub-hosted `ubuntu-24.04` while the
  repository was private. From 2026-11-12 to 2026-11-14 that billed **4,178 minutes** (job start
  and end times, rounded up per job): `ci` on pull_request 1,881 (56 runs), `codeql` on
  pull_request 863, `ci` on push to main 770, `codeql` on push 424, `nightly` 158. The free 2,000
  minutes went, then the overage; at 07:32 UTC on 2026-11-14 every job failed before starting
  ("recent account payments have failed or your spending limit needs to be increased", run
  43571585635). A first version of this decision moved everything to the self-hosted runners.
  The owner then made the repository public. That changes both sides: standard GitHub-hosted
  runners cost nothing for a public repository (the 08:59 UTC nightly, run 43578342236, ran its
  12 jobs on hosted runners after the 07:32 refusals), and anyone can now open a pull request
  whose code, and whose copy of the workflows, would run on whatever runner it names, including
  a persistent self-hosted runner on a shared VPS. GitHub advises against self-hosted runners for
  public repositories. Storage limits stay: Actions cache 13.2 GB against 10 GB (rust-cache 11.6 GB
  in 15 entries, 7.6 GB of it from pull-request branches; image layer caches 1.6 GB in 199
  entries).
- Options considered: self-hosted runners for every job with a same-repository guard (a fork's
  pull request can delete the guard in its copy of the workflow; outside code on a machine that
  keeps state between jobs); self-hosted for scheduled jobs only (`workflow_dispatch` still lets
  any writer run any branch there); GitHub-hosted runners for everything (chosen: free, a fresh VM
  per job, nothing of ours to compromise).
- Decision:
  - **Standard GitHub-hosted runners only**, pinned image `ubuntu-24.04` (the `-arm` variant is
    allowed). No self-hosted runner, no larger runner (billed even for public repositories), no
    `-latest`. The D-0039 runners are not registered for this repository.
  - **Pull requests from forks run CI** like any other: with a read-only token
    (`contents: read`, `persist-credentials: false`), no secrets, and no `pull_request_target` or
    `workflow_run`. Secrets appear only in `release.yml` and `deploy.yml`, which run only on a
    pushed tag or by hand. Approval before an outside contributor's workflows run is a repository
    setting for the owner (recommended: all external contributors).
  - **Triggers:** `ci` (quality bar, compose integration, Autobahn, Semgrep, IaC) on pull requests
    and pushes to `main`, docs-only changes skipped. CodeQL (Rust and Actions, security-extended)
    on pull requests, pushes to `main`, weekly and on demand; it still fails on any finding
    (D-0015) and, outside pull requests, uploads to code scanning, which a public repository has
    for free (the API answered "no analysis found", not D-0015's 403). Container images moved out
    of `ci` into `images.yml`: pull requests that touch `deploy/docker/**` or the workflow,
    nightly at 02:23 UTC, and on demand (six builds of the whole workspace on every push before).
  - **Concurrency everywhere:** a new push to a pull request cancels its older `ci` and `codeql`
    runs; every commit on `main` is checked to the end; `images` and `nightly` runs supersede
    older ones.
  - **Storage inside the free limits:** `Swatinem/rust-cache` saves only on `main` (pull requests
    restore it); image builds use no Actions cache and upload no build record; `junit` and the
    Autobahn reports are uploaded only when the job fails, and nightly fuzz and benchmark
    artifacts are kept 14 days (the default was 90).
  - **Enforced, not remembered:** `cargo xtask workflows` (the first step of `cargo xtask ci
    fast`, `xtask/src/workflows.rs`) fails on a runner that is not a standard hosted Ubuntu image
    (self-hosted, a label list, an expression, `-latest`, a larger runner), on `secrets.` outside
    `release.yml` and `deploy.yml`, on those two gaining a trigger other than a pushed tag or
    `workflow_dispatch`, on `pull_request_target` or `workflow_run`, on an action not pinned to a
    full commit SHA (or a docker action without a digest), on top-level permissions other than
    `contents: read`, and on a workflow without a concurrency group (release excepted). 14 tests,
    one of them over the repository's own workflows; it rejects both `main`'s private-repo layout
    (21 violations under the first version's rules) and the self-hosted layout.
- Evidence: `cargo nextest run -p xtask` (14 passed); `cargo xtask ci fast` on the branch (macOS
  arm64, 2026-11-14): workflow policy, rustfmt, clippy, doctests, rustdoc, deny, audit, vet and
  smoke pass; nextest 357 of 359, the two failures being Linux-only behaviour that fails the same
  way on `main` on macOS (`reuseport_listeners_share_the_port_and_the_connections`: macOS does not
  spread `SO_REUSEPORT` connections, `[0, 0, 0, 128]`; `inv7_bounded_and_validated_input`:
  `ConnectionClosed`); the minute, cache and run figures above from the GitHub API on 2026-11-14.
- Consequences:
  - CI costs nothing while the repository is public. If it goes private again, the free 2,000
    minutes return as the limit and this decision must be revisited before the next pull request.
  - D-0039 is superseded; its scripts stay for a private forge (Forgejo) later, never for this
    public repository.
  - Fork pull requests can spend our hosted time but not our money or secrets; the approval
    setting is the brake on abuse.
  - A broken image from a crate change shows up in the nightly `images` run, not in the pull
    request.
  - The owner should set the Actions budget to $0 with "stop usage" on the organization, so a
    larger runner or a return to private cannot bill anything.

## D-0044 — The send library: `api` stores on partition owners over node links, admitted for the store of a send only; system frames signed and in a reserved format

- Date: 2026-11-14
- Status: accepted; built (report P02-send); **approved by the owner on 2026-11-14** ("Approve
  both": this decision and its two threat-model rows).
- Decided by: agent, under the owner's standing instruction (D-0041: "think like someone from
  Proton or a big corp"), then approved by the owner; D-0037 left the route from `api` to the
  partition owners to decide when the send crate was built.
- Context: D-0037 replaced the internal Send API with a `send` library linked by `delivery`,
  `api`, `calls` and `worker`, plus `PUT /v1/messages`. The mailbox is Iggy with one owner node per
  partition (D-0029, D-0034); a send must be stored by its partitions' owners. `api` is the
  internet-facing service most likely to be compromised.
- Options considered for how `api` reaches the owners:
  - **(A) Admit `api.` certificates to the delivery nodes' node links**, as a delivery node that
    does not own a partition reaches the owner today. Same wire, same code path, one hop. Risk:
    the node link serves every mailbox operation and key-store copies.
  - **(B) A send link on delivery nodes that accepts `api.` and runs the send path there.** It is
    the internal network Send API the owner removed in D-0037 (a second protocol, a second hop,
    a second path that can drift from the WebSocket's).
  - **(C) `api` owns partitions or writes Iggy itself.** Puts key stores and the master key on the
    internet-facing host, and a second writer per partition; refused outright.
  - **(D) Queue sends in Valkey for a delivery node to store.** Acknowledging before the mailbox
    holds the send breaks invariant 1 (the database is the only durable truth); refused.
- Decision: **(A), with authorization per kind of peer**, so `api` gets exactly what it needs:
  - **Node links admit by role.** `songbird_link::serve_admitting` takes one handler per
    certificate-name prefix (prefixes must end with a dot); a peer is served only by its own
    kind's handler. Delivery nodes keep every operation; peers listed in
    `[delivery.cluster] senders` (only `api.`, `calls.`, `worker.` may be listed; default none)
    get a **store-only handler**: `Store` is served, everything else (`Fetch`, `Ack`, `Count`,
    `Expire`, `Forget`, `Known`, `KeyStore`, and `PartDone`) is refused and counted
    (`IggyMailbox::sender_requests_refused`, runbook entry).
  - **`api` holds no mailbox.** `IggySender` (in `crates/mailbox`) implements only the store
    (`StoreSend`, a one-method trait every `Mailbox` also has): no Iggy client, no key store, no
    master key, no lease, no membership. It reads the partition leases (one `MGET` every third of
    a lease; the lease value names the owner) and points each partition at its owner. It splits a
    send with the owners' own routine (`store_on`, shared with `IggyMailbox`) and the delivery
    nodes' idempotency secret, so a retry over the other transport is a replay. It never sends
    `PartDone`: the home partition's re-driver completes a sender's multi-partition bookkeeping
    (about 2 s later), so a compromised `api` cannot mark another send's part stored early.
  - **Routing from `api`** uses `ValkeyRouter::publisher`: the delivery nodes' channel naming and
    sealing, no subscriber connections.
  - **One send path.** `crates/send` holds the device-list check, the reserved-prefix and
    duplicate checks, the idempotent store, routing, push on offline (`Pusher`, moved from
    `delivery`) and the device directory (moved from `delivery`, re-exported there). `delivery`
    calls `SendPath::store`, writes `SendResult`, then `SendPath::route`, then flushes once: the
    same order and the same allocations as before; `SendPath` is a set of borrows (no `Arc`
    clone per send: a reference count shared by every core would be a contended cache line).
  - **`PUT /v1/messages`.** Body: a WebSocket `Send` frame byte for byte, under the same
    `LimitsConfig`. Signed like every `api` request, and **authenticated from the headers before
    the body is read** (the signature covers the declared digest; `Content-Length` required and
    bounded first). Answer: `200` with the `SendResult` frame the WebSocket would send; HTTP errors
    only before the send path. Rate limit: the WebSocket's per-device numbers, counted across all
    `api` nodes in `valkey-hot` by a GCRA on Valkey's clock, under a keyed hash of the device
    (key from HKDF of the router's cluster secret); fails closed.
  - **System frames** (`crates/proto::system`, signer in `crates/send::system`): server notices
    travel as mailbox envelopes (durable, ordered, pushed) in a reserved format: an 8-byte magic,
    the recipient device, issue time, random notice ID (the send's idempotency key), kind, a body
    of at most 1 KiB, and an Ed25519 signature by the system key over a domain-separated
    transcript. The send path refuses user envelopes that begin with the magic (new reject reason
    9, `ReservedEnvelope`), so system frames are never user messages in either direction. `proto`
    stays free of cryptography (clients verify with their own aws-lc-rs); the system key is not
    deployed until a service issues notices (Prompt 3: "device list changed").
- Evidence (Apple M5 Pro, 18 cores, 48 GiB, macOS 27.2.1; compose images as pinned, under podman;
  report P02-send):
  - Allocation gate unchanged: 5.20 allocations (798 bytes) per message, before and after
    (`server_allocations_per_message_stay_within_budget`, budget 5.5).
  - Live tests on Iggy, SurrealDB, Valkey and RustFS on this Mac: 217 of 218 in the compose job's
    first command (the exception, `inv7_bounded_and_validated_input`, failed identically on
    `main` on macOS; the test is now robust to how macOS reports a refused write),
    `songbird-api` 8, `songbird-jobs` 12, `songbird-push` 7; `cargo xtask ci fast` passes.
  - New tests: `a_send_only_node_stores_on_the_owners_and_may_do_nothing_else` (Iggy cluster),
    `each_admitted_kind_of_peer_gets_only_its_own_handler`, five `send_api` end-to-end tests on
    Iggy (auth, limits and rate, idempotency across transports, offline push, system frames),
    send-path and system-frame unit tests with a fixed byte vector.
  - `bench/m7/suite.sh` on the VMPal VM `songbird-m7` (arm64, 12 vCPUs, kernel 7.0), `main`
    against this branch in one session, `IGGY_TMPFS=1`, checker PASS everywhere: steady p99
    2,339 against 2,124 µs, CPU per 10k messages 3.370 against 3.369 s, group p99 9,517 against
    9,991 µs, storm recovery 16.4 against 15.4 s; at 80,000/s (five runs each) p99 means 2,916
    against 3,056 µs, ranges overlapping (Mann–Whitney U = 8, not significant), server CPU per
    10k deliveries 0.427 s on both. No measurable cost from the extraction.
- Consequences: `api` can send with no new protocol and no extra hop, and can neither read nor
  delete anyone's messages; a compromised `api` can still store envelopes into mailboxes (as a
  compromised delivery node can: ciphertext recipients drop; threat model §6). `calls` and
  `worker` link the same crate and are admitted the same way when they send. Two threat-model
  rows, approved by the owner on 2026-11-14. A device using both transports has two rate budgets (at most
  twice the per-device rate).

## D-0043 — Milestone 7: the beta's load profile, how the targets are measured, where

- Date: 2026-11-13
- Status: accepted; results in report P02-M7. The owner approved the M7 gate on 2026-11-13,
  accepting this profile and the soak's move to the beta server.
- Decided by: agent, under the owner's delegation (D-0041: "think like someone from Proton or a
  big corp"); the master prompt leaves the targets to "validate and adjust with the owner"
- Context: M7's gate is "targets met, or gaps documented and accepted by the owner". The prompt
  states targets "at the beta's peak load" without defining that load, and the reference
  hardware (the beta server, a Hetzner `ccx33`: 8 dedicated vCPUs, 32 GB) does not exist yet.
- Decision:
  - **The beta's peak, defined.** 20,000 accounts at about 1.5 devices each: **30,000
    connections**, every one online at once (the worst case, not the average). **1,000 sends a
    second** 1:1 (about 4 messages a second for every 100 online people, several times a
    messaging app's evening peak) and **20 group sends a second** to 256-member groups (5,100
    deliveries a second). All with TLS 1.3 and the production session limits.
  - **What each target means, and how it is measured:**
    - *Server-side p99 send-to-deliver under 2 ms* is measured where it arises: from a `Send`
      frame read to the send durably stored, group commit included
      (`songbird_delivery_store_seconds`, a lock-free per-core histogram). Routing and the
      socket write add tens of microseconds. The load generator's client-observed latency is
      reported beside it as an upper bound.
    - *Recovery from a reconnect storm of every beta connection within 60 s, zero loss*:
      `loadgen --scenario storm` drops all 30,000 connections at once under peak traffic and
      times until the last device is connected again; the checker must pass.
    - *No allocation per message*: the M3 gate test stays (routing, writes and ACKs allocate
      nothing; the send path's five allocations are budgeted at 5.5).
    - *Cross-node p99 under 5 ms*: measured in M4 (cluster mode); not repeated unless the code
      on that path changes.
  - **Where:** a Linux VM on the owner's Mac (VMPal; Apple Silicon, arm64, 12 vCPUs, 32 GB,
    Ubuntu 26.04, kernel 7.0) runs the whole suite (`bench/m7/suite.sh`). It is not the
    reference hardware: an arm64 VM and a laptop. Its virtual disk takes about 9 ms per fsync
    (macOS flushes to its SSD on every one), so durable latency on it says nothing about a
    server's NVMe. The suite therefore runs Iggy's data in memory (`IGGY_TMPFS=1`) to measure
    the software path, as P02-M3 did, and the durable figure is a go-live check on the `ccx33`
    with the same command.
  - **Baselines:** each host class keeps its own baseline file; the suite fails when a tracked
    figure is more than 10 % worse. CI keeps the deterministic gates (allocations per message,
    the checker in every live test); timing gates need a quiet, fixed machine, so they run where
    the suite runs, not on shared CI runners.
- Results (report P02-M7):
  - **io_uring against epoll, measured again** (kernel 7.0, arm64, `bench/runtime-echo`): epoll
    9% faster on 1 core and 16% on 4, with lower p99. **epoll stays** (D-0028); io_uring adds
    kernel attack surface for a gain that is, at best, a few percent on other machines.
  - **mimalloc against glibc:** equal at light load, mimalloc's p99 27% lower at 80,000/s.
    **mimalloc stays.**
  - **ACK checkpoints** no longer take an Iggy write of their own under traffic; the beta's p99
    store latency dropped from ≤3 ms to ≤2 ms.
  - The 24-hour soak did not run (the owner's Mac cannot stay up a day). **The owner accepted the
    gap: the soak (`bench/m7/soak.sh`) runs on the beta server before launch**, a go-live check
    beside the durable-latency run of `bench/m7/suite.sh`.
- Consequences: the targets can be checked against definitions that are written down; the
  reference-hardware run (durable storage, x86) remains a go-live step.

## D-0042 — Hetzner only: off-site backups and OpenTofu state at Hetzner Object Storage

- Date: 2026-11-13
- Status: accepted (supersedes the provider choice in D-0040 and D-0041)
- Decided by: owner ("we dont use scaleway. only hetzner for now"); the agent designed the
  Hetzner setup
- Context: D-0040 and D-0041 put the off-site backups, and D-0009 the OpenTofu state of Hetzner
  environments, at Scaleway. The owner uses Hetzner only.
- Evidence (Hetzner's documentation, read 2026-11-13: Object Storage FAQ "Buckets & objects" and
  "S3 credentials"):
  - Object Lock with retention in **compliance mode**, set at bucket creation; versioning;
    lifecycle policies; console "Protection" against bucket deletion.
  - A key reaches every bucket **of its own project**; across projects, a bucket policy grants
    individual keys specific actions (`arn:aws:iam:::user/p<project>:<access key>`; `NotAction`
    is not supported). Keys are created in the console only.
  - Bucket settings take up to 15 minutes to apply everywhere.
- Decision:
  - **Backups:** Hetzner Object Storage in **Helsinki** (`hel1`), another location than the
    servers, so a site loss cannot take both.
    - Project `songbird-<env>-backups`: the bucket, and one key, the owner's admin key.
    - Project `songbird-<env>-backup-keys`: the writer, journal and verifier keys, and nothing
      else. The bucket policy grants writer and journal only `PutObject`/`PutObjectRetention`
      under `registry/` and `deletions/`, the verifier only reads and lists. So the data host and
      the worker still cannot read, list, delete or reconfigure, as in D-0040 and D-0041.
    - Compliance-mode Object Lock, versioned, expiry per prefix one day after its lock, console
      Protection on.
    - `deploy/bin/backups-up <env> apply` configures it (owner-run, AWS CLI against Hetzner's S3
      API; Hetzner Object Storage has no OpenTofu resources), and `check` proves with each key
      that Hetzner enforces every limit, the admin key unable to delete a locked version
      included, before the bucket is used. `deploy/bin/backups-policy selftest` runs in
      `validate-iac`. The Scaleway OpenTofu root is removed.
  - **OpenTofu state** of every environment: Hetzner Object Storage in Helsinki, in a project
    that holds only state.
  - **Code defaults:** `hel1.your-objectstorage.com:443`, region `hel1` (Ansible, `songbird-admin`,
    the worker's journal).
  - The Scaleway environment code (D-0009) stays, dormant, for a second provider later.
- Consequences: backups, state and servers share one provider and account. Different projects,
  keys and locations cover a compromised server, a lost location and operator error; they do not
  cover losing the Hetzner account itself (suspension, a legal order against Hetzner). Accepted by
  the owner for now. Cheap cover, optional: the owner downloads the newest monthly backup with the
  verifier key to an encrypted offline disk once a month (runbook step 10). Revisit when a second
  provider is added.

## D-0041 — Owner delegation of the M6 decisions; the deletion journal

- Date: 2026-11-13
- Status: accepted
- Decided by: agent, under the owner's explicit delegation ("i let you chose. think like someone
  from proton or a big corp"); the owner stated they have only a Hetzner VPS
- Context: report P02-M6 left five decisions to the owner: the gate, D-0040's four choices, the
  threat-model row for backups, a deletion journal, and what going live needs.
- Decisions:
  1. **M6 gate: approved under delegation, effective when PR #38's CI is green** (the bar the
     owner applied to M5). Met on 2026-11-13: 13 of 13 checks passed; merged as `d7894b5`. An author approving their own gate is weak separation of duties; the
     green CI on Linux, with the Iggy path and the new live tests, is the independent check.
  2. **D-0040's choices confirmed:**
     - RPO 5 minutes and RTO 2 hours for the registry.
     - **Superseded by D-0042 (owner: Hetzner only).** ~~Off-site at a different company from
       production.~~ Production is at Hetzner, so the backups go to Scaleway (EU, Object Lock in compliance mode, cents a month at this size).
       Backups at the production provider fail exactly when they are needed: account suspension,
       a legal order against the provider, a provider-wide incident. Hetzner Object Storage was
       not chosen even as a stopgap: nothing is deployed yet, so there is no gap to fill, and a
       stopgap tends to stay. If production ever moves to Scaleway, the backups move to the
       other provider (to build then).
     - The mailbox is never backed up.
     - The owner's identity is offline. A hardware key is not required: one paper copy in a
       safe, and one passphrase-encrypted copy (`age -p`) on a USB stick at a second place, the
       passphrase in the owner's password manager.
  3. **The threat-model row (deleted accounts in encrypted backups up to 93 days) is approved.**
     The master prompt's retention (7 daily, 4 weekly, 3 monthly) needs it; what makes it
     acceptable is that a restore can no longer bring a deleted account back (4).
  4. **The deletion journal is built.** Erasure (GDPR Art. 17, and the product's promise) must
     survive a restore.
     - Before deleting anything, the worker puts an empty tombstone in the backup bucket:
       `deletions/<time>-<HMAC-SHA256(journal key, account ID)>`, locked in compliance mode for 93
       days (as long as the oldest backup that can hold the account), expired the day after.
     - The worker's credential can only put under `deletions/` (bucket policy); it is required
       in staging and production (configuration validation), so no deployed worker deletes
       without a tombstone.
     - `songbird-admin backup restore` reads the journal key **before** restoring (no key: it
       refuses, unless `--no-journal`, which is audit-logged), then, after the import, starts the
       deletion again, through the outbox, of every restored account with a tombstone. The
       worker then clears every store again. Account IDs are random and never reused, so a
       tombstone's age does not matter; only `--before` (after a compromise) limits which count.
     - A tombstone holds no account ID: without the journal key a reader of the bucket learns
       only that some account was deleted at some time. That is a new §1 row, approved here.
  5. **Going live** needs, from the owner (provider per D-0042): the Hetzner backup projects
     and keys, the owner's age public key, and a second small Hetzner server for the
     verification environment (about EUR 4 a month). Nothing is deployed yet, so nothing is
     needed now.
- Evidence: `a_deletion_after_the_backup_survives_the_restore` (RustFS, SurrealDB, age: the
  account deleted after the backup comes back disabled with its deletion queued, the others
  untouched; a restore without the key refuses before touching anything);
  `account_deletion_is_verified_across_every_store` now also checks the tombstone and that it
  holds no identifier; the backups root's `tofu test` (journal writer limited to
  `deletions/*`).
- Consequences: the worker needs the journal's put-only key and the journal key; restores need
  the journal key (environment SOPS). A restore after a compromise uses `--before` for the
  journal too: tombstones written by an attacker holding the worker's key could otherwise
  delete accounts. Deployment templates for the services' own sections (the worker's included)
  are still to write; until then the validation refuses a deployed worker without the journal.

## D-0040 — Milestone 6: backups that hold no messages, age-encrypted off-site copies, store generation with client resend

- Date: 2026-11-13
- Status: accepted; built and measured in M6 (report P02-M6); the four choices below confirmed
  under the owner's delegation (D-0041)
- Decided by: agent under the owner's standing instruction ("think like a Proton founder")
- Context: master prompt §6 and milestone M6. What holds state that cannot be rebuilt changed
  since the prompt was written: the mailbox is Apache Iggy (D-0029) with per-message keys that are
  never backed up (D-0031); SurrealDB holds the account registry (D-0036); RustFS holds
  attachments (30 days at most) and, from Prompt 10, users' backup vaults.
- SurrealDB 4's recommended method (read 2026-11-13 in SurrealDB's "Backups & recovery" and
  `surreal db export` documentation): logical export (`surreal db export`, restored with
  `surreal db import`), optionally with volume snapshots; no point-in-time recovery or incremental
  backups (each export is one moment); copying SurrealKV files is safe only with the datastore
  closed; a dump holds no access signing keys and no record history (Songbird uses neither).
- Owner choices (recommendations, awaiting confirmation):
  1. Targets as proposed in §6: registry RPO ≤ 5 min, RTO ≤ 2 h; vaults RPO ≤ 24 h.
  2. Off-site, write-once copies at Scaleway Object Storage (Object Lock, EU), confirming D-0009.
     **Superseded by D-0042: Hetzner Object Storage, another project and location.**
  3. **The mailbox is never backed up.** Its envelopes could not be opened anyway (D-0031: keys
     never backed up); after a restore clients resend what has no delivery receipt (store
     generation). No message ciphertext outlives its delivery or 30 days in a backup.
  4. The age identity that can decrypt backups is held offline by the owner, in two places
     (a hardware key and a paper copy); never on a server.
- Decision (design):
  - **What is backed up:** the SurrealDB registry, by `surreal db export` every 5 minutes, through
    `songbird-admin backup`. Not backed up: the mailbox (above), Valkey (rebuilds itself; the
    outbox re-creates critical jobs), attachments (transient ciphertext; a resend after a restore
    re-uploads), secrets (SOPS in the repository already). Vaults join when Prompt 10 builds them.
  - **Encryption:** with the reference `age` tool (pinned, checksum-verified), to two recipients:
    the owner's offline identity and the restore-verification identity (below). No custom crypto.
  - **Off-site and immutable:** uploaded with S3 Object Lock in compliance mode, retention set
    per object (5-minute exports 24 h; the first export of each day 7 days, of each week 4 weeks,
    of each month 3 months), expiry by the bucket's lifecycle rule. The production credential can
    only put objects: it can neither read, list nor delete them, so a compromised server can
    neither read old backups (which hold deleted accounts until retention ends) nor destroy them.
  - **Daily restore verification:** not on production (a decryption key there would let an
    attacker recover deleted accounts from backups). This departs from the master prompt's
    "daily `worker` job" on purpose: the `worker` runs on production hosts. A separate verification job with its own
    read-only credential and its own age identity restores the latest export into a scratch
    SurrealDB, runs consistency checks (counts, references between devices and accounts, a sample
    of reads), and alerts when the latest verified backup is older than 24 hours. Its identity is
    rotated by re-encrypting nothing: new exports use the new recipient; old ones age out.
  - **Store generation:** stored in SurrealDB (`store:g`), read by delivery at startup
    and followed like the directory; every restore, and every loss of the mailbox, increments it
    (`songbird-admin generation bump`, part of the restore runbook). Clients that see it grow
    reset their cursor (built in M1) and resend every message without a delivery receipt from the
    last 30 days, reusing each message's idempotency key (a mailbox that survived replays it; a
    lost one stores it again); recipients drop repeats by the message ID inside the envelope.
  - **Drills:** `bench/m6/restore-drill.sh` times a full restore on the compose stack under load:
    back up, destroy SurrealDB and Iggy data, restore, bump the generation, let clients resend;
    the delivery checker must pass. The gate's timed drill runs it.
- Implementation (M6):
  - `songbird-admin backup run|verify|restore`, `generation show|bump`, `accounts import`;
    restore, bump and import need `--confirm <namespace>/<database>` and write an append-only
    audit log; restore only into an empty database.
  - **Restores trust the server's records, not the uploader's:** the writer key can put, and a
    put to an existing name makes a new current version on a versioned bucket, while the age
    recipients are public. So a compromised data host could replace an older backup with one
    of its own. Restores read the first stored version of each name, chosen by the server's
    `LastModified`; a name written twice, or a backup named for another time than it was
    stored at, fails verification and is never restored; `--before` restores from before a
    compromise.
  - The backup timer exports as a SurrealDB database user with the `VIEWER` role.
  - Off-site: `deploy/tofu/envs/backups`, an owner-only root (its own state and an
    Organization-level key, not the deploy pipeline's): a dedicated Scaleway project, a
    versioned bucket with compliance-mode Object Lock, expiry per tier one day after its lock, a
    writer limited to `PutObject`/`PutObjectRetention` and a verifier limited to reading.
  - Verification runs on a host in an environment of its own (topology role `verify`, which the
    topology module refuses beside production roles), set up by the owner with
    `backup-verify.yml`; its identity and read key are in an owner-only SOPS file.
  - The client keeps a bounded send log (100,000 messages, 30 days) and remembers receipts that
    arrive before its own `SendResult`, so a confirmed message is never resent.
- Evidence: report P02-M6; `bench/m6/restore-drill.sh` (3,000 devices at 5,000/s: outage 1.23 s,
  checker PASS; without resend 11,581 lost); `an_overwritten_backup_is_never_restored_and_verify_reports_it`;
  the tofu test of the backups root.
- Consequences: backups contain account and device metadata only (encrypted, write-once,
  unreadable from production); recovery of undelivered messages depends on clients (Prompt 4's
  delivery receipts make it complete; until then the Rust client keeps its own send log). Needs
  from the owner: confirmation of the four choices, a Scaleway project with Object Storage, and
  the age identity's public key.

## D-0039 — CI on a self-hosted runner, isolated, with Forgejo to follow

- Date: 2026-11-13
- Status: superseded by D-0045 (the repository became public: GitHub-hosted runners cost nothing,
  and a self-hosted runner must never run code from outside pull requests); the scripts stay for a
  private forge later
- Decided by: owner (GitHub Actions minutes exhausted; "do both, runner first": a self-hosted
  runner now, a move to self-hosted Forgejo next); setup by agent.
- Context: the private repository's 2,000 free monthly minutes ran out during M5. The nightly
  workflow and six image builds per push were the largest consumers.
- Decision:
  - **Runners** on the owner's Hetzner VPS (`coolify.sockudo.io`, Falkenstein; 8 x86 cores,
    16 GB, Ubuntu 24.04), which also runs Coolify and the Sockudo docs. Two runners
    (`songbird-1` with the `songbird-stack` label for jobs that start the compose stack or bind
    fixed ports, one at a time; `songbird-2`), as systemd services.
  - **Isolation:** an unprivileged `ci` user with its own **rootless Docker** daemon: CI jobs
    cannot see or reach the host's Docker (verified: `permission denied` on the host socket, zero
    containers visible). Caps through systemd slices: runners 6 cores and 7 GB, the rootless
    daemon and its containers 4 cores and 5 GB. Runner units with `NoNewPrivileges`,
    `ProtectSystem=full`, private `/tmp`. No Songbird production secrets on the host.
  - **Not ephemeral:** an ephemeral runner re-registers for every job, which needs a long-lived
    GitHub token on the host; persistent runners on a private repository that only the owner
    pushes to keep less on the box. Registration tokens (one hour) are fetched with `gh` and
    passed over SSH, never stored.
  - **Rootless details:** AppArmor profile for `rootlesskit` (Ubuntu 24.04 restricts unprivileged
    user namespaces); containers reach the host's loopback through `10.0.2.2`, used by
    `cargo xtask autobahn` when `SONGBIRD_AUTOBAHN_HOST` is set; rustup 1.29.3 from upstream,
    checksum-verified (Ubuntu's 1.26 cannot install the toolchain `rust-toolchain.toml` names
    without arguments).
  - **Workflows:** `ci`, `codeql` and `nightly` run on the runners; `release` and `deploy` stay on
    GitHub-hosted runners (rare; they sign and attest releases). Docs-only pull requests skip
    `ci` and `codeql`.
  - Reproducible: `deploy/ci-runner/provision.sh` and `add-runner.sh` (idempotent).
  - Runner units use `KillMode=mixed`, not GitHub's suggested `process`: with a private `/tmp`, a
    listener that outlived a restart kept taking jobs in a namespace whose `/tmp` was gone (the
    first CI run's CodeQL and Ansible jobs failed on it).
- Consequences: no minute limits; CI depends on one VPS (a dead VPS means local checks until it is
  rebuilt from the scripts). The host's other workloads share the kernel. Next: Forgejo
  (self-hosted forge and Forgejo Actions), with GitHub kept as a private mirror; that move is
  recorded when made.

## D-0038 — Prompt 3 amendment: invite-only registration

- Date: 2026-11-13
- Status: accepted (prompt amended; built in Prompt 3)
- Decided by: owner (prompt amendment)
- Context: the launch is invite-only. Registration abuse controls in Prompt 3 (proof of work,
  per-IP throttles) do not limit who joins, and an invite system is the usual place where a
  messenger quietly records its social graph.
- Decision: `docs/prompts/03-identity.md` gains an **Invite-only launch** item under "Also build":
  - a server-side invite-only mode, switchable off without a migration;
  - random invite codes stored only as hashes, single- or multi-use, with expiry, and
    rate-limited checks so codes cannot be guessed;
  - a per-user invite allowance (configurable, for example five) and admin-generated batches;
  - **who invited whom is not stored by default**: only per-user counts. Storing the link for
    abuse handling would need the owner's decision here and a threat-model row.
  - Tests: valid, expired, reused and guessed codes; quota limits; mode switched off.
- Consequences: Prompt 3 has not started, so nothing is built yet; its issue gains the item in
  Build and its tests in Gate. Open owner decision: whether to store inviter links for abuse
  handling (default no).

## D-0037 — Prompt 2 amendment: no internal network Send API; a `send` library and `PUT /v1/messages`

- Date: 2026-11-13
- Status: accepted (prompt amended); built (D-0044, report P02-send)
- Decided by: owner (prompt amendment)
- Context: Prompt 2 asked for an internal, mutual-TLS "Send API" so `api`, `calls` and `worker`
  could send through the store-then-route path. A network hop adds latency, a second protocol to
  secure, and a second path that can drift from the WebSocket's.
- Decision: `docs/prompts/02-realtime.md` replaces that item:
  - the **`send` crate** (validate, store, publish on the device's Valkey channel) is a library
    that `delivery`, `api`, `calls` and `worker` link directly, so every send takes the same
    tested path with no extra hop;
  - server-originated notices (for example "your device list changed") are server-signed
    system frames, never user messages;
  - for clients with no open WebSocket (notification quick-reply, share extension, failed
    socket), `api` exposes **`PUT /v1/messages`**, which takes the same encrypted envelopes as
    the WebSocket `Send` frame and calls the same crate.
  - The amended text says "store in SurrealDB"; the store is the mailbox, which is Apache Iggy
    since D-0029 (SurrealDB holds the registry), so the crate stores through the `Mailbox` trait.
- Evidence: no internal Send API exists in the code (searched 2026-11-13), so there is nothing
  to refactor. The worker's device link (D-0036, `[delivery.internal]`) is not a send interface:
  it accepts only `worker.` certificates and only Forget and Known.
- Consequences: still to build in Prompt 2, before its final gate:
  - extract the send path from `services/delivery/src/session.rs` (device-list check,
    idempotency, store, route, push on offline) into `crates/send`;
  - `api`'s `PUT /v1/messages` on it, signed like every `api` request, with the same limits as the
    `Send` frame; server-signed system frames in `crates/proto`;
  - `api`, `calls` and `worker` then need the mailbox and router connections `delivery` has.
    With Iggy partitions owned by delivery nodes (D-0034), a send from `api` reaches a
    partition's owner over node links like a non-owning delivery node does, which needs `api`
    nodes admitted to those links (today `delivery.` certificates only); to decide when built.

## D-0036 — Milestone 5: sealed jobs, push that leaks nothing, a minimal account registry, uploads through `api`, deletion proven across every store

- Date: 2026-11-12
- Status: accepted (design); built and measured in M5
- Decided by: owner for scope (a minimal account and device registry in SurrealDB now; push
  tokens and attachment records added to the threat model's list); design by agent under the
  owner's standing instruction ("think like a Proton founder or a top-tier engineer").
- Context: M5 builds `jobs` with an outbox, `push`, `worker` and `api` with RustFS uploads. Gate:
  an offline device receives a push end to end; account deletion verified across every store.
  No account store existed (delivery read a static file); the mailbox is Iggy (D-0029), owned
  per partition by delivery nodes (D-0034); RustFS listens on loopback behind an mTLS tunnel
  (D-0035), so clients can never reach it directly.
- Decision:
  - **Account registry (`crates/accounts`, SurrealDB).** Tables `account` and `device` (IDs,
    the Ed25519 auth key, a `disabled` flag), `directory:version` bumped by every change. Delivery
    loads its directory from SurrealDB and reloads it when the version changes (polled every
    second; the static file stays for development). Registration proper (usernames, keys, proof
    of work) is Prompt 3; until then accounts are created through an internal mutual-TLS endpoint
    of `api`.
  - **Jobs (`crates/jobs`, Valkey Streams on `valkey-jobs`).** One stream per kind (`push`,
    `cleanup`, `deletion`; `backup` arrives with M6), one consumer group each. Jobs carry IDs
    only, and **every job is sealed** (AES-256-GCM-SIV under a key derived from the jobs secret,
    bound to the stream and the job ID): `valkey-jobs` persists to disk, and a plaintext queue of
    device IDs would be a record of who was messaged when. Per-job `XACK` plus `XDEL` (an
    acknowledged job leaves memory and the AOF rewrite). Stuck jobs: `XPENDING … IDLE` then
    `XCLAIM` (instead of `XAUTOCLAIM`, because `XPENDING` also returns the delivery count, which
    bounds retries of a job that crashes its consumer). Failures: exponential backoff with full
    jitter through a delayed sorted set promoted by a Lua script; after the last attempt a
    dead-letter stream (capped) with a metric to alert on. Every handler is idempotent.
  - **Outbox.** A SurrealDB `outbox` table written in the same transaction as the change that
    needs a job (account deletion); a relay in `api` and `worker` moves rows to the stream and
    deletes them. A crash between the two repeats a job, which handlers tolerate.
  - **Delivery → push.** `Routed::Offline` enqueues a push job without touching the hot path:
    a per-core debounce (one job per device per window) and a bounded buffer flushed as one
    pipelined `XADD` batch; when Valkey is down the buffer drops and counts (the message is safe
    in the mailbox; the device fetches it at its next connection; runbook).
  - **Push tokens.** Registered through `api`, **sealed to the push service's X25519 public key**
    (ephemeral X25519, HKDF-SHA256, AES-256-GCM; `api` and `worker` hold only the public key, so
    only `push` can ever read a token), stored under a keyed hash of the device ID (HMAC-SHA256,
    push-index secret) so the table alone does not link a token to a device. Deleted at once when
    the provider reports the token unregistered, and with the device.
  - **Push service.** APNs (HTTP/2, ES256 provider token, `mutable-content: 1`, one generic alert
    text, constant `apns-collapse-id`), FCM HTTP v1 (OAuth 2 service-account token, high-priority
    data message with no data, constant `collapse_key`), UnifiedPush (Web Push, RFC 8291
    `aes128gcm` and RFC 8292 VAPID, constant body). Every push looks identical whatever caused
    it. Per-device debounce, a circuit breaker per provider, retries through the job queue.
    Provider credentials exist only in this service's configuration. All crypto through
    aws-lc-rs; HTTP/2 through `hyper`'s client connection API (adds `h2`).
  - **API without a framework.** `hyper` server directly (already in the tree), not Axum: a
    handful of endpoints, bodies bounded before reading, fewer crates to audit. Device requests
    are signed with the device's Ed25519 key over a domain-separated transcript (method, path,
    server name, timestamp, nonce, body digest); timestamps within 60 s and nonces single-use
    (keyed hash in `valkey-hot`, expiring). Account creation and test hooks are on a separate
    mutual-TLS listener.
  - **Attachments.** Uploads stream through `api` to RustFS (RustFS is never exposed): a grant
    fixes an exact padded size and how many recipient fetches are due; the upload must match the
    size exactly; object names are 32 random bytes (no identifiers). After upload the record holds
    no link to the uploader. Each download decrements the count; at zero a cleanup job deletes
    the object, and the worker deletes everything older than 30 days. Our own S3 client (SigV4
    with aws-lc-rs over `hyper`), instead of the AWS SDK and its dependency tree. Each service
    gets its own least-privilege RustFS credential (D-0035).
  - **Account deletion.** `api` disables the account's devices (sessions refused within a
    directory reload) and writes the outbox row in one transaction. The worker then, each step
    idempotent: asks delivery, over a mutual-TLS internal link that accepts only `worker.`
    certificates and only these operations, to close the devices' sessions and **forget** their
    mailboxes (pending entries acknowledged, so their keys are punctured, and counters dropped);
    deletes push tokens, pending upload
    grants and their partial objects, device and account rows; repeats the mailbox step after
    the directory has propagated; and verifies every store holds nothing (SurrealDB rows, mailbox
    state on the owner, push tokens, RustFS objects of pending grants). Attachments already sent
    are not linked to the account by design; they go when fetched or after 30 days. Backups are
    M6's.
- Implementation (refinements while building):
  - **No tombstones.** The design had a short tombstone so a send racing the deletion would skip
    the device; built instead: the worker forgets the mailboxes again once every node has
    reloaded its directory (`propagation_ms`, default 3 s), then verifies. Simpler (no new
    state in the writer, nothing that expires) and the verification proves the result.
  - **Mailbox `Forget` record** (sealed format 4; format 3 still read): acknowledges the
    devices' pending entries (keys punctured) and drops their counters; replay applies it, so a
    restart or a standby's takeover keeps nothing. Routed to the partition owner over node
    links (`Forget`, `Known`). `Mailbox::forget` and `Mailbox::knows` on every backend.
  - **Directory** swapped whole with `arc-swap` 1.9.5 (constitution: hot-reloaded state;
    readers take no lock; one new crate plus `rustversion`, a proc-macro). `arc-swap` is exempted
    in `cargo vet` pending an audit (its publisher is not trusted by two imported sets, D-0016),
    like the other client libraries; `h2`'s publisher record is in `supply-chain/imports.lock`.
    `Router::close_everywhere` closes a deleted device's session on every node.
  - **API**: `hyper` 1.12.0 directly; the `api` crate is a library so tests run it in process;
    a signed client (`songbird_api::client`) for tests and tools. A download counts its fetch
    when its last byte is handed to hyper (hyper stops polling a body at `Content-Length`, so
    end-of-stream never comes; found by the live test).
  - **Push service** (`services/push`, built on `crates/push-providers`): debounce keyed by the
    push-row hash (no device IDs in memory), first attempts only; an unregistered token's row
    is deleted only if it still holds the token that failed (a re-registration in between is
    kept); a VoIP token reported unregistered deletes the whole APNs row (the app re-registers).
  - `crates/valkey` holds the self-healing connection (moved from the router) and shared
    settings; `crates/accounts` the registry and push-token sealing; `crates/link::devices` the
    worker's request format.
- Evidence (Apple M5 Pro, 18 cores, 48 GiB, macOS 27.2.1; Docker 6.1.4 Linux VM; compose images
  as pinned; release build; loopback; [P02-M5 report](reports/P02-M5.md)):
  - Gate: `tests/e2e/tests/gate_m5.rs` `an_offline_device_receives_a_push_end_to_end` and
    `account_deletion_is_verified_across_every_store` pass (3 of 3 runs) on SurrealDB (registry
    and mailbox), valkey-hot, valkey-jobs and RustFS. Iggy cannot run on this machine (the Docker
    VM's kernel refuses io_uring even unconfined); the Iggy forget test
    (`forgotten_devices_leave_nothing_readable_and_no_counters`) and the gate on Iggy run in the
    Linux compose CI job.
  - Job queue: 74,800 enqueues/s (pipelined batches of 256), 166,600 consumed, acknowledged and
    deleted per second (`m5_numbers`).
  - Attachments through `api` to RustFS: 1 MiB upload p50 11.1 ms / p99 12.1 ms (90 MiB/s),
    download p50 3.2 ms / p99 3.7 ms; 16 MiB upload p50 114 ms / p99 120 ms (141 MiB/s),
    download p50 13.4 ms / p99 14.4 ms (1.2 GiB/s).
  - Live tests: `crates/jobs` (5), `crates/accounts` (3), `crates/s3` (5), `services/api` (3),
    `services/push` (1 covering bursts, calls, unregistered, retry, no identifiers); unit tests
    for sealing, signing, record and wire formats (Forget, Known), push-token sealing and every
    provider (RFC 8291 vector, SigV4 vectors).
- Consequences: Valkey still learns nothing (routing D-0034; jobs sealed); new metadata limited
  to the two rows the owner approved; the static directory is no longer the only source of
  devices, so `delivery` becomes deployable once Prompt 3 adds registration.
- Dependencies (s3): `crates/s3` (`songbird-s3`) adds no crate to `Cargo.lock`. It enables
  hyper 1.12.0's `client` feature in its own manifest (the workspace keeps `http1` and `server`);
  that feature's dependencies `want`, `pin-project-lite` and `smallvec` were already locked.
  Everything else (aws-lc-rs for HMAC-SHA256 and SHA-256, hyper-util 0.1.21's `TokioIo`,
  tokio-rustls, rustls, bytes, futures-core, hex, zeroize, thiserror) was already a workspace
  dependency. APIs read in the pinned sources first: hyper `client::conn::http1::{Builder,
  SendRequest::{ready, try_send_request, is_closed}}`, `TrySendError::take_message` and the h1
  dispatcher's body loop (it stops polling a body at `Content-Length`, so the upload body holds
  the completing chunk back until the source confirms its end); http-body 1.1.0 `Body`,
  `Frame`, `SizeHint`; aws-lc-rs 1.19.0 `hmac::{Key, sign, HMAC_SHA256}`, `digest::digest`;
  tokio-rustls 0.26.6 `TlsConnector::connect`. SigV4 is checked against AWS's published vectors
  (`get-vanilla`, `get-vanilla-query-order-key-case`, `get-vanilla-empty-query-key`, and the S3
  GET object, PUT object, GET lifecycle and list examples); `crates/s3/tests/rustfs_live.rs` runs
  against RustFS 1.0.5 in the compose job.
- Dependencies (push providers, `crates/push-providers`): one crate new to `Cargo.lock`, **`h2`
  0.4.20**, brought by enabling `hyper`'s `client` and `http2` features (HTTP/2 to APNs and FCM
  through `hyper::client::conn::http2`; Apple requires HTTP/2). Published by Sean McArthur like
  `hyper`, so covered in `cargo vet` by a `trusted.h2` entry under D-0016's rule; MIT. Its own
  dependencies were all in the tree already. Direct dependencies already in the tree, now in
  `[workspace.dependencies]`: `serde_json` 1.0.154 (service-account file, OAuth and provider error
  bodies, JWT claims; off the hot path, response bodies capped at 16 KiB before parsing) and
  `webpki-roots` 1.0.12 (Mozilla's roots compiled in, so the push service does not depend on the
  container's CA bundle; CDLA-Permissive-2.0, exception already in `deny.toml`). Not taken:
  `jsonwebtoken`, `p256`, `ring`, `base64` (JWTs, VAPID and RFC 8291 use our own 60-line
  unpadded base64url, tested against RFC 4648), `url` (endpoints parsed with `http::Uri` plus
  explicit checks). All signing, ECDH, HKDF and AES-GCM through aws-lc-rs 1.19.0; APIs read in the
  pinned sources (hyper 1.12.0 `client::conn::{http1,http2}`, `server::conn::{http1,http2}`;
  hyper-util 0.1.21 `rt::{TokioExecutor, TokioIo, TokioTimer}`; aws-lc-rs `agreement::{PrivateKey,
  agree}`, `signature::{EcdsaKeyPair, RsaKeyPair}`, `hkdf`, `aead::LessSafeKey`; http-body-util
  0.1.5 `Limited`; rcgen 0.14.10 for the mocks' test CA, behind the `test-support` feature).

## D-0035 — Object storage: RustFS replaces Garage

- Date: 2026-11-12
- Status: accepted
- Decided by: owner ("replace garage with rustfs"); a change to the constitution's locked stack.
- Context: Garage v2.5.0 (D-0007) was the S3 store for encrypted attachments and backup vaults.
  No Songbird crate talks to S3 yet (attachment uploads arrive in P02-M5), so the change touches
  only docker compose, CI, Ansible, secrets generation and the documentation.
- Options considered: keep Garage v2.5.0; RustFS 1.0.5 (Apache-2.0, written in Rust). No benchmark
  between the two was run; the owner chose RustFS directly. Upload throughput and p99 are measured
  on RustFS in P02-M5.
- Evidence (read from the RustFS 1.0.5 source and its GitHub releases and advisories, 2026-11-12):
  - Releases: 1.0.0 GA on 2026-09-16; 1.0.5 on 2026-11-10 (five patch releases in eight weeks).
  - 43 published GitHub security advisories since December 2025, mostly authorization flaws (IAM,
    bucket policies, presigned uploads, service accounts). Eight affect 1.0.0 or 1.0.1 and are
    fixed in 1.0.2, three of them high: Object Lock COMPLIANCE bypassed by force-deleting a bucket;
    the FTP, SFTP and WebDAV listeners bypass bucket policies; SSE-C objects expose the plaintext
    MD5 as their ETag. None is published against 1.0.5.
  - Defaults that matter here: the web console is on (`RUSTFS_CONSOLE_ENABLE`, default true); an
    update check calls `https://version.rustfs.com/latest.json` (`RUSTFS_CHECK_UPDATE`, default
    true); FTP, FTPS, SFTP and WebDAV are off by default; the "Connect" heartbeat runs only when
    `RUSTFS_CONNECT_*` is configured. The admin API is served on the S3 port. Root credentials come
    from `RUSTFS_ACCESS_KEY` and `RUSTFS_SECRET_KEY` (access key at least 3 characters, secret at
    least 8); the image entrypoint warns on the default `rustfsadmin` pair. Health: `/health/ready`.
    The image runs as uid 10001. A single data path skips the distinct-disk check, so single node,
    single disk needs no unsafe override.
- Decision: RustFS 1.0.5, single node and single disk (like Garage with replication factor 1), in
  compose and on the data host:
  - only the S3 API listens: `127.0.0.1:9000` on the data host, reached from the private network
    through one ghostunnel mutual-TLS tunnel on port 9010 (Garage needed three ports: RPC 3901,
    S3 3910 and admin 3913);
  - console, FTP, FTPS, SFTP, WebDAV and the update check set off explicitly; logs to stdout;
  - container without capabilities, `no-new-privileges`, data in `/var/lib/songbird/rustfs`
    owned by uid 10001;
  - secrets `rustfs_access_key` and `rustfs_secret_key` (generated by `deploy/bin/secrets-init`
    and `cargo xtask dev-env`) replace Garage's RPC secret, admin token and metrics token. No
    layout step: `cargo xtask compose up` and the data role no longer apply one.
- Consequences:
  - Objects stay client-side ciphertext, so an authorization flaw in RustFS exposes ciphertext,
    sizes and object names, not content. Keep it that way: object names carry no identifiers.
  - Watch RustFS advisories and upgrade promptly; the image digest is still to be pinned (see Open
    owner decisions).
  - P02-M5: give each service its own least-privilege RustFS credential instead of the root pair;
    prefer uploads through `api` or presigned PUT with an exact length (presigned POST and
    presigned uploads have had three advisories); do not enable SSE-C (plaintext-MD5 ETag
    advisory) or Object Lock until their fixes have aged.
  - Multi-node later (vault RPO, three copies across zones) needs RustFS distributed mode or site
    replication and `RUSTFS_RPC_SECRET`; decided when Prompt 10 arrives.
  - Developers with an existing `.env` run `cargo xtask dev-env --force`.

## D-0034 — Milestone 4: Valkey routing that sees nothing, owned partitions over node links, replicated key stores

- Date: 2026-11-12
- Status: accepted (design); built and measured in M4
- Decided by: agent under the owner's delegation ("choose the best variant for maximum speed and
  security; think like someone from Proton"; the owner dropped the Valkey-against-Iggy routing
  comparison)
- Context: M4 runs several delivery nodes. The master prompt's cross-node design assumed a shared
  SurrealDB mailbox; with Iggy (D-0029) every partition has exactly one writer, and with D-0031
  its key store is on that writer's disk.
- Options considered for the key store across nodes: (A) local disk only: a dead node's partitions
  wait for it and are lost with its disk; (B) the key store in persistent, replicated Valkey: fast
  takeover, but sealed keys in a networked store and deleted keys in its append-only file until a
  rewrite; (C) the owner replicates its key store to a standby node over a mutual-TLS link: fast
  takeover, keys never leave our nodes.
- Decision:
  - **Routing: Valkey pub/sub** (master prompt default), one channel per device, the
    subscriptions being the presence registry (`PUBLISH` returns 0: offline; a new connection
    publishes `close` before subscribing). Valkey learns nothing: channel names are keyed hashes of
    the device ID (HMAC-SHA256, cluster secret), payloads (envelope notifications, close) are
    sealed with a cluster key (AES-256-GCM-SIV, bound to channel and kind; HKDF from the cluster
    secret in the secrets store, rotatable), connections use TLS
    with the internal CA (D-0012). Re-subscribe in batches after a Valkey restart or failover.
  - **Mailbox partitions: one owner node each**, a lease in Valkey (renewed; expiry hands the
    partition over), Iggy producer epochs fencing any stale owner (D-0029). Sends, fetches and
    ACKs for a partition owned elsewhere go to the owner over **node links**: per-core framed TCP
    with mutual TLS (internal CA), so envelopes never pass through a third-party store.
  - **Key stores: option C, three copies.** The owner copies every key-store write to two
    standby nodes over node links, the second in another zone (D-0034 revision, after the owner
    asked whether Proton or Signal would accept one standby: no; three copies across failure
    domains is the norm). A new key window seals records only once at least one standby holds
    it; punctures follow within the key interval, so an acknowledged message is unopenable on
    every copy within about a second. On the owner's death its first live standby takes the
    lease and the partitions.
- From Sockudo's Redis Cluster work (sockudo/sockudo#510 and its sharded pub/sub transport),
  adopted: subscriber connections are pinged and replaced when silent (half-open connections
  otherwise miss messages); no single hot channel (our per-device names are keyed hashes, so they
  spread evenly over hash slots; any future node-wide channel is bucketed across slots); changes of
  channel layout (rotating the cluster secret, moving from `PUBLISH` to sharded `SPUBLISH` when
  Valkey becomes a cluster) roll out in adjacent-compatible phases (publish both, subscribe both,
  drop the old), duplicates during the overlap being dropped by `seq`; for the cluster, a
  slot-aware subscriber with one connection per shard primary (`redis-rs` exposes no sharded
  subscribe on its cluster client); no replica connections (the router never reads).
- Implementation (refinements found while building and chaos-testing):
  - Every Valkey connection used for commands (publishes, leases, members) is a self-healing
    multiplexed connection that drops itself on an I/O error, timeout or refusal and reconnects
    on the next command: `redis-rs` multiplexed connections stayed dead after a Valkey restart
    (chaos run 2 hung, leases lapsed and two nodes ran one partition until Iggy fenced it).
  - Leases: `SET NX PX` and Lua renew/release keyed by the holder; default lease 3 s. Members
    in a sorted set whose expiry scores come from Valkey's clock (a Lua script reads `TIME`), so
    no node clock matters. Placement by rendezvous ranking (SHA-256 of member and partition):
    the first should own the partition; its standbys are the next one and the next one in a
    zone neither of them is in.
  - Takeover of a free partition: the old owner's first live standby that holds a copy at once,
    the next a lease period later, anyone else after three. One extra lease period only if the
    owner may be alive: still a member, or Valkey just lost everything (each node notices when
    its own registration vanished). No wait after a graceful handover (`released` marker). A
    node still running a partition whose lease vanished takes it back with `SET NX` without
    restarting anything; it stops only if another node holds the lease.
  - Rebalancing: an owner hands a partition to the node ranked first for it once that node has
    been a member for two lease periods and holds the whole store (it is then the first
    standby), one partition per round; the handover is the graceful one.
  - Key-store replication: the paged key store's write returns a replica (the changed pages);
    a standby validates page names and epochs before applying. A new standby, or one a copy
    failed to reach, first gets the whole store, which also deletes key files it no longer
    lists. A node that has not been a standby of a partition for three lease periods deletes its
    copy. A window becomes usable only when the replicator reports a standby holding everything
    (or no other node is live).
  - Node links: a stopped server closes its open connections (a stopped node acknowledges
    nothing); a client fails calls on a connection the peer closed at once.
  - Close messages older than 30 s are ignored (a replayed close cannot evict a session).
- Dependencies: `redis` 1.8.1 (default features off; Tokio and rustls only), bringing `arcstr`,
  `async-lock` and `xxhash-rust` (the last under BSL-1.0, an OSI-approved permissive licence:
  exception in `deny.toml`); all four exempted in `cargo vet` pending audits (same footing as the
  other client libraries).
- Evidence (AMD Ryzen 9 7950X, three delivery nodes of 2 cores, Iggy on tmpfs, Valkey 9.1.3
  container, twelve partitions; [P02-M4 report](reports/P02-M4.md)): cross-node p99 0.7 ms at
  5,000 messages/s, 1.0 ms at 20,000, 2.0 ms at 50,000 (two of three messages crossing nodes);
  chaos phases (partition owner `kill -9`, Valkey restart without persistence, owner SIGTERM,
  two nodes killed at once, rolling restart) pass the checker with no loss, duplicate or
  reordering; failover about 3 s after a crash at a 3 s lease, 4.6 s with two nodes killed.
  Tests: `crates/router/tests/valkey_live.rs` (6, including dropping every Valkey connection
  mid-run), `crates/link/tests/link.rs` (4), `crates/mailbox/tests/iggy_cluster.rs` (8: remote
  partitions, standby takeover with nothing lost, graceful handover, leases lost with Valkey
  taken back without restarts, renewals after every connection dropped, a joining node takes
  its share, two nodes die at once, a window waits for a standby); placement and key-store
  copy unit tests; node-to-node requests fuzzed in `iggy_record`.
- Consequences: losing any two nodes loses no pending message (two standbys per partition);
  with fewer live nodes than copies, writes continue on the copies that exist; Valkey sees
  opaque channels and blobs only; the node link is new code with its own tests and fuzz target. DirectLinks for live routing stays an alternative to measure only if the
  Valkey hop shows in the cross-node p99.

## D-0033 — Unsafe exception: a test-only counting allocator

- Date: 2026-11-12
- Status: accepted
- Decided by: agent (constitution: an `unsafe` exception must be measured, reviewed and recorded)
- Context: M3's gate requires proving that steady-state delivery does not allocate per message,
  and keeping the proof in CI. Counting allocations needs a `GlobalAlloc`, which can only be
  implemented with `unsafe`; every Songbird crate forbids `unsafe_code`.
- Options considered: `dhat` (does the counting inside its own `unsafe`, but adds about eight crates
  to the audit backlog and counts process-wide, so the test's clients would be counted with the
  server); `heaptrack` or valgrind's DHAT (external tools, not a CI assertion; used to attribute the
  sites); our own allocator in a separate test-only crate.
- Decision: `crates/alloc-count` (`songbird-alloc-count`): a `GlobalAlloc` that forwards every call
  to `std::alloc::System` with the caller's own arguments and counts allocations on threads that
  opted in (a const-initialised thread-local flag, two relaxed atomics). Its lint table is the
  workspace's except `unsafe_code = "deny"`, allowed on the one `impl` with a `SAFETY` comment per
  block. No service depends on it; only `tests/e2e` (dev-dependency). Its own test checks the
  per-thread counting; the deep CI tier runs it under Miri. semgrep's `unsafe-usage` rule is waived
  on exactly those four lines (`nosemgrep` with the rule's id), nowhere else.
- Evidence: `tests/e2e/tests/allocations.rs` (budget 5.5 allocations per message; measured 5.20),
  valgrind DHAT attributing each site (report P02-M3).
- Consequences: one reviewed `unsafe` block family, test-only. Any new `unsafe` elsewhere still
  needs its own entry.

## D-0032 — Milestone 3: thread-per-core runtime, device directory, inboxes, metrics, compio

- Date: 2026-11-12
- Status: accepted; the runtime choice confirmed by the owner (2026-11-12: "let's stick with
  tokio")
- Decided by: agent, by benchmark (master prompt §4: "benchmark X against Y, decide by p99 and
  throughput")
- Context: M1 and M2 ran `delivery` on one current-thread Tokio runtime: one core.
- Hardware for every number below: AMD Ryzen 9 7950X (16 cores, 32 threads), 30 GiB, Linux
  7.3.1, Rust 1.99.1; NVMe with btrfs (zstd); loopback; same session. Load generator on other
  physical cores than the server.
- Decisions:
  - **Runtime** (`crates/runtime`): one OS thread per worker, pinned to its own CPU
    (`sched_getaffinity` order, so `taskset` and cgroup cpusets confine it), each running a
    current-thread Tokio runtime; one listener per worker with `SO_REUSEPORT` (`socket2`), bound
    on the main thread before workers start. Pinning uses `rustix` (already in the graph, safe
    API) instead of `core_affinity` (adds `num_cpus`, unmaintained since 2023). No new crates.
    `[delivery.runtime] workers` (0: every allowed CPU), `pin`, `backlog`, `storage_threads`.
  - **Storage runtime**: the mailbox runs on its own multi-thread runtime (`storage_threads`,
    default one per Iggy partition), so partition writers run in parallel beside the delivery
    cores; on the main thread they capped a node at about 216,000 messages/s and fell to 165,000
    with 8 cores.
  - **Device directory: papaya** (`bench/directory`, own lockfile; 200,000 devices, 98 %
    lookups, 2 % attach/detach churn):

    | Threads | Mutex<HashMap> | papaya | scc 3.8.8 | dashmap 6.2.1 |
    | --- | --- | --- | --- | --- |
    | 1 | 44.9 M ops/s | 29.8 M | 36.7 M | 39.0 M |
    | 4 | 11.3 M (p99 3.7 µs) | 112.4 M (p99 80 ns) | 100.8 M (80 ns) | 100.7 M (90 ns) |
    | 8 | 7.6 M (p99 8.5 µs) | 215.9 M (90 ns) | 187.2 M (81 ns) | 181.4 M (100 ns) |
    | 16 | 4.8 M (p99 30.6 µs) | 362.6 M (211 ns) | 134.9 M (381 ns) | 127.9 M (421 ns) |

    papaya has the highest throughput and the best p99 from 8 threads up (its p99.9 is 0.6–0.8 µs
    from epoch reclamation; scc's 0.1–0.5 µs); already in the graph through the Iggy SDK, vet
    exempted. Routing reads the session handle under papaya's guard without cloning it.
  - **Cross-core inbox: tokio mpsc kept.** At 2 M messages/s spread over 8 cores: p99 6–22 µs for
    tokio mpsc, 7–160 µs for crossbeam with a `Notify`, 0.87 ms for kanal; flat out, tokio and
    crossbeam both about 22–35 M msgs/s, kanal 13–15 M with 2–11 % drops. tokio mpsc needs no
    extra crate and is async-native; sessions keep their per-session bounded inboxes.
  - **Metrics: per-core shards** (`songbird_telemetry::sharded`): one value per core on its own
    128-byte line, summed at scrape; workers name their shard at start.
  - **compio: not adopted (D-0028; owner's decision).** Same sockudo-ws echo, 512 connections,
    4 in flight, 256 bytes (`bench/runtime-echo`): per-core Tokio 176,000–178,000 round trips/s
    on 1 core and 717,000–718,000 on 4; compio (io_uring) 180,000 and 745,000–748,000: +2–4 %,
    p99 alike. Against it: io_uring on the internet-facing service (a large kernel attack surface
    with a history of privilege-escalation bugs, and the seccomp relaxation the Iggy container
    already needs), a second runtime per core (the Iggy SDK is Tokio-only), compio is pre-1.0 with
    breaking minor releases. Revisit with io_uring against epoll in M7.
- Consequences: throughput scales with cores (report P02-M3: 70,600 messages/s on 1 core to
  626,000 on 8 with the in-memory mailbox); the Iggy curve and its limits are in the report.

## D-0031 — Mailbox deletion on acknowledgement: puncturable keys, then physical deletion within minutes

- Date: 2026-11-12
- Status: accepted (owner delegated: "I am letting you handle this like a Proton co-founder")
- Decided by: agent, under the owner's standing delegation for security and privacy calls
- Context: with D-0029 an acknowledged envelope left the index at once but stayed on disk, in
  the clear form the client sent (MLS ciphertext, plus its recipient device and `seq`), until
  its Iggy segment was deleted: about 14 hours by default. The owner asked for deletion on
  acknowledgement and "deleted after delivery too somehow", and for Songbird to be behind
  nobody. Iggy, like any append-only log, cannot remove one message from a segment.
- Options considered:
  - **Keep plaintext records, delete segments sooner.** Shorter segments and expiry only move
    the bound; a quiet node keeps its active segment for hours; no deletion at ACK time.
  - **A store with in-place deletes** (FoundationDB, as Signal uses, or Valkey). A delete there
    is logical: the bytes stay in the storage engine until its compaction, and in the SSD until
    garbage collection. It would also give up Iggy's throughput (D-0029).
  - **One random key per message in a key table.** Deletion is exact, but the key table grows
    and is rewritten per message: the same in-place-delete problem moved to a smaller file.
  - **Puncturable encryption (chosen).** Every record sealed under keys derived from a
    Goldreich–Goldwasser–Micali tree per time window; an ACK punctures the leaf, which removes
    the ability to derive it from anything kept. The key state is small when ACKs arrive in
    order and proportional to the pending messages otherwise. The log keeps only ciphertext
    that nothing can open.
- Decision (`crates/mailbox/src/iggy/{keys,sealed}.rs`):
  - **Sealed records** (record format version 3). Each window (10 minutes) has a random metadata
    key and a GGM tree of depth 24 (16.7 M slots; PRG: AES-256 on four counter blocks, aws-lc-rs).
    Each entry (envelope, recipient device, `seq`) is sealed under its own leaf key (AES-256-GCM,
    used once, so a constant nonce); a shared group body has its own slot, punctured with the
    record's last entry; each remote part of a multi-partition send has its own slot, punctured
    once stored on its partition. Record metadata (idempotency tag and fingerprint, times, MLS
    group and epoch, ACK checkpoints, counter snapshots) is sealed with the window's metadata key.
    The associated data binds every slot to its partition, window and position.
  - **Deletion on acknowledgement.** The ACK is written to the log first (merged into the next
    store append, or alone on the ACK tick), then the leaves are punctured in memory, and the key
    store on disk follows within `key_interval` (1 s): from then on nothing on the node — log,
    key store, master key together — can decrypt the entry, its recipient or its `seq`
    (`acknowledged_messages_cannot_be_decrypted_even_with_every_key` opens every record of every
    partition with every key on disk and finds the acknowledged messages in none). The log's ACK
    records are the authority: after a crash, replay punctures again.
  - **Key store** (one directory per partition, under `key_dir`, sealed with the node's master
    key, a 256-bit secret given by SecretRef). Slots written since the last write leave the tree
    for individual keys (a "mark"), so out-of-order ACKs cannot blow the tree up; those keys are
    kept in pages of 1,024 slots. A write is copy-on-write: changed pages go to new files named by
    the writer's epoch and the write's generation, then the head (windows, trees, marks, the page
    list with each page's generation and key count) replaces the old one atomically (fsync,
    rename, fsync of the directory), then the replaced files are deleted. Load refuses the store
    if any listed page is missing, sealed for another place or generation, or holds another number
    of keys: a lost page would otherwise make pending messages look acknowledged
    (`start_up_refuses_a_lost_key_page`). A new window is in the store before any record sealed
    with it is appended (invariant 1).
  - **Fencing and key uniqueness.** A new writer opens a fresh window (random keys) and writes
    the key store with its producer epoch before it appends anything; an older writer's writes are
    then refused (`KeyError::Fenced`, the writer stops as when Iggy fences it). Replay also marks
    every replayed record's slots as used. Found by the end-to-end restart run (174 messages lost):
    the key store lags the log by up to `key_interval`, a restarted writer handed out slots already
    used after the last write, so a new message got the same leaf key (and constant nonce) as an
    old one and acknowledging the old one destroyed the new one
    (`a_restart_never_reuses_key_slots` fails without the fix). Writes are serialised, so an older
    change set never lands after a newer one (a race found while making the store incremental: the
    periodic write and a new window's write could land out of order and drop the window). Window
    ids should carry the writer's epoch before M4 runs two writers at a handover.
  - **Physical deletion within minutes.** Pending entries older than `relocate_after` (15 min)
    are re-sealed forward by compaction (every minute); the low-water offset follows; a window
    is destroyed (metadata key and tree) once all its records are below it; Iggy deletes sealed
    segments below it after `message_expiry` (10 min), with 16 MiB segments. Delivered ciphertext
    leaves the disk about 25 minutes after delivery plus the time to fill a segment (on a quiet
    node, hours: still unopenable from the first second).
  - **Idempotency in its own topic** (`{topic}-idem`): the keyed tag, the recipient-set
    fingerprint (HMAC, server secret), the time and the log offset; no content, no device. Kept
    7 hours (6-hour retry window plus margin) so the mailbox log is not held for the idempotency
    window. A retry after delivery is a replay that returns `seq` 0 for the entries already
    acknowledged (nothing left to route).
- Evidence:
  - Correctness: 15 Iggy live tests (3 consecutive clean runs), 16 end-to-end invariant tests
    on Iggy, key-store unit tests (punctures, out-of-order ACKs, page rewrites, crash between
    pages and head, lost and misplaced pages, destroyed windows, fencing), the `iggy_record` fuzz
    target. End to end through `delivery` (report P02-M2 addendum): steady 1,000–80,000/s
    offered, reconnect storm, Iggy frozen for 8 s, restart: the checker found no loss,
    duplicate or reordering.
  - Cost of sealing (bench `STORE=iggyprod`, random envelopes, same machine, base = main
    `819d07e`): on this disk both versions are fsync-bound and indistinguishable (1,200–17,000
    sends/s for both, run to run); on tmpfs (CPU-bound ceiling) 341,000 against 432,000 sends/s
    at 128 senders (0.79×) and 428,000 against 863,000 at 1,024 senders (0.50×). Writer time at
    1,024 senders: append 67 %, sealing 14 %, applying 10 %, planning 4 %. One GGM expansion
    costs 136 ns; sealing a 256-byte entry 181 ns.
  - Key-store cost (release build, this disk): with 1 M pending entries the store is 36 MB; a
    write after 100 scattered ACKs replaces 100 pages (3.6 MB), holds the state lock 1.8 ms and
    takes 80 ms. The first, whole-file design rewrote 36 MB and held the lock 25 ms on every
    write (every 50 ms): about 720 MB/s of writes, found by measurement before merging.
  - Measurement note: the comparison benchmarks of M2 sent constant-byte envelopes; this disk is
    btrfs with zstd, which compressed them almost away, so plaintext stores looked several
    times faster than with real (incompressible) ciphertext. `bench/stores` and the load
    generator now send random bytes; M2's disk numbers are optimistic (report addendum).
- Consequences:
  - Ahead of the field as far as public documentation goes: Signal deletes a delivered message
    from its database (FoundationDB), WhatsApp and Threema delete on delivery; in each the
    bytes remain in the storage engine and the SSD until compaction and garbage collection.
    Here they are unopenable within about a second and gone from the log within minutes.
  - **The key store is part of the mailbox.** Losing `key_dir` (or the master key) loses every
    pending message: the log alone cannot be read. It must be on the same persistent disk as
    the node (not tmpfs), never in backups (a backup would keep keys that ACKs destroyed), and
    the master key in the secrets store (D-0010). A restored Iggy backup without its matching
    key store does not start (runbook).
    **Owner decision (2026-11-12): accepted** ("i accept the backup decision"): key stores are
    never backed up; a node that loses its disk loses its undelivered messages until M4
    replicates partitions, as the master prompt's beta targets allow (undelivered envelopes may
    be lost on total server loss; clients resend on a new `store_generation`).
  - **Limits.** Overwritten and deleted files can survive in SSD flash until garbage collection,
    so an attacker with the raw flash and the master key might recover an older key page: the
    data volumes must use full-disk encryption with a key that never touches the disk
    (Prompt 13 deploy work, issue to file). Keys in memory are wiped on drop, best effort.
    Record sizes, times and partitions stay visible in Iggy's message headers until the segment
    is deleted.
  - Sealing halves the CPU ceiling of a partition writer (above); the disk, not the CPU, bounds
    a node today. M3 (thread-per-core) should pipeline the writer (seal the next batch while the
    current one appends).
  - Write amplification: pending entries are rewritten every 15 minutes (relocation), and the
    key store writes about 36 bytes per new pending entry plus the pages its ACKs touch.
  - MSRV raised to Rust 1.95 for `File::lock` (D-0030 note).

## D-0030 — The official Apache Iggy SDK, and what it brings into the tree

- Date: 2026-11-12
- Status: accepted (owner: "hope you use official iggy sdk")
- Decided by: owner
- Context: the Iggy mailbox (D-0029) needs a client. The agent proposed its own minimal TCP client
  (as for SurrealDB, D-0025) because the SDK has no optional features: every transport (TCP, QUIC,
  HTTP, WebSocket) is compiled in, with `quinn`, `reqwest`, `tungstenite`, `ring` and 236 crates in
  all (154 new to the workspace). The owner chose the official SDK.
- Decision: `iggy = "=0.12.0"` (pinned with the server image 0.10.0), used over TCP only.
  - **Crypto and TLS stay on aws-lc-rs.** The SDK's TCP/TLS path installs aws-lc-rs as rustls's
    provider itself (`links/dial.rs`); our connection code installs it as the process default
    before the SDK builds any TLS configuration. `ring` is compiled in for QUIC (never enabled)
    and for request identifiers in the protocol layer; it is not used for our TLS or crypto.
    Proposed constitution wording in Open owner decisions.
  - **Logs (invariant 6).** The SDK logs server addresses and names at several levels. The crates
    `iggy`, `iggy_common` and `iggy_binary_protocol` are forced off in every log filter
    (`songbird_telemetry::SILENCED_TARGETS`, applied after the configured filter); the mailbox
    logs every SDK failure itself as a numeric error code. `inv6_no_identifiers_in_logs_or_metrics`
    captures at TRACE through the production filter.
  - **Supply chain.** `cargo vet`: 153 crates newly exempted (`supply-chain/config.toml`), four
    existing exemptions raised from safe-to-run to safe-to-deploy (`borsh`, `borsh-derive`,
    `fastrand`, `proc-macro-crate`), imports now fully audit 156 crates. Audit first: `iggy`,
    `iggy_common`, `iggy_binary_protocol`, `ring`, `bytemuck`. `cargo deny`: per-crate licence
    exceptions for `foldhash` (Zlib) and `webpki-roots`/`webpki-root-certs` (CDLA-Permissive-2.0,
    the Mozilla root list), both permissive; no global allow changed.
- Consequences: a large dependency tree for one TCP protocol; the SDK tracks the server, so
  upgrades move together (the SDK README: mixed versions and rolling upgrades are unsupported).
  Our own client stays an option once Iggy's protocol is stable (1.0).
- Note (2026-11-12): `iggy =0.12.0` needs Rust 1.95; the workspace MSRV (`rust-version`) is
  raised from 1.88 to 1.95 (1.88 was declared but never checked), which D-0031's key store also
  needs (`File::lock`). The pinned toolchain (D-0001) is newer and unchanged.

## D-0029 — Mailbox on Apache Iggy

- Date: 2026-11-12
- Status: accepted (owner: "go with iggy, build it for production")
- Decided by: owner, after the comparison below; design by agent
- Context: SurrealDB capped a node at about 7,000 end-to-end messages/s (D-0027). The owner asked
  to compare Apache Iggy with Redis/Valkey and take "the best and fastest".
- Evidence: same `Mailbox` trait and closed-loop workload for all stores, every write fsynced
  before the store answers, ACK deletes included, client-style idempotent retries, 8 s per run,
  two interleaved rounds, a `seq` check after each run (96 runs, all gap-free and duplicate-free).
  SurrealDB 4.0.1 (SurrealKV `sync=every`), Valkey 9.1.3 (AOF `appendfsync always`; one instance,
  and four sharded by device), Iggy 0.10.0 (topic `durability = persisted`, 4 shards); servers on
  CPUs 4–7. On this machine's disk (btrfs, 0.5–3.6 ms per fsync) and on tmpfs (no fsync cost:
  each store's own ceiling). Sends/s, best of two rounds:

  | Workload | SurrealDB | Valkey ×1 | Valkey ×4 | Iggy | Iggy, tmpfs | Valkey ×4, tmpfs |
  | --- | --- | --- | --- | --- | --- | --- |
  | 1 sender | 273 | 1,403 | 1,346 | 368 | 8,868 | 26,313 |
  | 128 senders | 6,272 | 36,261 | 42,346 | 15,612 | 444,717 | 259,046 |
  | 1,024 senders | 7,088 | 42,378 | 123,670 | 153,128 | 1,410,584 | 337,594 |
  | 10-device sends, 128 senders (entries/s) | 7,271 | 93,156 | 81,500 | 163,044 | 1,909,177 | 348,316 |
  | 100-device groups, 64 senders (entries/s) | 3,458 | 160,031 | 242,598 | 647,467 | 4,766,768 | 548,448 |

  Iggy has the highest ceiling by far and wins whenever batches form; Valkey has the lowest
  latency for a lone send. On this disk both have poor tails from btrfs fsync stalls: Iggy's
  p99.9 sits at 0.4–0.7 s from 16 senders up, Valkey's varies from 0.02 to 0.9 s. The reference
  hardware (Prompt 13) decides the tail. Script and raw output:
  `bench/stores` (`stores-matrix` runs reproduce it).
- Decision (production design, `crates/mailbox/src/iggy`):
  - **Log and partitions.** One topic, `partitions` partitions (default 4); a device belongs to
    one partition by a hash of its ID. One writer task per partition owns that partition's state:
    each device's last and acknowledged `seq`, the index of pending entries (log offsets, not
    ciphertext), idempotency tags (8-byte prefix to offset, about 30 bytes per send, verified
    against the record on a hit) and MLS epochs. Requests queued during an append form the next
    batch (natural group commit). Our own record format inside Iggy messages (`record.rs`,
    bounded decoder, fuzz target `iggy_record`).
  - **Durability.** Topic `durability = persisted`: Iggy fsyncs before it confirms; a send is
    acknowledged only after that (invariant 1).
  - **One writer per partition (fencing).** Each writer is an idempotent Iggy producer with a fixed
    id and a new epoch at every start, recorded in the log. Iggy refuses older epochs
    (`ProducerEpochFenced`): a second process cannot corrupt `seq`s; the fenced node reports not
    ready. A batch whose outcome is unknown is resent with the same producer sequence and written
    once. One delivery node per topic until M4 adds partition ownership across nodes.
  - **Multi-partition sends.** A send's home partition is its MLS group's, else the partition
    holding most of its recipients (so a 1:1 send or a send to one account is usually one
    append; a first version keyed the home by the idempotency key, which made almost every send
    two appends and flooded the re-driver after a restart, found by the end-to-end run). The home
    record holds the home devices' entries and the other partitions' parts; the parts are then
    stored on their partitions under the same tag (a send without a key gets a random internal
    tag).
  - **Idempotency.** Each tag is stored with a fingerprint (SHA-256, 16 bytes) of the send's whole
    recipient set; a reused key is a retry only when both match, otherwise reason 7. Reuse is
    detected wherever the new send shares a partition with the earlier one; a reuse for devices
    on entirely different partitions is stored as a new send (it never suppresses a message). A re-driver completes parts that failed,
    in the process after 2 s and after a restart from recent home records. A send is refused
    with nothing stored, or recorded and eventually stored for every recipient, without relying
    on the client to retry.
  - **MLS ordering.** The epoch check runs on the group's home partition writer; one commit per
    epoch wins (`mls_commits_are_ordered_by_epoch`).
  - **Start-up.** Each partition is replayed from its low-water offset to rebuild the state. A
    record that does not decode stops the start (`IggyMailboxError::Unreadable`): skipping it
    would lose counters and reuse `seq`s. Found by the end-to-end run: a first build skipped
    unreadable records with a warning, the record format then changed without a version bump,
    684,229 records were skipped, counters restarted and the clients dropped every new message
    as already seen (the checker reported them lost). The record version is now 2 and bumped
    on every format change.
  - **Compaction (every minute), off the write path.** Entries older than 30 days are dropped
    from the index (invariant 3); a compactor task reads the pending entries older than
    `relocate_after` (1 h) on its own connection, many records per poll, and the writer copies
    them forward in batches of about 1 MiB (at most 100,000 entries per pass); a counter snapshot (every
    device's last and acknowledged `seq`, MLS epochs, the producer epoch) is written, so no `seq`
    is ever reused although old records go; the low-water offset (an Iggy consumer offset, which
    also stops Iggy deleting segments past it) moves up to the oldest record still needed (pending
    entries, the 6-hour idempotency window, incomplete parts). Iggy deletes sealed segments below
    it after the topic's `message_expiry` (8 h).
  - **Container.** Iggy 0.10 needs io_uring; `deploy/compose/iggy-seccomp.json` is Docker's
    default profile (moby/profiles `seccomp/v0.1.0`, Apache-2.0) plus `io_uring_setup`,
    `io_uring_enter`, `io_uring_register`, nothing else. HTTP, QUIC and WebSocket listeners off;
    TCP with TLS on the private address (server-authenticated TLS and the root password; Iggy
    has no client certificates).
  - SurrealDB keeps its mailbox backend for development and comparison; deployments require Iggy
    (configuration validation).
  - **Amendment (2026-11-13, owner): Podman for development.** Under Podman (the macOS development
    machine's engine) Iggy could not start: Fedora's SELinux policy denies io_uring to containers,
    and compose sends `iggy-seccomp.json`'s contents where Podman's compatibility API expects a
    path. `cargo xtask compose` detects Podman (`docker version` names the Podman Engine;
    `SONGBIRD_COMPOSE_PODMAN=0|1` forces it) and adds `deploy/compose/compose.podman.yaml`, which
    runs the Iggy container alone with `label=disable` and `seccomp=unconfined`, keeping
    `no-new-privileges` and the loopback-only port. Development only: servers run Iggy under
    Docker with the strict profile (Ansible, unchanged). Evidence: on Podman 6.1.4 (Fedora CoreOS
    VM, kernel 7.2.7) all six compose services healthy; the Iggy mailbox suite (16), the M5 gate
    on Iggy (2) and the invariant tests on Iggy (16) pass.
- Evidence (correctness): `crates/mailbox/tests/iggy_live.rs` (10 tests: seq rules, idempotency
  across partitions, 1,000-device shared body, MLS ordering, 32 concurrent senders gap-free,
  restart rebuilds state and fences the old writer, compaction keeps counters across a restart,
  expiry, bulk ACKs, the re-driver in process and after a restart); the 16 end-to-end invariant
  tests on Iggy; record and state unit tests; fuzzing.
- Consequences:
  - **Deletion is not immediate on disk.** An acknowledged envelope leaves the index at once and
    the disk when its segment is deleted: by default up to about 14 hours (6-hour idempotency
    window plus 8-hour segment expiry). SurrealDB deleted on ACK. Shorter windows trade against
    the retry horizon (D-0021); owner to confirm (Open owner decisions). **Superseded by D-0031:**
    records are sealed under puncturable keys, an acknowledged entry is unopenable within about
    a second and its segment deleted within minutes (relocation after 15 minutes, 10-minute
    segment expiry, idempotency in its own topic).
  - Iggy is pre-1.0 (an Apache top-level project since August 2026); its storage format changed
    between 0.9 and 0.10, so upgrades need a migration plan (runbook).
  - Every pending entry costs about 32 bytes of memory in its writer, every device about 80
    bytes, every idempotency tag about 30 bytes; record payloads are cached up to `cache_mib`.

## D-0028 — Milestone 3 runtime candidates: per-core Tokio and compio

- Date: 2026-11-11
- Status: decided: stay on per-core Tokio (owner, 2026-11-12: "let's stick with tokio"), after the
  M3 measurement (D-0032: compio +2–4 % on an echo benchmark). io_uring against epoll is measured
  again in M7 (master prompt), on the reference hardware
- Decided by: owner suggestion, recorded by agent
- Context: the owner pointed out that sockudo-ws supports compio for true thread-per-core. The
  constitution locks "per-core single-threaded Tokio runtimes"; a different runtime is a
  locked-stack change.
- Decision: M3 builds both behind a feature: per-core current-thread Tokio with `SO_REUSEPORT`
  (the locked stack) and compio (io_uring/completion based; sockudo-ws `compio-runtime` feature,
  its own split streams). Decided by p99 and throughput on the same hardware; the owner approves
  any switch. Things to check first: compio's maintenance and release cadence, how the SurrealDB
  RPC client and Valkey (`redis-rs`, Tokio-only) run beside it (likely a Tokio side runtime per
  core or per node), and that the M2 flamegraph's kernel share (syscalls, softirq) is what
  completion I/O removes.
- Consequences: no runtime change before the benchmark.

## D-0027 — SurrealDB mailbox: one writer, cached counters with compare-and-set, bulk statements

- Date: 2026-11-11
- Status: superseded for deployments by D-0029 (the SurrealDB backend remains for development)
- Decided by: agent
- Context: master prompt §4: per-device `seq` in the same transaction as the write, group commit,
  batched ACK deletes, "measure SurrealDB writes per second early: it sets the ceiling".
- Evidence (AMD Ryzen 9 7950X, Linux 7.3.1, SurrealDB 4.0.1 in podman pinned to 4 cores,
  SurrealKV with `sync=every` on NVMe/btrfs; `crates/mailbox/examples/commit_bench.rs`;
  report P02-M2):
  - SurrealQL costs 0.1–1.5 ms per statement; the in-memory engine is barely faster than the
    durable one, so execution, not fsync, dominates (a bare durable one-row create is 0.9 ms).
  - First design (one transaction with `FOR` loops and closures computing `seq`s server-side):
    7.7 ms per lone send, 3,900 sends/s peak.
  - Final design: 3.3 ms per lone send, 12,000–14,000 one-recipient sends/s peak (256 per
    transaction), about 20,000 mailbox entries/s for group fan-out; a 1,000-device shared send
    takes 51 ms (554 ms before).
  - End to end through `delivery` (store, deliver, ACK delete) the node saturates at about
    7,150 messages/s; the checker passes in steady, reconnect-storm and SurrealDB-pause runs.
- Decision:
  - **Durability**: SurrealKV, `surrealkv:///data/songbird.db?sync=every` (compose, Ansible):
    every commit is fsynced before the mailbox answers (invariant 1). v4.0.1 opens `rocksdb:`
    paths as SurrealKV anyway, logging the sync mode, which is checked at start in the logs.
  - **One writer task per process** owns every write: store batches and ACK deletes are serial,
    so they never conflict with each other. Batches form from whatever queued during the
    previous commit (`max_wait` 0 measured best at every load; up to 256 sends).
  - **Counters**: `seqctr:[device].last` is cached in the writer; the writer computes every
    `seq`, record ID and row; one bulk `INSERT … ON DUPLICATE KEY UPDATE last = IF last =
    $input.prev { $input.last } ELSE { THROW }` advances counters only where they still hold
    the cached value. Another writer (a second node) makes it throw; the writer reloads and
    retries, so a `seq` is never reused or skipped. Acknowledgements live in `ackctr`, written
    only by ACK flushes (an early design that kept both in one record aborted every store and ACK
    transaction against each other: 55 s for a 1-second test).
  - **Statements per batch**: idempotency `INSERT`, counter `INSERT`, optional bodies, mailbox
    rows; a duplicate idempotency key fails the batch with `AlreadyExists`, the writer answers
    the replays and retries the rest; an MLS commit runs alone with its epoch check.
  - **ACK deletes**: merged per device, flushed every 50 ms (or at 4,096 devices) in one
    transaction: explicit entry IDs since the previous ACK, shared-body reference counts of the
    entries that went (bodies deleted at zero), `ackctr`. Queued ACKs are taken on every writer
    wake-up (found under load: a send-biased loop starved and then dropped them).
  - **Capacity** (`max_per_device`) is a soft limit from cached counters; it guards storage, it
    is not an invariant.
- Consequences: one writer per node caps the store at one transaction in flight; M3 tries
  several writers sharded by device (a send to devices of several shards goes to one, the
  counter guard keeps the others correct). Group fan-out is bounded by SurrealDB's per-row cost
  (~40–50 µs). TiKV (the master prompt's later cluster) may change every number here.

## D-0026 — Send path: device lists, shared bodies, MLS commit ordering

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: master prompt §5 `send` (device lists, idempotency, MLS ordering) and Prompt 2's
  "shared bodies for group fan-out".
- Decision:
  - **Wire**: `Send` gains a flags byte after the idempotency key: `0x01` MLS commit (followed
    by `group [16]`, `epoch u64`), `0x02` shared body (`count × device_id`, then one
    `len u32 | body`). Unknown flags are refused. New rejection reason 8 `StaleEpoch`
    (not retryable as is). Protocol v1 changed before any client shipped.
  - **Device lists**: every addressed device must exist (unknown ones are `extra`) and a send
    that addresses an account must address all its devices except the sender's own
    (`missing`); the answer is `SendResult` status 2 `DeviceMismatch` and nothing is stored.
    Accounts come from the static directory (an `account` per device, default: its own) until
    the identity service (Prompt 3) owns devices and key versions (`stale` stays empty until then).
  - **Shared bodies**: one `body:[id]` record per send with a reference count, one compact
    `mailbox:[device, seq]` entry per device linking to it; the server routes the same `Bytes`
    to every connected recipient. Deleted when the last device acknowledges or after 30 days.
  - **MLS ordering**: `mls_group:[group].epoch` compared and advanced in the same transaction
    as the store; exactly one of several concurrent commits from one epoch wins, the others get
    `StaleEpoch` and nothing stored. Who may commit to a group is Prompt 5's (group membership).
- Evidence: `tests/e2e` `send_path_device_lists_shared_bodies_and_mls_ordering`;
  `crates/mailbox/tests/surreal_live.rs` (`shared_body_is_stored_once`: 1,000 devices, one body,
  1,000 entries without ciphertext, body deleted after the last ACK;
  `mls_commits_are_ordered_by_epoch`: 10 concurrent commits, one wins); proto vectors.
- Consequences: senders learn the device list of an account they write to (as with Signal and
  WhatsApp); the server learns group IDs and epochs (16 opaque bytes chosen by clients).

## D-0025 — SurrealDB client: our own minimal RPC client, not the official SDK

- Date: 2026-11-11
- Status: accepted (owner may overrule)
- Decided by: agent
- Context: the mailbox needs SurrealDB from Rust. The official SDK `surrealdb` 4.0.1 with only
  `protocol-ws` and `rustls` pulls 264 crates (geo, argon2, a second WebSocket stack,
  tokio-tungstenite, webpki roots…), more than the whole workspace's tree, each needing a vet
  audit or exemption.
- Options considered: the SDK; HTTP `/sql` with JSON (binary envelopes do not survive JSON); our
  own client over the WebSocket RPC with CBOR.
- Evidence: the protocol was specified from the 4.0.1 sources (`surrealdb-rpc-4.0.1`
  `request.rs`, `response.rs`, `method.rs`; `surrealdb-types-4.0.1/src/cbor/convert.rs` for the
  tags; the SDK's `engine/remote/ws` for cross-checks) before writing code;
  `crates/surreal/tests/live.rs` against the pinned server (bytes, record IDs, per-statement
  errors, 200 pipelined requests); CBOR property tests and RFC 8949 vectors; fuzz target
  `surreal_cbor` (30 s, 1.46 M runs, no crash; nightly 10 min).
- Decision: `crates/surreal` (no new dependency): a bounded CBOR subset codec (definite lengths,
  depth 64, lengths checked against remaining input before allocating, trailing bytes refused)
  and a pipelined RPC client over `sockudo-ws` (subprotocol `cbor`, `/rpc`, `signin` as root,
  `use`, `query`), TLS through rustls with the internal CA. Server error messages can quote
  record IDs: `Display`/`Debug` show only the kind. Root credentials for now; a least-privilege
  database user is Prompt 12's.
- Consequences: we track SurrealDB's RPC protocol ourselves (pinned server version; live tests in
  CI's compose job catch drift).

## D-0024 — Milestone 1 decisions compared with Signal and WhatsApp; changes adopted

- Date: 2026-11-11
- Status: accepted
- Decided by: agent (the owner delegated D-0018, D-0020 and D-0021: "see how others would do
  that, like WhatsApp and Signal; if you have better ideas, do them")
- Context: the M1 gate report asked the owner to rule on three proposals.
- Evidence (primary sources, read 2026-11-11):
  - Signal-Server `main`, `websocket/WebSocketConnection.java`: one reactive stream per device
    from the message store; "new message" events only wake the reader; at most 256 messages sent
    and not acknowledged (`MESSAGE_SENDER_MAX_CONCURRENCY`); a slow client is never actively
    disconnected; a device connecting elsewhere is closed with 4409. The previous design
    (until 2024–2025) used a "new message available" flag plus a re-read of the store, the same as
    our lag flag. No server-side send idempotency: clients drop duplicates (libsignal
    `DuplicateMessageException`; Signal-Desktop dedupes by sender and timestamp). Devices
    authenticate the WebSocket with HTTP Basic and a random per-device token stored as a salted
    HKDF hash (`auth/SaltedTokenHash.java`): a bearer secret, no channel binding.
  - WhatsApp Encryption Overview (2026-10-30), "Transport Security": Noise Pipes with Curve25519;
    the client authenticates with a per-device static key pair and the server stores only the
    public key; Noise binds that authentication to the session by construction. The document
    does not say how retries are deduplicated or how slow clients are handled.
  - RFC 9266 (`tls-exporter` channel binding) is used by XMPP SCRAM-PLUS (XEP-0440); no messenger
    found using it with TLS, because WhatsApp gets the property from Noise.
- Decisions:
  1. **D-0020 accepted, plus a bounded un-acked window.** Keep "store is the truth, notifications
     are hints, a full inbox means re-read the mailbox" (Signal's earlier design). Add Signal's
     in-flight bound: at most `max_unacked` (default 256) envelopes sent and not acknowledged per
     connection; delivery pauses at the limit and resumes with a drain on the next ACK. This
     bounds memory per session regardless of socket progress. The write-stall close (4100) stays
     as the last resort, with the timeout raised from 10 s to 30 s for mobile networks (a stalled
     reader holds nobody else up).
  2. **D-0021 accepted, hardened.** Signal keeps no server-side idempotency, so our record is
     extra; it now holds HMAC-SHA256(server secret, key) instead of the key, lives 6 h (the
     client's retry horizon) instead of 24 h, and is scoped to the recipient devices: the same key
     for other recipients is refused (`SendResult` reason 7, not retryable) and stores nothing.
     Apps still drop duplicates by the message ID inside the envelope (as Signal's clients do),
     which covers retries after 6 h.
  3. **D-0018 accepted, with TLS channel binding.** A separate per-device Ed25519 key matches
     WhatsApp's separate static key and is stronger than Signal's bearer token (the server holds
     only public keys). The signed transcript now ends with the 32-byte RFC 9266 TLS exporter
     (label `EXPORTER-Channel-Binding`, no context), which gives what Noise gives WhatsApp: a
     relay that terminates TLS (misissued certificate, interception proxy) cannot reuse the
     device's signature on its own session to the server. TLS must therefore end at `delivery`
     itself (no TLS-terminating load balancer in front; `SO_REUSEPORT` spreading is in-process).
- Evidence for the changes: `tests/e2e` `unacked_window_bounds_in_flight`,
  `tls_connection_works` (a valid signature without this session's binding is refused with
  4000); `crates/mailbox` `retried_send_is_stored_once` (keyed hash, recipient scope);
  `crates/proto/tests/vectors.rs` (new transcript vector).
- Consequences: protocol v1 changes before any client ships (transcript, reason 7). Revisit
  the Noise alternative only if TLS termination ever has to move off the delivery host.

## D-0023 — Autobahn conformance: two passes, image pinned by digest

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: the M1 gate requires the Autobahn suite to pass against `delivery`'s own WebSocket
  configuration. The Songbird endpoint is binary-only and authenticated, so the suite runs against
  `songbird-delivery-autobahn`, an echo server built from the same upgrade code and
  `ws::ws_config` (same limits, timeouts, buffers, no compression). Production limits refuse
  messages over 1 MiB (invariant 7); the suite sends up to 16 MiB.
- Options considered: run everything and accept failures; raise the production limit; two passes.
- Evidence: `cargo xtask autobahn` (wstest 0.8.2, `crossbario/autobahn-testsuite@sha256:519915fb…`,
  run with podman 5.8.9 on the development machine, report P02-M1): production pass 493 cases,
  274 OK, 3 INFORMATIONAL, 216 UNIMPLEMENTED (groups 12 and 13, permessage-deflate, deliberately not
  offered), 0 FAILED; the 24 cases with 4–16 MiB messages (9.1.4–6, 9.2.4–6, 9.3.\*, 9.4.\*) are
  excluded there and run in the second pass with the limit raised to 16 MiB: 54 of 54 group-9
  cases OK. Run without the exclusion, those 24 are FAILED: the connection is failed on the frame
  header, before buffering.
- Decision: `cargo xtask autobahn` runs both passes, fails on any other outcome, and CI runs it on
  every push (`autobahn` job, reports kept 30 days). Docker or podman, whichever is installed.
- Consequences: a regression in framing, UTF-8 validation, close handling or fragmentation fails
  CI. Revisit if compression is ever offered (it should not be: ciphertext does not compress).

## D-0022 — Load generator v0, delivery checker and the milestone 1 baselines

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: M1 gate: "checker passes; first baseline numbers versus Sockudo".
- Decision: `tools/loadgen` drives the real `songbird-client` (one client task per simulated
  device), sends 1:1 traffic at a fixed total rate to random peers, acknowledges every delivery,
  and runs the checker (`tools/loadgen/src/checker.rs`): nothing lost, nothing delivered twice,
  `seq` strictly increasing and gap-free, per-sender FIFO, nothing unexpected. Each run tags its
  payloads with a random run ID, so messages an earlier run left in a mailbox are reported
  (`stale_from_earlier_runs`) instead of miscounted. The `reconnect` scenario drops one random
  connection every 20 ms and every connection at once halfway through. `echo` measures raw
  WebSocket round trips against any echo server. `tools/loadgen/bench/m1.sh` reproduces every
  gate number with the server pinned to CPU 0 and the load generator to CPUs 8–15.
- Baseline choice: the sockudo-ws library's own `simple_echo` example (3.1.1, built from the
  registry source, `TOKIO_WORKER_THREADS=1`) against `songbird-delivery-autobahn` (our upgrade and
  configuration, current-thread runtime), same client, same machine. The Sockudo *server* speaks
  the Pusher protocol (channels, HTTP publish API); a like-for-like driver is milestone 3 work,
  when `delivery` runs on all cores. **Owner decision (2026-11-11): the library baseline is
  accepted for M1.** The Pusher-protocol driver (`songbird-loadgen pusher`) stays for a later
  comparison against the Sockudo server (5.2.0, `sha256:802c27d7…`) if wanted.
- Evidence: report P02-M1 (AMD Ryzen 9 7950X, Linux 7.3.1, Rust 1.99.1).
- Consequences: every later load, chaos and failover run uses the same checker.

## D-0021 — Milestone 1 stand-ins: in-memory mailbox and in-process router; idempotency keys

- Date: 2026-11-11
- Status: accepted (idempotency record hardened by D-0024)
- Decided by: agent
- Context: M1 builds the session algorithm "on an in-memory mailbox"; SurrealDB is M2 and Valkey
  routing M4.
- Decision: `songbird-mailbox` defines the `Mailbox` trait (`store_batch`, `fetch_after`,
  `ack_up_to`, `expire_older_than`, `store_generation`) and ships `MemoryMailbox`;
  `songbird-router` defines the `Router` trait (`attach`, `detach`, `route`, `route_ephemeral`) and
  ships `LocalRouter`. Both stand-ins use one `std::sync::Mutex` each: a lock between socket read
  and socket write, which the constitution forbids on the hot path. It is accepted only for these
  stand-ins and disappears with SurrealDB group commit (M2) and the per-core directory (M3).
  `store_batch` takes the send's idempotency key and stores it with the resulting `seq`s (24 h at
  first; D-0024: keyed hash, 6 h, scoped to the recipients);
  a retry returns the original result (`replayed`) and is routed again (duplicates are dropped by
  the recipient session). The key is 16 random bytes chosen by the client and is not stored with
  the sender's identity. M2 moves it into the same SurrealDB transaction as the envelopes. The
  server's listener runs `expire_older_than(30 days)` hourly until the worker takes it over (M5).
- Evidence: `crates/mailbox/src/memory.rs` tests (gap-free per-device `seq`, never reused, all or
  nothing, retry stored once, expiry, property test of fetch ordering);
  `tests/e2e` `inv8_store_unavailable_rejects_retryably_and_keeps_sockets`.
- Consequences: M1 numbers measure the WebSocket and session path, not storage; the SurrealDB
  write path (M2) sets the real ceiling.

## D-0020 — Session algorithm and backpressure

- Date: 2026-11-11
- Status: accepted (owner delegated; extended with an un-acked window by D-0024)
- Decided by: agent
- Context: master prompt §5 `delivery`: subscribe before drain, deduplicate by `seq`, gaps trigger
  a drain, safety sweep, "bounded per-connection queue; when full, disconnect with a 41xx code".
- Options considered: (a) disconnect when the inbox is full, as written; (b) drop the notification,
  mark the session lagged, drain from the mailbox, and disconnect only when the socket itself
  stops accepting writes.
- Evidence: `services/delivery/src/ordering.rs` (`Cursor`, property test
  `every_seq_once_in_order`: random stores, dropped, duplicated and late notifications, inbox
  overflow, any page size: the socket sees every `seq` once, in order); `tests/e2e`
  `inv2_per_device_order_without_gaps_or_duplicates` (inbox of 8, four concurrent senders, a dropped
  connection) and `inv5_slow_client_never_slows_anyone_else`.
- Decision: (b). The router never blocks: `try_send` into a bounded inbox (default 256); on full
  it sets a lock-free lag flag and wakes the session, which drains the mailbox after its cursor.
  Ephemeral events use a separate lane (default 32) and are simply dropped. A socket write or
  flush that makes no progress for `write_timeout_ms` (default 10 s) closes the connection with
  4100 and a retry hint. Under (a), a session that is merely busy draining after a reconnect storm
  would be disconnected and reconnect into the same storm; under (b) it catches up, and nothing
  waits on it either way. The session attaches before its first drain; live `seq = cursor + 1` is
  sent inline, `seq ≤ cursor` dropped, anything else drains. Writes are fed and flushed once per
  wake-up (up to 64 live envelopes per batch). The safety sweep runs every 120 s ± 20 % per
  session (1 s while the store is failing).
- Consequences: invariant 5 holds without disconnect storms. Timers are Tokio timers per session
  for now; M3 measures a per-core sweep against them.

## D-0019 — Wire protocol v1: hand-encoded hot frames, prost control frames, close codes

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: master prompt §5 (client wire protocol), `docs/PROTOCOL.md` §1–§3.
- Decision: one WebSocket binary message per frame; type byte then big-endian fixed-width fields;
  hot frames hand-encoded (`Send`, `Ack`, `Ephemeral`, `Deliver`, `SendResult`); control frames
  (`Hello`, `Auth`, `AuthOk`, `Error`, `DeviceMismatch`) are protobuf via `prost` 0.14.5 derive
  on hand-written structs (no `.proto` build step, no `protoc`). Decoders check every length
  against `Limits` before allocating, reject trailing bytes (one encoding per frame) and return
  payloads as zero-copy `Bytes` slices. `Debug` never prints device IDs, keys or payloads
  (`prost(skip_debug)` plus manual impls). Close codes: 4000–4099 never reconnect, 4100–4199
  back off, 4200–4299 reconnect now; retry hint `retry_ms=<n>` in the close reason. Defaults:
  envelope 64 KiB, 1,024 recipients, `Send` 1 MiB, ephemeral 4 KiB, control body 4 KiB.
- Evidence: `crates/proto` unit and property tests (round trips, truncation, trailing bytes, limits
  before allocation, redacted `Debug`, test vectors); fuzz targets `proto_client_frame` and
  `proto_server_frame` (30 s each locally: 3.7 M and 3.0 M runs, no crash; nightly 10 min).
- Consequences: a `Deliver` frame is built by copying the envelope behind a 9-byte header (one copy
  per delivery). M3 removes it (zero allocation per message) by writing the header as its own
  vectored segment.

## D-0018 — Device authentication: Ed25519 challenge signature with aws-lc-rs

- Date: 2026-11-11
- Status: accepted (owner delegated; TLS channel binding added by D-0024)
- Decided by: agent
- Context: master prompt §5 handshake: the device signs a domain-separated string containing the
  nonce and server name.
- Options considered: XEdDSA with the libsignal identity key; a separate Ed25519 device
  authentication key; bearer tokens (forbidden).
- Decision: a per-device Ed25519 authentication key, verified with aws-lc-rs 1.19.0 (already in
  the tree through rustls, so no new cryptography crate). Transcript
  `"songbird.v1/device-auth\0" || len || server_name || nonce[32] || device_id[16] ||
  protocol_version`, server name from the client's own configuration. 32-byte nonce from the OS per
  connection; `Auth` within 5 s (4102 otherwise). An unknown device and a bad signature get the
  same close code (4000) and both run one signature verification (against a dummy key for an
  unknown device), so authentication is not a device-existence oracle. `Auth.last_acked_seq` also
  acknowledges, so a lost `Ack` frame still deletes. Milestone 1 reads public keys from a static
  file (`[delivery.directory] file`), refused by validation in staging and production.
- Evidence: `services/delivery/src/directory.rs` tests; `crates/proto/src/auth.rs` test vector;
  `tests/e2e` `inv9_server_side_authorization` (unknown device, other server name, replayed
  signature, wrong key: all 4000; the client stops instead of reconnecting).
- Consequences: Prompt 3 decides whether the authentication key stays separate from the identity
  key and replaces the static directory with the identity service.

## D-0017 — Dependencies of Prompt 2, milestone 1

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: Constitution: no new dependency without a recorded reason; read the pinned version's
  source first.
- Evidence: APIs used were read in the registry sources of these exact versions before writing
  code: sockudo-ws `handshake::{parse_request, generate_accept_key, build_response}`,
  `WebSocketStream::from_raw_with_leftover`, its `Stream`/`Sink` implementations and `close`,
  `client::WebSocketClient::connect_raw`, `Config`/`ConfigBuilder`, `init_clock`, error variants;
  prost 0.14.5 `Message` and the derive's `skip_debug`; aws-lc-rs 1.19.0
  `signature::{Ed25519KeyPair, UnparsedPublicKey, ED25519}`. `cargo deny` clean (advisories,
  licences, bans, sources); `cargo vet --locked`: 120 fully audited, 3 partially, 76 exempted.
- Decision (version, reason):
  - `sockudo-ws` 3.1.1, `default-features = false`, feature `tokio-runtime` only (locked stack).
    Not enabled: `permessage-deflate` (ciphertext does not compress), `http2`/`http3`/`io-uring`
    (later experiments), `mimalloc` (we install it ourselves). Its `simd` feature is a no-op in
    3.1.1 (SIMD paths are always compiled in). We use our own HTTP upgrade on top of its
    `handshake` module so a wrong path or subprotocol gets an HTTP error, and drive one unified
    stream per connection from a single task: no split, so no lock between read and write.
  - `prost` 0.14.5 (control frames; locked stack).
  - `futures-core`, `futures-sink` 0.3.34: the `Stream` and `Sink` traits only, to poll sockudo-ws
    without `futures-util`.
  - `aws-lc-rs` 1.19.0 as a direct dependency (device authentication; already in the tree).
  - Transitive, new to the lock file: `quanta` 0.13.0 and `raw-cpuid` 11.6.0 (sockudo-ws clock),
    `sha1` 0.11 with `digest`, `block-buffer`, `crypto-common`, `hybrid-array`, `typenum`,
    `const-oid` (the handshake's `Sec-WebSocket-Accept`), `tokio-util` 0.7.20, `prost-derive`
    0.14.5 with `itertools` 0.15 and `either`, and wasm-only crates pulled by `quanta`'s
    `web-sys` dependency (`wasm-bindgen*`, `js-sys`, `web-sys`, `futures-util`, `futures-task`,
    `slab`, never built for our targets).
  - cargo-vet: `either` (cuviper), `rustversion` (dtolnay), `slab` and `tokio-util` (Darksonn)
    are covered by trust entries under D-0016's rule (publishers already trusted); 22 crates are
    new exemptions in `supply-chain/config.toml`, 6 of them wasm-only. Highest-value audits to
    do: `sockudo-ws` (parses every client byte; contains `unsafe` SIMD masking), `prost`,
    `quanta`.
- Consequences: Songbird crates stay `#![forbid(unsafe_code)]`; the `unsafe` in sockudo-ws and
  quanta is a supply-chain risk tracked with the exemptions.

## D-0016 — cargo-vet: trust publishers that two or more imported organizations trust

- Date: 2026-11-11
- Status: accepted
- Decided by: agent (with owner approval; the owner accepted the 57 remaining exemptions as
  a recorded risk for the P01 gate)
- Context: P01 gate item "every pinned dependency verified": `cargo vet` had 38 crates fully
  audited and 129 exempted. The six imported audit sets were current; `cargo vet prune` removed
  nothing.
- Options considered: import the three remaining registry sets (actix, ariel-os, fermyon); trust
  publishers; audit crates ourselves; leave the exemptions.
- Evidence: the three extra sets vetted no additional crate, so they are not imported. `cargo vet
  suggest` showed 70 exempted crates published by people that two or more of Mozilla, ISRG,
  Bytecode Alliance, Google and Zcash trust outright.
- Decision: `supply-chain/audits.toml` trusts, for `safe-to-deploy` until 2027-11-11, the crates
  in our graph published by dtolnay, Kenny Kerr (kennykerr), epage, cuviper, seanmonstar, Amanieu,
  BurntSushi, rust-lang-owner, sunfishcode, alexcrichton, Darksonn, Thomasdezeeuw and the
  `taiki-e/pin-project-lite` GitHub team. A trust entry covers only versions that publisher
  released. Publishers trusted by one organization only (ctz, djc, Manishearth) are not trusted.
  No audits were certified: an audit entry claims a person read the code.
  Result: 110 crates vetted, 57 exempted.
- Consequences: trust entries expire on 2027-11-11 and must be renewed (`cargo vet trust`) or
  replaced. A new version from a different publisher fails `cargo vet` until audited or exempted.
  The 57 remaining exemptions include the TLS stack (rustls, rustls-webpki, ring, aws-lc-rs,
  aws-lc-sys, tokio-rustls, rcgen), the x509/asn1 parsers used by `tools/admin`, tracing, time,
  libc, getrandom, mimalloc and prometheus-client. Burning them down needs human audits or new
  imported audit sets; start with the TLS and allocation paths.

## D-0015 — CodeQL gates on its own SARIF; no code scanning upload

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: both CodeQL jobs failed on PR 17. The job token lacked `actions: read`, which
  codeql-action needs on a private repository, and code scanning is not enabled on this private
  repository (the code scanning API answers 403), so an upload could not succeed either.
- Options considered: enable GitHub Code Security (paid; owner action); drop CodeQL; run CodeQL
  with `upload: never` and gate on the SARIF.
- Decision: `codeql.yml` grants `actions: read` and `contents: read` only, runs analysis with
  `upload: never`, fails the job when the SARIF holds any result (each shown as an annotation), and
  keeps the SARIF as a 30-day artifact.
- Amendment (2026-11-11, owner-approved during P02 M1): `rust/hard-coded-cryptographic-value`
  flags the fixed nonce of the published device-auth test vector (`crates/proto/tests/vectors.rs`,
  `PROTOCOL.md` §13). The gate skips that one rule for files under a `tests/` directory only;
  checked against the run's SARIF (the same rule in `src/` still fails the job). Waived results
  are printed as `::notice` annotations so they stay visible. Unit tests that do not need a fixed
  value use random nonces instead.
- Consequences: no Security tab alerts or dismissal workflow; a false positive is fixed in code or
  excluded in the workflow. If code scanning is enabled later, switch back to uploading and grant
  `security-events: write`. Done in D-0045: the repository is public, so code scanning is free;
  CodeQL still gates on its SARIF and, outside pull requests, also uploads it.

## D-0014 — Hosts: containers on host networking, nftables authoritative, bastion-only SSH

- Date: 2026-11-11
- Status: proposed
- Decided by: agent (awaiting owner approval)
- Context: Prompt 1 security: private network for every internal service, firewall allowing only
  client ports. Docker's default bridge publishes ports through its own NAT rules, which bypass a
  host INPUT policy.
- Options considered: Docker bridge plus `DOCKER-USER` rules; host networking with Docker's iptables
  management off; podman with quadlets; bare systemd units.
- Evidence: `deploy/ansible/roles/firewall/templates/nftables.conf.j2` (default drop, per-role
  public ports, internal ports from the private range only), validated by `nft -c` at deploy time;
  `deploy/tofu/modules/topology/tests/topology.tftest.hcl` proves edge exposes only 443, data
  hosts expose nothing, SSH only from admin ranges, never 0.0.0.0/0.
- Decision: every container uses host networking and binds to the private address (internal
  services) or the public one (client ports); Docker runs with `bridge: none`, `iptables: false`.
  Two firewall layers: the provider firewall (OpenTofu) and nftables (Ansible). SSH: only the
  bastion, only from `admin_cidrs` (held in SOPS); other hosts accept SSH only from the bastion's
  private address. Ansible connects as root with the deploy key (cloud-init disables passwords).
- Consequences: no container-to-container isolation by network namespace; isolation comes from
  users, read-only root filesystems, dropped capabilities and mTLS. Revisit root SSH (owner
  decision above).

## D-0013 — Release pipeline: reproducible, signed, attested; staged rollout with rollback

- Date: 2026-11-11
- Status: accepted (pending first tagged release in CI)
- Decided by: agent
- Context: Prompt 1: reproducible builds, signed artifacts, SBOM per release, staged rollout with
  automatic rollback on failed health checks.
- Options considered: Nix for hermetic builds; a pinned builder container plus path remapping and
  `--locked`; signing with a long-lived key versus Sigstore keyless.
- Evidence: `cargo xtask release --verify` builds every release binary twice in different target
  directories and fails unless all are bit-identical; local run on the development machine is in
  `docs/reports/P01-foundations.md`. `.github/workflows/release.yml` runs the same command inside
  `rust:1.99.1-trixie@sha256:8a4e90c7…` with `cargo-cyclonedx 0.5.10`.
- Decision: release binaries are built in the pinned builder image (`SOURCE_DATE_EPOCH` from the
  commit, `--remap-path-prefix` for the workspace and Cargo home, `-C strip=symbols`, `--locked`,
  toolchain from `rust-toolchain.toml`), verified twice, summed in `SHA256SUMS`, signed with cosign
  keyless (GitHub OIDC), and attested (SLSA provenance; CycloneDX SBOM per binary). Images are
  distroless (`gcr.io/distroless/cc-debian13:nonroot@sha256:cc66d222…`), pushed to GHCR, signed and
  attested. `deploy.yml` verifies signatures (`deploy/bin/verify-release`) before a staged
  rollout: one canary host, then 25 %, then the rest; any failed `/readyz` or a non-zero
  `songbird_telemetry_refused_callsites_total` rolls that host back and stops the rollout
  (`deploy/ansible/rollout.yml`).
- Consequences: reproducibility is proven per build, not assumed. Independent rebuilds by a third
  party are possible with the same image digest and commit. Nix remains an option if the
  container approach ever drifts.

## D-0012 — Internal mutual TLS certificate authority

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: Prompt 1: mutual TLS CA for service-to-service traffic, rotation scripted from day one.
- Options considered: step-ca (online CA), OpenBao PKI, an offline CA in our own admin tool.
- Evidence: `tools/admin/tests/mtls.rs` completes a real TLS 1.3 mutual handshake with rustls
  (aws-lc-rs) using issued certificates and refuses a certificate from another CA;
  `tools/admin/src/ca.rs` tests check key modes 0600, chain layout, and that public names or
  addresses are refused.
- Decision: `songbird-admin ca` (rcgen 0.14.10 with aws-lc-rs). ECDSA P-256. Root CA 10 years,
  key only in `deploy/secrets/<env>.root-ca.sops.yaml` (owner recipient only). Intermediate 1 year,
  path length 0, name-constrained to `songbird.internal` and private/loopback ranges, in the deploy
  secrets. Leaves 30 days (at most 90), one per component per host, `serverAuth`+`clientAuth`.
  `deploy/ansible/rotate-certs.yml` renews leaves inside a 10-day window (or all with
  `force_rotate=true`) and restarts the affected components; site.yml runs it on every build.
  Valkey uses native TLS with required client certificates (TLS 1.3 only); Garage (single-node
  layout applied idempotently by the data role) has its S3 and admin
  APIs sit behind ghostunnel 1.11.5 mTLS tunnels (Garage replaced by RustFS behind one tunnel,
  D-0035); SurrealDB gets server TLS with password auth (it
  has no client-certificate authentication); Songbird services load their certificates when their
  internal listeners arrive (Prompt 2+).
- Consequences: no online CA to attack or keep available. Revisit if certificate volume grows
  (step-ca or OpenBao). Intermediate renewal is an owner task (runbook `mtls-rotation.md`).

## D-0011 — Censorship-resistant proxy

- Date: 2026-11-11
- Status: accepted (link domain open)
- Decided by: agent
- Context: Prompt 1 WhatsApp-parity item: a separately deployable TLS proxy anyone can run,
  forwarding encrypted bytes only, logging nothing, configured by a proxy link.
- Options considered: nginx `stream` with `ssl_preread` (Signal's design); HAProxy (WhatsApp's);
  a small Rust relay.
- Evidence: `services/proxy/tests/relay.rs` (client → outer TLS to the proxy → inner TLS to an
  upstream echo server, 200 KB round trip; unknown SNI refused; silent clients closed at the
  handshake timeout; plain HTTP probes get no HTTP or upstream data); `services/proxy/src/sni.rs`
  property tests and the `client_hello` fuzz target; `services/proxy/src/link.rs` tests and the
  `proxy_link` fuzz target.
- Decision: `songbird-proxy`, Rust with rustls (aws-lc-rs) and Tokio. Outer TLS with the
  operator's certificate (ALPN `h2`, `http/1.1` so it resembles a web server), then route by the
  inner `ClientHello` SNI to an allowlist of Songbird upstreams, then copy bytes both ways with an
  idle timeout. No logging code at all; Docker logging disabled in the operator compose file. The
  SNI scanner is bounded and allocation-free (scratch sized by `max_client_hello_bytes`). Multi-
  thread Tokio runtime (not on the Songbird hot path; operators run it on small VPSes). Links:
  `songbird://proxy#host[:port]` and `https://<web-domain>/proxy#host[:port]`, host in the
  fragment so the web server never learns it.
- Consequences: a single binary and image, no nginx config for operators to get wrong. A client
  using Encrypted Client Hello would expose only the public name to the proxy; the allowlist must
  then list that name. Proxy operators see client IPs (stated in the operator README and threat
  model).

## D-0010 — Secrets: SOPS with age

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: Prompt 1: secrets never in plain files, documented rotation.
- Options considered: SOPS + age; OpenBao.
- Evidence: `deploy/bin/secrets-init` run end to end against a scratch key (report P01): 25 values
  encrypted, CA material decrypts and validates. `crates/types/src/secret.rs` tests: literal
  secrets in config files are rejected at parse time; world-readable secret files are refused;
  oversized ones are rejected before reading.
- Decision: SOPS 3.13.3 with age 1.3.2. Per environment, `deploy/secrets/<env>.sops.yaml`
  (recipients: owner + that environment's CI key) and `<env>.root-ca.sops.yaml` (owner only).
  `.sops.yaml` is generated from `deploy/secrets/recipients/*.txt`. Services never read secrets
  from config files: `SecretRef` is `{ file = … }` or `{ env = … }` only. Local development uses a
  generated, git-ignored `.env` with throwaway values (`cargo xtask dev-env`).
- Consequences: no secret server to run. OpenBao stays an option if dynamic secrets are needed.
  Rotation: `docs/RUNBOOKS/secrets-rotation.md`.

## D-0009 — Infrastructure as code: OpenTofu + Ansible; Hetzner and Scaleway

- Date: 2026-11-11
- Status: proposed (second provider needs owner confirmation)
- Decided by: agent
- Context: Prompt 1: a complete environment at Hetzner and, unchanged apart from provider
  variables, at a second EU provider.
- Options considered: NixOS (needs custom images or nixos-infect on both providers) versus Ansible
  on stock Debian 13; second provider Scaleway, OVHcloud, UpCloud.
- Evidence: `deploy/bin/validate-iac`: `tofu test` plans the beta (one host) and staging (six
  hosts) topologies at both providers against mocked APIs; topology rules tested; `ansible-lint`
  (production profile) and syntax checks pass.
- Decision: OpenTofu 1.13.9 (hcloud provider 1.70.2, scaleway 2.91.0, lock files with hashes for
  linux/darwin amd64/arm64). One provider-independent `topology` module (hosts, roles, private
  addresses from a fixed per-host `index` so adding a host never re-addresses or replaces another,
  public firewall rules) feeds two thin provider modules; environment files
  (`environments/*.tfvars`) are shared and provider files (`providers/*.tfvars`) hold only region
  and machine types. State: S3 backend at the *other* provider, with OpenTofu client-side state
  and plan encryption (passphrase in SOPS). Ansible core 2.21.6 (newest on the CI runner's Python
  3.12), ansible-lint 26.11.0, all Python packages hash-pinned; collections community.docker 5.4.0,
  ansible.posix 2.2.2, community.sops 2.5.1. Scaleway (France) as second provider: mature OpenTofu
  provider with VPC, IPAM and security groups, EU regions only.
  Tool downloads in CI are checksum-verified (`deploy/bin/install-tools`).
- Consequences: one command per provider (`deploy/bin/env-up <env> <cloud>`). The timed rebuild
  at the second provider is the gate drill; it needs the owner's provider accounts.

## D-0008 — Continuous integration: fast and deep tiers, everything pinned

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: Prompt 1: CI running the constitution's quality bar, split into fast and deep checks.
- Decision: CI calls `cargo xtask` so local and CI runs are identical.
  Fast (`ci.yml`, every push and PR): rustfmt; clippy pedantic plus selected restriction lints with
  `-D warnings`; nextest; doctests; rustdoc `-D warnings`; cargo-deny; cargo-audit; cargo-vet;
  service smoke test (every service starts, `/readyz`, `/metrics` audited for sensitive names and
  refused callsites, clean exit on SIGTERM); docker compose environment health; distroless image
  build per service; Semgrep (`p/rust`, `p/secrets`, `.semgrep/songbird.yml`); IaC validation.
  CodeQL (`codeql.yml`, Rust and Actions, security-extended).
  Deep (`nightly.yml`): each fuzz target 10 minutes; ASan and TSan (nightly, `-Zbuild-std`); Miri
  on `songbird-types`; criterion benchmarks; the fast tier on a cold cache.
  Pins: actions by commit SHA (checkout v7.0.1 `3d3c42e5…`, upload-artifact v7.0.2 `cf430e03…`,
  download-artifact v8.0.2 `9000827c…`, rust-cache v2.9.3 `d1527c8f…`, install-action v2.90.3
  `cfdb7ed4…`, codeql-action v4.39.3 `33d7b85d…`, setup-buildx v4.4.1 `f87e5991…`, build-push
  v7.4.0 `c3c9e263…`, login v4.8.0 `8aa5831f…`, cosign-installer v4.1.3 `756fe2d2…`,
  attest-build-provenance v4.2.2 `4d101475…`, attest-sbom v4.1.0 `c6043329…`, setup-opentofu
  v2.0.2 `a1320f89…`). Tools: cargo-nextest 0.9.151, cargo-deny 0.20.2, cargo-audit 0.22.2,
  cargo-vet 0.10.2, cargo-fuzz 0.13.2, cargo-cyclonedx 0.5.10, Semgrep 1.185.0 by digest.
- Consequences: shared runners make nightly benchmark numbers trend-only; decisions use the
  reference hardware (Prompt 13). Amended by D-0045: container images in their own workflow
  (image changes, nightly, on demand) instead of on every push; `cargo xtask workflows` enforces
  the pins, hosted runners only, and where secrets may appear.

## D-0007 — Local and CI environment: docker compose, images pinned by digest

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: Prompt 1: compose for SurrealDB, `valkey-hot`, `valkey-jobs`, Garage, LiveKit, with
  health checks.
- Evidence: `cargo xtask compose up` on the development machine: all five containers healthy;
  SurrealDB refuses unauthenticated queries (403) and accepts the generated root credentials;
  Valkey refuses commands without the password; `valkey-jobs` reports `appendfsync everysec`;
  Garage reports "fully operational" after the single-node layout step; anonymous S3 is refused
  (403); LiveKit answers `OK`.
- Decision: `deploy/compose/compose.yaml`, every image by digest:
  SurrealDB v4.0.1 `sha256:408eb8e5c9436670525e2bc2634b1e50ba74ce1b447eaf0094c35f73b67f9cb6`;
  Apache Iggy 0.10.0 `sha256:176125a0580f526b0fee8b98ccf4ed5f77e243066183243d87282abce4851bf3`
  (added 2026-11-12, D-0029);
  Valkey 9.1.3-alpine3.24 `sha256:bf09d58e96d6b4524a15396b65b242bbe18beebbcc15a04d8ee75ddac62fae0b`;
  Garage v2.5.0 `sha256:677e5b758e4425a325b811c425c026bf35a4bf42f7ddbaa773de3f4fd779a5a5`
  (replaced by RustFS 1.0.5 on 2026-11-12, D-0035);
  LiveKit v1.13.11 `sha256:25cd902de7cacbd5f1f779f6a6ac1eae20ba8f56b0935934ce6b0ab80d1e3d20`;
  ghostunnel v1.11.5 `sha256:65c4e34ec0ee0f30ea1f72b7411ec70a91c32fc50f067a8466ad2adb1e7420f2`
  (deployments). Both Valkeys use `noeviction`: silently evicting registry or rate-limit keys
  would break invariants 4 and 5, so a full Valkey must fail loudly instead. `valkey-hot` has no
  persistence; `valkey-jobs` AOF `everysec`. Host ports 47xxx on 127.0.0.1 (overridable with
  `SB_PORT_*`) to avoid clashing with other local stacks. Credentials are random per machine.
- Consequences: Prompt 2's integration tests start from `cargo xtask compose up`.

## D-0006 — Configuration: TOML plus environment overrides, strict and validated

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: Prompt 1: configuration loading with validation; invariant 7 (bounded input).
- Options considered: `figment`, `config`, a small loader on `toml` + `serde`.
- Evidence: `crates/types/src/config.rs` tests: file then environment precedence; unknown fields
  and unknown override keys are errors; all violations reported at once with dotted paths;
  bounds on file size (checked before reading), override count, value length, key depth;
  multi-line override values rejected; error messages do not echo values. `config_toml` fuzz
  target.
- Decision: own loader (one dependency, `toml` 1.2.1, instead of a framework). Overrides are
  `SONGBIRD__SECTION__KEY=value`; values parse as TOML, else string. Staging and production refuse
  a non-private admin address and non-JSON logs.
- Consequences: no hot reload yet; `arc-swap` reload arrives with the first hot setting.

## D-0005 — Telemetry: non-blocking tracing, callsite redaction, prometheus-client

- Date: 2026-11-11
- Status: accepted for Prompt 1; per-core hot-path counters to be benchmarked in Prompt 2
- Decided by: agent
- Context: Prompt 1: tracing with a non-blocking writer, Prometheus metrics, a redaction layer that
  refuses known sensitive field names (invariant 6).
- Options considered: metrics facade + `metrics-exporter-prometheus` versus `prometheus-client`;
  redaction by rewriting values versus refusing callsites.
- Evidence: `crates/telemetry/tests/redaction.rs` (sensitive events and spans never reach the
  writer; panic mode fails loudly); `metrics.rs` tests (sensitive metric names rejected; exposition
  audit); `admin.rs` test (endpoints, 405, 404, oversized headers rejected). The smoke test checks
  every running service exposes `songbird_telemetry_refused_callsites_total 0`.
- Decision: `tracing-appender` non-blocking writer in **lossy** mode (a full buffer drops lines and
  counts them; logging never blocks a task). The redaction layer decides once per callsite from
  static field names (no per-event cost) and disables the callsite globally. `prometheus-client`
  0.26 (official OpenMetrics client, no exporter framework); admin endpoint on hyper 1.12 HTTP/1
  with header timeout, 8 KiB buffer, 32 headers, 64 connections. Semgrep rules
  (`.semgrep/songbird.yml`) catch sensitive log fields and unredacted IDs at review time.
- Consequences: field names matter; values formatted into the message text are not inspected,
  which is why IDs and secrets redact themselves in `Debug`.

## D-0004 — Dependencies of Prompt 1

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: Constitution: no new dependency without a recorded reason; read the exact pinned
  version's source before use.
- Evidence: APIs used were read in the registry sources of these exact versions before writing
  code (getrandom `fill`, tracing-appender `NonBlockingBuilder`/`ErrorCounter`, prometheus-client
  `Collector`/`DescriptorEncoder`, toml `Table::try_into`, hyper `http1::Builder`, rustls
  `ServerConfig::builder_with_provider`, pki-types `PemObject`, rcgen `Issuer`/`CertifiedIssuer`,
  x509-parser `parse_x509_pem`). `cargo deny` (advisories, licences, bans incl. no OpenSSL,
  sources) and `cargo audit` clean; `cargo vet`: 38 crates fully audited through imported audit
  sets (Mozilla, Google, Bytecode Alliance, ISRG, Zcash, Embark), 129 exempted and listed in
  `supply-chain/config.toml` (burn-down is an open risk, report P01). Reduced to 57 by D-0016.
- Decision (version, reason): tokio 1.54.1 (runtime, locked stack); rustls 0.23.46 + aws-lc-rs
  1.19.0 / aws-lc-sys 0.47.0, tokio-rustls 0.26.6, rustls-pki-types 1.16.0 (TLS, locked stack);
  mimalloc 0.1.52 (allocator, locked stack); tracing 0.1.44, tracing-subscriber 0.3.23,
  tracing-appender 0.2.5 (logging); prometheus-client 0.26.0 (metrics); hyper 1.12.0, hyper-util
  0.1.21, http-body-util 0.1.5, bytes 1.12.1 (admin endpoint); serde 1.0.229, toml 1.2.1
  (configuration); thiserror 2.0.21 (error types); getrandom 0.4.3 (ID randomness straight from
  the OS); hex 0.4.3 (ID text form off the hot path); zeroize 1.9.1 (secrets wiped on drop);
  rcgen 0.14.10, x509-parser 0.18.1, time 0.3.55 (CA tool); clap 4.6.7, anyhow 1.0.104 (CLI only);
  proptest 1.12.0, tempfile 3.27.0 (tests); libfuzzer-sys 0.4 (fuzz crate, outside the workspace).
- Consequences: the server stack takes no OpenSSL (enforced by `deny.toml`).

## D-0003 — Identifiers: 16 random bytes, typed, redacted

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: Prompt 1: IDs as 16-byte random values; invariant 6.
- Options considered: UUIDv4/v7 crates; ULIDs; plain random bytes.
- Evidence: `crates/types/src/id.rs` tests (round trips, wrong lengths rejected without
  allocation, `Debug` prints only the kind); `id_parse` fuzz target.
- Decision: `Id<K>` over `[u8; 16]` from `getrandom::fill`; kinds via `define_id!`; no timestamp
  or host data inside (v7-style IDs would leak creation time); `Debug` redacted, no `Display`;
  hex only through `to_hex_unredacted()`, which Semgrep forbids in log and format macros.
- Consequences: IDs are not time-sortable; ordering uses `seq`, never IDs.

## D-0002 — `crates/service`: one bootstrap for every service

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Context: five services need identical startup, health, metrics and shutdown; Prompt 1 lists
  only `types` and `telemetry`.
- Decision: `crates/service` provides `Service` and `main::<S>()`: CLI (`--config`,
  `--check-config`, `--version`), config load and validation, tracing, metrics, admin endpoint,
  SIGTERM/SIGINT drain with a configured grace period. It runs one current-thread Tokio runtime
  until `crates/runtime` (thread-per-core, Prompt 2) replaces it for the hot path.
- Consequences: Prompt 2 swaps the runtime under `run_service` without touching services.

## D-0001 — Workspace, toolchain and lints

- Date: 2026-11-11
- Status: accepted
- Decided by: agent
- Decision: Cargo workspace (resolver 3, edition 2024, `rust-version = 1.88`); toolchain pinned
  to 1.99.1 in `rust-toolchain.toml` (reproducible builds; bump deliberately and record);
  layout `crates/`, `services/` (delivery, api, push, worker, calls, proxy), `tools/` (loadgen,
  admin), `xtask/`, `deploy/`, `docs/`, `tests/`, `fuzz/` (outside the workspace, nightly).
  Workspace lints: `unsafe_code = "forbid"`; clippy pedantic plus restriction lints
  (`unwrap_used`, `expect_used`, `indexing_slicing`, `panic`, `as_conversions`, `print_*`,
  `dbg_macro`, `todo`, `unimplemented`, `allow_attributes_without_reason`, …); CI denies warnings.
  Release profile: fat LTO, one codegen unit, `panic = "abort"`, stripped; `profiling` profile for
  flamegraphs. The proxy lives in `services/proxy` because it is a deployable binary, though third
  parties run it.
- Consequences: tests may unwrap and index (`clippy.toml`); production code may not.

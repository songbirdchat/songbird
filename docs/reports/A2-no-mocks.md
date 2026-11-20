# Nothing mocked: the fix list (owner's rule, 2026-11-19)

The owner's rule: everything must be really implemented, with no mocks, placeholders or settings that
do nothing. This list comes from an audit of every control in `apps/ui` (2026-11-19). Each item was
traced from the control to whatever reads its field.

**What each item needs:**
- **a**: app behaviour only.
- **b**: app-core.
- **c**: a server endpoint that already exists.
- **d**: a server endpoint that does not exist yet (a backend gap).
- **e**: a native platform module.

NSH 0.6 already provides the share sheet, image and file pickers, the save dialog, sounds and launch at
login. `DESKTOP_GAPS.md` predates that.

Status: ☐ open · ☑ done (with the commit).

## Done

- ☑ A real account starts empty on the real backend; demo data only for fixtures and tests (60c3a86)
- ☑ Onboarding: real registration and username claim, with no fake delay (60c3a86)
- ☑ Invite links are the real PROTOCOL §14.10 card everywhere; the system share sheet (76d9a1f)
- ☑ Profile photo: system picker, encrypted avatar object, in the encrypted profile; contacts' real names and photos from their profiles; illustrated stand-ins only in fixtures (D-0140)
- ☑ Username change for real; the username link encrypted, reset and opened (D-0140)
- ☑ Account report saved; account deletion erases this device; linked devices real (D-0140)
- ☑ Wallpaper, status expiry, recovery sheet, keep-archived, chat export, Songbird's own tones
- ☑ Attachments in app-core: encrypted upload, verified download, end to end (D-0141)
- ☑ Photos and documents in real chats: system pickers, sent through app-core, shown and saved; received files kept encrypted in the store; auto-download for photos honoured; HD uploads keep full size (D-0141)
- ☑ Communities and private channels; the public directory only in fixtures (D-0149)
- ☑ Groups: remove, leave, add later, photos and files (D-0147); invite links, join requests (D-0148)
- ☑ Groups: an OpenMLS provider on aws-lc-rs (D-0145); creating a group of real contacts, joining by Welcome, group messages both ways (D-0146)
- ☑ Block (server, local drop, profile-key rotation), report with franking proofs, typing indicators (D-0144)
- ☑ Reactions, edits and delete for everyone in real chats, end to end; reactions kept in the store (schema 3); the read-receipts setting honoured (D-0143)
- ☑ Location: self-hosted OpenStreetMap tiles, the map picker, real places sent and shown (D-0142)
- ☑ 1:1 text chats through app-core: send, receive, ticks, read receipts, new chat by username (675bfff)

- ☑ Disappearing messages, polls, pins, safety numbers from real keys (and accepting a changed key), mute durations, GIFs, mark all read; group reactions, edits and deletes (D-0154)

- ☑ Retries: the outbox flushed every 15 s, offline sends kept, group operations retried after 409, 429, 503 (D-0157)

- ☑ Sticker packs from links, installed packs in the picker, stickers in real chats (D-0159)

- ☑ Broadcast lists send; presence and nudges to real contacts (D-0160)

- ☑ Locked chats open with the app passcode; real storage and network usage (D-0162)

- ☑ Chat-list state follows the account across devices (D-0163)

- ☑ HTTPS to Songbird's hosts goes through the proxy too, fail closed (D-0164)

- ☑ Backups hold app-core's chats and files; frequency, media, a second copy and saved settings (D-0165)

## Open, by what they need

**a (app only, or an existing endpoint):**
- Google Drive, OneDrive and Dropbox stores (the crate has them; OAuth needs the owner's apps)
- parental rules enforced on the child's app

**b (app-core):**
- security notifications (KT)

**Groups, the rest (b, c):** new devices joining by external commit. (Roles on received commits: D-0155; the change log: D-0156.)

**M6 attachments, the rest (a, b, c):** the user's own GPS position (e; picking on the map is done). (View-once and video playback: D-0158; voice notes: D-0161.)

**Calls (b, c, e):** done for 1:1 and group calls (D-0151, D-0152): LiveKit media end-to-end
encrypted, devices, screen share, verification symbols from the call's key, signed hellos, raised
hands. Call links are real too (D-0153): create, join, lobby, admit, revoke, per-device keys. Left: the
phone speaker route and the iOS and Android builds of
call media (libwebrtc's mobile archives are pinned, the native hosts are not wired).

**e (native modules):**
- Face ID and fingerprint
- block screenshots and hide content in the app switcher
- alternate app icons
- notification actions
- passkeys
- StoreKit and Play billing
- push hosts (deferred by the owner)
- photo-library save
- background backup scheduling

**d (backend gaps):**
- resume a cancelled renewal (billing)
- a public channel directory (public channels await the owner's legal review, D-0065 §4)
- crash reports (opt-in, scrubbed)
- the WhatsApp bridge (Prompt 14's legal gate)

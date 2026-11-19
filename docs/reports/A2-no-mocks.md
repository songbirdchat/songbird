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
- ☑ Location: self-hosted OpenStreetMap tiles, the map picker, real places sent and shown (D-0142)
- ☑ 1:1 text chats through app-core: send, receive, ticks, read receipts, new chat by username (675bfff)

## Open, by what they need

**a (app only, or an existing endpoint):**
- GIF picker (`/v1/gifs/*`)
- proxy for HTTP requests too
- installed sticker packs in the picker
- mute durations
- backup frequency, include media and second destination
- Google Drive, OneDrive and Dropbox stores (the crate has them; OAuth needs the owner's apps)
- parental rules enforced on the child's app
- lock chat unlocked with the app passcode

**b (app-core):**
- read receipts and typing indicators honour their settings
- reactions, edits, deletes for everyone, pins and polls in real chats
- disappearing timers (`TimerChange`, `EXPIRES`)
- block (`/v1/blocks`) and report (`/v1/reports`)
- mark all read
- broadcast lists
- presence and nudges to contacts
- safety numbers from real keys; security notifications (KT)
- sticker packs from links (`/v1/packs`)
- chat list state synced (records)
- network usage counted
- storage per chat

**M5 groups and communities (b, c):** create, members, roles, invite links, join requests, change log,
permissions enforced.

**M6 attachments (a, b, c):** photos, documents, voice notes (microphone, e), the user's own GPS position (e; picking on the map is done), view-once,
download, save, HD, auto-download.

**Calls (b, c, e):** LiveKit client media, devices, screen share, call links, verification symbols.

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

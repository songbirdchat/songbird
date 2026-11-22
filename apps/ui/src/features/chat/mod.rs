//! A5: the conversation — bubbles of every type, ticks, replies,
//! reactions, composer, voice notes, message info, chat info.
//!
//! - `header`: the floating capsules: back (phones), the chat's avatar,
//!   name and subtitle, calls, the More menu; or the search capsule.
//! - `list`: the virtualized messages (time labels, sender runs, the
//!   status under the user's latest message, swipe to reply, typing
//!   dots, jump to latest).
//! - `theme`: per-chat themes (canvas and bubble colours) and their
//!   picker.
//! - `bubbles`: one look per `Body` variant.
//! - `voice`: voice-note playback and the recorder's clock.
//! - `composer`: the input, attachments, recording and the bars that
//!   replace it (requests, blocked, admins only, selection).
//! - `actions`: the reaction bar and message menu.
//! - `sheets`: emoji, attachments, poll and event creators, contacts,
//!   stickers, forward, delete, disappearing messages.
//! - `info` and `message_info`: the pushed pages.

mod actions;
mod bubbles;
mod composer;
mod header;
mod info;
mod list;
pub(crate) mod logic;
mod message_info;
mod scene;
mod sheets;
pub(crate) mod theme;
mod voice;

use std::time::Duration;

use nsh_core::{keymap::When, prelude::*};

use crate::{
    nav,
    store::{Body, ChatId, ChatKind, MsgId, Store, store},
    ui::*,
};

pub use info::chat_info;
pub use message_info::message_info;
pub use theme::{theme_row, theme_sheet};

/// Opens a chat: the detail pane on desktop, a pushed page on phones.
pub fn open(chat: ChatId) {
    store().mark_read(chat);
    if wide() {
        nav::pop_to_root();
        nav::open_chat().set(nav::OpenChat(Some(chat)));
    } else {
        nav::push(format!("chat:{chat}"), move || {
            nsh_base::nav_page()
                .key(format!("chat:{chat}"))
                .header_visible(false)
                .child(conversation(chat))
        });
    }
}

/// Something asked of a conversation from outside it (the media viewer's
/// Reply, contact info's Search): the conversation takes it when it is
/// (or becomes) the one showing `chat`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ask {
    Reply(MsgId),
    Search,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Pending(Option<(ChatId, Ask)>);

/// Opens `chat` with the reply bar set to `msg` and the input focused
/// (the media viewer's Reply). Phones: back to the chat's page when it is
/// in the stack (pages above it close), else it is pushed. Desktops: the
/// chat in the detail pane, as [`open`].
pub fn reply_to(chat: ChatId, msg: MsgId) {
    ask(chat, Ask::Reply(msg));
}

/// Opens `chat` with search in chat started (contact info's Search).
pub fn search_in(chat: ChatId) {
    ask(chat, Ask::Search);
}

fn ask(chat: ChatId, what: Ask) {
    global::<Pending>().set(Pending(Some((chat, what))));
    if !wide() {
        let key = format!("chat:{chat}");
        let at = nav::routes().with_untracked(|r| r.0.iter().position(|x| x.key == key.as_str()));
        if let Some(i) = at {
            store().mark_read(chat);
            nav::truncate(i + 1);
            return;
        }
    }
    open(chat);
}

/// Takes what was asked of this conversation (see [`Pending`]), once the
/// conversation is on screen.
fn take_pending(cv: Conv) {
    let pending = global::<Pending>();
    effect(move || {
        let Some((chat, what)) = pending.get().0 else {
            return;
        };
        if chat != cv.chat {
            return;
        }
        // Next turn: the input exists and can take focus; the signal is
        // not written while this effect is reading it.
        set_timeout(Duration::from_millis(0), move || {
            if pending.get_untracked().0 != Some((chat, what)) {
                return;
            }
            pending.set(Pending(None));
            match what {
                Ask::Reply(id) => cv.start_reply(id),
                Ask::Search => batch(|| {
                    cv.query.set(String::new());
                    cv.hit.set(0);
                    cv.searching.set(true);
                }),
            }
        });
    });
}

/// The message a menu was asked for (a right click, a held finger, the
/// actions button): its bubble's frame in window points, measured as the
/// menu opens, and where it sits in its run (the lifted copy draws the
/// same corners).
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct MenuAt {
    pub id: MsgId,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub place: logic::Place,
}

/// Keyframe timing from the motion tokens.
pub(crate) fn timing(ms: f32, easing: Easing) -> KeyframeTiming {
    let mut t = KeyframeTiming::default();
    t.duration_ms = ms;
    t.easing = easing;
    t
}

/// A looping timing (typing dots, the recording dot), `delay` before.
pub(crate) fn looping(ms: f32, delay: f32) -> KeyframeTiming {
    let mut t = timing(ms, Easing::EASE);
    t.iterations = None;
    t.delay_ms = delay;
    t
}

/// A screen-reader label that follows the state.
pub(crate) fn say<S: Into<SharedString>>(
    f: impl Fn() -> S + 'static,
) -> impl Fn() -> Option<SharedString> + 'static {
    move || Some(f().into())
}

/// A voice note being recorded: held (released sends) or locked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rec {
    Held,
    Locked,
}

/// The conversation's state: what the user is doing in it. `Copy`, so
/// rows and listeners capture it; per-message state lives here keyed by
/// id, never in a recycled cell.
#[derive(Clone, Copy)]
pub(crate) struct Conv {
    pub chat: ChatId,
    pub s: Store,
    /// The desktop layout (decided when the conversation is made).
    pub wide: bool,
    pub draft: Signal<String>,
    pub reply: Signal<Option<MsgId>>,
    pub editing: Signal<Option<MsgId>>,
    pub menu: Signal<Option<MenuAt>>,
    pub selecting: Signal<bool>,
    pub selected: Signal<Vec<MsgId>>,
    /// Messages a forward or delete sheet acts on.
    pub acting_on: Signal<Vec<MsgId>>,
    pub forward_open: Signal<bool>,
    pub delete_open: Signal<bool>,
    pub attach_open: Signal<bool>,
    /// Desktops: the paperclip's frame (x, y, w, h) when the attach
    /// popover opened, which it is anchored to.
    pub attach_at: Signal<Option<(f32, f32, f32, f32)>>,
    pub emoji_open: Signal<bool>,
    pub reactions_open: Signal<bool>,
    pub poll_open: Signal<bool>,
    pub event_open: Signal<bool>,
    pub contact_open: Signal<bool>,
    pub sticker_open: Signal<bool>,
    pub location_open: Signal<bool>,
    /// The camera sheet (a photo to send).
    pub camera_open: Signal<bool>,
    pub timer_open: Signal<bool>,
    pub searching: Signal<bool>,
    pub query: Signal<String>,
    /// Which match (from the newest) search shows.
    pub hit: Signal<usize>,
    /// A message flashed after a jump (reply quote, search).
    pub highlight: Signal<Option<MsgId>>,
    /// The message just sent: it rises in.
    pub fresh: Signal<Option<MsgId>>,
    /// The message whose reaction just changed: its chip pops.
    pub popped: Signal<Option<MsgId>>,
    /// The list rests at its end.
    pub following: Signal<bool>,
    /// Messages that arrived while scrolled up.
    pub unseen: Signal<u32>,
    pub list: ElementRef,
    pub input: ElementRef,
    pub rec: Signal<Option<Rec>>,
    pub rec_secs: Signal<u32>,
    /// The mic's press ended a hold (its press must not start again).
    pub held_press: Signal<bool>,
    pub voice: voice::Player,
    pub shake: Motion,
    /// The chat's theme under the user's appearance choice.
    pub look: Memo<theme::Look>,
    /// The user's latest message that went out: the only one with a
    /// status line under it.
    pub latest_own: Memo<Option<MsgId>>,
    /// How far the floating header and composer reach into the canvas
    /// (their measured frames): the list keeps its content clear of them.
    pub top_inset: Signal<f32>,
    pub bottom_inset: Signal<f32>,
}

impl Conv {
    fn new(chat: ChatId, s: Store) -> Self {
        let draft = s.chat(chat).map(|c| c.draft).unwrap_or_default();
        Conv {
            chat,
            s,
            wide: wide(),
            draft: signal(draft),
            reply: signal(None),
            editing: signal(None),
            menu: signal(None),
            selecting: signal(false),
            selected: signal(Vec::new()),
            acting_on: signal(Vec::new()),
            forward_open: signal(false),
            delete_open: signal(false),
            attach_open: signal(false),
            attach_at: signal(None),
            emoji_open: signal(false),
            reactions_open: signal(false),
            poll_open: signal(false),
            event_open: signal(false),
            contact_open: signal(false),
            sticker_open: signal(false),
            location_open: signal(false),
            camera_open: signal(false),
            timer_open: signal(false),
            searching: signal(false),
            query: signal(String::new()),
            hit: signal(0),
            highlight: signal(None),
            fresh: signal(None),
            popped: signal(None),
            following: signal(true),
            unseen: signal(0),
            list: element_ref(),
            input: element_ref(),
            rec: signal(None),
            rec_secs: signal(0),
            held_press: signal(false),
            voice: voice::Player::new(),
            shake: motion(),
            look: theme::Look::of(chat),
            latest_own: computed(move || s.latest_own(chat)),
            top_inset: signal(0.0),
            bottom_inset: signal(0.0),
        }
    }

    /// The conversation's canvas (its theme's background).
    pub fn canvas(self) -> AdaptiveColor {
        self.look.get().canvas()
    }

    /// A group, community or channel (sender names, receipts per member).
    pub fn is_group(self) -> bool {
        self.s.chat(self.chat).is_some_and(|c| {
            matches!(
                c.kind,
                ChatKind::Group | ChatKind::Community | ChatKind::Channel { .. }
            )
        })
    }

    /// Scrolls to message `id` and flashes it.
    pub fn jump_to(self, id: MsgId) {
        let Some(index) = self.s.index_of(self.chat, id) else {
            toast("That message is no longer in this chat");
            return;
        };
        self.list
            .scroll_to_index(index as u32, percentage(0.5), px(0.0), true);
        self.highlight.set(Some(id));
        let highlight = self.highlight;
        set_timeout(Duration::from_millis(1400), move || {
            if highlight.get_untracked() == Some(id) {
                highlight.set(None);
            }
        });
    }

    /// Back to the newest message.
    pub fn to_latest(self) {
        self.list.scroll_to_end(true);
        self.unseen.set(0);
        self.following.set(true);
    }

    /// The user's reaction: added, changed or taken back; its chip pops.
    pub fn react(self, id: MsgId, emoji: &str) {
        self.menu.set(None);
        self.s.react(self.chat, id, emoji);
        self.popped.set(Some(id));
        let popped = self.popped;
        set_timeout(Duration::from_millis(700), move || {
            if popped.get_untracked() == Some(id) {
                popped.set(None);
            }
        });
    }

    pub fn start_reply(self, id: MsgId) {
        batch(|| {
            self.menu.set(None);
            self.editing.set(None);
            self.reply.set(Some(id));
        });
        self.input.focus();
    }

    pub fn start_edit(self, id: MsgId) {
        let Some(m) = self.s.message(self.chat, id).filter(logic::editable) else {
            return;
        };
        batch(|| {
            self.menu.set(None);
            self.reply.set(None);
            self.editing.set(Some(id));
            self.draft.set(logic::copy_text(&m));
        });
        self.input.focus();
    }

    pub fn cancel_edit(self) {
        batch(|| {
            self.editing.set(None);
            self.draft
                .set(self.s.chat(self.chat).map(|c| c.draft).unwrap_or_default());
        });
    }

    /// Sends the draft (or saves the edit).
    pub fn send_draft(self) {
        let text = self.draft.get_untracked().trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(id) = self.editing.get_untracked() {
            self.s.edit_message_text(self.chat, id, text);
            batch(|| {
                self.editing.set(None);
                self.draft
                    .set(self.s.chat(self.chat).map(|c| c.draft).unwrap_or_default());
            });
            return;
        }
        let reply = self.reply.get_untracked();
        let id = self.s.send_text(self.chat, text, reply);
        self.sent(id);
    }

    /// After anything the user sends: it rises in, the reply bar goes,
    /// the list follows.
    pub fn sent(self, id: MsgId) {
        batch(|| {
            self.fresh.set(Some(id));
            self.reply.set(None);
            self.draft.set(String::new());
        });
        self.to_latest();
    }

    /// Sends a body from the attachment sheet or a picker.
    pub fn send_body(self, body: Body) {
        let reply = self.reply.get_untracked();
        let draft = self.draft.get_untracked();
        let id = self.s.send(self.chat, body, reply);
        self.sent(id);
        // An attachment leaves the typed text where it was.
        self.draft.set(draft.clone());
        self.s.set_draft(self.chat, draft);
    }

    pub fn toggle_selected(self, id: MsgId) {
        self.selected.update(|v| {
            if let Some(i) = v.iter().position(|x| *x == id) {
                v.remove(i);
            } else {
                v.push(id);
            }
        });
    }

    pub fn start_select(self, id: MsgId) {
        batch(|| {
            self.menu.set(None);
            self.selected.set(vec![id]);
            self.selecting.set(true);
        });
    }

    pub fn end_select(self) {
        batch(|| {
            self.selecting.set(false);
            self.selected.set(Vec::new());
        });
    }

    pub fn ask_forward(self, ids: Vec<MsgId>) {
        batch(|| {
            self.menu.set(None);
            self.acting_on.set(ids);
            self.forward_open.set(true);
        });
    }

    pub fn ask_delete(self, ids: Vec<MsgId>) {
        batch(|| {
            self.menu.set(None);
            self.acting_on.set(ids);
            self.delete_open.set(true);
        });
    }

    /// Copies the messages' text, oldest first.
    pub fn copy(self, ids: &[MsgId]) {
        let chat = self.chat;
        let text = self.s.messages.with_untracked(|m| {
            m.get(&chat)
                .map(|v| {
                    v.iter()
                        .filter(|x| ids.contains(&x.id))
                        .map(logic::copy_text)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default()
        });
        set_clipboard(text);
        self.menu.set(None);
        toast(if ids.len() > 1 {
            "Messages copied"
        } else {
            "Message copied"
        });
    }

    /// Closes the topmost thing open (Escape).
    fn dismiss(self) {
        if self.menu.with_untracked(Option::is_some) {
            self.menu.set(None);
        } else if self.wide && self.attach_open.get_untracked() {
            // The desktop attach popover (sheets close themselves).
            self.attach_open.set(false);
        } else if self.selecting.get_untracked() {
            self.end_select();
        } else if self.searching.get_untracked() {
            self.searching.set(false);
            self.query.set(String::new());
        } else if self.editing.get_untracked().is_some() {
            self.cancel_edit();
        } else if self.reply.get_untracked().is_some() {
            self.reply.set(None);
        } else if self.rec.get_untracked().is_some() {
            self.rec.set(None);
        }
    }
}

/// Writes a measured inset when it moved by a whole point or more (frames
/// settle in fractions; the list's padding shouldn't churn on them).
fn set_if_changed(at: Signal<f32>, to: f32) {
    let to = to.ceil();
    if (at.get_untracked() - to).abs() >= 1.0 {
        at.set(to);
    }
}

/// Watches the chat while it is open: counts what arrives while the
/// reader is scrolled up, marks the chat read, shakes on a nudge.
fn watch(cv: Conv) {
    let s = cv.s;
    let chat = cv.chat;
    let seen = stored((s.message_count(chat), s.last_message(chat).map(|m| m.id)));
    effect(move || {
        let count = s.message_count(chat);
        let last = s.last_message(chat);
        let (was_count, was_last) = seen.with(|x| *x);
        let last_id = last.as_ref().map(|m| m.id);
        if last_id == was_last && count == was_count {
            return;
        }
        seen.replace((count, last_id));
        let Some(last) = last else { return };
        if last_id != was_last && matches!(last.body, Body::Nudge) {
            // Off globally, or for this person: no shake (PROTOCOL §10.5).
            let nudges = s.settings.with_untracked(|x| x.nudges)
                && last
                    .from
                    .and_then(|from| s.contact(from))
                    .is_none_or(|c| !c.nudges_off);
            if nudges || last.mine() {
                shake(cv);
            }
        }
        if count > was_count && !last.mine() {
            if cv.following.get_untracked() {
                s.mark_read(chat);
            } else {
                cv.unseen.update(|n| *n += (count - was_count) as u32);
            }
        }
    });
}

/// Sends a nudge. The first one this device ever sends is confirmed first,
/// so a nudge never happens by surprise (D-0101 question 9); the answer is
/// kept on this device.
pub(crate) fn send_nudge(s: Store, chat: ChatId) {
    if crate::backend::nudge::confirmed() {
        s.nudge_with_echo(chat);
        return;
    }
    let name = s
        .peer(chat)
        .map(|p| {
            p.name
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .unwrap_or_else(|| "Their".into());
    ask_first(
        "Send a nudge?",
        format!(
            "{name}'s chat window shakes, or their phone vibrates, to get their attention. They can turn nudges off for you. Songbird only asks this once."
        ),
        "Send nudge",
        move || {
            crate::backend::nudge::set_confirmed();
            s.nudge_with_echo(chat);
        },
    );
}

/// The nudge: the conversation shakes once (no motion when reduced).
fn shake(cv: Conv) {
    let spec = MotionSpec::new(timing(420.0, Easing::EaseOut)).values(
        MotionChannel::TranslateX,
        [0.0, -9.0, 9.0, -7.0, 7.0, -4.0, 4.0, 0.0],
    );
    let _ = cv.shake.play(spec);
}

/// Desktop keys: Escape closes the topmost layer, Up edits the last
/// message when the draft is empty.
fn keys(cv: Conv) {
    if !is_desktop() {
        return;
    }
    on_key("escape", move |_| cv.dismiss());
    let options = KeyOptions {
        when: When::Always,
        pass_through: true,
        ..KeyOptions::default()
    };
    on_key_with("up", options, move |_| {
        if cv.draft.with_untracked(String::is_empty)
            && cv.editing.get_untracked().is_none()
            && let Some((id, _)) = cv.s.last_own_text(cv.chat)
        {
            cv.start_edit(id);
        }
    });
}

/// How far a scroll-edge fade reaches past the capsule it sits behind.
const SCRIM_TAIL: f32 = 24.0;

/// A scroll-edge fade at the top (under the header) or the bottom (under
/// the composer), as tall as that layer's measured frame plus a tail. The
/// canvas colour is solid behind the capsule and clears over the tail, so
/// the first and last lines of a message fade rather than slice.
fn edge_scrim(cv: Conv, top: bool) -> View {
    let inset = if top { cv.top_inset } else { cv.bottom_inset };
    // The solid part ends where the capsule's own band ends.
    let solid = move || {
        let h = inset.get();
        if h <= 0.0 {
            0.0
        } else {
            (h / (h + SCRIM_TAIL)).clamp(0.0, 1.0)
        }
    };
    let v = view()
        .absolute()
        .left_0()
        .right_0()
        .h(move || px(inset.get() + SCRIM_TAIL))
        .z_index(1)
        .pointer_events(PointerEvents::None)
        .accessibility_elements_hidden(true);
    let v = if top {
        v.top_0().bg_linear_to_b()
    } else {
        v.bottom_0().bg_linear_to_t()
    };
    v.gradient_from(move || cv.canvas())
        .gradient_via(move || cv.canvas().opacity(0.92))
        .gradient_via_position(move || Percentage(solid()))
        .gradient_to(move || cv.canvas().opacity(0.0))
}

/// The conversation with its own header, composer and overlays.
pub fn conversation(chat: ChatId) -> AnyElement {
    let s = store();
    if s.chat(chat).is_none() {
        return view()
            .flex_1()
            .bg(move || c::chat_bg())
            .child(empty_state(
                Icon::Chats,
                "Chat not found",
                "This chat was deleted.",
                None,
            ))
            .into_element();
    }
    let cv = Conv::new(chat, s);
    watch(cv);
    take_pending(cv);
    keys(cv);
    voice::install(cv);
    composer::install_recorder(cv);
    let title = s.chat(chat).map(|c| c.title).unwrap_or_default();
    // The canvas fills the pane it is given. The messages run under the
    // floating header and composer (REDESIGN.md "Content floats"), which
    // are measured so the list keeps its first and last messages clear of
    // them. A theme change cross-fades the canvas (a colour fade, kept
    // with reduced motion).
    let root = view()
        .flex_1()
        .w_full()
        .bg(move || cv.canvas())
        .transition_colors()
        .duration(motion::SLOW)
        .pb_keyboard()
        .child(
            // The stage holds no padding, so its floating layers sit on
            // the keyboard's edge, not under it.
            view()
                .flex_1()
                .overflow_hidden()
                .bind_motion(&cv.shake)
                // The wallpaper chosen in Settings → Chats, over chats that
                // keep the default theme (a chat's own theme takes over).
                .child(view().absolute().inset_0().bg(move || {
                    let own_theme = s
                        .chat(chat)
                        .is_some_and(|c| c.theme != crate::store::ChatTheme::default());
                    if own_theme {
                        AdaptiveColor::TRANSPARENT
                    } else {
                        crate::features::settings::chats::wallpaper_tint(
                            s.settings.with(|x| x.chat_wallpaper),
                        )
                    }
                }))
                // The chat's scene, behind the messages (Prompt 9).
                .child(scene::scene_layer(chat))
                .child(list::message_list(cv))
                // Scroll-edge fades: messages dissolve under the floating
                // header and composer instead of being cut at the pane's
                // edge. Solid in the canvas colour behind each capsule,
                // clear a little past it; they never take a press.
                .child(edge_scrim(cv, true))
                .child(edge_scrim(cv, false))
                .child(list::jump_button(cv))
                .child(
                    view()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        // Above the scroll-edge fades (z 1).
                        .z_index(2)
                        .on_layout(move |b: &Bounds<Points>| {
                            set_if_changed(cv.top_inset, b.size.height.0)
                        })
                        .child(header::header(cv)),
                )
                .child(
                    view()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .z_index(2)
                        .on_layout(move |b: &Bounds<Points>| {
                            set_if_changed(cv.bottom_inset, b.size.height.0)
                        })
                        .child(composer::bottom(cv)),
                ),
        )
        .child(sheets::all(cv))
        .child(actions::overlay_menu(cv));
    // Desktops: files dropped on the conversation are sent to it (A13).
    // Wrapped last, so the drop overlay sits above everything.
    let root = if is_desktop() {
        crate::features::desktop::on_file_drop(
            root,
            format!("Drop to send to {title}"),
            move |files| {
                // A real chat: each file is read here, then encrypted and uploaded by app-core
                // like any attachment (D-0184).
                if crate::backend::chat::owns(chat) {
                    let max = songbird_app_core::attachments::MAX_FILE as u64;
                    let paths: Vec<std::path::PathBuf> = files
                        .iter()
                        .filter_map(|f| crate::features::desktop::file_path(&f.uri))
                        .filter(|p| {
                            let ok = std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() <= max);
                            if !ok {
                                toast("That file can't be sent: files are up to 256 MB.");
                            }
                            ok
                        })
                        .collect();
                    let picked = nsh_core::picker::read_files(
                        &paths,
                        &nsh_core::picker::PickRequest::new(
                            true,
                            u32::try_from(paths.len()).unwrap_or(u32::MAX),
                        ),
                    );
                    let photos = !picked.is_empty() && picked.iter().all(|f| f.mime.starts_with("image/"));
                    crate::backend::chat::send_picked(s, chat, picked, photos);
                    return;
                }
                for f in files {
                    s.send(
                        chat,
                        crate::store::Body::File {
                            name: f.name.clone(),
                            size: "—".into(),
                            kind: f.mime.clone(),
                        },
                        None,
                    );
                }
            },
        )
    } else {
        root
    };
    root.into_element()
}

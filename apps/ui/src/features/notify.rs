//! Notifications (Prompt 8, D-0098): what the device shows, decided on the
//! device. The server never learns which chats are muted or what the user
//! turned off; it only wakes the device (mobile) or the app is already
//! connected (desktop, which keeps running in the tray).
//!
//! [`decide`] is the one rule set, pure and tested: the desktop notifier
//! below uses it now, and the mobile notification extensions use the same
//! rules (their settings mirrored where the extension can read them).

use nsh_core::{notifications::Notification, prelude::*};

use crate::store::{Arrival, Chat, ChatKind, Settings, Store, store};

/// Something that may deserve a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A message from someone else (`sender` named in groups).
    Message {
        sender: Option<String>,
        preview: String,
    },
    /// Someone reacted to one of the user's messages.
    Reaction { sender: String, emoji: String },
    /// An incoming call.
    Call { video: bool },
}

/// Where the user is: a notification for the chat they are reading is noise.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Focus {
    /// The app's window has the focus.
    pub app_focused: bool,
    /// The chat open now, if any.
    pub open_chat: Option<u64>,
}

/// What to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Stable per chat: a newer notice replaces the older one.
    pub id: String,
    pub title: String,
    pub body: String,
    /// No sound (the user turned sounds off).
    pub silent: bool,
}

/// The text shown when the user hides notification content.
pub const PRIVATE_TITLE: &str = "Songbird";

/// Whether and what to notify for `event` in `chat`.
pub fn decide(event: &Event, chat: &Chat, settings: &Settings, focus: Focus) -> Option<Notice> {
    let group = !matches!(chat.kind, ChatKind::Direct(_) | ChatKind::WhatsApp(_));
    let reading = focus.app_focused && focus.open_chat == Some(chat.id);
    let (title, body) = match event {
        Event::Message { sender, preview } => {
            let on = if group {
                settings.notify_groups
            } else {
                settings.notify_messages
            };
            if !on || chat.muted || reading {
                return None;
            }
            let body = match sender {
                Some(s) if group => format!("{s}: {preview}"),
                _ => preview.clone(),
            };
            (chat.title.clone(), body)
        }
        Event::Reaction { sender, emoji } => {
            let on = if group {
                settings.reactions_groups
            } else {
                settings.reactions_messages
            };
            if !on || chat.muted || reading {
                return None;
            }
            (
                chat.title.clone(),
                format!("{sender} reacted {emoji} to your message"),
            )
        }
        // A call rings even in a muted chat (muting is about messages), and
        // even while the chat is open: it needs an answer.
        Event::Call { video } => {
            if !settings.notify_calls {
                return None;
            }
            let kind = if *video { "video" } else { "voice" };
            (chat.title.clone(), format!("Incoming {kind} call"))
        }
    };
    let (title, body) = if settings.hide_notification_content {
        let generic = match event {
            Event::Call { .. } => "Incoming call",
            Event::Message { .. } | Event::Reaction { .. } => "New message",
        };
        (PRIVATE_TITLE.to_string(), generic.to_string())
    } else {
        (title, body)
    };
    Some(Notice {
        id: format!("chat:{}", chat.id),
        title,
        body,
        silent: !settings.notify_sound,
    })
}

/// The app's focus, kept by [`install`].
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct AppFocused(bool);

/// Starts the notifier (once, in the root): shows notifications for new
/// messages and incoming calls by [`decide`], keeps the icon's badge at the
/// unread count, opens the chat a pressed notification is about, and asks
/// for permission once.
pub fn install(focus_of: impl Fn() -> Option<u64> + 'static) {
    let s = store();
    let focused = global::<AppFocused>();
    focused.set(AppFocused(true));
    nsh_core::element::desktop::main_window().on_focus_change(move |f| focused.set(AppFocused(f)));
    // The badge follows the unread count.
    effect(move || {
        let n = s.total_unread();
        untracked(|| set_badge(n));
    });
    // Mutes for 8 hours or a week lift on time.
    s.end_mutes();
    set_interval(std::time::Duration::from_secs(60), move || s.end_mutes());
    // Ask once, when the user has anything switched on.
    spawn(async move {
        use nsh_core::notifications::NotificationPermission as P;
        if notification_permission().await == P::Default {
            let _ = request_notification_permission().await;
        }
    });
    watch(s, focused, focus_of, move |n: Notice| {
        let mut notification = Notification::new(n.id, n.title, n.body).silent(n.silent);
        // The user's message tone (Settings → Notifications), Songbird's own.
        let tone = s.ext().with_untracked(|x| x.message_tone);
        if let Some(path) = crate::sounds::asset(&tone) {
            notification = notification.sound(path);
        } else if tone == "None" {
            notification = notification.silent(true);
        }
        spawn(async move {
            let _ = show_notification(notification).await;
        });
    });
    // A pressed notification opens its chat (and the window).
    spawn(async move {
        loop {
            let id = next_notification_press().await;
            if let Some(chat) = id.strip_prefix("chat:").and_then(|c| c.parse().ok()) {
                crate::features::desktop::show_main_window();
                crate::features::chat::open(chat);
            }
        }
    });
}

/// The store's half of the notifier: new messages and incoming calls,
/// decided by [`decide`], handed to `sink` (the system's notifications in
/// the app; a list in tests).
fn watch(
    s: Store,
    focused: Signal<AppFocused>,
    focus_of: impl Fn() -> Option<u64> + 'static,
    sink: impl Fn(Notice) + 'static,
) {
    s.on_arrival(move |s, a| {
        let focus = Focus {
            app_focused: focused.get_untracked().0,
            open_chat: focus_of(),
        };
        let (chat, event) = match a {
            Arrival::Message {
                chat,
                sender,
                preview,
            } => (chat, Event::Message { sender, preview }),
            Arrival::Call { chat, video } => (chat, Event::Call { video }),
        };
        notify(s, chat, &event, focus, &sink);
    });
}

/// Preview builds only (debug, or `SONGBIRD_PREVIEW_BUILD` at compile time):
/// `SONGBIRD_DEMO_ARRIVALS=<seconds>` (or that number in a
/// `songbird-demo-arrivals` file in the app's data folder) hides the window as closing it to the
/// tray would, then has the first direct contact write and, later, call: to
/// see the notifications on screen without driving the UI (D-0098).
pub fn demo_from_env() {
    let preview = crate::PREVIEW_BUILD;
    // The environment, or (emulators and devices, which pass none through) a
    // `songbird-demo-arrivals` file in the app's data folder.
    let from_file =
        || data_dir().and_then(|d| std::fs::read_to_string(d.join("songbird-demo-arrivals")).ok());
    let Some(secs) = std::env::var("SONGBIRD_DEMO_ARRIVALS")
        .ok()
        .or_else(from_file)
        .and_then(|v| v.trim().parse::<u64>().ok())
    else {
        return;
    };
    if !preview {
        return;
    }
    let s = store();
    let Some((chat, peer)) = s.chats.get_untracked().iter().find_map(|c| match c.kind {
        ChatKind::Direct(p) if !c.muted => Some((c.id, p)),
        _ => None,
    }) else {
        return;
    };
    let after = std::time::Duration::from_secs(secs);
    set_timeout(after, || nsh_core::element::desktop::main_window().hide());
    set_timeout(after + std::time::Duration::from_secs(2), move || {
        s.push_message(
            chat,
            Some(peer),
            crate::store::Body::Text("Are we still on for tonight?".into()),
        );
    });
    set_timeout(after + std::time::Duration::from_secs(8), move || {
        crate::features::calls::simulate_incoming(chat, true);
    });
}

fn notify(s: Store, chat: u64, event: &Event, focus: Focus, sink: &dyn Fn(Notice)) {
    // A supervised child's quiet hours (D-0166): nothing notifies.
    if crate::backend::rules::quiet_now() {
        return;
    }
    let Some(c) = s.chat(chat) else { return };
    if let Some(n) = s.settings.with_untracked(|st| decide(event, &c, st, focus)) {
        sink(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::seed_for_tests as seed;

    fn chat(kind: ChatKind, muted: bool) -> Chat {
        let mut c = seed::seed()
            .chats
            .into_iter()
            .next()
            .expect("a seeded chat");
        c.id = 7;
        c.title = "Ana".into();
        c.kind = kind;
        c.muted = muted;
        c
    }

    fn settings() -> Settings {
        seed::seed().settings
    }

    fn msg(text: &str) -> Event {
        Event::Message {
            sender: Some("Ana".into()),
            preview: text.into(),
        }
    }

    const AWAY: Focus = Focus {
        app_focused: false,
        open_chat: None,
    };

    #[test]
    fn a_message_notifies_with_its_chat_and_text() {
        let n = decide(
            &msg("hi"),
            &chat(ChatKind::Direct(1), false),
            &settings(),
            AWAY,
        )
        .unwrap();
        assert_eq!(
            (n.title.as_str(), n.body.as_str(), n.id.as_str()),
            ("Ana", "hi", "chat:7")
        );
        let g = decide(&msg("hi"), &chat(ChatKind::Group, false), &settings(), AWAY).unwrap();
        assert_eq!(g.body, "Ana: hi", "groups name the sender");
    }

    #[test]
    fn muted_chats_stay_quiet_but_calls_still_ring() {
        let muted = chat(ChatKind::Direct(1), true);
        assert_eq!(decide(&msg("hi"), &muted, &settings(), AWAY), None);
        let r = Event::Reaction {
            sender: "Ana".into(),
            emoji: "👍".into(),
        };
        assert_eq!(decide(&r, &muted, &settings(), AWAY), None);
        assert!(decide(&Event::Call { video: true }, &muted, &settings(), AWAY).is_some());
    }

    #[test]
    fn each_switch_turns_its_kind_off() {
        let direct = chat(ChatKind::Direct(1), false);
        let group = chat(ChatKind::Group, false);
        let mut s = settings();
        s.notify_messages = false;
        assert_eq!(decide(&msg("x"), &direct, &s, AWAY), None);
        assert!(
            decide(&msg("x"), &group, &s, AWAY).is_some(),
            "groups have their own switch"
        );
        s.notify_groups = false;
        assert_eq!(decide(&msg("x"), &group, &s, AWAY), None);
        s.notify_calls = false;
        assert_eq!(
            decide(&Event::Call { video: false }, &direct, &s, AWAY),
            None
        );
        let r = Event::Reaction {
            sender: "Ana".into(),
            emoji: "🎉".into(),
        };
        let mut s = settings();
        s.reactions_messages = false;
        assert_eq!(decide(&r, &direct, &s, AWAY), None);
        s.reactions_messages = true;
        assert!(decide(&r, &direct, &s, AWAY).is_some());
    }

    #[test]
    fn the_chat_being_read_is_not_notified_but_a_background_one_is() {
        let c = chat(ChatKind::Direct(1), false);
        let reading = Focus {
            app_focused: true,
            open_chat: Some(7),
        };
        assert_eq!(decide(&msg("x"), &c, &settings(), reading), None);
        // Open but the window in the background (the user is elsewhere).
        let behind = Focus {
            app_focused: false,
            open_chat: Some(7),
        };
        assert!(decide(&msg("x"), &c, &settings(), behind).is_some());
        // Another chat open.
        let other = Focus {
            app_focused: true,
            open_chat: Some(8),
        };
        assert!(decide(&msg("x"), &c, &settings(), other).is_some());
        // A call rings even in the chat being read.
        assert!(decide(&Event::Call { video: false }, &c, &settings(), reading).is_some());
    }

    #[test]
    fn hidden_content_shows_no_name_and_no_text() {
        let mut s = settings();
        s.hide_notification_content = true;
        let c = chat(ChatKind::Group, false);
        let n = decide(&msg("the secret"), &c, &s, AWAY).unwrap();
        assert_eq!(
            (n.title.as_str(), n.body.as_str()),
            (PRIVATE_TITLE, "New message")
        );
        let call = decide(&Event::Call { video: true }, &c, &s, AWAY).unwrap();
        assert_eq!(
            (call.title.as_str(), call.body.as_str()),
            (PRIVATE_TITLE, "Incoming call")
        );
    }

    #[test]
    fn sounds_follow_the_setting() {
        let c = chat(ChatKind::Direct(1), false);
        let mut s = settings();
        s.notify_sound = true;
        assert!(!decide(&msg("x"), &c, &s, AWAY).unwrap().silent);
        s.notify_sound = false;
        assert!(decide(&msg("x"), &c, &s, AWAY).unwrap().silent);
    }

    static SHOWN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    /// A root that runs the notifier on a fresh store and drives it: a
    /// message from someone else, the user's own, one in a muted chat, two
    /// in one batch, one in the chat being read, and a ring.
    #[component]
    fn NotifyProbe() -> impl IntoElement {
        let s = Store::new();
        crate::store::install(s);
        let focused = signal(AppFocused(false));
        let open = signal(None::<u64>);
        let count = signal(0_u32);
        watch(
            s,
            focused,
            move || open.get_untracked(),
            move |n: Notice| {
                SHOWN.lock().unwrap().push(n.body);
                count.update(|c| *c += 1);
            },
        );
        let (chat, peer) = s
            .chats
            .get_untracked()
            .iter()
            .find_map(|c| match c.kind {
                ChatKind::Direct(p) => Some((c.id, p)),
                _ => None,
            })
            .expect("a direct chat");
        set_timeout(std::time::Duration::from_millis(20), move || {
            let text = |t: &str| crate::store::Body::Text(t.into());
            s.push_message(chat, Some(peer), text("hello"));
            s.push_message(chat, None, text("mine"));
            s.edit_chat(chat, |c| c.muted = true);
            s.push_message(chat, Some(peer), text("muted"));
            s.edit_chat(chat, |c| c.muted = false);
            batch(|| {
                s.push_message(chat, Some(peer), text("first of two"));
                s.push_message(chat, Some(peer), text("second of two"));
            });
        });
        set_timeout(std::time::Duration::from_millis(60), move || {
            focused.set(AppFocused(true));
            open.set(Some(chat));
            s.push_message(chat, Some(peer), crate::store::Body::Text("reading".into()));
            s.arrive(Arrival::Call { chat, video: true });
        });
        text(move || format!("shown: {}", count.get()))
    }

    /// The wiring on the real engine, headless.
    #[test]
    fn the_store_s_events_reach_the_notifier() -> Result<(), String> {
        let mut app =
            nsh_testing::Headless::start(nsh_engine::EngineConfig::new(nsh_engine::Root::of::<
                NotifyProbe,
            >()))
            .map_err(|e| e.to_string())?;
        app.wait_for("text:shown: 4")?;
        assert_eq!(
            *SHOWN.lock().unwrap(),
            [
                "hello",
                "first of two",
                "second of two",
                "Incoming video call"
            ]
        );
        Ok(())
    }
}

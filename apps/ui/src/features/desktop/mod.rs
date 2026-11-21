//! A13 desktop: keyboard shortcuts, ⌘K jump-to (`jump`), the cheat sheet,
//! native menus, the tray / menu bar icon with the unread count, chats in
//! their own window, dropping files, and the desktop settings. Everything here runs only on
//! desktops (`is_desktop()`); what NSH lacks is in `DESKTOP_GAPS.md`.

pub mod jump;
pub mod keys;

use nsh_core::{
    desktop::{MenuRole, TrayTitleRun, WindowAttributes},
    element::{
        FileDragEvent, FileDropEvent,
        desktop::{
            MenuItem, WindowRef, main_window, open_window, set_application_menu, set_tray, tray,
            window as new_window,
        },
    },
    keymap::When,
    platform::Os,
    prelude::*,
};

use crate::{
    features,
    nav::{self, Tab},
    store::{ChatId, Store, extras, store},
    ui::{extras_parts::key_combo, *},
};

use keys::{Command, EXTRA, GROUPS, SHORTCUTS, keys_for};

/// Bumped when the user asks to search the chat list (⌘F, the View menu). The chat
/// list watches it and focuses its search field:
/// `effect(move || if global::<SearchFocus>().get().0 > 0 { search.focus() })`.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct SearchFocus(pub u64);

/// The chat open now: the desktop detail pane's, or the top pushed chat
/// page in the narrow layout.
pub fn current_chat() -> Option<ChatId> {
    if wide() {
        return nav::open_chat().get_untracked().0;
    }
    nav::routes().with_untracked(|r| {
        r.0.iter().rev().find_map(|route| {
            route
                .key
                .strip_prefix("chat:")
                .and_then(|id| id.parse().ok())
        })
    })
}

/// Runs a shortcut's command (keys, menus and the tray share it).
fn run(s: Store, command: Command) {
    match command {
        Command::JumpTo => jump::toggle(),
        Command::Search => {
            nav::pop_to_root();
            nav::tab().set(Tab::Chats);
            global::<SearchFocus>().update(|f| f.0 += 1);
        }
        Command::NewChat => features::chats::new_chat(),
        Command::NextChat | Command::PreviousChat => {
            let step = if command == Command::NextChat { 1 } else { -1 };
            let ids: Vec<ChatId> = s.inbox().iter().map(|c| c.id).collect();
            if let Some(next) = keys::cycle(&ids, current_chat(), step) {
                nav::tab().set(Tab::Chats);
                if !wide() {
                    nav::pop_to_root();
                }
                features::chat::open(next);
            }
        }
        Command::Tab(i) => {
            nav::pop_to_root();
            nav::tab().set(Tab::ALL[i.min(3)]);
        }
        Command::Shortcuts => {
            let open =
                nav::routes().with_untracked(|r| r.0.last().is_some_and(|x| x.key == SHEET_KEY));
            if open { nav::pop() } else { shortcuts_sheet() }
        }
        Command::MarkRead => match current_chat() {
            Some(id) => {
                s.mark_read(id);
                toast("Marked as read");
            }
            None => {
                for c in s.inbox() {
                    s.mark_read(c.id);
                }
                toast("All chats marked as read");
            }
        },
        Command::Settings => {
            nav::pop_to_root();
            nav::tab().set(Tab::You);
        }
    }
}

/// Registers the desktop keyboard shortcuts, the application menus and
/// the tray icon. Call once in the root's body (they live as long as it).
pub fn install_shortcuts() {
    if !is_desktop() {
        return;
    }
    let s = store();
    for sc in SHORTCUTS {
        let command = sc.command;
        // Ctrl+Tab moves between chats even while typing in the composer.
        let when = if sc.keys.contains("tab") {
            When::Always
        } else {
            When::Auto
        };
        on_key_with(
            sc.keys,
            KeyOptions {
                when,
                ..KeyOptions::default()
            },
            move |_| run(s, command),
        );
    }
    desktop_menus();
    s.restore_tray_choice();
    desktop_tray();
    close_to_tray();
}

/// While the tray / menu bar icon shows, closing the main window hides it
/// and Songbird keeps running there, connected, so messages and calls still
/// notify (D-0098; desktops have no push service). Quit from the tray's
/// menu or the app menu. Without the icon, closing works as the system's
/// does.
fn close_to_tray() {
    let window = main_window();
    let w = window.clone();
    effect(move || {
        let keep = !extras().with(|e| e.tray_hidden);
        untracked(|| {
            let mut a = WindowAttributes::default();
            a.close_on_press = Some(!keep);
            w.set(a);
        });
    });
    let w = window.clone();
    window.on_close_press(move |_| {
        if extras().with_untracked(|e| e.tray_hidden) {
            w.close();
        } else {
            w.hide();
        }
    });
}

/// Brings the main window back (from the tray, a notification).
pub fn show_main_window() {
    let w = main_window();
    w.show();
    w.focus();
}

const SHEET_KEY: &str = "shortcuts";

/// The shortcut cheat sheet: a page listing every shortcut with its keys
/// as this platform shows them (⌘K on macOS, Ctrl+K elsewhere). Escape or
/// ⌘/ closes it.
pub fn shortcuts_sheet() {
    nav::push(SHEET_KEY, || {
        if is_desktop() {
            on_key("escape", |_| nav::pop());
        }
        let body = view()
            .w_full()
            .max_w(px(760.0))
            .self_center()
            .child(view().px_4().pt_4().child(muted(
                "Shortcuts work anywhere in Songbird. The menus show them next to each command too.",
            )))
            .children(GROUPS.iter().map(|g| {
                // One row per action; its alternate keys share the row.
                let mut rows: Vec<(&'static str, Vec<&'static str>)> = Vec::new();
                let all = SHORTCUTS
                    .iter()
                    .filter(|s| s.group == *g)
                    .map(|s| (s.label, s.keys))
                    .chain(EXTRA.iter().filter(|(_, _, eg)| eg == g).map(|(k, l, _)| (*l, *k)));
                for (label, keys) in all {
                    match rows.iter_mut().find(|(l, _)| *l == label) {
                        Some((_, ks)) => ks.push(keys),
                        None => rows.push((label, vec![keys])),
                    }
                }
                let rows: Vec<AnyElement> = rows.into_iter().map(|(label, keys)| shortcut_row(label, keys)).collect();
                view().child(section_title(*g)).child(group_plain(rows))
            }));
        page("Keyboard shortcuts", page_scroll(body))
    });
}

/// A cheat-sheet row: what it does, and its keys.
fn shortcut_row(label: &'static str, keys: Vec<&'static str>) -> AnyElement {
    let shown: Vec<String> = keys.iter().map(|k| key_label(k)).collect();
    let spoken = format!("{label}: {}", shown.join(" or "));
    let n = shown.len();
    Row::new(label)
        .trailing(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .label(spoken)
                .children(shown.into_iter().enumerate().map(move |(i, k)| {
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(key_combo(&k))
                        .child((i + 1 < n).then(|| {
                            text("or")
                                .text_size(px(font::CAPTION))
                                .text_color(move || c::muted_fg())
                        }))
                })),
        )
        .build()
}

/// The native application menus: File, Edit, View, Chat, Window, Help (and
/// the app menu on macOS). Shortcuts in them come from the same table as
/// the key bindings. Call in an owner that lives as long as the app.
pub fn desktop_menus() {
    if !is_desktop() {
        return;
    }
    let s = store();
    let mac = os() == Os::MacOs;
    let item = move |label: &str, command: Command| {
        let m = MenuItem::new(label).on_select(move || run(s, command));
        match keys_for(command) {
            Some(k) => m.shortcut(k),
            None => m,
        }
    };
    let mut menus = Vec::new();
    if mac {
        menus.push(MenuItem::submenu(
            "Songbird",
            [
                MenuItem::role(MenuRole::About),
                MenuItem::separator(),
                item("Settings…", Command::Settings),
                MenuItem::separator(),
                MenuItem::role(MenuRole::Hide),
                MenuItem::role(MenuRole::HideOthers),
                MenuItem::role(MenuRole::ShowAll),
                MenuItem::separator(),
                MenuItem::role(MenuRole::Quit),
            ],
        ));
    }
    let mut file = vec![
        item("New chat", Command::NewChat),
        MenuItem::new("New group…").on_select(features::groups::new_group),
        MenuItem::separator(),
        MenuItem::new("Open chat in new window").on_select(move || {
            if let Some(id) = current_chat() {
                let _ = pop_out_chat(s, id);
            }
        }),
        MenuItem::separator(),
        MenuItem::role(MenuRole::Close),
    ];
    if !mac {
        file.push(item("Settings…", Command::Settings));
        file.push(MenuItem::separator());
        file.push(MenuItem::role(MenuRole::Quit).label("Exit"));
    }
    menus.push(MenuItem::submenu("File", file));
    menus.push(MenuItem::submenu(
        "Edit",
        [
            MenuItem::role(MenuRole::Undo),
            MenuItem::role(MenuRole::Redo),
            MenuItem::separator(),
            MenuItem::role(MenuRole::Cut),
            MenuItem::role(MenuRole::Copy),
            MenuItem::role(MenuRole::Paste),
            MenuItem::role(MenuRole::SelectAll),
        ],
    ));
    menus.push(MenuItem::submenu(
        "View",
        [
            item("Chats", Command::Tab(0)),
            item("Calls", Command::Tab(1)),
            item("Spaces", Command::Tab(2)),
            item("You", Command::Tab(3)),
            MenuItem::separator(),
            item("Jump to…", Command::JumpTo),
            item("Search chats", Command::Search),
            MenuItem::separator(),
            MenuItem::role(MenuRole::ToggleFullScreen),
        ],
    ));
    menus.push(MenuItem::submenu(
        "Chat",
        [
            item("Next chat", Command::NextChat),
            item("Previous chat", Command::PreviousChat),
            MenuItem::separator(),
            item("Mark as read", Command::MarkRead),
        ],
    ));
    menus.push(MenuItem::submenu(
        "Window",
        [
            MenuItem::role(MenuRole::Minimize),
            MenuItem::role(MenuRole::Zoom),
        ],
    ));
    menus.push(MenuItem::submenu(
        "Help",
        [
            MenuItem::new("Songbird help").on_select(features::plans::help),
            item("Keyboard shortcuts", Command::Shortcuts),
            MenuItem::separator(),
            MenuItem::new("Service status").on_select(|| {
                let _ = open_url("https://status.songbird.chat");
            }),
        ],
    ));
    set_application_menu(menus);
}

/// The tray / menu bar icon: the feather, the unread count beside it (in
/// the menu bar's own text colour), and a short menu. Follows the unread
/// count and the user's "Show in menu bar" choice.
fn desktop_tray() {
    let s = store();
    effect(move || {
        let hidden = extras().with(|e| e.tray_hidden);
        let unread = s.total_unread();
        untracked(|| {
            if hidden {
                set_tray(None);
                return;
            }
            let title = if unread > 0 {
                vec![TrayTitleRun::new(unread.to_string())]
            } else {
                Vec::new()
            };
            set_tray(
                tray()
                    .icon(Icon::Feather.src())
                    .tooltip(match unread {
                        0 => "Songbird".to_string(),
                        1 => "Songbird · 1 unread message".to_string(),
                        n => format!("Songbird · {n} unread messages"),
                    })
                    .title(title)
                    .menu([
                        MenuItem::new("Open Songbird").on_select(show_main_window),
                        MenuItem::new("New chat").on_select(move || {
                            show_main_window();
                            run(s, Command::NewChat);
                        }),
                        MenuItem::separator(),
                        MenuItem::role(MenuRole::Quit),
                    ]),
            );
        });
    });
}

/// Opens `chat` in a window of its own (Yahoo Messenger style), sharing
/// the app's store. A second call for the same chat focuses its window.
pub fn pop_out_chat(s: Store, chat: ChatId) -> Option<WindowRef> {
    let title = s
        .chat(chat)
        .map(|c| c.title)
        .unwrap_or_else(|| "Chat".into());
    open_window(
        new_window()
            .id(format!("chat:{chat}"))
            .title(title)
            .default_size(size(px(420.0), px(640.0)))
            .content(move || {
                // Windows render beside the root, outside its context.
                provide_context(s);
                view()
                    .flex_1()
                    .bg(move || c::background())
                    .text_color(move || c::foreground())
                    .child(features::chat::conversation(chat))
                    .child(toast_layer())
            }),
    )
}

/// A file dropped on the app.
#[derive(Debug, Clone, PartialEq)]
pub struct DroppedFile {
    /// Its `file:` URL.
    pub uri: String,
    /// Its name, decoded ("Floor plan v3.pdf").
    pub name: String,
    /// Its MIME type by name, `""` when unknown.
    pub mime: String,
}

/// The file name of a `file:` URL, percent-decoded.
pub fn file_name(uri: &str) -> String {
    let last = uri.trim_end_matches('/').rsplit('/').next().unwrap_or(uri);
    percent_decoded(last)
}

/// The local path of a `file:` URL (`file:///Users/a/b%20c.pdf`), percent-decoded; `None`
/// for any other URL.
pub fn file_path(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri
        .strip_prefix("file://localhost")
        .or_else(|| uri.strip_prefix("file://"))?;
    let path = percent_decoded(rest);
    // `file:///C:/x` on Windows: the drive letter follows the slash.
    let path = match path.as_bytes() {
        [b'/', d, b':', ..] if d.is_ascii_alphabetic() && cfg!(windows) => path[1..].to_string(),
        _ => path,
    };
    (!path.is_empty()).then(|| std::path::PathBuf::from(path))
}

fn percent_decoded(last: &str) -> String {
    let bytes = last.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&last[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Makes `target` a drop destination for files (desktops): while files
/// hover it, a calm overlay says what will happen (`hint`, such as "Drop
/// to send to Ana"), and `on_drop` gets the files. For the conversation.
pub fn on_file_drop(
    target: View,
    hint: impl Into<SharedString>,
    on_drop: impl Fn(Vec<DroppedFile>) + 'static,
) -> View {
    let hint: SharedString = hint.into();
    let over = signal(false);
    let reduced = motion::reduced();
    target
        .on_file_drag_enter(move |_: &FileDragEvent| over.set(true))
        .on_file_drag_leave(move || over.set(false))
        .on_file_drop(move |e: &FileDropEvent| {
            over.set(false);
            let files = e
                .uris
                .iter()
                .enumerate()
                .map(|(i, uri)| DroppedFile {
                    uri: uri.clone(),
                    name: file_name(uri),
                    mime: e.mime_types.get(i).cloned().unwrap_or_default(),
                })
                .collect();
            on_drop(files);
        })
        .child(
            view()
                .absolute()
                .inset_0()
                .m_3()
                .items_center()
                .justify_center()
                .gap_2()
                .rounded(move || radius::lg())
                .border_2()
                .border_color(move || c::primary())
                .bg(move || c::accent())
                // Only shows: never takes a press, so the conversation
                // under it (header, menus) stays usable.
                .pointer_events(PointerEvents::None)
                .opacity(move || if over.get() { 0.96_f32 } else { 0.0 })
                .scale(move || if over.get() || reduced { 1.0_f32 } else { 0.98 })
                .transition_all()
                .duration(motion::FAST)
                .ease_out()
                .accessibility_elements_hidden(true)
                .child(icon(Icon::Upload, 28.0).text_color(move || c::primary()))
                .child(
                    text(hint)
                        .text_size(px(font::TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(move || c::on_accent()),
                ),
        )
}

/// Desktop settings, pushed (You → Desktop): open at login, the menu bar
/// icon, and the shortcuts.
pub fn desktop_settings() {
    nav::push("desktop-settings", || {
        let s = store();
        let ext = extras();
        // The system's login item may have changed in its own settings.
        s.refresh_launch_at_login();
        let tray_name = if os() == Os::MacOs {
            "Show in menu bar"
        } else {
            "Show in system tray"
        };
        let body = view()
            .w_full()
            .max_w(px(760.0))
            .self_center()
            .child(section_title("Startup"))
            .child(group([toggle_row(
                "Open at login",
                Some("Songbird starts quietly in the background when you sign in."),
                move || s.settings.with(|x| x.launch_at_login),
                move |v| s.set_launch_at_login(v),
            )]))
            .child(section_title(if os() == Os::MacOs { "Menu bar" } else { "System tray" }))
            .child(group([toggle_row(
                tray_name,
                Some("The feather shows your unread count. Closing the window keeps Songbird running there, so messages and calls still notify you."),
                move || !ext.with(|e| e.tray_hidden),
                move |v| s.set_tray_hidden(!v),
            )]))
            .child(section_title("Keyboard"))
            .child(group([Row::new("Keyboard shortcuts")
                .icon(Icon::Keyboard)
                .trailing(key_combo(&key_label("mod+/")))
                .on_press(shortcuts_sheet)
                .build()]));
        page("Desktop", page_scroll(body))
    });
}

#[cfg(test)]
mod tests {
    use super::{file_name, file_path};

    #[test]
    fn file_paths_are_decoded() {
        assert_eq!(
            file_path("file:///Users/ana/Floor%20plan%20v3.pdf"),
            Some(std::path::PathBuf::from("/Users/ana/Floor plan v3.pdf"))
        );
        assert_eq!(file_path("https://songbird.chat/x"), None);
    }

    #[test]
    fn file_names_are_decoded() {
        assert_eq!(
            file_name("file:///Users/a/Floor%20plan%20v3.pdf"),
            "Floor plan v3.pdf"
        );
        assert_eq!(file_name("file:///tmp/r%C3%A2u.jpg"), "râu.jpg");
        assert_eq!(file_name("file:///tmp/50%"), "50%");
        assert_eq!(file_name("file:///tmp/dir/"), "dir");
    }
}

//! ⌘K jump-to (desktops; Ctrl+K elsewhere): a centred card over the dimmed
//! window that finds a chat, a person or an action by a few letters, and
//! opens it with Return.
//!
//! - The model ([`results`]) is plain Rust and unit-tested: what a query
//!   finds, ranked, in sections, with the matched run of each title.
//! - The view ([`layer`]) is one overlay, mounted once by the root; it
//!   shows while [`JumpOpen`] is set.
//!
//! Matching folds case and diacritics ("stef" finds "Ștefănescu") and ranks
//! a whole title before its start, a word's start before the middle of a
//! word, and the title before the keywords; ties keep the list's own order
//! (recent chats first).

use nsh_core::{keymap::When, platform::Os, prelude::*};

use crate::{
    features,
    nav::{self, Tab},
    store::{Chat, ChatId, ChatKind, ContactId, Store, store},
    ui::{floating, *},
};

use super::keys::Command;

// ---------------------------------------------------------------- model

/// What a result opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Chat(ChatId),
    /// A space's home (a `ChatKind::Community` in the store).
    Space(ChatId),
    /// Someone the user has no chat with yet: opens a new one.
    Person(ContactId),
    Action(Act),
}

/// The actions jump-to offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    NewChat,
    NewGroup,
    NewSpace,
    FindChannels,
    MarkAllRead,
    Shortcuts,
    /// One of the four destinations, by its place in `Tab::ALL`.
    Tab(usize),
    /// A settings page, by its You row's title.
    Settings(&'static str),
    /// No match: the New chat page, to find someone new.
    NewChatWith,
}

/// What leads a result's row.
#[derive(Debug, Clone, PartialEq)]
pub enum Lead {
    /// The chat's own picture.
    Chat(ChatId),
    Person {
        name: String,
        seed: u64,
    },
    Glyph(Icon),
}

/// Something jump-to can find.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub target: Target,
    pub title: String,
    /// More words that find it (a username, "dark" for Appearance); never
    /// highlighted.
    pub keywords: String,
    /// The muted word at the row's end: its kind or its shortcut.
    pub hint: String,
    pub lead: Lead,
}

/// A result: a candidate and the run of its title the query matched, in
/// chars (`start`, `len`).
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub target: Target,
    pub title: String,
    pub hint: String,
    pub lead: Lead,
    pub cut: Option<(usize, usize)>,
}

impl Hit {
    fn of(c: &Candidate, cut: Option<(usize, usize)>) -> Self {
        Hit {
            target: c.target,
            title: c.title.clone(),
            hint: c.hint.clone(),
            lead: c.lead.clone(),
            cut,
        }
    }

    /// The title in three parts: before, the match, after.
    pub fn parts(&self) -> (String, String, String) {
        let chars: Vec<char> = self.title.chars().collect();
        match self.cut {
            Some((start, len)) if start + len <= chars.len() => (
                chars[..start].iter().collect(),
                chars[start..start + len].iter().collect(),
                chars[start + len..].iter().collect(),
            ),
            _ => (self.title.clone(), String::new(), String::new()),
        }
    }

    /// What screen readers say for the row.
    pub fn spoken(&self) -> String {
        if self.hint.is_empty() {
            self.title.clone()
        } else {
            format!("{}, {}", self.title, self.hint)
        }
    }
}

/// A titled run of results.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub title: &'static str,
    pub hits: Vec<Hit>,
}

/// What the card shows for a query.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Results {
    pub sections: Vec<Section>,
    /// A calm line above the results ("No matches for “zq”").
    pub note: Option<String>,
}

impl Results {
    /// The rows in the order the keys move through them.
    pub fn rows(&self) -> Vec<&Hit> {
        self.sections.iter().flat_map(|s| s.hits.iter()).collect()
    }
}

/// Everything jump-to searches, in each kind's own order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pool {
    /// Chats and spaces, most recent first.
    pub chats: Vec<Candidate>,
    /// People with no chat yet, A–Z.
    pub people: Vec<Candidate>,
    pub actions: Vec<Candidate>,
    /// How many of `chats` the empty query shows as Recent (the inbox's
    /// head; archived chats and spaces come after it).
    pub recent: usize,
    /// The actions the empty query suggests, as indices into `actions`.
    pub suggested: Vec<usize>,
}

/// How many results each section shows for a query.
const MAX_CHATS: usize = 5;
const MAX_PEOPLE: usize = 4;
const MAX_ACTIONS: usize = 5;
/// The empty query's Recent and suggested actions.
const MAX_RECENT: usize = 5;

/// A character as jump-to compares it: lower case, Latin accents and
/// Romanian comma-below letters folded ("Ș" finds "s"). One char for one
/// char, so a match's position maps back onto the title.
fn fold_char(c: char) -> char {
    let lower = c.to_lowercase().next().unwrap_or(c);
    match lower {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ă' | 'ā' | 'ą' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ę' | 'ě' | 'ē' => 'e',
        'ğ' => 'g',
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ı' => 'i',
        'ł' | 'ľ' => 'l',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => 'o',
        'ř' => 'r',
        'ș' | 'ş' | 'ś' | 'š' | 'ß' => 's',
        'ț' | 'ţ' | 'ť' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => 'u',
        'ý' | 'ÿ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        other => other,
    }
}

/// `s` folded, one char per char of `s`.
pub fn fold(s: &str) -> Vec<char> {
    s.chars().map(fold_char).collect()
}

/// The query as matched: folded, trimmed, inner spaces collapsed to one.
fn folded_query(query: &str) -> Vec<char> {
    let words: Vec<&str> = query.split_whitespace().collect();
    fold(&words.join(" "))
}

fn starts_with(hay: &[char], at: usize, needle: &[char]) -> bool {
    hay.len() >= at + needle.len() && hay[at..at + needle.len()] == *needle
}

/// Whether position `at` of `hay` starts a word.
fn word_start(hay: &[char], at: usize) -> bool {
    at == 0 || !hay[at - 1].is_alphanumeric()
}

/// How well `q` (folded) finds a title and its keywords: a rank (lower is
/// better) and the matched run of the title. `None`: no match.
///
/// 0 the whole title · 1 its start · 2 a word's start · 3 inside a word ·
/// 4 the keywords, or every word of a query of several found at word
/// starts across title and keywords ("set priv").
pub fn score(title: &str, keywords: &str, q: &[char]) -> Option<(u8, Option<(usize, usize)>)> {
    if q.is_empty() {
        return None;
    }
    let t = fold(title);
    let n = q.len();
    if t == q {
        return Some((0, Some((0, n))));
    }
    if starts_with(&t, 0, q) {
        return Some((1, Some((0, n))));
    }
    let positions: Vec<usize> = (0..t.len()).filter(|&i| starts_with(&t, i, q)).collect();
    if let Some(&at) = positions.iter().find(|&&i| word_start(&t, i)) {
        return Some((2, Some((at, n))));
    }
    if let Some(&at) = positions.first() {
        return Some((3, Some((at, n))));
    }
    let k = fold(keywords);
    if (0..k.len()).any(|i| starts_with(&k, i, q)) {
        return Some((4, None));
    }
    // Several words: each at a word's start, in the title or the keywords.
    let words: Vec<&[char]> = q.split(|c| *c == ' ').filter(|w| !w.is_empty()).collect();
    if words.len() > 1 {
        let found = |w: &[char]| {
            (0..t.len()).any(|i| word_start(&t, i) && starts_with(&t, i, w))
                || (0..k.len()).any(|i| word_start(&k, i) && starts_with(&k, i, w))
        };
        if words.iter().all(|w| found(w)) {
            let first = words[0];
            let cut = (0..t.len())
                .find(|&i| word_start(&t, i) && starts_with(&t, i, first))
                .map(|i| (i, first.len()));
            return Some((4, cut));
        }
    }
    None
}

/// The best `max` of `pool` for `q`, best first, ties in the pool's order.
fn ranked(pool: &[Candidate], q: &[char], max: usize) -> Vec<(u8, Hit)> {
    let mut found: Vec<(u8, usize, Hit)> = pool
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            score(&c.title, &c.keywords, q).map(|(rank, cut)| (rank, i, Hit::of(c, cut)))
        })
        .collect();
    found.sort_by_key(|(rank, i, _)| (*rank, *i));
    found
        .into_iter()
        .take(max)
        .map(|(rank, _, hit)| (rank, hit))
        .collect()
}

/// What the card shows for `query`.
///
/// Empty: Recent (the inbox's head) and the suggested actions. Otherwise
/// Chats, People and Actions, each section ranked, and the sections in
/// the order of their best match (so "new" puts Actions first). Nothing
/// found: a note and one way on, a new chat.
pub fn results(query: &str, pool: &Pool) -> Results {
    let q = folded_query(query);
    if q.is_empty() {
        let mut sections = Vec::new();
        let recent: Vec<Hit> = pool
            .chats
            .iter()
            .take(pool.recent.min(MAX_RECENT))
            .map(|c| Hit::of(c, None))
            .collect();
        if !recent.is_empty() {
            sections.push(Section {
                title: "Recent",
                hits: recent,
            });
        }
        let suggested: Vec<Hit> = pool
            .suggested
            .iter()
            .filter_map(|&i| pool.actions.get(i))
            .map(|c| Hit::of(c, None))
            .collect();
        if !suggested.is_empty() {
            sections.push(Section {
                title: "Actions",
                hits: suggested,
            });
        }
        return Results {
            sections,
            note: None,
        };
    }
    let mut found: Vec<(u8, usize, Section)> = [
        ("Chats", &pool.chats, MAX_CHATS),
        ("People", &pool.people, MAX_PEOPLE),
        ("Actions", &pool.actions, MAX_ACTIONS),
    ]
    .into_iter()
    .enumerate()
    .filter_map(|(order, (title, pool, max))| {
        let hits = ranked(pool, &q, max);
        let best = hits.first()?.0;
        Some((
            best,
            order,
            Section {
                title,
                hits: hits.into_iter().map(|(_, h)| h).collect(),
            },
        ))
    })
    .collect();
    found.sort_by_key(|(best, order, _)| (*best, *order));
    if found.is_empty() {
        let shown = query.split_whitespace().collect::<Vec<_>>().join(" ");
        return Results {
            sections: vec![Section {
                title: "",
                hits: vec![Hit {
                    target: Target::Action(Act::NewChatWith),
                    title: format!("Start a new chat with “{shown}”"),
                    hint: String::new(),
                    lead: Lead::Glyph(Icon::Compose),
                    cut: None,
                }],
            }],
            note: Some(format!("No matches for “{shown}”")),
        };
    }
    Results {
        sections: found.into_iter().map(|(_, _, s)| s).collect(),
        note: None,
    }
}

// ---------------------------------------------------------------- the pool from the store

/// The muted word after a chat's title.
fn chat_hint(c: &Chat) -> &'static str {
    if c.archived {
        return "Archived";
    }
    match c.kind {
        ChatKind::Direct(_) => "Chat",
        ChatKind::WhatsApp(_) => "WhatsApp",
        ChatKind::Group => "Group",
        ChatKind::Channel { .. } => "Channel",
        ChatKind::Community => "Space",
    }
}

/// An action's row: its label, the words that find it, its glyph and the
/// shortcut that does the same (shown as its hint).
fn action(act: Act, title: &str, keywords: &str, glyph: Icon, keys: Option<&str>) -> Candidate {
    Candidate {
        target: Target::Action(act),
        title: title.into(),
        keywords: keywords.into(),
        hint: keys.map(key_label).unwrap_or_default(),
        lead: Lead::Glyph(glyph),
    }
}

/// Everything there is to find now. Locked chats and message requests
/// never show: jump-to is as private as the chat list's search.
fn pool(s: Store) -> Pool {
    let inbox = s.inbox();
    let recent = inbox.len();
    let in_inbox: Vec<ChatId> = inbox.iter().map(|c| c.id).collect();
    let rest: Vec<Chat> = s.chats.with_untracked(|cs| {
        cs.iter()
            .filter(|c| !in_inbox.contains(&c.id) && !c.locked && !c.request)
            .cloned()
            .collect()
    });
    let username = |c: &Chat| match c.kind {
        ChatKind::Direct(p) | ChatKind::WhatsApp(p) => {
            s.contact(p).map(|p| p.username).unwrap_or_default()
        }
        _ => String::new(),
    };
    let chats: Vec<Candidate> = inbox
        .iter()
        .filter(|c| !c.locked)
        .chain(rest.iter())
        .map(|c| Candidate {
            target: if c.kind == ChatKind::Community {
                Target::Space(c.id)
            } else {
                Target::Chat(c.id)
            },
            title: c.title.clone(),
            keywords: username(c),
            hint: chat_hint(c).into(),
            lead: Lead::Chat(c.id),
        })
        .collect();
    // People with a chat already show as that chat.
    let with_chat: Vec<ContactId> = s.chats.with_untracked(|cs| {
        cs.iter()
            .filter_map(|c| match c.kind {
                ChatKind::Direct(p) | ChatKind::WhatsApp(p) => Some(p),
                _ => None,
            })
            .collect()
    });
    let mut people: Vec<Candidate> = s.contacts.with_untracked(|ps| {
        ps.iter()
            .filter(|p| !p.blocked && !with_chat.contains(&p.id))
            .map(|p| Candidate {
                target: Target::Person(p.id),
                title: p.name.clone(),
                keywords: handle(&p.username),
                hint: "Person".into(),
                lead: Lead::Person {
                    name: p.name.clone(),
                    seed: p.id,
                },
            })
            .collect()
    });
    people.sort_by_key(|p| fold(&p.title).into_iter().collect::<String>());

    let mut actions = vec![
        action(
            Act::NewChat,
            "New chat",
            "message start conversation contact compose write",
            Icon::Compose,
            Some("mod+n"),
        ),
        action(
            Act::NewGroup,
            "New group",
            "create members people",
            Icon::Users,
            None,
        ),
        action(
            Act::NewSpace,
            "New space",
            "create community communities groups",
            Icon::Spaces,
            None,
        ),
        action(
            Act::FindChannels,
            "Find channels",
            "follow directory browse news",
            Icon::Megaphone,
            None,
        ),
        action(
            Act::MarkAllRead,
            "Mark all as read",
            "unread clear seen",
            Icon::Check,
            None,
        ),
        action(
            Act::Tab(0),
            "Go to Chats",
            "inbox list",
            Icon::Chats,
            Some("mod+1"),
        ),
        action(
            Act::Tab(1),
            "Go to Calls",
            "history phone video",
            Icon::Calls,
            Some("mod+2"),
        ),
        action(
            Act::Tab(2),
            "Go to Spaces",
            "communities groups channels",
            Icon::Spaces,
            Some("mod+3"),
        ),
        action(
            Act::Tab(3),
            "Go to You",
            "profile settings me",
            Icon::You,
            Some("mod+4"),
        ),
        action(
            Act::Shortcuts,
            "Keyboard shortcuts",
            "keys help cheat sheet",
            Icon::Keyboard,
            Some("mod+/"),
        ),
    ];
    actions.extend(
        features::settings::jump_pages()
            .into_iter()
            .map(|p| Candidate {
                target: Target::Action(Act::Settings(p.title)),
                title: p.label,
                keywords: format!("settings {}", p.keywords),
                hint: String::new(),
                lead: Lead::Glyph(p.glyph),
            }),
    );
    // Empty query: making something new, then the settings people change most.
    let suggested = [
        "New chat",
        "New group",
        "New space",
        "Settings: Privacy",
        "Settings: Appearance",
        "Linked devices",
    ]
    .iter()
    .filter_map(|t| actions.iter().position(|a| a.title == *t))
    .collect();
    Pool {
        chats,
        people,
        actions,
        recent,
        suggested,
    }
}

/// Opens what a result points at.
fn open(s: Store, target: Target) {
    match target {
        Target::Chat(id) => show_chat(id),
        Target::Person(p) => show_chat(s.direct_chat(p)),
        Target::Space(id) => {
            nav::pop_to_root();
            nav::tab().set(Tab::ALL[2]);
            features::groups::community_home(id);
        }
        Target::Action(act) => match act {
            Act::NewChat | Act::NewChatWith => super::run(s, Command::NewChat),
            Act::NewGroup => features::groups::new_group(),
            Act::NewSpace => features::groups::new_community(),
            Act::FindChannels => {
                nav::pop_to_root();
                nav::tab().set(Tab::ALL[2]);
                features::groups::find_channels();
            }
            Act::MarkAllRead => {
                for c in s.inbox() {
                    s.mark_read(c.id);
                }
                toast("All chats marked as read");
            }
            Act::Shortcuts => super::shortcuts_sheet(),
            Act::Tab(i) => super::run(s, Command::Tab(i)),
            Act::Settings(title) => features::settings::open_page(title),
        },
    }
}

/// A chat, opened with the Chats list beside it.
fn show_chat(id: ChatId) {
    nav::tab().set(Tab::Chats);
    if !wide() {
        nav::pop_to_root();
    }
    features::chat::open(id);
}

// ---------------------------------------------------------------- view

/// Whether jump-to shows.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct JumpOpen(pub bool);

fn shown() -> Signal<JumpOpen> {
    global::<JumpOpen>()
}

/// ⌘K: opens jump-to, or closes it when it shows.
pub fn toggle() {
    shown().update(|o| o.0 = !o.0);
}

/// The card's width, as `REDESIGN.md` sets it.
const WIDTH: f32 = 560.0;
/// The card's distance from the window's top: the upper third, where the
/// eye already is, so results grow down without the card moving.
const TOP: f32 = 96.0;
/// Rows and section labels.
const ROW_H: f32 = 36.0;
const LABEL_H: f32 = 28.0;
/// The field's height, a step above the list's rows.
const FIELD_H: f32 = 52.0;
/// The results scroll past this height (about ten rows and their labels).
const LIST_MAX_H: f32 = 400.0;
/// The leading picture or glyph's lane, and a glyph in it.
const LEAD: f32 = 24.0;
const GLYPH: f32 = 16.0;

/// The jump-to layer: mount it once in the app's root, over everything
/// but the call surface. Nothing shows (and no key is bound) until ⌘K.
pub fn layer() -> AnyElement {
    if !is_desktop() {
        return view().into_element();
    }
    when(move || shown().get().0, palette).into_element()
}

/// The open card, its dim and its keys.
fn palette() -> AnyElement {
    let s = store();
    // Read once per opening: what can be found doesn't change under the
    // user while the card shows.
    let pool = pool(s);
    let query = signal(String::new());
    let found = computed(move || query.with(|q| results(q, &pool)));
    let count = computed(move || found.with(|r| r.rows().len()));
    let lit = signal(0usize);
    // Where the pointer last moved, shared by the rows (see `row`).
    let pointer = stored(None::<(f32, f32)>);
    // A new query starts at its best match.
    effect(move || {
        query.with(|_| ());
        untracked(|| lit.set(0));
    });
    let reduced = motion::reduced();
    let timing = |ms: f32| features::chat::timing(ms, Easing::EaseOut);
    let card_in = motion().entering(
        MotionSpec::new(timing(motion::FAST))
            .values(MotionChannel::Opacity, [0.0, 1.0])
            .values(
                MotionChannel::Scale,
                [if reduced { 1.0 } else { 0.98 }, 1.0],
            ),
    );
    let dim_in = motion()
        .entering(MotionSpec::new(timing(motion::FAST)).values(MotionChannel::Opacity, [0.0, 1.0]));
    let leaving = stored(false);

    // Esc and a press outside: the card and the dim fade out, faster than
    // they came in, then the layer goes.
    let dismiss = move || {
        if leaving.replace(true) {
            return;
        }
        let out =
            MotionSpec::new(timing(motion::INSTANT)).values(MotionChannel::Opacity, [1.0, 0.0]);
        let _ = dim_in.play(out.clone());
        let end = card_in.play(out);
        spawn(async move {
            end.await;
            shown().set(JumpOpen(false));
        });
    };
    // A choice: the card goes at once and the destination takes over.
    let choose = move |i: usize| {
        let Some(target) = found.with_untracked(|r| r.rows().get(i).map(|h| h.target)) else {
            return;
        };
        shown().set(JumpOpen(false));
        open(s, target);
    };

    let keys = |when| KeyOptions {
        when,
        repeat: true,
        ..KeyOptions::default()
    };
    on_key_with("down", keys(When::Always), move |_| {
        let n = count.get_untracked();
        if n > 0 {
            lit.update(|i| *i = (*i + 1) % n);
        }
    });
    on_key_with("up", keys(When::Always), move |_| {
        let n = count.get_untracked();
        if n > 0 {
            lit.update(|i| *i = (*i + n - 1) % n);
        }
    });
    on_key_with(
        "enter",
        KeyOptions {
            when: When::Always,
            ..KeyOptions::default()
        },
        move |_| choose(lit.get_untracked()),
    );
    on_key_with(
        "escape",
        KeyOptions {
            when: When::Always,
            ..KeyOptions::default()
        },
        move |_| dismiss(),
    );

    overlay()
        .absolute()
        .inset_0()
        .child(
            pressable()
                .absolute()
                .inset_0()
                .bind_motion(&dim_in)
                .bg(move || c::overlay())
                .accessibility_label("Close jump to")
                .on_press(dismiss),
        )
        .child(
            view()
                .absolute()
                .inset_0()
                .items_center()
                .pt(px(TOP))
                .px_6()
                // Presses beside the card reach the dim under it.
                .pointer_events(PointerEvents::BoxNone)
                .child(
                    view()
                        .w(px(WIDTH))
                        .max_w(relative(1.0))
                        .bind_motion(&card_in)
                        .style_when(|| true, floating::pane_style)
                        .shadow_xl()
                        .label("Jump to")
                        .child(field(query))
                        .child(separator())
                        .child(list(found, lit, pointer, choose))
                        .child(separator())
                        .child(footer()),
                ),
        )
        .into_element()
}

/// The search field: large, borderless, with the focus from the start.
fn field(query: Signal<String>) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .h(px(FIELD_H))
        .px_4()
        .child(icon(Icon::Search, 18.0).text_color(move || c::muted_fg()))
        .child(
            nsh_base::input()
                .flex_1()
                .auto_focus(true)
                .value(move || query.get())
                .on_change(move |t: &str| query.set(t.to_owned()))
                .placeholder("Search chats, people and actions")
                .label("Jump to a chat, person or action")
                .placeholder_color(|| color(c::muted_fg()))
                .caret_color(|| color(c::primary()))
                .text_size(px(font::TITLE))
                .text_color(move || c::foreground()),
        )
}

/// The results: a calm note when nothing matched, then the sections.
fn list(
    found: Memo<Results>,
    lit: Signal<usize>,
    pointer: Stored<Option<(f32, f32)>>,
    choose: impl Fn(usize) + Copy + 'static,
) -> Scroll {
    scroll().max_h(px(LIST_MAX_H)).child(dynamic(move || {
        let r = found.get();
        untracked(move || {
            let mut at = 0;
            let mut body = view().px(px(6.0)).py(px(6.0));
            if let Some(note) = r.note.clone() {
                body = body.child(
                    text(note)
                        .px(px(10.0))
                        .pt_2()
                        .pb_2()
                        .text_size(px(font::body()))
                        .text_color(move || c::muted_fg()),
                );
            }
            for section in &r.sections {
                if !section.title.is_empty() {
                    body = body.child(label(section.title));
                }
                for hit in &section.hits {
                    body = body.child(row(hit.clone(), at, lit, pointer, choose));
                    at += 1;
                }
            }
            body.into_element()
        })
    }))
}

/// A section's label: small, muted, sitting on the rows it names.
fn label(title: &'static str) -> View {
    view()
        .h(px(LABEL_H))
        .justify_end()
        .px(px(10.0))
        .pb_1()
        .role(AccessibilityRole::Header)
        .child(
            text(title)
                .text_size(px(font::CAPTION))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || c::muted_fg()),
        )
}

/// One result: its picture or glyph, its title with the match in the
/// semibold weight, and the muted hint. The highlight is a calm fill that
/// moves with the keys at once (no fade to lag behind them); the pointer
/// moves it too, but only when the pointer itself moves, so a list that
/// scrolls under a resting pointer keeps the keys' choice.
fn row(
    hit: Hit,
    at: usize,
    lit: Signal<usize>,
    pointer: Stored<Option<(f32, f32)>>,
    choose: impl Fn(usize) + 'static,
) -> AnyElement {
    let on = move || lit.get() == at;
    let me = element_ref();
    // The highlighted row stays in view as the keys move past the edge.
    effect(move || {
        if on() {
            me.reveal(
                RevealOptions::default()
                    .vertical(ScrollAlignment::Nearest)
                    .animated(false),
            );
        }
    });
    let (before, matched, after) = hit.parts();
    let title = text(before)
        .child(text(matched).font_weight(FontWeight::SEMIBOLD))
        .child(text(after))
        .flex_1()
        .number_of_lines(1)
        .text_size(px(font::body()))
        .text_color(move || c::foreground());
    pressable()
        .element_ref(&me)
        .accessibility_label(hit.spoken())
        .selected(on)
        .on_press(move || choose(at))
        .on_mouse_move(move |e: &MouseEvent| {
            let p = (e.page_position.x.0, e.page_position.y.0);
            if pointer.replace(Some(p)) != Some(p) && !on() {
                lit.set(at);
            }
        })
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .h(px(ROW_H))
        .px(px(10.0))
        .rounded(move || radius::control())
        .bg(move || {
            if on() {
                c::accent()
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .child(lead(&hit.lead))
        .child(title)
        .child((!hit.hint.is_empty()).then(|| {
            text(hit.hint.clone())
                .text_size(px(font::SMALL))
                .text_color(move || c::muted_fg())
                .accessibility_elements_hidden(true)
        }))
        .into_element()
}

/// The 24pt lane that leads a row: a chat's picture, a person's avatar, or
/// a 16pt glyph centred in it, so every title starts on one line.
fn lead(lead: &Lead) -> View {
    let lane = view()
        .size(px(LEAD))
        .items_center()
        .justify_center()
        .accessibility_elements_hidden(true);
    match lead {
        Lead::Chat(id) => match store().chat(*id) {
            Some(chat) => lane.child(features::groups::chat_avatar_for(&chat, LEAD)),
            None => lane,
        },
        Lead::Person { name, seed } => lane.child(avatar(name.clone(), *seed, LEAD)),
        Lead::Glyph(glyph) => lane
            .text_color(move || c::muted_fg())
            .child(icon(*glyph, GLYPH)),
    }
}

/// The keys, as small caps: ↑↓ to move, ↵ to open, esc to close. Escape
/// closes at once, query or not, so the hint never changes. Hidden from
/// screen readers, which hear the field's and the rows' own roles.
fn footer() -> View {
    let esc = if os() == Os::MacOs { "esc" } else { "Esc" };
    let hint = |caps: Vec<&'static str>, what: &'static str| {
        view()
            .flex_row()
            .items_center()
            .gap_1()
            .children(caps.into_iter().map(key_cap))
            .child(
                text(what)
                    .pl_1()
                    .text_size(px(font::CAPTION))
                    .text_color(move || c::muted_fg()),
            )
    };
    view()
        .flex_row()
        .items_center()
        .gap_4()
        .h(px(ROW_H))
        .px_4()
        .accessibility_elements_hidden(true)
        .child(hint(vec!["↑", "↓"], "to move"))
        .child(hint(vec!["↵"], "to open"))
        .child(hint(vec![esc], "to close"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(s: &str) -> Vec<char> {
        folded_query(s)
    }

    fn chat(id: ChatId, title: &str) -> Candidate {
        Candidate {
            target: Target::Chat(id),
            title: title.into(),
            keywords: String::new(),
            hint: "Chat".into(),
            lead: Lead::Chat(id),
        }
    }

    fn act(act: Act, title: &str, keywords: &str) -> Candidate {
        Candidate {
            target: Target::Action(act),
            title: title.into(),
            keywords: keywords.into(),
            hint: String::new(),
            lead: Lead::Glyph(Icon::Plus),
        }
    }

    fn sample() -> Pool {
        Pool {
            chats: vec![
                chat(1, "Ana Popescu"),
                chat(2, "Familia 🏡"),
                chat(3, "Design team"),
                chat(4, "Ioana Ștefănescu"),
                chat(5, "Banana bread club"),
            ],
            people: vec![Candidate {
                target: Target::Person(9),
                title: "Andrei Stan".into(),
                keywords: "@andrei".into(),
                hint: "Person".into(),
                lead: Lead::Person {
                    name: "Andrei Stan".into(),
                    seed: 9,
                },
            }],
            actions: vec![
                act(Act::NewChat, "New chat", "message"),
                act(Act::NewGroup, "New group", "create"),
                act(
                    Act::Settings("Privacy"),
                    "Settings: Privacy",
                    "settings blocked last seen",
                ),
                act(
                    Act::Settings("Appearance"),
                    "Settings: Appearance",
                    "settings theme dark",
                ),
            ],
            recent: 3,
            suggested: vec![0, 1],
        }
    }

    #[test]
    fn folding_drops_case_and_diacritics() {
        assert_eq!(fold("Ștefănescu"), fold("stefanescu"));
        assert_eq!(fold("ÎNȚELEGERE"), fold("intelegere"));
        assert_eq!(fold("Straße").len(), "Straße".chars().count());
    }

    #[test]
    fn ranks_whole_then_start_then_word_then_inside() {
        assert_eq!(score("Ana", "", &q("ana")).map(|s| s.0), Some(0));
        assert_eq!(score("Ana Popescu", "", &q("ana")).map(|s| s.0), Some(1));
        assert_eq!(score("Mihai Ana", "", &q("ana")).map(|s| s.0), Some(2));
        assert_eq!(score("Banana", "", &q("ana")).map(|s| s.0), Some(3));
        assert_eq!(
            score("Settings: Appearance", "theme dark", &q("dark")).map(|s| s.0),
            Some(4)
        );
        assert_eq!(score("Ana", "", &q("zq")), None);
        assert_eq!(score("Ana", "", &q("   ")), None);
    }

    #[test]
    fn the_cut_falls_on_the_title_in_chars() {
        let (rank, cut) = score("Ioana Ștefănescu", "", &q("STEF")).unwrap();
        assert_eq!((rank, cut), (2, Some((6, 4))));
        let hit = Hit {
            target: Target::Chat(4),
            title: "Ioana Ștefănescu".into(),
            hint: String::new(),
            lead: Lead::Chat(4),
            cut,
        };
        assert_eq!(
            hit.parts(),
            ("Ioana ".into(), "Ștef".into(), "ănescu".into())
        );
    }

    #[test]
    fn several_words_find_across_title_and_keywords() {
        let (rank, cut) =
            score("Settings: Privacy", "blocked last seen", &q("set  blocked")).unwrap();
        assert_eq!(rank, 4);
        assert_eq!(cut, Some((0, 3)));
        assert_eq!(score("Settings: Privacy", "blocked", &q("set zz")), None);
    }

    #[test]
    fn empty_query_shows_recent_and_suggestions() {
        let r = results("", &sample());
        let titles: Vec<&str> = r.sections.iter().map(|s| s.title).collect();
        assert_eq!(titles, ["Recent", "Actions"]);
        assert_eq!(r.sections[0].hits.len(), 3);
        assert_eq!(r.sections[1].hits[0].title, "New chat");
        assert_eq!(r.note, None);
    }

    #[test]
    fn best_matches_first_within_and_across_sections() {
        let r = results("an", &sample());
        let chats = &r.sections[0];
        assert_eq!(chats.title, "Chats");
        // "Ana Popescu" starts with it; "Banana bread club" has it inside a word.
        assert_eq!(chats.hits[0].title, "Ana Popescu");
        assert_eq!(chats.hits.last().unwrap().title, "Banana bread club");
        assert!(r.sections.iter().any(|s| s.title == "People"));

        let r = results("new", &sample());
        assert_eq!(r.sections[0].title, "Actions");
        assert_eq!(r.rows()[0].target, Target::Action(Act::NewChat));
    }

    #[test]
    fn people_match_by_username() {
        let r = results("@andr", &sample());
        assert_eq!(r.rows()[0].target, Target::Person(9));
        assert_eq!(r.rows()[0].cut, None);
    }

    #[test]
    fn nothing_found_offers_a_new_chat() {
        let r = results("  zq   x ", &sample());
        assert_eq!(r.note.as_deref(), Some("No matches for “zq x”"));
        let rows = r.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].target, Target::Action(Act::NewChatWith));
        assert_eq!(rows[0].title, "Start a new chat with “zq x”");
    }

    #[test]
    fn sections_are_capped() {
        let mut pool = sample();
        pool.chats = (0..20).map(|i| chat(i, &format!("Team {i}"))).collect();
        let r = results("team", &pool);
        assert_eq!(r.sections[0].hits.len(), MAX_CHATS);
    }
}

//! Seed data for the mock store: people, chats of every kind, every
//! message type, calls and devices. Names cover Romanian diacritics and a
//! right-to-left script, so text handling is exercised from the start.

use std::collections::HashMap;

use crate::ui::Tick;

use super::model::*;

pub struct Seed {
    pub me: Profile,
    pub contacts: Vec<Contact>,
    pub chats: Vec<Chat>,
    pub messages: HashMap<ChatId, Vec<Message>>,
    pub calls: Vec<CallRecord>,
    pub call_links: Vec<CallLink>,
    pub devices: Vec<Device>,
    pub settings: Settings,
    pub session: Session,
}

/// A safety number for a contact id: 12 groups of 5 digits.
fn safety(id: u64) -> String {
    let mut x = id.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    (0..12)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            format!("{:05}", x % 100_000)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn person(id: ContactId, name: &str, username: &str, about: &str, presence: Presence) -> Contact {
    Contact {
        id,
        name: name.into(),
        username: username.into(),
        about: about.into(),
        status_line: None,
        presence,
        verified: false,
        key_changed: false,
        safety_number: safety(id),
        blocked: false,
        nudges_off: false,
    }
}

/// A new chat with defaults.
pub fn chat(id: ChatId, kind: ChatKind, title: &str) -> Chat {
    let kind_seed = match kind {
        ChatKind::Direct(c) | ChatKind::WhatsApp(c) => Some(c),
        _ => None,
    };
    Chat {
        id,
        kind,
        title: title.into(),
        about: String::new(),
        members: Vec::new(),
        permissions: GroupPermissions::default(),
        announcement_only: false,
        community: None,
        pinned: false,
        muted: false,
        muted_until: 0,
        archived: false,
        favourite: false,
        locked: false,
        marked_unread: false,
        unread: 0,
        mentions: 0,
        disappearing: None,
        draft: String::new(),
        typing: None,
        request: false,
        join_requests: Vec::new(),
        invite_link: format!("https://songbird.chat/g/{:x}", id * 7919),
        lists: Vec::new(),
        // A person's picture follows the person: a direct chat takes its
        // contact's seed, so the photo matches in every list and header.
        seed: match kind_seed {
            Some(contact) => contact,
            None => id,
        },
        theme: ChatTheme::default(),
    }
}

/// A canned answer for the mock contact to "type".
pub fn reply_for(seed: u64) -> &'static str {
    const REPLIES: [&str; 8] = [
        "Sounds good 👍",
        "Haha, yes!",
        "On my way",
        "Let me check and get back to you",
        "Perfect, see you then",
        "Thanks! ❤️",
        "Can we talk later? In a meeting",
        "Sure, send it over",
    ];
    REPLIES[(seed % REPLIES.len() as u64) as usize]
}

struct Thread {
    chat: ChatId,
    next: MsgId,
    out: Vec<Message>,
}

impl Thread {
    fn new(chat: ChatId) -> Self {
        Self {
            chat,
            next: chat * 1000,
            out: Vec::new(),
        }
    }
    fn add(&mut self, from: Option<ContactId>, at: Minutes, body: Body) -> &mut Message {
        self.next += 1;
        self.out.push(Message {
            id: self.next,
            chat: self.chat,
            from,
            body,
            at,
            status: Tick::Read,
            reply_to: None,
            reactions: Vec::new(),
            edited: false,
            starred: false,
            pinned: false,
            forwarded: false,
            receipts: Vec::new(),
            album: None,
        });
        self.out.last_mut().unwrap()
    }
    fn text(&mut self, from: Option<ContactId>, at: Minutes, t: &str) -> &mut Message {
        self.add(from, at, Body::Text(t.into()))
    }
    fn last_id(&self) -> MsgId {
        self.next
    }
}

fn react(m: &mut Message, emoji: &str, by: Vec<Option<ContactId>>) {
    m.reactions.push(Reaction {
        emoji: emoji.into(),
        by,
    });
}

/// A real account's starting point: nobody, no chats, no calls. Only the
/// defaults of settings and session come from [`seed`]; everything else
/// arrives from the server and app-core (the owner's rule: no mock data).
pub fn empty() -> Seed {
    let base = seed();
    Seed {
        me: Profile {
            name: String::new(),
            username: String::new(),
            about: String::new(),
            status_line: None,
            presence: Presence::Online,
            seed: 42,
        },
        contacts: Vec::new(),
        chats: Vec::new(),
        messages: HashMap::new(),
        calls: Vec::new(),
        call_links: Vec::new(),
        devices: Vec::new(),
        // The demo's choices, minus what only the server or the user can say (D-0183: a real
        // account showed the demo's PIN as on, WhatsApp as linked and invites it never had).
        settings: Settings {
            two_step_pin: false,
            passkey: false,
            whatsapp_linked: false,
            backup_on: false,
            backup_last: None,
            backup_size: String::new(),
            ..base.settings
        },
        session: Session {
            recovery_key: String::new(),
            invites_left: 0,
            invites_used: 0,
            ..base.session
        },
    }
}

/// The demo data, for screenshot fixtures only (see `store::demo`).
pub fn seed() -> Seed {
    // ---------------------------------------------------------- people
    let mut contacts = vec![
        person(
            1,
            "Ana Popescu",
            "ana.popescu",
            "Architect · Cluj",
            Presence::Online,
        ),
        person(
            2,
            "Mihai Ionescu",
            "mihai",
            "🚴 weekends",
            Presence::LastSeen(Some(-35)),
        ),
        person(
            3,
            "Ioana Ștefănescu",
            "ioana.st",
            "Books, sunsets and long walks 📚",
            Presence::Away,
        ),
        person(
            4,
            "Andrei Dumitrescu",
            "andrei_d",
            "Product @ Bitdefender",
            Presence::Busy,
        ),
        person(
            5,
            "Elena Rădulescu",
            "elena.r",
            "Mama de doi 🌻",
            Presence::LastSeen(Some(-60 * 26)),
        ),
        person(
            6,
            "Sofia Martínez",
            "sofia.mtz",
            "Barcelona → Bucharest",
            Presence::Online,
        ),
        person(
            7,
            "Lukas Weber",
            "lukasw",
            "Coffee first",
            Presence::LastSeen(None),
        ),
        person(
            8,
            "Amélie Durand",
            "amelie",
            "Photographe",
            Presence::LastSeen(Some(-180)),
        ),
        person(
            9,
            "يوسف الحسن",
            "yusuf.h",
            "أحب السفر والقراءة",
            Presence::Online,
        ),
        person(
            10,
            "Priya Natarajan",
            "priya.n",
            "Building things",
            Presence::LastSeen(Some(-12)),
        ),
        person(
            11,
            "Tomás Silva",
            "tomas",
            "⚽",
            Presence::LastSeen(Some(-60 * 50)),
        ),
        person(12, "Mama", "mama.radu", "", Presence::LastSeen(Some(-8))),
        person(
            13,
            "Vlad Georgescu",
            "vladg",
            "Running a marathon in May",
            Presence::Online,
        ),
        person(
            14,
            "Cristina Pavel",
            "cristina.p",
            "",
            Presence::LastSeen(Some(-300)),
        ),
        person(
            15,
            "Dr. Marin – Clinic",
            "clinica.marin",
            "Appointments Mon–Fri",
            Presence::LastSeen(None),
        ),
        person(
            16,
            "Unknown sender",
            "deal.hunter.88",
            "",
            Presence::LastSeen(None),
        ),
    ];
    contacts[0].verified = true;
    contacts[0].status_line = Some("Designing a library 📚".into());
    contacts[3].status_line = Some("In a meeting until 3".into());
    contacts[5].key_changed = true;
    contacts[11].verified = true;

    // ---------------------------------------------------------- chats
    let mut chats = Vec::new();
    let mut messages: HashMap<ChatId, Vec<Message>> = HashMap::new();
    let mut put = |chats: &mut Vec<Chat>, c: Chat, t: Thread| {
        messages.insert(c.id, t.out);
        chats.push(c);
    };

    // 1: Ana — every message type, pinned, verified.
    {
        let mut c = chat(1, ChatKind::Direct(1), "Ana Popescu");
        c.pinned = true;
        c.unread = 2;
        c.favourite = true;
        c.lists = vec!["Work".into()];
        let mut t = Thread::new(1);
        t.add(
            None,
            -60 * 24 * 3,
            Body::System(
                "Only you and Ana can read this chat. Messages and calls are end-to-end encrypted."
                    .into(),
            ),
        );
        t.text(
            Some(1),
            -60 * 24 * 2 - 30,
            "Hi! Did you get the floor plans?",
        );
        t.add(
            None,
            -60 * 24 * 2 - 28,
            Body::File {
                name: "Floor plan v3.pdf".into(),
                size: "2.4 MB".into(),
                kind: "PDF".into(),
            },
        );
        t.text(
            Some(1),
            -60 * 24 - 200,
            "The light in the reading room is perfect now",
        );
        t.add(
            Some(1),
            -60 * 24 - 199,
            Body::Image {
                caption: "Reading room, north side".into(),
                alt: "A bright reading room with tall windows and wooden shelves".into(),
                w: 4,
                h: 3,
                hd: true,
            },
        );
        let img = t.last_id();
        react(t.out.last_mut().unwrap(), "😍", vec![None]);
        t.add(
            Some(1),
            -60 * 24 - 198,
            Body::Image {
                caption: String::new(),
                alt: "Coffee on the drafting table by the window".into(),
                w: 3,
                h: 4,
                hd: false,
            },
        );
        t.text(None, -60 * 24 - 190, "Wow. That's beautiful 😍")
            .reply_to = Some(img);
        t.add(
            Some(1),
            -60 * 24 - 120,
            Body::Voice {
                secs: 42,
                played: true,
            },
        );
        t.add(
            None,
            -60 * 24 - 100,
            Body::Voice {
                secs: 17,
                played: false,
            },
        )
        .status = Tick::Played;
        t.add(
            Some(1),
            -300,
            Body::Poll {
                question: "Team lunch on Friday?".into(),
                options: vec![
                    ("Pizza".into(), 3),
                    ("Sushi".into(), 5),
                    ("Salad bar".into(), 1),
                ],
                mine: vec![1],
                multiple: false,
                closed: false,
            },
        );
        t.add(
            Some(1),
            -240,
            Body::Event {
                title: "Site visit".into(),
                when: "Thu 19 Nov, 10:00".into(),
                place: "Strada Memorandumului 21, Cluj".into(),
                going: 3,
                mine: None,
            },
        );
        t.add(
            None,
            -200,
            Body::Location {
                label: "Piața Unirii, Cluj-Napoca".into(),
                live: false,
                at: Some((46.7697, 23.5896)),
            },
        );
        t.add(
            Some(1),
            -150,
            Body::Contact {
                name: "Mihai Ionescu".into(),
                username: "mihai".into(),
            },
        );
        let m = t.add(
            None,
            -90,
            Body::Link {
                text: "This is the one I meant https://songbird.chat/blog/privacy".into(),
                url: "https://songbird.chat/blog/privacy".into(),
                title: "What Songbird can see".into(),
                site: "songbird.chat".into(),
            },
        );
        m.edited = true;
        t.add(
            Some(1),
            -60,
            Body::Sticker {
                emoji: "🦉".into(),
                pack: "Night owls".into(),
            },
        );
        t.add(
            Some(1),
            -40,
            Body::ViewOnce {
                photo: true,
                opened: false,
            },
        );
        let m = t.text(None, -20, "Can you send me the updated budget before 3?");
        m.status = Tick::Read;
        m.starred = true;
        t.text(Some(1), -6, "Yes! Finishing it now");
        t.text(Some(1), -5, "Also — are we still on for the call later? 🙂");
        put(&mut chats, c, t);
    }

    // 2: Family group, pinned, with mentions.
    {
        let mut c = chat(2, ChatKind::Group, "Familia 🏡");
        c.theme = ChatTheme::Rose;
        c.pinned = true;
        c.unread = 5;
        c.mentions = 1;
        c.lists = vec!["Family".into()];
        c.about = "Weekend plans, photos and recipes".into();
        c.members = vec![
            Member {
                contact: None,
                role: Role::Owner,
            },
            Member {
                contact: Some(12),
                role: Role::Admin,
            },
            Member {
                contact: Some(5),
                role: Role::Member,
            },
            Member {
                contact: Some(13),
                role: Role::Member,
            },
            Member {
                contact: Some(14),
                role: Role::Member,
            },
        ];
        let mut t = Thread::new(2);
        t.add(
            None,
            -60 * 24 * 10,
            Body::System("You created the group \"Familia 🏡\"".into()),
        );
        t.text(
            Some(12),
            -60 * 24 - 60,
            "Duminică facem sarmale. Cine vine?",
        );
        let m = t.text(Some(5), -60 * 24 - 50, "Noi venim! 🙋‍♀️");
        react(m, "❤️", vec![Some(12), None]);
        t.add(
            Some(13),
            -60 * 3,
            Body::Image {
                caption: "".into(),
                alt: "Two children building a snowman".into(),
                w: 3,
                h: 4,
                hd: false,
            },
        );
        t.add(
            Some(13),
            -60 * 3 + 1,
            Body::Image {
                caption: "First snow!".into(),
                alt: "A snow-covered garden at dawn".into(),
                w: 4,
                h: 3,
                hd: false,
            },
        );
        t.add(
            Some(12),
            -45,
            Body::Voice {
                secs: 64,
                played: false,
            },
        );
        t.text(Some(14), -30, "@Radu can you bring the wine? 🍷");
        t.text(Some(12), -12, "Și desert, te rog!");
        t.text(Some(5), -9, "Mihai says hi 👋");
        put(&mut chats, c, t);
    }

    // 3: Mihai — read receipts, failed message, typing.
    {
        let mut c = chat(3, ChatKind::Direct(2), "Mihai Ionescu");
        c.lists = vec!["Friends".into()];
        let mut t = Thread::new(3);
        t.text(Some(2), -60 * 5, "Ride on Saturday? 🚴");
        t.text(None, -60 * 5 + 3, "If it doesn't rain!").status = Tick::Read;
        t.text(Some(2), -60 * 4, "Forecast says sun ☀️");
        t.text(None, -15, "Great, 9:00 at the park gate?").status = Tick::Failed;
        put(&mut chats, c, t);
    }

    // 4: Work group with join requests, admin.
    {
        let mut c = chat(4, ChatKind::Group, "Design team");
        c.theme = ChatTheme::Slate;
        c.unread = 12;
        c.muted = true;
        c.lists = vec!["Work".into()];
        c.about = "Weekly crits on Thursdays. Be kind.".into();
        c.join_requests = vec![10, 11];
        c.members = vec![
            Member {
                contact: Some(1),
                role: Role::Owner,
            },
            Member {
                contact: None,
                role: Role::Admin,
            },
            Member {
                contact: Some(4),
                role: Role::Member,
            },
            Member {
                contact: Some(6),
                role: Role::Member,
            },
            Member {
                contact: Some(8),
                role: Role::Member,
            },
            Member {
                contact: Some(10),
                role: Role::Member,
            },
        ];
        let mut t = Thread::new(4);
        t.text(
            Some(4),
            -60 * 2,
            "Uploaded the new onboarding flows to the board",
        );
        t.add(
            Some(8),
            -60 * 2 + 5,
            Body::Image {
                caption: "Moodboard v2".into(),
                alt: "A grid of muted green and sand colour swatches".into(),
                w: 1,
                h: 1,
                hd: true,
            },
        );
        let m = t.text(
            Some(6),
            -50,
            "Love the calmer palette. The ticks are much clearer now",
        );
        react(m, "👍", vec![Some(4), Some(1)]);
        react(m, "🔥", vec![Some(8)]);
        let m = t.text(None, -40, "Crit moved to 4pm, see you there");
        m.status = Tick::Delivered;
        m.receipts = vec![
            (1, Some(-39), Some(-30)),
            (4, Some(-39), None),
            (6, Some(-38), Some(-20)),
            (8, None, None),
        ];
        t.text(Some(4), -3, "Who has the Figma license?");
        put(&mut chats, c, t);
    }

    // 5: Sofia — key changed notice.
    {
        let c = chat(5, ChatKind::Direct(6), "Sofia Martínez");
        let mut t = Thread::new(5);
        t.text(Some(6), -60 * 24 * 2, "¡Hola! Landed in Bucharest");
        t.add(None, -60 * 24 + 10, Body::System("Sofia Martínez's safety number changed. This usually means they reinstalled Songbird or got a new phone. Tap to verify.".into()));
        t.text(
            Some(6),
            -60 * 24 + 20,
            "New phone! Lost the old one at the airport 🙈",
        );
        t.add(Some(6), -60 * 24 + 21, Body::Nudge);
        put(&mut chats, c, t);
    }

    // 6: Channel.
    {
        let mut c = chat(
            6,
            ChatKind::Channel {
                followers: 18_420,
                admin: false,
            },
            "Songbird News",
        );
        c.about = "Product updates from the Songbird team. Followers can't see each other.".into();
        let mut t = Thread::new(6);
        t.add(None, -60 * 24 * 4, Body::Image { caption: "Songbird 1.0 beta is here. Invite-only for now — thank you for helping us build a calmer messenger.".into(), alt: "The Songbird logo, a small bird on a branch".into(), w: 16, h: 9, hd: true }).from = Some(99);
        t.add(None, -60 * 24, Body::Text("New: chat lists. Group your chats into Family, Work or anything you like. Swipe right on Chats to try it.".into())).from = Some(99);
        put(&mut chats, c, t);
    }

    // 7: Community + its groups.
    {
        let mut c = chat(7, ChatKind::Community, "Cluj Climbers");
        c.about = "Bouldering and sport climbing around Cluj. Be safe, be kind.".into();
        c.members = (1..=8)
            .map(|i| Member {
                contact: Some(i),
                role: if i == 1 { Role::Owner } else { Role::Member },
            })
            .chain(std::iter::once(Member {
                contact: None,
                role: Role::Admin,
            }))
            .collect();
        put(&mut chats, c, Thread::new(7));
        let mut a = chat(8, ChatKind::Group, "Announcements");
        a.community = Some(7);
        a.announcement_only = true;
        a.members = vec![
            Member {
                contact: None,
                role: Role::Admin,
            },
            Member {
                contact: Some(1),
                role: Role::Owner,
            },
        ];
        let mut t = Thread::new(8);
        t.text(
            Some(1),
            -60 * 6,
            "Gym is closed on Sunday for route setting 🧗",
        );
        put(&mut chats, a, t);
        let mut g = chat(9, ChatKind::Group, "Weekend trips");
        g.theme = ChatTheme::Moss;
        g.community = Some(7);
        g.unread = 3;
        g.members = vec![
            Member {
                contact: None,
                role: Role::Member,
            },
            Member {
                contact: Some(13),
                role: Role::Admin,
            },
            Member {
                contact: Some(7),
                role: Role::Member,
            },
        ];
        let mut t = Thread::new(9);
        t.text(Some(13), -60 * 8, "Turzii Gorge on Saturday, who's in?");
        t.add(
            Some(7),
            -60 * 7,
            Body::Poll {
                question: "Leave at".into(),
                options: vec![("7:00".into(), 2), ("8:30".into(), 4)],
                mine: Vec::new(),
                multiple: false,
                closed: false,
            },
        );
        put(&mut chats, g, t);
    }

    // 10: Yusuf (RTL).
    {
        let c = chat(10, ChatKind::Direct(9), "يوسف الحسن");
        let mut t = Thread::new(10);
        t.text(Some(9), -60 * 9, "صباح الخير! هل وصلت الطرود؟");
        t.text(
            None,
            -60 * 9 + 4,
            "Good morning! Yes, everything arrived 🙏",
        );
        put(&mut chats, c, t);
    }

    // 11: Mama — disappearing messages on, voice notes.
    {
        let mut c = chat(11, ChatKind::Direct(12), "Mama");
        c.disappearing = Some(Disappear::Week);
        c.favourite = true;
        c.lists = vec!["Family".into()];
        let mut t = Thread::new(11);
        t.add(None, -60 * 24 * 6, Body::System("Disappearing messages are on. New messages will disappear from this chat 7 days after they're sent.".into()));
        t.add(
            Some(12),
            -60 * 2,
            Body::Voice {
                secs: 95,
                played: false,
            },
        );
        t.add(
            Some(12),
            -60 * 2 + 1,
            Body::Voice {
                secs: 23,
                played: false,
            },
        );
        t.add(
            Some(12),
            -60 * 2 + 2,
            Body::Voice {
                secs: 51,
                played: false,
            },
        );
        put(&mut chats, c, t);
    }

    // 12: Lukas — call history.
    {
        let c = chat(12, ChatKind::Direct(7), "Lukas Weber");
        let mut t = Thread::new(12);
        t.add(
            Some(7),
            -60 * 24 * 3,
            Body::Call {
                video: true,
                missed: false,
                secs: 1260,
            },
        );
        t.add(
            Some(7),
            -60 * 26,
            Body::Call {
                video: false,
                missed: true,
                secs: 0,
            },
        );
        t.text(Some(7), -60 * 26 + 1, "Call me when you're free");
        put(&mut chats, c, t);
    }

    // 13: Andrei — draft.
    {
        let mut c = chat(13, ChatKind::Direct(4), "Andrei Dumitrescu");
        c.draft = "Let's sync on the roadmap tomorrow".into();
        let mut t = Thread::new(13);
        t.text(Some(4), -60 * 30, "Sent you the Q1 doc");
        t.add(
            Some(4),
            -60 * 30 + 1,
            Body::File {
                name: "Q1 roadmap.xlsx".into(),
                size: "184 KB".into(),
                kind: "Spreadsheet".into(),
            },
        );
        put(&mut chats, c, t);
    }

    // 14: WhatsApp chat (A15).
    {
        let mut c = chat(14, ChatKind::WhatsApp(11), "Tomás Silva");
        c.unread = 1;
        let mut t = Thread::new(14);
        t.text(Some(11), -60 * 3, "Bom dia! Game tonight at 8?");
        put(&mut chats, c, t);
    }

    // 15: Message request.
    {
        let mut c = chat(15, ChatKind::Direct(16), "deal.hunter.88");
        c.request = true;
        let mut t = Thread::new(15);
        t.text(
            Some(16),
            -60 * 4,
            "Hi! You won a free phone 📱 claim it here: https://free-phone.example",
        );
        put(&mut chats, c, t);
    }

    // 16: Archived — the 10,000-message chat for the performance budget.
    {
        let mut c = chat(16, ChatKind::Direct(3), "Ioana Ștefănescu");
        c.archived = true;
        let mut t = Thread::new(16);
        let lines = [
            "Did you see the sunset today?",
            "Haha yes 😂",
            "I'll call you after work",
            "Sending the photos now",
            "Ok!",
            "Ce mai faci? Totul bine?",
            "Running 10 minutes late, sorry 🙏",
            "That book was amazing, thank you for the recommendation",
        ];
        for i in 0..10_000_i64 {
            let from = if i % 3 == 0 { None } else { Some(3) };
            t.text(
                from,
                -60 * 24 * 60 + i * 8,
                lines[(i as usize * 7 + 3) % lines.len()],
            );
        }
        put(&mut chats, c, t);
    }

    // 17: Clinic (Away).
    {
        let mut c = chat(17, ChatKind::Direct(15), "Dr. Marin – Clinic");
        c.archived = true;
        let mut t = Thread::new(17);
        t.text(
            Some(15),
            -60 * 24 * 9,
            "Your appointment is confirmed for Monday at 11:30.",
        );
        put(&mut chats, c, t);
    }

    // 18: Priya.
    {
        let c = chat(18, ChatKind::Direct(10), "Priya Natarajan");
        let mut t = Thread::new(18);
        t.text(None, -60 * 24 * 5, "Thanks for the intro!").status = Tick::Read;
        t.add(
            Some(10),
            -60 * 24 * 5 + 2,
            Body::Sticker {
                emoji: "🙌".into(),
                pack: "Cheers".into(),
            },
        );
        put(&mut chats, c, t);
    }

    // ---------------------------------------------------------- calls
    let call = |id, chat, title: &str, seed, video, group, direction, at, secs| CallRecord {
        id,
        chat,
        title: title.into(),
        seed,
        video,
        group,
        direction,
        at,
        secs,
    };
    let calls = vec![
        call(
            1,
            1,
            "Ana Popescu",
            1,
            true,
            false,
            CallDirection::Outgoing,
            -60 * 2,
            754,
        ),
        call(
            2,
            12,
            "Lukas Weber",
            12,
            false,
            false,
            CallDirection::Missed,
            -60 * 26,
            0,
        ),
        call(
            3,
            2,
            "Familia 🏡",
            2,
            true,
            true,
            CallDirection::Incoming,
            -60 * 24 - 300,
            2_410,
        ),
        call(
            4,
            3,
            "Mihai Ionescu",
            3,
            false,
            false,
            CallDirection::Incoming,
            -60 * 24 * 2,
            312,
        ),
        call(
            5,
            12,
            "Lukas Weber",
            12,
            true,
            false,
            CallDirection::Incoming,
            -60 * 24 * 3,
            1_260,
        ),
        call(
            6,
            4,
            "Design team",
            4,
            true,
            true,
            CallDirection::Outgoing,
            -60 * 24 * 4,
            3_190,
        ),
        call(
            7,
            5,
            "Sofia Martínez",
            5,
            false,
            false,
            CallDirection::Missed,
            -60 * 24 * 6,
            0,
        ),
    ];
    let call_links = vec![CallLink {
        id: 1,
        name: "Weekly design crit".into(),
        url: "https://songbird.chat/call/7f3a9c".into(),
        lobby: true,
    }];

    let devices = vec![
        Device {
            id: 1,
            name: "iPhone 15".into(),
            kind: DeviceKind::Phone,
            last_active: "This device".into(),
            this_device: true,
        },
        Device {
            id: 2,
            name: "MacBook Air".into(),
            kind: DeviceKind::Desktop,
            last_active: "Active now".into(),
            this_device: false,
        },
        Device {
            id: 3,
            name: "Windows PC (Office)".into(),
            kind: DeviceKind::Desktop,
            last_active: "Last active 3 days ago".into(),
            this_device: false,
        },
    ];

    let settings = Settings {
        last_seen: Audience::Contacts,
        online: Audience::Contacts,
        photo: Audience::Contacts,
        about: Audience::Contacts,
        status: Audience::Contacts,
        groups_add: Audience::Contacts,
        read_receipts: true,
        typing_indicators: true,
        discoverable: true,
        message_requests: true,
        inactivity_months: 12,
        default_timer: None,
        app_lock: false,
        block_screenshots: false,
        hide_in_switcher: true,
        link_previews: true,
        notify_messages: true,
        notify_groups: true,
        notify_calls: true,
        reactions_messages: true,
        reactions_groups: false,
        notify_sound: true,
        hide_notification_content: false,
        theme: ThemeChoice::System,
        chat_wallpaper: 0,
        animations: true,
        default_reactions: ["👍", "❤️", "😂", "😮", "😢", "🙏"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        enter_sends: true,
        keep_archived: true,
        save_to_photos: false,
        nudges: true,
        sound_stickers_autoplay: false,
        auto_photos: (true, true),
        auto_audio: (true, true),
        auto_video: (true, false),
        auto_docs: (true, false),
        hd_uploads: false,
        low_data_calls: false,
        proxy: None,
        backup_on: false,
        backup_destination: BackupDestination::SongbirdServers,
        backup_include_media: true,
        backup_daily: true,
        backup_last: None,
        backup_size: "1.2 GB".into(),
        two_step_pin: true,
        passkey: false,
        plan: Plan::Free,
        whatsapp_linked: true,
        parental_controls: false,
        crash_reports: false,
        launch_at_login: false,
    };

    Seed {
        me: Profile {
            name: "Radu".into(),
            username: "radu".into(),
            about: "Building Songbird 🐦".into(),
            status_line: None,
            presence: Presence::Online,
            seed: 42,
        },
        contacts,
        chats,
        messages,
        calls,
        call_links,
        devices,
        settings,
        session: Session {
            onboarded: false,
            recovery_key: "SB4K-7Q2M-XN8P-3HVA-R9TE-LW6C-JD5F-2KBG".into(),
            invites_left: 5,
            invites_used: 3,
            offline: false,
        },
    }
}

//! The plain data of plans, help and support: what each plan includes,
//! its prices, the store that sells it, the help articles and the log
//! scrubber. No NSH here, so `cargo test` covers it.

use nsh_core::platform::Os;

use crate::store::{Billing, Plan};

// ---------------------------------------------------------------- plans

/// What a plan is, as its card shows it.
pub struct PlanInfo {
    pub plan: Plan,
    pub name: &'static str,
    pub tagline: &'static str,
    /// Exactly what it adds, nothing more.
    pub includes: &'static [&'static str],
    /// Price in euro cents a month and a year (0: free).
    pub monthly_cents: u32,
    pub yearly_cents: u32,
}

/// The three plans. mock: prices come from the store (StoreKit, Play
/// Billing, the EU provider) in the user's currency.
pub const PLANS: [PlanInfo; 3] = [
    PlanInfo {
        plan: Plan::Free,
        name: "Free",
        tagline: "Everything you need to talk privately",
        includes: &[
            "Unlimited messages, calls and groups",
            "Up to 5 linked devices",
            "Files up to 2 GB",
            "Every security and privacy feature",
        ],
        monthly_cents: 0,
        yearly_cents: 0,
    },
    PlanInfo {
        plan: Plan::Plus,
        name: "Plus",
        tagline: "A little extra, and it keeps Songbird independent",
        includes: &[
            "Up to 10 linked devices",
            "Files up to 4 GB",
            "App icons",
            "Chat themes",
            "Priority support",
        ],
        monthly_cents: 299,
        yearly_cents: 2999,
    },
    PlanInfo {
        plan: Plan::Family,
        name: "Family",
        tagline: "Plus for up to 6 people",
        includes: &[
            "Everything in Plus, for up to 6 people",
            "Each person keeps their own private account",
            "Manage who is in the family",
        ],
        monthly_cents: 499,
        yearly_cents: 4999,
    },
];

pub fn info(plan: Plan) -> &'static PlanInfo {
    PLANS.iter().find(|p| p.plan == plan).unwrap_or(&PLANS[0])
}

/// "€2.99" from cents.
pub fn euros(cents: u32) -> String {
    format!("€{}.{:02}", cents / 100, cents % 100)
}

/// "€2.99 a month" / "€29.99 a year" / "Free".
pub fn price_label(plan: Plan, billing: Billing) -> String {
    let p = info(plan);
    match (p.monthly_cents, billing) {
        (0, _) => "Free".into(),
        (_, Billing::Monthly) => format!("{} a month", euros(p.monthly_cents)),
        (_, Billing::Yearly) => format!("{} a year", euros(p.yearly_cents)),
    }
}

/// A paid plan's price in one line: "€2.99 a month, or €29.99 a year
/// (save 16%)". The saving is the plain comparison with twelve monthly
/// payments, never a "discount" invented for urgency.
pub fn price_line(plan: Plan) -> Option<String> {
    let p = info(plan);
    (p.monthly_cents > 0).then(|| {
        let twelve = p.monthly_cents * 12;
        let save = (100.0 * (1.0 - p.yearly_cents as f32 / twelve as f32)).round() as u32;
        let saving = if save > 0 {
            format!(" (save {save}%)")
        } else {
            String::new()
        };
        format!(
            "{} a month, or {} a year{saving}",
            euros(p.monthly_cents),
            euros(p.yearly_cents)
        )
    })
}

/// The features that are always free, whatever the plan.
pub const ALWAYS_FREE: &[&str] = &[
    "End\u{2011}to\u{2011}end encryption for every message and call",
    "Safety numbers and verification",
    "Disappearing messages and app lock",
    "Encrypted backups",
    "Every privacy setting",
];

/// Who sells the plan on this platform, and how it is managed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provider {
    pub name: &'static str,
    pub manage: &'static str,
}

pub fn provider(os: Os) -> Provider {
    match os {
        Os::Ios | Os::MacOs => Provider {
            name: "App Store",
            manage: "Your subscription is managed in your Apple account settings.",
        },
        Os::Android => Provider {
            name: "Google Play",
            manage: "Your subscription is managed in Google Play, under Payments and subscriptions.",
        },
        Os::Windows | Os::Linux | Os::Web => Provider {
            name: "EU payment provider",
            manage: "Your subscription is managed by our EU payment provider. You can cancel here at any time.",
        },
    }
}

/// The paid feature a paywall names, in a sentence.
pub fn paywall_title(feature: &str) -> String {
    format!("{feature} are part of Plus")
}

// ---------------------------------------------------------------- help

/// A help article: a title, a one-line summary and paragraphs.
pub struct Article {
    pub id: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub body: &'static [&'static str],
}

pub const ARTICLES: [Article; 10] = [
    Article {
        id: "can-see",
        title: "What Songbird can see",
        summary: "What our servers know, and what they never see.",
        body: &[
            "Your messages, calls, photos, files, group names and profile are end\u{2011}to\u{2011}end encrypted. Only the people in the chat can read them. Songbird's servers pass them on without being able to open them.",
            "To deliver messages, the servers need a little information: your account and devices, their public keys, when a device last connected, and encrypted messages waiting for delivery. Delivered messages are deleted from the server.",
            "Servers see your IP address while you are connected, as any internet service does. We don't log it. For calls, the call server sees a random name for each participant, never who you are.",
            "We never see your contacts, who you talk to in groups, or what you say. You can read every detail in the privacy centre.",
        ],
    },
    Article {
        id: "backups",
        title: "Backups",
        summary: "Keep your chats when you change phones.",
        body: &[
            "Backups are encrypted on your device with a key only you have, before they leave it. You can keep them on Songbird's servers, in an encrypted file, or in a cloud drive you choose.",
            "Write down your recovery key and keep it somewhere safe. Without it, nobody, not even Songbird, can open your backup.",
            "Turn backups on in You, then Chats, then Backups.",
        ],
    },
    Article {
        id: "safety-numbers",
        title: "Verifying safety numbers",
        summary: "Make sure you are talking to the right person.",
        body: &[
            "Every chat has a safety number. When yours matches the other person's, nobody can be listening in between.",
            "Open the chat, tap their name, then Verify safety number. Scan their code in person, or compare the 60 digits over a call you trust.",
            "If a safety number changes, it usually means they reinstalled Songbird or got a new phone. You'll see a calm notice in the chat, and you can verify again.",
        ],
    },
    Article {
        id: "from-whatsapp",
        title: "Switching from WhatsApp",
        summary: "Bring your friends, keep your groups going.",
        body: &[
            "Songbird works like the messenger you know: chats, groups, calls, voice messages, stickers and status lines.",
            "Invite friends with your invite link. When they join, a chat with you is waiting for them.",
            "While your friends switch, you can link WhatsApp and see those chats in Songbird. They stay end\u{2011}to\u{2011}end encrypted, but Meta can still see who you talk to and when.",
        ],
    },
    Article {
        id: "linking",
        title: "Using Songbird on a computer",
        summary: "Link a computer or tablet with a QR code.",
        body: &[
            "Open Songbird on your computer, then on your phone go to You, then Linked devices, then Link a device, and scan the code.",
            "Linked devices get their own keys. Your phone doesn't need to stay online.",
        ],
    },
    Article {
        id: "disappearing",
        title: "Disappearing messages",
        summary: "Messages that delete themselves after a while.",
        body: &[
            "Turn on disappearing messages in a chat's info. New messages disappear after 24 hours, 7 days or 4 weeks, for everyone in the chat.",
            "People can still take screenshots or photos of their screen, so only share what you are comfortable with.",
        ],
    },
    Article {
        id: "invites",
        title: "Invite codes and the beta",
        summary: "Why Songbird is invite-only for now.",
        body: &[
            "Songbird is in an invite-only beta so we can grow carefully and keep the service fast and reliable.",
            "Each person gets a few invite codes. You can see whether a code was used, but never who used it.",
        ],
    },
    Article {
        id: "plans",
        title: "Songbird Plus and Family",
        summary: "What Plus adds, and what stays free.",
        body: &[
            "Plus adds more linked devices, larger files, app icons, chat themes and priority support. Family gives Plus to up to 6 people.",
            "Security and privacy features are always free. A paid plan never changes how your messages are protected.",
            "You can cancel at any time. Your plan stays until the end of the period you paid for.",
        ],
    },
    Article {
        id: "notifications",
        title: "Notifications not arriving",
        summary: "What to check when messages arrive late.",
        body: &[
            "Check that notifications are allowed for Songbird in your phone's settings, and that battery saving isn't stopping it.",
            "Notifications are encrypted too. Your phone's push service only sees that something arrived, never what.",
        ],
    },
    Article {
        id: "delete-account",
        title: "Deleting your account",
        summary: "What happens when you leave.",
        body: &[
            "Go to You, then Account, then Delete account. Your account, devices and keys are removed from our servers.",
            "Messages you sent stay on the devices of the people who received them, as with any messenger.",
        ],
    },
];

/// The articles matching `query` (title first, then summary, then body),
/// in order of relevance; all of them for an empty query.
pub fn search(query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return (0..ARTICLES.len()).collect();
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    let mut scored: Vec<(u32, usize)> = ARTICLES
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            let title = a.title.to_lowercase();
            let summary = a.summary.to_lowercase();
            let body = a.body.join(" ").to_lowercase();
            let mut score = 0;
            for w in &words {
                if title.contains(w) {
                    score += 6;
                } else if summary.contains(w) {
                    score += 3;
                } else if body.contains(w) {
                    score += 1;
                } else {
                    return None;
                }
            }
            Some((score, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

pub fn article(id: &str) -> Option<&'static Article> {
    ARTICLES.iter().find(|a| a.id == id)
}

// ---------------------------------------------------------------- support

/// Topics of the contact form.
pub const TOPICS: [&str; 5] = ["Account", "Messages", "Calls", "Billing", "Something else"];

/// A sample of what the app's log holds before scrubbing. mock: app-core
/// keeps a rolling log of the last 24 hours with no message content.
pub const SAMPLE_LOG: &str = "\
10:20:31 INFO  net: connected to eu-1.songbird.chat from 81.196.44.12
10:20:31 INFO  devices: device 7f3a9c21e4b05d88 registered for radu@example.com
10:20:32 WARN  push: token 4b8e1f0a9c7d2e3f6a5b refreshed
10:21:05 INFO  chat: delivered 3 envelopes to @mihai (+40 721 555 123)
10:21:40 ERROR media: upload of 2.4 MB failed, retrying in 4 s
10:22:02 INFO  calls: joined room 9c1d2e7f as participant 52aa
10:24:10 INFO  backup: finished, 128 MB";

/// The log with everything that could identify someone replaced: email
/// addresses, phone numbers, @usernames, IP addresses and long hex ids
/// (device ids, tokens, rooms). Only this form is ever attached.
pub fn scrub(log: &str) -> String {
    log.lines().map(scrub_line).collect::<Vec<_>>().join("\n")
}

fn scrub_line(line: &str) -> String {
    // Phone numbers span several words: "+40 721 555 123".
    let mut out: Vec<String> = Vec::new();
    let mut in_phone = false;
    for word in line.split(' ') {
        let bare = word.trim_matches(|c: char| "()[],;.".contains(c));
        let is_digits = !bare.is_empty() && bare.chars().all(|c| c.is_ascii_digit());
        if bare.starts_with('+') && bare.len() > 2 && bare[1..].chars().all(|c| c.is_ascii_digit())
        {
            in_phone = true;
            out.push(word.replace(bare, "[phone]"));
            continue;
        }
        if in_phone && is_digits {
            // Swallow the rest of the number into the one marker.
            if word.ends_with(')') {
                if let Some(last) = out.last_mut() {
                    last.push(')');
                }
            }
            continue;
        }
        in_phone = false;
        out.push(scrub_word(word));
    }
    out.join(" ")
}

fn scrub_word(word: &str) -> String {
    let bare = word.trim_matches(|c: char| "()[],;".contains(c));
    let replace = |marker: &str| word.replacen(bare, marker, 1);
    if bare.contains('@') && bare.find('@').is_some_and(|i| i > 0) && bare.contains('.') {
        return replace("[email]");
    }
    if let Some(rest) = bare.strip_prefix('@') {
        if !rest.is_empty() {
            return replace("@[user]");
        }
    }
    let parts: Vec<&str> = bare.split('.').collect();
    if parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()))
    {
        return replace("[ip]");
    }
    if bare.len() >= 8
        && bare.chars().all(|c| c.is_ascii_hexdigit())
        && bare.chars().any(|c| c.is_ascii_alphabetic())
    {
        return replace("[id]");
    }
    word.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_read_plainly() {
        assert_eq!(price_label(Plan::Plus, Billing::Monthly), "€2.99 a month");
        assert_eq!(price_label(Plan::Plus, Billing::Yearly), "€29.99 a year");
        assert_eq!(price_label(Plan::Free, Billing::Yearly), "Free");
        assert_eq!(
            price_line(Plan::Plus).unwrap(),
            "€2.99 a month, or €29.99 a year (save 16%)"
        );
        assert!(price_line(Plan::Free).is_none());
    }

    #[test]
    fn plus_lists_exactly_what_it_adds() {
        let plus = info(Plan::Plus);
        assert_eq!(plus.includes.len(), 5);
        assert!(plus.includes.contains(&"Chat themes"));
        assert!(info(Plan::Family).includes[0].contains("6 people"));
    }

    #[test]
    fn provider_follows_the_platform() {
        assert_eq!(provider(Os::Ios).name, "App Store");
        assert_eq!(provider(Os::Android).name, "Google Play");
        assert_eq!(provider(Os::Linux).name, "EU payment provider");
    }

    #[test]
    fn search_ranks_titles_first_and_needs_every_word() {
        assert_eq!(search("").len(), 10);
        assert_eq!(search("backups")[0], 1);
        assert_eq!(search("safety number")[0], 2);
        assert!(search("whatsapp")[0] == 3);
        assert!(search("zebra").is_empty());
    }

    #[test]
    fn every_article_has_a_unique_id() {
        for a in &ARTICLES {
            assert_eq!(article(a.id).map(|x| x.title), Some(a.title));
        }
    }

    #[test]
    fn scrub_removes_what_identifies_people() {
        let out = scrub(SAMPLE_LOG);
        for secret in [
            "81.196.44.12",
            "radu@example.com",
            "7f3a9c21e4b05d88",
            "4b8e1f0a9c7d2e3f6a5b",
            "@mihai",
            "721",
            "555 123",
            "9c1d2e7f",
        ] {
            assert!(!out.contains(secret), "{secret} left in:\n{out}");
        }
        assert!(out.contains("[ip]"));
        assert!(out.contains("[email]"));
        assert!(out.contains("@[user] ([phone])"));
        // Ordinary words and numbers stay.
        assert!(out.contains("2.4 MB failed, retrying in 4 s"));
        assert!(out.contains("10:24:10 INFO  backup: finished, 128 MB"));
    }
}

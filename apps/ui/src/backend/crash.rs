//! Crash reports, opt-in and scrubbed on the device (D-0170).
//!
//! When the app panics, a hook writes one report to this device: the app's version and
//! platform, the panic's message and place, and the names of Songbird's own functions on the
//! stack, each **scrubbed** first (IDs, numbers, quoted text, addresses, usernames, URLs and
//! paths replaced with placeholders, so no message, name or key can ride along). At the next
//! start, if the user turned crash reports on, it goes to support as an **unsigned** ticket of
//! kind `crash`, sealed to the support team's key (the server stores ciphertext and never learns
//! the account or device); either way the file is deleted. Reports are never sent while off.

use std::path::PathBuf;

use nsh_core::platform::HttpRequest;
use nsh_core::prelude::*;
use songbird_keys::provisioning::{Domain, ProvisioningKey, seal_with};

use super::{endpoint, plans, problem, send, wire};
use crate::store::Store;

const FILE: &str = "songbird-crash.json";
/// The most of each field kept.
const MAX_FIELD: usize = 2048;
/// Stack frames kept.
const MAX_FRAMES: usize = 40;

/// Where the report goes, found once at install: a panic on any thread can write it.
static PATH: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

fn path() -> Option<PathBuf> {
    PATH.get_or_init(|| data_dir().map(|d| d.join(FILE)))
        .clone()
}

fn is_id_char(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// Replaces anything that could identify someone or carry their content with placeholders.
#[must_use]
pub fn scrub(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(MAX_FIELD));
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() && out.len() < MAX_FIELD {
        let c = chars[i];
        // Quoted text: what someone wrote, a name, a value.
        if c == '"' || c == '\'' || c == '`' {
            let end = chars[i + 1..].iter().position(|x| *x == c);
            if let Some(n) = end {
                out.push(c);
                out.push('…');
                out.push(c);
                i += n + 2;
                continue;
            }
        }
        // A word: decide by its whole shape.
        if c.is_alphanumeric() || matches!(c, '@' | '/' | ':' | '.' | '_' | '-' | '\\' | '~' | '+')
        {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric()
                    || matches!(
                        chars[i],
                        '@' | '/'
                            | ':'
                            | '.'
                            | '_'
                            | '-'
                            | '\\'
                            | '~'
                            | '+'
                            | '%'
                            | '='
                            | '?'
                            | '&'
                            | '#'
                    ))
            {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            out.push_str(&scrub_word(&word));
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn scrub_word(w: &str) -> String {
    let lower = w.to_ascii_lowercase();
    if lower.contains("://") || lower.starts_with("www.") {
        return "<url>".into();
    }
    if w.contains('@') {
        return if w.starts_with('@') {
            "@<user>".into()
        } else {
            "<email>".into()
        };
    }
    if w.contains('/') || w.contains('\\') {
        // A path: only a Rust source file's name, which says where, not whose.
        let name = w.rsplit(['/', '\\']).next().unwrap_or_default();
        let (file, rest) = name.split_once(':').unwrap_or((name, ""));
        return if file.ends_with(".rs") {
            let line: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if line.is_empty() {
                file.to_owned()
            } else {
                format!("{file}:{line}")
            }
        } else {
            "<path>".into()
        };
    }
    let hexish = w.chars().filter(|c| is_id_char(*c)).count();
    if hexish >= 8 && w.chars().all(|c| is_id_char(c) || c == 'x' || c == '-') {
        return "<id>".into();
    }
    let digits = w.chars().filter(char::is_ascii_digit).count();
    if digits >= 5 {
        return "<n>".into();
    }
    // Long tokens of mixed letters and digits (keys, base64) are not words.
    if w.len() >= 20 && digits > 0 {
        return "<token>".into();
    }
    w.to_owned()
}

/// Songbird's own functions on the stack, by name only (no addresses, no arguments).
fn frames(bt: &str) -> Vec<String> {
    bt.lines()
        .map(str::trim)
        .filter_map(|l| {
            let name = l.split_once(": ").map_or(l, |(n, rest)| {
                if n.chars().all(|c| c.is_ascii_digit()) {
                    rest
                } else {
                    l
                }
            });
            let name = name.split(" at ").next().unwrap_or(name).trim();
            let ours = ["songbird", "nsh_app", "nsh_core"]
                .iter()
                .any(|p| name.starts_with(p) || name.starts_with(&format!("<{p}")));
            ours.then(|| {
                // `…::h0123abcd` hashes go.
                let n = name.rsplit_once("::h").map_or(name, |(a, b)| {
                    if b.len() == 16 && b.chars().all(|c| c.is_ascii_hexdigit()) {
                        a
                    } else {
                        name
                    }
                });
                scrub(n)
            })
        })
        .take(MAX_FRAMES)
        .collect()
}

/// The report for a panic: what a crash report holds, already scrubbed.
#[must_use]
pub fn report(message: &str, place: Option<(&str, u32)>, backtrace: &str) -> serde_json::Value {
    let day = (nsh_core::time::unix_ms() as u64) / 86_400_000;
    serde_json::json!({
        "app": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "day": day,
        "panic": scrub(message),
        "at": place.map(|(f, l)| scrub(&format!("{f}:{l}"))),
        "frames": frames(backtrace),
    })
}

/// Installs the panic hook (before anything else runs). The previous hook still runs after.
pub fn install() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let _ = path();
    ONCE.call_once(install_hook);
}

fn install_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let place = info.location().map(|l| (l.file(), l.line()));
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        if let Some(Some(p)) = PATH.get() {
            let r = report(&message, place, &bt);
            let _ = std::fs::write(p, r.to_string());
        }
        previous(info);
    }));
}

/// At start, once connected: sends the waiting report if crash reports are on, and deletes it
/// either way.
pub fn flush(s: Store) {
    let Some(p) = path() else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(&p) else {
        return;
    };
    let _ = std::fs::remove_file(&p);
    if !s.settings.with_untracked(|x| x.crash_reports) || !super::is_live() {
        return;
    }
    super::run(async move {
        let key = plans::support_key().await?;
        let reply = ProvisioningKey::from_private(&wire::random::<32>())
            .map_err(|_| problem("Couldn't make a key."))?;
        let sealed = seal_with(Domain::Support, &key, text.as_bytes())
            .map_err(|_| problem("The server's support key isn't usable."))?;
        let b = serde_json::to_vec(&serde_json::json!({
            "kind": "crash",
            "reply_key": wire::hex(&reply.public()),
            "sealed": wire::hex(&sealed),
        }))
        .unwrap_or_default();
        // Unsigned: a crash report names no account and no device.
        let (st, _) = send(
            HttpRequest::new("POST", format!("{}/v1/support/tickets", endpoint().api))
                .header("content-type", "application/json")
                .header("content-length", b.len().to_string())
                .body(b),
        )
        .await?;
        if st != 201 {
            log::info!("crash report: {st}");
        }
        Ok(())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_carry_no_ids_names_text_or_paths() {
        let msg = r#"called `Result::unwrap()` on an `Err` value: chat 3f2a9c0d11e24b5f for @maria.k said "meet at 8" at /Users/radu/x.txt https://songbird.chat/u#abc 1234567 ana@example.org dGhpcyBpcyBhIGtleTEyMzQ1Ng"#;
        let s = scrub(msg);
        for leak in [
            "3f2a9c0d",
            "maria",
            "meet at 8",
            "radu",
            "songbird.chat",
            "1234567",
            "ana@",
            "dGhpcyBp",
            "Err value",
        ] {
            assert!(!s.contains(leak), "{leak} in {s}");
        }
        assert!(
            s.contains("called") && s.contains("<id>") && s.contains("@<user>"),
            "{s}"
        );
        let r = report(
            msg,
            Some(("/Users/radu/songbird/apps/ui/src/backend/chat.rs", 812)),
            "  12: songbird_app_core::messenger::Messenger::receive::h0123456789abcdef\n             at /Users/radu/songbird/crates/app-core/src/messenger.rs:900:5\n  13: std::rt::lang_start\n  14: nsh_app_songbird::backend::chat::receive::{{closure}}",
        );
        assert_eq!(r["at"], "chat.rs:812");
        assert_eq!(
            r["frames"],
            serde_json::json!([
                "songbird_app_core::messenger::Messenger::receive",
                "nsh_app_songbird::backend::chat::receive::{{closure}}"
            ])
        );
        assert!(!r.to_string().contains("radu"));
    }
}

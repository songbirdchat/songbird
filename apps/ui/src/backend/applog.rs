//! The app's own log, for support requests (D-0184): the technical events of this run, kept in
//! memory only (never written to disk), at most the last 24 hours and [`MAX_LINES`] lines.
//!
//! Every `log` record at info level or above is kept (besides whatever the developer's
//! `RUST_LOG` prints): its time, level, module and message. Songbird logs no message content;
//! anything else that could identify someone is replaced by [`super::crash::scrub`] before the
//! user previews it, and only that scrubbed form is ever attached, with their consent.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// Lines kept at most.
pub const MAX_LINES: usize = 2_000;
/// Longest message kept per line (longer ones are cut).
const MAX_MESSAGE: usize = 300;
/// How long a line is kept.
const KEEP_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone)]
struct Line {
    at_ms: u64,
    level: log::Level,
    target: String,
    message: String,
}

fn lines() -> &'static Mutex<VecDeque<Line>> {
    static LINES: OnceLock<Mutex<VecDeque<Line>>> = OnceLock::new();
    LINES.get_or_init(|| Mutex::new(VecDeque::with_capacity(256)))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Keeps one record (also from other threads).
fn keep(level: log::Level, target: &str, message: String) {
    let mut message = message;
    if message.len() > MAX_MESSAGE {
        let mut end = MAX_MESSAGE;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push('…');
    }
    let at_ms = now_ms();
    let Ok(mut l) = lines().lock() else { return };
    while l
        .front()
        .is_some_and(|x| at_ms.saturating_sub(x.at_ms) > KEEP_MS)
        || l.len() >= MAX_LINES
    {
        l.pop_front();
    }
    l.push_back(Line {
        at_ms,
        level,
        target: target.to_owned(),
        message,
    });
}

/// The logger: the developer's `env_logger` (as configured by `RUST_LOG`), plus the in-memory
/// log of info and above.
struct AppLog {
    console: Option<env_logger::Logger>,
}

impl log::Log for AppLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
            || self.console.as_ref().is_some_and(|c| c.enabled(metadata))
    }

    fn log(&self, record: &log::Record<'_>) {
        if record.level() <= log::Level::Info {
            keep(record.level(), record.target(), record.args().to_string());
        }
        if let Some(c) = &self.console
            && c.matches(record)
        {
            c.log(record);
        }
    }

    fn flush(&self) {
        if let Some(c) = &self.console {
            c.flush();
        }
    }
}

/// Installs the logger once (desktop `main` and the app's root both call it; the first wins).
pub fn init() {
    let console = env_logger::Builder::from_default_env().build();
    let max = console.filter().max(log::LevelFilter::Info);
    if log::set_boxed_logger(Box::new(AppLog {
        console: Some(console),
    }))
    .is_ok()
    {
        log::set_max_level(max);
    }
}

/// The log as the user previews it before anything is attached: one line per event, oldest
/// first, every line scrubbed on this device.
#[must_use]
pub fn scrubbed() -> String {
    let Ok(l) = lines().lock() else {
        return String::new();
    };
    let now = now_ms();
    l.iter()
        .filter(|x| now.saturating_sub(x.at_ms) <= KEEP_MS)
        .map(|x| {
            let secs = (x.at_ms / 1000) % 86_400;
            format!(
                "{:02}:{:02}:{:02} {:<5} {}: {}",
                secs / 3600,
                (secs / 60) % 60,
                secs % 60,
                x.level,
                x.target.rsplit("::").next().unwrap_or(&x.target),
                super::crash::scrub(&x.message)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_bounded_cut_and_scrubbed() {
        for i in 0..(MAX_LINES + 5) {
            keep(log::Level::Info, "songbird::net", format!("line {i}"));
        }
        keep(
            log::Level::Warn,
            "songbird::net",
            format!("connected from 81.196.44.12 {}", "x".repeat(MAX_MESSAGE)),
        );
        let n = lines().lock().unwrap().len();
        assert!(n <= MAX_LINES);
        let out = scrubbed();
        assert!(!out.contains("81.196.44.12"), "{out}");
        let last = out.lines().last().unwrap_or_default();
        assert!(last.contains("WARN") && last.contains("net:") && last.ends_with('…'));
    }
}

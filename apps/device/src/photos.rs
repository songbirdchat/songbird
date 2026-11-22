//! Saving a photo or video to where the person keeps them (D-0174): the Photos library on iOS
//! (asking for add-only access, so Songbird can't read the library), the shared Pictures and
//! Movies collections on Android, and the Pictures folder on a computer (`Songbird` inside it).

#![forbid(unsafe_code)]

use std::path::PathBuf;

/// What is saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A still image.
    Photo,
    /// A video clip.
    Video,
}

/// Why it wasn't saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The person (or device management) didn't allow it.
    Denied,
    /// The system refused or failed (no space, no library).
    Failed,
}

/// The answer, on a thread of the system's choosing.
pub type Done = Box<dyn FnOnce(Result<(), Failure>) + Send>;

/// Saves `bytes` (a file named `name`, of `mime`).
pub fn save(bytes: Vec<u8>, kind: Kind, name: &str, mime: &str, done: Done) {
    let _ = mime;
    #[cfg(target_os = "ios")]
    return crate::apple::save_media(&bytes, kind, name, done);
    #[cfg(target_os = "android")]
    return crate::android::save_media(&bytes, kind, name, mime, done);
    #[allow(unreachable_code, reason = "the computers save to a folder")]
    {
        let name = safe_name(name);
        std::thread::spawn(move || done(save_to_folder(&bytes, kind, &name)));
    }
}

/// A file name with nothing that names a folder or a device.
pub(crate) fn safe_name(name: &str) -> String {
    let base: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .take(120)
        .collect();
    let base = base.trim().trim_start_matches('.');
    if base.is_empty() {
        "Songbird".into()
    } else {
        base.into()
    }
}

/// The person's Pictures folder: the XDG one on Linux, else `~/Pictures`.
fn pictures() -> Option<PathBuf> {
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)?;
    #[cfg(target_os = "linux")]
    if let Ok(dirs) = std::fs::read_to_string(home.join(".config/user-dirs.dirs")) {
        for line in dirs.lines() {
            if let Some(v) = line.strip_prefix("XDG_PICTURES_DIR=") {
                let v = v
                    .trim_matches('"')
                    .replace("$HOME", &home.to_string_lossy());
                return Some(PathBuf::from(v));
            }
        }
    }
    Some(home.join("Pictures"))
}

#[allow(dead_code, reason = "phones save to their libraries")]
fn save_to_folder(bytes: &[u8], kind: Kind, name: &str) -> Result<(), Failure> {
    let _ = kind;
    let dir = pictures().ok_or(Failure::Failed)?.join("Songbird");
    std::fs::create_dir_all(&dir).map_err(|_| Failure::Failed)?;
    // Never over an existing file: "name (2).jpg" and so on.
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_owned(), format!(".{e}")),
        _ => (name.to_owned(), String::new()),
    };
    for n in 1..1000 {
        let file = if n == 1 {
            dir.join(name)
        } else {
            dir.join(format!("{stem} ({n}){ext}"))
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
        {
            Ok(mut f) => {
                use std::io::Write as _;
                return f.write_all(bytes).map_err(|_| Failure::Failed);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(Failure::Failed),
        }
    }
    Err(Failure::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_never_name_a_folder() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("C:\\x\\a:b.jpg"), "ab.jpg");
        assert_eq!(safe_name(".hidden"), "hidden");
        assert_eq!(safe_name(""), "Songbird");
    }
}

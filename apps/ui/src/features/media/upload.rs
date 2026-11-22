//! Attachment send progress: a ring with the percentage, Cancel while it
//! runs, Resume after the network drops. For the chat's attachment
//! bubbles and anything else that uploads (pack links here).

use nsh_core::{element::ImageSrc, prelude::*};

use crate::ui::*;

/// Where an upload is. Progress only moves forward; a lost network pauses
/// it where it was (uploads resume, D-0083's resumable grants).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Upload {
    Uploading(f32),
    /// The network went away at this progress; Resume continues.
    Paused(f32),
    Done,
    Cancelled,
}

impl Upload {
    /// 0.0 ..= 1.0.
    pub fn progress(self) -> f32 {
        match self {
            Upload::Uploading(p) | Upload::Paused(p) => p.clamp(0.0, 1.0),
            Upload::Done => 1.0,
            Upload::Cancelled => 0.0,
        }
    }
    pub fn percent(self) -> u32 {
        (self.progress() * 100.0).round() as u32
    }
    /// Moves on by `by` (a network tick).
    pub fn advance(self, by: f32) -> Self {
        match self {
            Upload::Uploading(p) if p + by >= 1.0 => Upload::Done,
            Upload::Uploading(p) => Upload::Uploading(p + by),
            other => other,
        }
    }
    pub fn network_lost(self) -> Self {
        match self {
            Upload::Uploading(p) => Upload::Paused(p),
            other => other,
        }
    }
    pub fn resume(self) -> Self {
        match self {
            Upload::Paused(p) => Upload::Uploading(p),
            Upload::Cancelled => Upload::Uploading(0.0),
            other => other,
        }
    }
    pub fn cancel(self) -> Self {
        match self {
            Upload::Uploading(_) | Upload::Paused(_) => Upload::Cancelled,
            other => other,
        }
    }
    /// What screen readers hear and the caption shows.
    pub fn label(self) -> String {
        match self {
            Upload::Uploading(_) => format!("Sending, {}%", self.percent()),
            Upload::Paused(_) => format!("Paused at {}%. Waiting for a connection", self.percent()),
            Upload::Done => "Sent".into(),
            Upload::Cancelled => "Cancelled".into(),
        }
    }
    fn caption(self) -> String {
        match self {
            Upload::Uploading(_) => format!("{}%", self.percent()),
            Upload::Paused(_) => "Paused".into(),
            Upload::Done => "Sent".into(),
            Upload::Cancelled => "Cancelled".into(),
        }
    }
}

/// The ring's SVG: a faint track, the progress arc from 12 o'clock, and
/// the action in the middle (× to cancel, ↑ to resume or retry, ✓ done).
pub fn ring_markup(state: Upload) -> String {
    const R: f32 = 19.0;
    let circumference = 2.0 * std::f32::consts::PI * R;
    let offset = circumference * (1.0 - state.progress());
    let glyph = match state {
        Upload::Uploading(_) => r#"<path d="M18 18l12 12M30 18L18 30"/>"#,
        Upload::Paused(_) | Upload::Cancelled => r#"<path d="M24 31V17M18 23l6-6 6 6"/>"#,
        Upload::Done => r#"<path d="M17 24.5l5 5 9-10"/>"#,
    };
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48" viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"><circle cx="24" cy="24" r="{R}" stroke-width="3" opacity="0.3"/><circle cx="24" cy="24" r="{R}" stroke-width="3" stroke-dasharray="{circumference:.2}" stroke-dashoffset="{offset:.2}" transform="rotate(-90 24 24)"/><g stroke-width="2.6">{glyph}</g></svg>"#
    )
}

/// The ring with its caption. Pressing it cancels a running upload and
/// resumes a paused or cancelled one. Sits on a dark disc so it reads
/// over a photo and on a plain surface alike.
pub fn upload_progress(
    state: impl Fn() -> Upload + Copy + 'static,
    on_cancel: impl Fn() + 'static,
    on_resume: impl Fn() + 'static,
) -> View {
    view()
        .items_center()
        .gap_1()
        .child(
            press_surface(Press::OnDark, move || match state() {
                Upload::Uploading(_) => on_cancel(),
                Upload::Paused(_) | Upload::Cancelled => on_resume(),
                Upload::Done => {}
            })
            .accessibility_label(move || {
                let s = state();
                Some(SharedString::from(match s {
                    Upload::Uploading(_) => format!("{}. Cancel", s.label()),
                    Upload::Paused(_) | Upload::Cancelled => format!("{}. Resume", s.label()),
                    Upload::Done => s.label(),
                }))
            })
            .size(px(48.0))
            .rounded_full()
            .bg(move || c::overlay())
            .items_center()
            .justify_center()
            .child(svg(move || ImageSrc::svg_markup(&ring_markup(state()))).size(px(48.0))),
        )
        .child(
            text(move || state().caption())
                .text_size(px(font::CAPTION))
                .font_weight(FontWeight::MEDIUM)
                .tabular_nums()
                .accessibility_elements_hidden(true),
        )
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_upload_pauses_resumes_and_finishes() {
        let u = Upload::Uploading(0.5).network_lost();
        assert_eq!(u, Upload::Paused(0.5));
        assert_eq!(u.advance(0.2), u, "a paused upload does not move");
        let u = u.resume().advance(0.25);
        assert_eq!(u.percent(), 75);
        assert_eq!(u.advance(0.3), Upload::Done);
        assert_eq!(Upload::Done.cancel(), Upload::Done);
        assert_eq!(Upload::Uploading(0.2).cancel(), Upload::Cancelled);
        assert_eq!(Upload::Cancelled.resume(), Upload::Uploading(0.0));
    }

    #[test]
    fn labels_say_what_happens() {
        assert_eq!(Upload::Uploading(0.42).label(), "Sending, 42%");
        assert_eq!(
            Upload::Paused(0.42).label(),
            "Paused at 42%. Waiting for a connection"
        );
        assert!(ring_markup(Upload::Uploading(0.5)).contains("stroke-dashoffset"));
    }
}

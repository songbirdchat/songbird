//! Mock profile photos: an illustrated portrait per person, generated
//! from a seed, so every contact has a stable, distinct picture until
//! app-core delivers real (end-to-end encrypted) profile photos.
//!
//! The colours here are *picture content* (skin, hair, backdrop), the
//! "data or raster content whose colour is itself the data" the design
//! guides allow outside the theme. Nothing in the app's chrome uses them.

use std::{cell::RefCell, collections::HashMap};

use nsh_core::element::ImageSrc;

/// Soft two-stop backdrops (top, bottom).
const BACKDROPS: [(&str, &str); 10] = [
    ("#CFE6DF", "#9FCBBE"),
    ("#E8DCCB", "#D2BB9C"),
    ("#D9DEF2", "#AEB8E3"),
    ("#F1D6D2", "#E2ABA4"),
    ("#DCE8CF", "#B5CF9C"),
    ("#E9D9EE", "#CDAFD8"),
    ("#D3E6EE", "#A3CADA"),
    ("#F2E5C7", "#E2C88C"),
    ("#E0E0DA", "#BFC0B5"),
    ("#D7E1DC", "#A9BDB3"),
];

const SKIN: [&str; 6] = [
    "#F3D3BD", "#E8B996", "#D29A72", "#B57A55", "#8C5A3C", "#6A4330",
];
const HAIR: [&str; 7] = [
    "#2B2220", "#4A3426", "#7A4E2D", "#B07A45", "#D8B46A", "#9A9A98", "#5B2E2A",
];
const SHIRT: [&str; 8] = [
    "#2F5D57", "#3F4A6B", "#8A5A44", "#5B6B3A", "#6B3F5E", "#2E4F6E", "#7A6A4A", "#44474F",
];

fn mix(seed: u64, lane: u64) -> u64 {
    let mut x = seed.wrapping_add(lane).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^ (x >> 29)
}

/// Whether this (mock) person has set a photo: most have, a few show
/// initials, as in real life.
pub fn has_photo(seed: u64) -> bool {
    // Illustrated stand-ins only in the demo fixtures; a real account shows
    // real photos (`backend::avatar`) or initials.
    crate::store::demo() && (seed == ME_SEED || mix(seed, 7) % 6 != 0)
}

/// The user's own seed (the mock store's `me.seed`): the user has set a
/// photo, so their avatar on the rail, in You and in calls is a portrait.
pub const ME_SEED: u64 = 42;

/// The portrait's SVG markup in a 64 × 64 box.
fn markup(seed: u64) -> String {
    let pick = |lane: u64, n: usize| (mix(seed, lane) % n as u64) as usize;
    let (top, bottom) = BACKDROPS[pick(1, BACKDROPS.len())];
    let skin = SKIN[pick(2, SKIN.len())];
    let me = seed == ME_SEED;
    // The user's own portrait is drawn on purpose, not rolled: a man with
    // short dark hair and a short beard (the mock profile is "Radu").
    let hair = if me {
        HAIR[1]
    } else {
        HAIR[pick(3, HAIR.len())]
    };
    let shirt = if me {
        SHIRT[5]
    } else {
        SHIRT[pick(4, SHIRT.len())]
    };
    let style = if me { 4 } else { pick(5, 6) };
    let glasses = !me && pick(6, 5) == 0;
    let id = format!("g{seed}");

    // Hair behind the head (long styles), the head, then hair on top.
    let (back, front) = match style {
        // Short, side part.
        0 => (
            String::new(),
            format!(
                r#"<path d="M20 27c0-8 5.5-13 12-13s12 4.5 12 12c-3-3-7-4.5-12-4.5-4 0-8 1.8-12 5.5z" fill="{hair}"/>"#
            ),
        ),
        // Long, past the shoulders.
        1 => (
            format!(
                r#"<path d="M18 30c0-10 6-16 14-16s14 6 14 16v16c0 3-2 5-5 5H23c-3 0-5-2-5-5z" fill="{hair}"/>"#
            ),
            format!(
                r#"<path d="M20 28c1-8 6-12.5 12-12.5S43 20 44 28c-4-5-8-6.5-12-6.5S24 23 20 28z" fill="{hair}"/>"#
            ),
        ),
        // Bun.
        2 => (
            String::new(),
            format!(
                r#"<circle cx="32" cy="12.5" r="5" fill="{hair}"/><path d="M20 27c0-7.5 5.5-12.5 12-12.5S44 19.5 44 27c-3.5-3.5-7.5-5-12-5s-8.5 1.5-12 5z" fill="{hair}"/>"#
            ),
        ),
        // Curly crown.
        3 => (
            String::new(),
            format!(
                r#"<g fill="{hair}"><circle cx="22.5" cy="23" r="4.5"/><circle cx="27" cy="17.5" r="5"/><circle cx="33" cy="15.5" r="5.2"/><circle cx="38.5" cy="18" r="4.8"/><circle cx="42" cy="23.5" r="4.2"/></g>"#
            ),
        ),
        // Short with a beard.
        4 => (
            String::new(),
            format!(
                r#"<path d="M21 25.5c0-7 5-11.5 11-11.5s11 4.5 11 11.5c-3-2.2-6.5-3.2-11-3.2s-8 1-11 3.2z" fill="{hair}"/><path d="M22.5 31c0 7 4.3 11.5 9.5 11.5s9.5-4.5 9.5-11.5c-1.5 3-4 4.2-5 4.2-1.5-1.3-3-1.8-4.5-1.8s-3 .5-4.5 1.8c-1 0-3.5-1.2-5-4.2z" fill="{hair}"/>"#
            ),
        ),
        // Bob.
        _ => (
            format!(
                r#"<path d="M19 29c0-9 5.8-14.5 13-14.5S45 20 45 29v7c0 2-1.5 3-3 3H22c-1.5 0-3-1-3-3z" fill="{hair}"/>"#
            ),
            format!(
                r#"<path d="M20.5 27.5c1.5-7 6-11 11.5-11s10 4 11.5 11c-5-1-9-3.5-11-6.5-2.5 3.5-6.5 5.5-12 6.5z" fill="{hair}"/>"#
            ),
        ),
    };
    let specs = if glasses {
        r##"<g fill="none" stroke="#1E2422" stroke-width="1.4"><circle cx="27.3" cy="30.5" r="3.6"/><circle cx="36.7" cy="30.5" r="3.6"/><path d="M30.9 30.5h2.2"/></g>"##
    } else {
        ""
    };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64"><defs><linearGradient id="{id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{top}"/><stop offset="1" stop-color="{bottom}"/></linearGradient></defs><rect width="64" height="64" fill="url(#{id})"/>{back}<path d="M12 64c1.5-11 9.5-17 20-17s18.5 6 20 17z" fill="{shirt}"/><path d="M27.5 41h9v7.5c-1.3 1.6-2.8 2.4-4.5 2.4s-3.2-.8-4.5-2.4z" fill="{skin}"/><ellipse cx="32" cy="29.5" rx="11" ry="12.5" fill="{skin}"/><path d="M28.5 36.2c1 .9 2.2 1.3 3.5 1.3s2.5-.4 3.5-1.3" fill="none" stroke="#000" stroke-opacity=".28" stroke-width="1.2" stroke-linecap="round"/><circle cx="27.6" cy="30.5" r="1.15" fill="#1E2422"/><circle cx="36.4" cy="30.5" r="1.15" fill="#1E2422"/>{front}{specs}</svg>"##
    )
}

/// A group's mock photo: a calm landscape (sky, sun, two hills) whose
/// palette comes from the seed.
fn scene(seed: u64) -> String {
    let pick = |lane: u64, n: usize| (mix(seed, lane) % n as u64) as usize;
    let (top, bottom) = BACKDROPS[pick(11, BACKDROPS.len())];
    let hills = [
        ("#5E8F7E", "#3F6E60"),
        ("#8C7A5B", "#6B5A3E"),
        ("#6C7FA8", "#4C5E86"),
        ("#A06F68", "#7D4F49"),
        ("#6E8A55", "#4F6A3A"),
    ];
    let (far, near) = hills[pick(12, hills.len())];
    let sun_x = 18 + pick(13, 28);
    let id = format!("s{seed}");
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64"><defs><linearGradient id="{id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{top}"/><stop offset="1" stop-color="{bottom}"/></linearGradient></defs><rect width="64" height="64" fill="url(#{id})"/><circle cx="{sun_x}" cy="22" r="7" fill="#FFF6DD" fill-opacity=".9"/><path d="M0 44c9-9 17-12 26-8s14 3 22-3 12-6 16-4v35H0z" fill="{far}"/><path d="M0 52c10-6 20-7 30-3s20 3 34-4v19H0z" fill="{near}"/></svg>"##
    )
}

/// The group scene as an image source, built once per seed.
pub fn scenery(seed: u64) -> ImageSrc {
    thread_local! {
        static CACHE: RefCell<HashMap<u64, ImageSrc>> = RefCell::new(HashMap::new());
    }
    CACHE.with_borrow_mut(|c| {
        c.entry(seed)
            .or_insert_with(|| ImageSrc::svg_markup(&scene(seed)))
            .clone()
    })
}

/// The portrait as an image source, built once per seed.
pub fn portrait(seed: u64) -> ImageSrc {
    thread_local! {
        static CACHE: RefCell<HashMap<u64, ImageSrc>> = RefCell::new(HashMap::new());
    }
    CACHE.with_borrow_mut(|c| {
        c.entry(seed)
            .or_insert_with(|| ImageSrc::svg_markup(&markup(seed)))
            .clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portraits_are_stable_and_differ() {
        assert_eq!(markup(3), markup(3));
        assert_ne!(markup(3), markup(4));
        assert!(markup(9).starts_with("<svg"));
        // The user's portrait is the short-haired, bearded style.
        assert!(markup(ME_SEED).contains("M22.5 31c0 7"));
        // Most people have a photo, not everyone.
        let with = (0..60).filter(|&s| has_photo(s)).count();
        assert!((35..60).contains(&with), "{with}");
    }
}

/// A mock photo for a message: an illustrated scene chosen from its
/// description (`alt`), framed at `w`:`h`, stable per `seed`. Stands in for
/// decrypted image thumbnails until app-core delivers them.
pub fn photo(alt: &str, seed: u64, w: u32, h: u32) -> ImageSrc {
    thread_local! {
        static CACHE: RefCell<HashMap<(String, u64, u32, u32), ImageSrc>> = RefCell::new(HashMap::new());
    }
    let key = (alt.to_lowercase(), seed, w.max(1), h.max(1));
    CACHE.with_borrow_mut(|c| {
        c.entry(key.clone())
            .or_insert_with(|| ImageSrc::svg_markup(&photo_markup(&key.0, seed, key.2, key.3)))
            .clone()
    })
}

fn photo_markup(alt: &str, seed: u64, w: u32, h: u32) -> String {
    let (vw, vh) = (w as f32 * 60.0, h as f32 * 60.0);
    let pick = |lane: u64, n: usize| (mix(seed, lane) % n as u64) as usize;
    let has = |words: &[&str]| words.iter().any(|k| alt.contains(k));
    let id = format!("p{seed}{w}{h}");
    let body = if has(&["room", "shelves", "library", "reading", "interior"]) {
        // A bright room: wall, floor, a tall window, shelves of books.
        let books: String = (0..14)
            .map(|i| {
                let x = vw * 0.56 + i as f32 * vw * 0.026;
                let bh = vh * (0.09 + (mix(seed, i) % 5) as f32 * 0.012);
                let col = [
                    "#8A5A44", "#3F4A6B", "#5B6B3A", "#B07A45", "#6B3F5E", "#2E4F6E",
                ][(mix(seed, i + 40) % 6) as usize];
                format!(
                    r#"<rect x="{x:.1}" y="{:.1}" width="{:.1}" height="{bh:.1}" fill="{col}"/>"#,
                    vh * 0.42 - bh,
                    vw * 0.022
                )
            })
            .collect();
        format!(
            r##"<rect width="{vw}" height="{vh}" fill="#EFE7DA"/><rect y="{fy:.1}" width="{vw}" height="{fh:.1}" fill="#C9A77F"/><rect x="{wx:.1}" y="{wy:.1}" width="{ww:.1}" height="{wh:.1}" rx="6" fill="url(#{id})"/><path d="M{mx:.1} {wy:.1}v{wh:.1}M{wx:.1} {my:.1}h{ww:.1}" stroke="#EFE7DA" stroke-width="8"/><rect x="{sx:.1}" y="{sy:.1}" width="{sw:.1}" height="10" fill="#7A5A3A"/><rect x="{sx:.1}" y="{sy2:.1}" width="{sw:.1}" height="10" fill="#7A5A3A"/>{books}<g transform="translate(0 {shift:.1})">{books}</g><defs><linearGradient id="{id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#BFDDF0"/><stop offset="1" stop-color="#EAF4F8"/></linearGradient></defs>"##,
            fy = vh * 0.72,
            fh = vh * 0.28,
            wx = vw * 0.1,
            wy = vh * 0.12,
            ww = vw * 0.3,
            wh = vh * 0.5,
            mx = vw * 0.25,
            my = vh * 0.37,
            sx = vw * 0.54,
            sy = vh * 0.42,
            sw = vw * 0.4,
            sy2 = vh * 0.62,
            shift = vh * 0.2,
        )
    } else if has(&["snow", "winter", "snowman"]) {
        format!(
            r##"<defs><linearGradient id="{id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#C9D8EA"/><stop offset="1" stop-color="#EEF3F8"/></linearGradient></defs><rect width="{vw}" height="{vh}" fill="url(#{id})"/><path d="M0 {g:.1}C{a:.1} {g2:.1} {b:.1} {g3:.1} {vw} {g:.1}V{vh}H0z" fill="#FFFFFF"/><path d="M{tx:.1} {ty:.1}l{tw:.1} {th:.1}h-{tw2:.1}z" fill="#3E5C50"/><circle cx="{cx:.1}" cy="{cy:.1}" r="{r1:.1}" fill="#FFFFFF" stroke="#D6E0EA" stroke-width="3"/><circle cx="{cx:.1}" cy="{cy2:.1}" r="{r2:.1}" fill="#FFFFFF" stroke="#D6E0EA" stroke-width="3"/><circle cx="{cx:.1}" cy="{cy2:.1}" r="4" fill="#E07B39"/>"##,
            g = vh * 0.62,
            g2 = vh * 0.55,
            g3 = vh * 0.7,
            a = vw * 0.3,
            b = vw * 0.7,
            tx = vw * 0.2,
            ty = vh * 0.3,
            tw = vw * 0.08,
            th = vh * 0.35,
            tw2 = vw * 0.16,
            cx = vw * 0.65,
            cy = vh * 0.68,
            r1 = vh * 0.1,
            cy2 = vh * 0.53,
            r2 = vh * 0.065,
        )
    } else if has(&["street", "town", "city", "dusk"]) {
        let bld: String = (0..9)
            .map(|i| {
                let bw = vw / 9.0;
                let bh = vh * (0.3 + (mix(seed, i + 7) % 6) as f32 * 0.07);
                let x = i as f32 * bw;
                let win: String = (0..((bh / 22.0) as i32).min(8))
                    .map(|j| format!(r##"<rect x="{:.1}" y="{:.1}" width="6" height="8" fill="#FFD98A" opacity="{}"/>"##, x + bw * 0.3, vh - bh + 10.0 + j as f32 * 20.0, if (mix(seed, i * 13 + j as u64) % 3) == 0 { "0.9" } else { "0.25" }))
                    .collect();
                format!(r##"<rect x="{x:.1}" y="{:.1}" width="{:.1}" height="{bh:.1}" fill="#2D2A44"/>{win}"##, vh - bh, bw - 3.0)
            })
            .collect();
        format!(
            r##"<defs><linearGradient id="{id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#5B4B8A"/><stop offset=".6" stop-color="#E58C6A"/><stop offset="1" stop-color="#F6C27A"/></linearGradient></defs><rect width="{vw}" height="{vh}" fill="url(#{id})"/>{bld}"##
        )
    } else if has(&["coffee", "cup", "table"]) {
        format!(
            r##"<rect width="{vw}" height="{vh}" fill="#8B6A4E"/><path d="M0 {k:.1}H{vw}" stroke="#7A5C43" stroke-width="4"/><path d="M0 {k2:.1}H{vw}" stroke="#7A5C43" stroke-width="4"/><ellipse cx="{cx:.1}" cy="{cy:.1}" rx="{rx:.1}" ry="{ry:.1}" fill="#F4EFE8"/><circle cx="{cx:.1}" cy="{cy2:.1}" r="{r:.1}" fill="#FFFFFF"/><circle cx="{cx:.1}" cy="{cy2:.1}" r="{r2:.1}" fill="#5A3A26"/><path d="M{hx:.1} {cy2:.1}a{hr:.1} {hr:.1} 0 1 1 0 {hd:.1}" fill="none" stroke="#FFFFFF" stroke-width="10"/>"##,
            k = vh * 0.33,
            k2 = vh * 0.66,
            cx = vw * 0.5,
            cy = vh * 0.55,
            rx = vw * 0.3,
            ry = vh * 0.3,
            cy2 = vh * 0.52,
            r = vh * 0.2,
            r2 = vh * 0.15,
            hx = vw * 0.5 + vh * 0.2,
            hr = vh * 0.06,
            hd = vh * 0.12,
        )
    } else if has(&["sea", "beach", "lake", "water"]) {
        format!(
            r##"<defs><linearGradient id="{id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#9CC8E8"/><stop offset="1" stop-color="#E8F2F8"/></linearGradient></defs><rect width="{vw}" height="{vh}" fill="url(#{id})"/><rect y="{hz:.1}" width="{vw}" height="{hh:.1}" fill="#3F7FA6"/><path d="M0 {sd:.1}C{a:.1} {s2:.1} {b:.1} {s3:.1} {vw} {sd:.1}V{vh}H0z" fill="#E9D3A8"/><circle cx="{sx:.1}" cy="{sy:.1}" r="{sr:.1}" fill="#FFF2C9"/>"##,
            hz = vh * 0.5,
            hh = vh * 0.5,
            sd = vh * 0.78,
            s2 = vh * 0.72,
            s3 = vh * 0.84,
            a = vw * 0.3,
            b = vw * 0.6,
            sx = vw * (0.25 + pick(3, 50) as f32 / 100.0),
            sy = vh * 0.28,
            sr = vh * 0.08,
        )
    } else {
        // Hills under a sky (scenery, at the photo's aspect).
        let (top, bottom) = BACKDROPS[pick(11, BACKDROPS.len())];
        format!(
            r##"<defs><linearGradient id="{id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{top}"/><stop offset="1" stop-color="{bottom}"/></linearGradient></defs><rect width="{vw}" height="{vh}" fill="url(#{id})"/><circle cx="{sx:.1}" cy="{sy:.1}" r="{sr:.1}" fill="#FFF6DD" fill-opacity=".9"/><path d="M0 {h1:.1}C{a:.1} {h2:.1} {b:.1} {h3:.1} {vw} {h4:.1}V{vh}H0z" fill="#5E8F7E"/><path d="M0 {h5:.1}C{a2:.1} {h6:.1} {b2:.1} {h7:.1} {vw} {h8:.1}V{vh}H0z" fill="#3F6E60"/>"##,
            sx = vw * (0.2 + pick(13, 60) as f32 / 100.0),
            sy = vh * 0.3,
            sr = vh * 0.1,
            h1 = vh * 0.65,
            h2 = vh * 0.45,
            h3 = vh * 0.6,
            h4 = vh * 0.55,
            a = vw * 0.3,
            b = vw * 0.6,
            h5 = vh * 0.8,
            h6 = vh * 0.7,
            h7 = vh * 0.82,
            h8 = vh * 0.75,
            a2 = vw * 0.35,
            b2 = vw * 0.7,
        )
    };
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{vw}" height="{vh}" viewBox="0 0 {vw} {vh}" preserveAspectRatio="xMidYMid slice">{body}</svg>"#
    )
}

#[cfg(test)]
mod photo_tests {
    use super::*;

    #[test]
    fn photos_pick_a_scene_from_the_description() {
        assert!(photo_markup("a bright reading room", 1, 4, 3).contains("#EFE7DA"));
        assert!(photo_markup("first snow in the garden", 1, 4, 3).contains("#FFFFFF"));
        assert!(photo_markup("a street at dusk", 1, 4, 3).contains("#2D2A44"));
        assert_eq!(photo_markup("x", 5, 16, 9), photo_markup("x", 5, 16, 9));
    }
}

/// A message's photo: the real one for a real attachment (empty until this
/// device has it), the illustration only in the demo fixtures.
pub fn message_photo(alt: &str, id: u64, w: u32, h: u32) -> ImageSrc {
    if crate::backend::chat::is_media(id) {
        crate::backend::chat::load_photo(id);
        crate::backend::chat::media_src(id).unwrap_or_else(|| ImageSrc::from(""))
    } else {
        photo(alt, id, w, h)
    }
}

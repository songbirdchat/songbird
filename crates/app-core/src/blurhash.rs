//! `BlurHash` (D-0186): a photo's blurred stand-in, a few dozen characters carried in its
//! `AttachmentPointer`, so the receiver draws the photo's colours at once and the photo itself
//! fades in when it has downloaded. The format is Wolt's (MIT): a few cosine components of the
//! image in linear light, quantised and written in base 83. Unlike Wolt's code, both sides sample
//! at pixel centres (a true DCT-II), so a flat image has no components to spare and comes back
//! flat; the format is unchanged, any decoder reads these hashes. Pure arithmetic, no
//! dependencies; decoding checks the length and every character, since the hash comes from the
//! other side.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::as_conversions,
    reason = "pixel and component arithmetic: every value is clamped before it is narrowed"
)]
#![allow(
    clippy::many_single_char_names,
    reason = "the format's own names: x and y components, i and j, r, g and b"
)]

use std::f32::consts::PI;

const ALPHABET: &[u8; 83] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz#$%*+,-.:;=?@[]^_{|}~";

/// The most components on either axis the format allows.
pub const MAX_COMPONENTS: u32 = 9;

fn encode83(mut value: u32, digits: usize, out: &mut String) {
    let mut buf = [0u8; 4];
    for slot in buf.iter_mut().take(digits).rev() {
        *slot = ALPHABET.get((value % 83) as usize).copied().unwrap_or(b'0');
        value /= 83;
    }
    for &b in buf.iter().take(digits) {
        out.push(char::from(b));
    }
}

fn decode83(s: &[u8]) -> Option<u32> {
    s.iter().try_fold(0u32, |acc, c| {
        let d = ALPHABET.iter().position(|a| a == c)?;
        acc.checked_mul(83)?.checked_add(d as u32)
    })
}

fn srgb_to_linear(v: u8) -> f32 {
    let v = f32::from(v) / 255.0;
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

fn sign_pow(v: f32, exp: f32) -> f32 {
    v.abs().powf(exp).copysign(v)
}

/// The hash of an image of `width` × `height` pixels, `rgb` (3 bytes a pixel, rows top to
/// bottom), with `x` × `y` components (1 to 9 each; 4 × 3 suits a photo). `None` for an empty
/// image, a buffer of the wrong length or components out of range.
#[must_use]
pub fn encode(x: u32, y: u32, width: u32, height: u32, rgb: &[u8]) -> Option<String> {
    if !(1..=MAX_COMPONENTS).contains(&x)
        || !(1..=MAX_COMPONENTS).contains(&y)
        || width == 0
        || height == 0
        || rgb.len() != (width as usize) * (height as usize) * 3
    {
        return None;
    }
    let (w, h) = (width as usize, height as usize);
    let linear: Vec<[f32; 3]> = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| p.map(srgb_to_linear))
        .collect();
    let mut factors = Vec::with_capacity((x * y) as usize);
    for j in 0..y {
        for i in 0..x {
            let norm = if i == 0 && j == 0 { 1.0 } else { 2.0 };
            let mut f = [0.0f32; 3];
            for py in 0..h {
                let by = (PI * j as f32 * (py as f32 + 0.5) / height as f32).cos();
                for px in 0..w {
                    let basis = (PI * i as f32 * (px as f32 + 0.5) / width as f32).cos() * by;
                    let p = linear.get(py * w + px).copied().unwrap_or([0.0; 3]);
                    for (fc, pc) in f.iter_mut().zip(p) {
                        *fc += basis * pc;
                    }
                }
            }
            let scale = norm / (w * h) as f32;
            factors.push(f.map(|c| c * scale));
        }
    }
    let mut out = String::with_capacity(4 + 2 * factors.len());
    encode83((x - 1) + (y - 1) * 9, 1, &mut out);
    let (dc, ac) = factors.split_first()?;
    let max = if ac.is_empty() {
        encode83(0, 1, &mut out);
        1.0
    } else {
        let actual = ac.iter().flatten().fold(0.0f32, |m, v| m.max(v.abs()));
        let quantised = (actual * 166.0 - 0.5).floor().clamp(0.0, 82.0) as u32;
        encode83(quantised, 1, &mut out);
        (quantised + 1) as f32 / 166.0
    };
    let [r, g, b] = dc.map(|c| u32::from(linear_to_srgb(c)));
    encode83((r << 16) + (g << 8) + b, 4, &mut out);
    for c in ac {
        let q = c.map(|v| {
            (sign_pow(v / max, 0.5) * 9.0 + 9.5)
                .floor()
                .clamp(0.0, 18.0) as u32
        });
        encode83(q[0] * 19 * 19 + q[1] * 19 + q[2], 2, &mut out);
    }
    Some(out)
}

/// The picture a hash describes, `width` × `height` pixels of straight RGBA (opaque). `None`
/// for a hash that isn't well formed (wrong length, a character outside the alphabet) or a
/// size of 0 or over 64 on a side (a stand-in is drawn small and scaled up).
#[must_use]
pub fn decode(hash: &str, width: u32, height: u32) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || width > 64 || height > 64 {
        return None;
    }
    let b = hash.as_bytes();
    let size = decode83(b.get(..1)?)?;
    let (nx, ny) = ((size % 9) + 1, (size / 9) + 1);
    if b.len() != 4 + 2 * (nx * ny) as usize {
        return None;
    }
    let max = (decode83(b.get(1..2)?)? + 1) as f32 / 166.0;
    let dc = decode83(b.get(2..6)?)?;
    if dc > 0xFF_FFFF {
        return None;
    }
    let mut colors = Vec::with_capacity((nx * ny) as usize);
    colors.push([
        srgb_to_linear((dc >> 16) as u8),
        srgb_to_linear((dc >> 8) as u8),
        srgb_to_linear(dc as u8),
    ]);
    for k in 1..(nx * ny) as usize {
        let v = decode83(b.get(4 + 2 * k..6 + 2 * k)?)?;
        if v >= 19 * 19 * 19 {
            return None;
        }
        let q = [v / (19 * 19), (v / 19) % 19, v % 19];
        colors.push(q.map(|c| sign_pow((c as f32 - 9.0) / 9.0, 2.0) * max));
    }
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for py in 0..height {
        for px in 0..width {
            let mut c = [0.0f32; 3];
            for j in 0..ny {
                let by = (PI * (py as f32 + 0.5) * j as f32 / height as f32).cos();
                for i in 0..nx {
                    let basis = (PI * (px as f32 + 0.5) * i as f32 / width as f32).cos() * by;
                    let col = colors
                        .get((i + j * nx) as usize)
                        .copied()
                        .unwrap_or([0.0; 3]);
                    for (cc, v) in c.iter_mut().zip(col) {
                        *cc += v * basis;
                    }
                }
            }
            out.extend(c.map(linear_to_srgb));
            out.push(255);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]
    use super::*;

    #[test]
    fn a_solid_colour_comes_back_as_that_colour() {
        let rgb: Vec<u8> = (0..16 * 16).flat_map(|_| [200u8, 40, 90]).collect();
        let hash = encode(4, 3, 16, 16, &rgb).unwrap();
        assert_eq!(hash.len(), 4 + 2 * 12);
        let px = decode(&hash, 8, 8).unwrap();
        // Sampled at pixel centres, a flat image comes back flat.
        let mut sum = [0u32; 3];
        for p in px.as_chunks::<4>().0 {
            for (s, v) in sum.iter_mut().zip(p) {
                *s += u32::from(*v);
            }
            assert!(p[0].abs_diff(200) <= 3 && p[1].abs_diff(40) <= 3 && p[2].abs_diff(90) <= 3);
            assert_eq!(p[3], 255);
        }
        let mean = sum.map(|s| s / 64);
        assert!(
            mean[0].abs_diff(200) <= 2 && mean[1].abs_diff(40) <= 2 && mean[2].abs_diff(90) <= 2
        );
    }

    #[test]
    fn a_left_to_right_change_survives() {
        // Dark on the left, light on the right: the decoded stand-in keeps that.
        let rgb: Vec<u8> = (0..32)
            .flat_map(|_| (0..32u8).flat_map(|x| [x * 8, x * 8, x * 8]))
            .collect();
        let hash = encode(4, 3, 32, 32, &rgb).unwrap();
        let px = decode(&hash, 16, 1).unwrap();
        assert!(px[0] + 60 < px[15 * 4]);
    }

    #[test]
    fn the_reference_hash_decodes() {
        // Wolt's README example.
        let px = decode("LEHV6nWB2yk8pyo0adR*.7kCMdnj", 32, 32).unwrap();
        assert_eq!(px.len(), 32 * 32 * 4);
    }

    #[test]
    fn malformed_hashes_and_sizes_are_refused() {
        assert!(decode("", 8, 8).is_none());
        assert!(decode("LEHV6nWB2yk8pyo0adR*.7kCMdn", 8, 8).is_none()); // one short
        assert!(decode("LEHV6nWB2yk8pyo0adR*.7kCMdnj0", 8, 8).is_none()); // one long
        assert!(decode("LEHV6nWB2yk8pyo0adR*.7kCMdn\u{e9}", 8, 8).is_none());
        assert!(decode("LEHV6nWB2yk8pyo0adR*.7kCMdnj", 0, 8).is_none());
        assert!(decode("LEHV6nWB2yk8pyo0adR*.7kCMdnj", 65, 8).is_none());
        assert!(encode(0, 3, 4, 4, &[0; 48]).is_none());
        assert!(encode(4, 3, 4, 4, &[0; 47]).is_none());
    }

    proptest::proptest! {
        #[test]
        fn decoding_any_string_never_panics(s in "\\PC{0,64}") {
            let _ = decode(&s, 8, 8);
        }

        #[test]
        fn every_encoding_decodes(w in 1u32..12, h in 1u32..12, seed in 0u8..255) {
            let rgb: Vec<u8> = (0..w * h * 3).map(|i| (i as u8).wrapping_mul(seed)).collect();
            let hash = encode(4, 3, w, h, &rgb).unwrap();
            proptest::prop_assert!(decode(&hash, 4, 4).is_some());
        }
    }
}

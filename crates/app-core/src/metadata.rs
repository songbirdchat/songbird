//! What a photo says about where and when it was taken, removed before it leaves the device
//! (D-0186). A phone's JPEG carries EXIF (GPS position, time, camera, owner), XMP and more; HD
//! photos go as they were picked, and a picker may hand over the location with them. Every
//! metadata segment goes except the colour profile (ICC) and Adobe's colour transform, which
//! change how the picture looks, and the orientation, kept as a minimal EXIF of one entry so
//! a portrait photo isn't shown on its side. PNG's text, time and EXIF chunks go too. Anything
//! not understood is sent as it was (the parser never guesses): only well-formed files change.

/// Bytes the way they go: JPEG and PNG cleaned, every other kind untouched.
#[must_use]
pub fn strip(bytes: &[u8]) -> Vec<u8> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        jpeg(bytes).unwrap_or_else(|| bytes.to_vec())
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(bytes).unwrap_or_else(|| bytes.to_vec())
    } else {
        bytes.to_vec()
    }
}

/// The EXIF orientation (1 to 8) of a TIFF block, if it has a valid one.
fn orientation(tiff: &[u8]) -> Option<u16> {
    let big = match tiff.get(..4)? {
        b"MM\0*" => true,
        b"II*\0" => false,
        _ => return None,
    };
    let u16_at = |at: usize| -> Option<u16> {
        let b: [u8; 2] = tiff.get(at..at + 2)?.try_into().ok()?;
        Some(if big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let b: [u8; 4] = tiff.get(at..at + 4)?.try_into().ok()?;
        Some(if big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    };
    let ifd = usize::try_from(u32_at(4)?).ok()?;
    let n = usize::from(u16_at(ifd)?);
    (0..n).find_map(|k| {
        let e = ifd + 2 + k * 12;
        // Tag 0x0112, type SHORT (3), one value.
        (u16_at(e)? == 0x0112 && u16_at(e + 2)? == 3 && u32_at(e + 4)? == 1)
            .then(|| u16_at(e + 8))
            .flatten()
            .filter(|o| (1..=8).contains(o))
    })
}

/// An APP1 segment holding only `orientation` (big-endian TIFF, one IFD entry).
fn exif_orientation(o: u16) -> Vec<u8> {
    let mut body = b"Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01".to_vec();
    body.extend_from_slice(&o.to_be_bytes());
    body.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    let mut seg = vec![0xFF, 0xE1];
    let len = u16::try_from(body.len() + 2).unwrap_or(u16::MAX);
    seg.extend_from_slice(&len.to_be_bytes());
    seg.extend_from_slice(&body);
    seg
}

fn jpeg(b: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(b.len());
    out.extend_from_slice(&[0xFF, 0xD8]);
    let mut at = 2;
    let mut orient = None;
    loop {
        // Markers may be padded with 0xFF bytes.
        while b.get(at) == Some(&0xFF) && b.get(at + 1) == Some(&0xFF) {
            at += 1;
        }
        if *b.get(at)? != 0xFF {
            return None;
        }
        let marker = *b.get(at + 1)?;
        if marker == 0xDA {
            // Start of scan: the picture follows, kept as it is (with the orientation first).
            if let Some(o) = orient.filter(|o| *o != 1) {
                out.extend(exif_orientation(o));
            }
            out.extend_from_slice(b.get(at..)?);
            return Some(out);
        }
        if (0xD0..=0xD7).contains(&marker) || marker == 0x01 || marker == 0xD8 {
            // No length: never before the scan in a well-formed file.
            return None;
        }
        let len = usize::from(u16::from_be_bytes(b.get(at + 2..at + 4)?.try_into().ok()?));
        if len < 2 {
            return None;
        }
        let seg = b.get(at..at + 2 + len)?;
        let payload = seg.get(4..)?;
        // Kept: APP0 (JFIF), APP2 (ICC profile), APP14 (Adobe colour transform), and the
        // tables, frame headers and rest of the picture's structure.
        let keep = match marker {
            0xE1 => {
                if let Some(tiff) = payload.strip_prefix(b"Exif\0\0") {
                    orient = orient.or_else(|| orientation(tiff));
                }
                false
            }
            // Other APPn (XMP, Photoshop, maker data) and comments.
            0xE3..=0xED | 0xEF | 0xFE => false,
            _ => true,
        };
        if keep {
            out.extend_from_slice(seg);
        }
        at += 2 + len;
    }
}

fn png(b: &[u8]) -> Option<Vec<u8>> {
    let mut out = b.get(..8)?.to_vec();
    let mut at = 8;
    while at < b.len() {
        let len = usize::try_from(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?)).ok()?;
        let kind = b.get(at + 4..at + 8)?;
        let chunk = b.get(at..at.checked_add(12)?.checked_add(len)?)?;
        // Text, time and EXIF say who, where and when; everything else draws the picture.
        if !matches!(kind, b"tEXt" | b"zTXt" | b"iTXt" | b"tIME" | b"eXIf") {
            out.extend_from_slice(chunk);
        }
        at += 12 + len;
        if kind == b"IEND" {
            return Some(out);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]
    use super::*;

    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut s = vec![0xFF, marker];
        s.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        s.extend_from_slice(payload);
        s
    }

    /// EXIF with orientation 6 and a GPS IFD pointer (little-endian, as iPhones write).
    fn exif(orientation: u16) -> Vec<u8> {
        let mut t = b"Exif\0\0II*\0\x08\0\0\0".to_vec();
        t.extend_from_slice(&2u16.to_le_bytes());
        t.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0]);
        t.extend_from_slice(&orientation.to_le_bytes());
        t.extend_from_slice(&[0, 0]);
        // GPSInfo (0x8825), LONG, pointing at a made-up GPS block.
        t.extend_from_slice(&[0x25, 0x88, 4, 0, 1, 0, 0, 0, 0x30, 0, 0, 0]);
        t.extend_from_slice(&[0, 0, 0, 0]);
        t.extend_from_slice(b"GPS 44.4268 N 26.1025 E");
        t
    }

    fn photo(app1: &[u8]) -> Vec<u8> {
        let mut j = vec![0xFF, 0xD8];
        j.extend(segment(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0"));
        j.extend(segment(0xE1, app1));
        j.extend(segment(
            0xE1,
            b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta>Bucharest</x:xmpmeta>",
        ));
        j.extend(segment(0xE2, b"ICC_PROFILE\0\x01\x01profile"));
        j.extend(segment(0xFE, b"taken by Radu"));
        j.extend(segment(0xDB, &[0; 65]));
        j.extend(segment(0xC0, &[8, 0, 16, 0, 16, 1, 1, 0x11, 0]));
        j.extend(segment(0xDA, &[1, 1, 0, 0, 0x3F, 0]));
        j.extend_from_slice(&[0x12, 0x34, 0xFF, 0x00, 0x56, 0xFF, 0xD9]);
        j
    }

    #[test]
    fn a_photo_loses_its_place_and_owner_but_not_how_it_looks_or_stands() {
        let clean = strip(&photo(&exif(6)));
        let s = String::from_utf8_lossy(&clean);
        assert!(!s.contains("GPS") && !s.contains("Bucharest") && !s.contains("Radu"));
        assert!(s.contains("JFIF") && s.contains("ICC_PROFILE"));
        // The picture's own bytes are untouched.
        assert!(clean.ends_with(&[0x12, 0x34, 0xFF, 0x00, 0x56, 0xFF, 0xD9]));
        // Its orientation stays, alone.
        let at = clean.windows(6).position(|w| w == b"Exif\0\0").unwrap();
        assert_eq!(orientation(&clean[at + 6..]), Some(6));
    }

    #[test]
    fn an_upright_photo_needs_no_exif_at_all() {
        let clean = strip(&photo(&exif(1)));
        assert!(!clean.windows(4).any(|w| w == b"Exif"));
    }

    #[test]
    fn png_text_time_and_exif_chunks_go() {
        let chunk = |kind: &[u8], data: &[u8]| {
            let mut c = u32::try_from(data.len()).unwrap().to_be_bytes().to_vec();
            c.extend_from_slice(kind);
            c.extend_from_slice(data);
            c.extend_from_slice(&[0, 0, 0, 0]);
            c
        };
        let mut p = b"\x89PNG\r\n\x1a\n".to_vec();
        p.extend(chunk(b"IHDR", &[0; 13]));
        p.extend(chunk(b"tEXt", b"Location\0Bucharest"));
        p.extend(chunk(b"eXIf", b"MM\0*GPS"));
        p.extend(chunk(b"IDAT", &[1, 2, 3]));
        p.extend(chunk(b"IEND", &[]));
        let clean = strip(&p);
        let s = String::from_utf8_lossy(&clean);
        assert!(!s.contains("Bucharest") && !s.contains("GPS"));
        assert!(s.contains("IHDR") && s.contains("IDAT") && s.contains("IEND"));
    }

    #[test]
    fn what_isnt_understood_is_left_as_it_was() {
        for b in [
            vec![0xFF, 0xD8, 0x00, 0x01],
            vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00],
            b"GIF89a....".to_vec(),
            Vec::new(),
        ] {
            assert_eq!(strip(&b), b);
        }
    }

    proptest::proptest! {
        #[test]
        fn any_bytes_are_handled(b in proptest::collection::vec(proptest::num::u8::ANY, 0..300)) {
            let mut j = vec![0xFF, 0xD8];
            j.extend(&b);
            let _ = strip(&j);
            let mut p = b"\x89PNG\r\n\x1a\n".to_vec();
            p.extend(&b);
            let _ = strip(&p);
        }
    }
}

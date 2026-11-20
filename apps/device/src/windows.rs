//! Windows: DPAPI and Windows Hello (D-0172).
//!
//! `unsafe` here: the DPAPI calls and freeing the buffer DPAPI allocates. Each block says why
//! it holds.

#![allow(
    unsafe_code,
    reason = "D-0172: DPAPI through the windows crate; each block says why it holds"
)]

use windows::{
    Security::Credentials::UI::{
        UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
    },
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    },
    core::HSTRING,
};
use zeroize::Zeroizing;

use crate::{
    biometrics::{Done, Failure, Kind},
    secure::{Error, Keep, fresh},
};

/// DPAPI's answer, copied out and its buffer freed.
fn take(out: &CRYPT_INTEGER_BLOB) -> Zeroizing<Vec<u8>> {
    let len = usize::try_from(out.cbData).unwrap_or(0);
    let v = if out.pbData.is_null() || len == 0 {
        Vec::new()
    } else {
        // SAFETY: DPAPI filled `pbData` with `cbData` bytes, valid until freed below.
        unsafe { std::slice::from_raw_parts(out.pbData, len) }.to_vec()
    };
    if !out.pbData.is_null() {
        // SAFETY: DPAPI allocated `pbData` with LocalAlloc; it is freed once, here.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
        }
    }
    Zeroizing::new(v)
}

fn protect(data: &[u8], encrypt: bool) -> Option<Zeroizing<Vec<u8>>> {
    let mut input = data.to_vec();
    let blob = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(input.len()).ok()?,
        pbData: input.as_mut_ptr(),
    };
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: `blob` points into `input`, alive for the call; `out` is written by DPAPI and
    // freed in `take`. This user's DPAPI key, no prompt.
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &raw const blob,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &raw mut out,
            )
        } else {
            CryptUnprotectData(
                &raw const blob,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &raw mut out,
            )
        }
    };
    zeroize::Zeroize::zeroize(&mut input);
    ok.ok()?;
    Some(take(&out))
}

pub(crate) fn vault_key(keep: &dyn Keep) -> Result<Zeroizing<[u8; 32]>, Error> {
    if let Some(wrapped) = keep.load() {
        let k = protect(&wrapped, false).ok_or(Error::Refused)?;
        let k: [u8; 32] = k.as_slice().try_into().map_err(|_| Error::Refused)?;
        return Ok(Zeroizing::new(k));
    }
    let k = fresh()?;
    let wrapped = protect(&k[..], true).ok_or(Error::Unavailable)?;
    keep.save(&wrapped);
    Ok(k)
}

pub(crate) fn biometrics() -> Option<Kind> {
    let a = UserConsentVerifier::CheckAvailabilityAsync()
        .ok()?
        .join()
        .ok()?;
    (a == UserConsentVerifierAvailability::Available).then_some(Kind::WindowsHello)
}

pub(crate) fn authenticate(reason: &str, done: Done) {
    let reason = HSTRING::from(reason);
    // The verifier's dialog is modal and its operation blocking-waited: off the UI thread.
    std::thread::spawn(move || {
        let r = UserConsentVerifier::RequestVerificationAsync(&reason)
            .and_then(|op| op.join())
            .map_err(|_| Failure::Unavailable)
            .and_then(|r| match r {
                UserConsentVerificationResult::Verified => Ok(()),
                UserConsentVerificationResult::Canceled => Err(Failure::Cancelled),
                UserConsentVerificationResult::RetriesExhausted => Err(Failure::NotRecognised),
                _ => Err(Failure::Unavailable),
            });
        done(r);
    });
}

/// Every top-level window of this process out of screen capture (Windows 10 2004+), or back.
pub(crate) fn protect_windows(on: bool) {
    use windows::Win32::{
        Foundation::{HWND, LPARAM},
        System::Threading::GetCurrentProcessId,
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowThreadProcessId, SetWindowDisplayAffinity,
            WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
        },
    };
    unsafe extern "system" fn each(hwnd: HWND, list: LPARAM) -> windows::core::BOOL {
        let mut pid = 0u32;
        // SAFETY: `hwnd` is a live window EnumWindows hands over; `pid` is ours to write.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
        // SAFETY: as above.
        if pid == unsafe { GetCurrentProcessId() } {
            // SAFETY: `list` is the `Vec` passed below, alive for the whole enumeration.
            unsafe { &mut *(list.0 as *mut Vec<HWND>) }.push(hwnd);
        }
        true.into()
    }
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: the callback only reads the window's process and pushes onto `windows`, which
    // outlives the call.
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&raw mut windows as isize)) };
    let affinity = if on { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
    for w in windows {
        // SAFETY: our own top-level windows, found above.
        let _ = unsafe { SetWindowDisplayAffinity(w, affinity) };
    }
}

/// One position from the Geolocator (asking the person first, as Windows does).
pub(crate) fn location(done: crate::location::Done) {
    use crate::location::{Failure, Position};
    use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator};
    std::thread::spawn(move || {
        let run = || -> Result<Position, Failure> {
            let access = Geolocator::RequestAccessAsync()
                .and_then(|op| op.join())
                .map_err(|_| Failure::Unavailable)?;
            if access != GeolocationAccessStatus::Allowed {
                return Err(Failure::Denied);
            }
            let g = Geolocator::new().map_err(|_| Failure::Unavailable)?;
            let pos = g
                .GetGeopositionAsync()
                .and_then(|op| op.join())
                .map_err(|_| Failure::Unavailable)?;
            let c = pos
                .Coordinate()
                .and_then(|c| Ok((c.Point()?.Position()?, c.Accuracy()?)))
                .map_err(|_| Failure::Unavailable)?;
            Ok(Position {
                latitude: c.0.Latitude,
                longitude: c.0.Longitude,
                accuracy_m: c.1,
            })
        };
        done(run());
    });
}

/// The icon of every top-level window of this process, from a PNG (D-0179): the taskbar shows
/// it. Windows takes PNG data as an icon resource directly.
pub(crate) fn set_icon(png: &[u8]) -> bool {
    use windows::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::Threading::GetCurrentProcessId,
        UI::WindowsAndMessaging::{
            CreateIconFromResourceEx, EnumWindows, GetSystemMetrics, GetWindowThreadProcessId,
            ICON_BIG, ICON_SMALL, LR_DEFAULTCOLOR, SM_CXICON, SM_CXSMICON, SendMessageW,
            WM_SETICON,
        },
    };
    unsafe extern "system" fn each(hwnd: HWND, list: LPARAM) -> windows::core::BOOL {
        let mut pid = 0u32;
        // SAFETY: `hwnd` is a live window EnumWindows hands over; `pid` is ours to write.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
        // SAFETY: as above.
        if pid == unsafe { GetCurrentProcessId() } {
            // SAFETY: `list` is the `Vec` passed below, alive for the whole enumeration.
            unsafe { &mut *(list.0 as *mut Vec<HWND>) }.push(hwnd);
        }
        true.into()
    }
    // The icons live as long as the process (windows keep using them): made once per change,
    // a handful in a session.
    let make = |metric| {
        // SAFETY: a plain system metric.
        let size = unsafe { GetSystemMetrics(metric) };
        // SAFETY: `png` is a whole PNG, which this call reads as an icon resource (version
        // 0x30000), scaled to `size`.
        unsafe { CreateIconFromResourceEx(png, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR) }
            .ok()
    };
    let (Some(big), Some(small)) = (make(SM_CXICON), make(SM_CXSMICON)) else {
        return false;
    };
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: the callback only reads the window's process and pushes onto `windows`, which
    // outlives the call.
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&raw mut windows as isize)) };
    for w in &windows {
        // SAFETY: our own windows, found above; the icons outlive them.
        unsafe {
            SendMessageW(
                *w,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(big.0 as isize)),
            );
            SendMessageW(
                *w,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(small.0 as isize)),
            );
        }
    }
    !windows.is_empty()
}

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

// ---------------------------------------------------------------- passkeys (D-0181)

/// Passkeys through Windows' own WebAuthn (`webauthn.dll`): Windows Hello, a security key, or a
/// phone nearby, chosen in the system's dialog. Windows doesn't bind an app to a domain, so the
/// app writes `clientDataJSON` itself with the relying party's web origin, which the server
/// checks like a browser's (`[api.keys.devices] origins`).
pub(crate) mod passkeys {
    use windows::{
        Win32::{
            Foundation::{ERROR_CANCELLED, HWND, LPARAM, NTE_USER_CANCELLED},
            Networking::WindowsWebServices::{
                WEBAUTHN_ASSERTION, WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE,
                WEBAUTHN_AUTHENTICATOR_ATTACHMENT_ANY,
                WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS,
                WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_1,
                WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS,
                WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_1, WEBAUTHN_CLIENT_DATA,
                WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
                WEBAUTHN_COSE_ALGORITHM_ECDSA_P256_WITH_SHA256,
                WEBAUTHN_COSE_ALGORITHM_RSASSA_PKCS1_V1_5_WITH_SHA256,
                WEBAUTHN_COSE_CREDENTIAL_PARAMETER,
                WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION,
                WEBAUTHN_COSE_CREDENTIAL_PARAMETERS, WEBAUTHN_CREDENTIAL,
                WEBAUTHN_CREDENTIAL_CURRENT_VERSION, WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
                WEBAUTHN_CREDENTIALS, WEBAUTHN_HASH_ALGORITHM_SHA_256,
                WEBAUTHN_RP_ENTITY_INFORMATION, WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
                WEBAUTHN_USER_ENTITY_INFORMATION, WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
                WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED, WebAuthNAuthenticatorGetAssertion,
                WebAuthNAuthenticatorMakeCredential, WebAuthNFreeAssertion,
                WebAuthNFreeCredentialAttestation, WebAuthNGetApiVersionNumber,
            },
            System::Threading::GetCurrentProcessId,
            UI::WindowsAndMessaging::{
                EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
            },
        },
        core::{HSTRING, PCWSTR},
    };

    use crate::passkeys::{AssertDone, Assertion, CreateDone, Created, Failure};

    /// Two minutes, as browsers give.
    const TIMEOUT_MS: u32 = 120_000;

    pub(crate) fn available() -> bool {
        // SAFETY: a plain query; webauthn.dll is part of Windows 10 1903 and later.
        unsafe { WebAuthNGetApiVersionNumber() >= 1 }
    }

    fn b64url(bytes: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for c in bytes.chunks(3) {
            let n = (u32::from(c[0]) << 16)
                | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
                | u32::from(*c.get(2).unwrap_or(&0));
            let take = c.len() + 1;
            for i in 0..take {
                out.push(char::from(T[((n >> (18 - 6 * i)) & 63) as usize]));
            }
        }
        out
    }

    /// `clientDataJSON` as a browser on the relying party's site writes it.
    fn client_data(kind: &str, challenge: &[u8], rp_id: &str) -> Vec<u8> {
        serde_json::json!({
            "type": kind,
            "challenge": b64url(challenge),
            "origin": format!("https://{rp_id}"),
            "crossOrigin": false,
        })
        .to_string()
        .into_bytes()
    }

    /// The window the system's dialog belongs to: Songbird's in front, else its first visible,
    /// else whichever is in front.
    fn window() -> HWND {
        unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> windows::core::BOOL {
            let mut pid = 0u32;
            // SAFETY: `hwnd` is a live window EnumWindows hands over; `pid` is ours to write.
            unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
            // SAFETY: plain queries; `found` is the HWND below, alive for the enumeration.
            unsafe {
                if pid == GetCurrentProcessId() && IsWindowVisible(hwnd).as_bool() {
                    *(found.0 as *mut HWND) = hwnd;
                    return false.into();
                }
            }
            true.into()
        }
        // SAFETY: plain queries of windows.
        unsafe {
            let front = GetForegroundWindow();
            let mut pid = 0u32;
            GetWindowThreadProcessId(front, Some(&raw mut pid));
            if pid == GetCurrentProcessId() {
                return front;
            }
            let mut found = HWND::default();
            let _ = EnumWindows(Some(each), LPARAM(&raw mut found as isize));
            // No window of ours yet (a console tool): the one in front owns the dialog.
            if found.is_invalid() { front } else { found }
        }
    }

    fn failure(e: &windows::core::Error) -> Failure {
        // The dialog's Cancel answers ERROR_CANCELLED (seen on Windows 11 25H2); some
        // authenticators answer NTE_USER_CANCELLED.
        if e.code() == NTE_USER_CANCELLED || e.code() == ERROR_CANCELLED.to_hresult() {
            Failure::Cancelled
        } else {
            Failure::NotAllowed
        }
    }

    /// Copies `n` bytes at `p` (owned by webauthn.dll until it is freed).
    fn copy(p: *const u8, n: u32) -> Vec<u8> {
        if p.is_null() || n == 0 {
            return Vec::new();
        }
        // SAFETY: webauthn.dll's answer holds `n` bytes at `p` until freed, after this copy.
        unsafe { std::slice::from_raw_parts(p, n as usize) }.to_vec()
    }

    pub(crate) fn create(
        rp_id: &str,
        user_id: &[u8],
        name: &str,
        challenge: &[u8],
        done: CreateDone,
    ) {
        let (rp_id, mut user_id, name) = (rp_id.to_owned(), user_id.to_vec(), name.to_owned());
        let mut json = client_data("webauthn.create", challenge, &rp_id);
        // The dialog is modal and blocks: on its own thread, not the app's.
        let spawned = std::thread::Builder::new()
            .name("songbird-passkey".into())
            .spawn(move || {
                let (rp_w, name_w, rp_name) = (
                    HSTRING::from(&*rp_id),
                    HSTRING::from(&*name),
                    HSTRING::from("Songbird"),
                );
                let rp = WEBAUTHN_RP_ENTITY_INFORMATION {
                    dwVersion: WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
                    pwszId: PCWSTR(rp_w.as_ptr()),
                    pwszName: PCWSTR(rp_name.as_ptr()),
                    pwszIcon: PCWSTR::null(),
                };
                let user = WEBAUTHN_USER_ENTITY_INFORMATION {
                    dwVersion: WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
                    cbId: u32::try_from(user_id.len()).unwrap_or(0),
                    pbId: user_id.as_mut_ptr(),
                    pwszName: PCWSTR(name_w.as_ptr()),
                    pwszIcon: PCWSTR::null(),
                    pwszDisplayName: PCWSTR(name_w.as_ptr()),
                };
                let mut algs = [
                    WEBAUTHN_COSE_CREDENTIAL_PARAMETER {
                        dwVersion: WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION,
                        pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
                        lAlg: WEBAUTHN_COSE_ALGORITHM_ECDSA_P256_WITH_SHA256,
                    },
                    WEBAUTHN_COSE_CREDENTIAL_PARAMETER {
                        dwVersion: WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION,
                        pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
                        lAlg: WEBAUTHN_COSE_ALGORITHM_RSASSA_PKCS1_V1_5_WITH_SHA256,
                    },
                ];
                let params = WEBAUTHN_COSE_CREDENTIAL_PARAMETERS {
                    cCredentialParameters: 2,
                    pCredentialParameters: algs.as_mut_ptr(),
                };
                let data = WEBAUTHN_CLIENT_DATA {
                    dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
                    cbClientDataJSON: u32::try_from(json.len()).unwrap_or(0),
                    pbClientDataJSON: json.as_mut_ptr(),
                    pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
                };
                let options = WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS {
                    dwVersion: WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_1,
                    dwTimeoutMilliseconds: TIMEOUT_MS,
                    dwAuthenticatorAttachment: WEBAUTHN_AUTHENTICATOR_ATTACHMENT_ANY,
                    bRequireResidentKey: true.into(),
                    dwUserVerificationRequirement: WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED,
                    dwAttestationConveyancePreference:
                        WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE,
                    ..Default::default()
                };
                // SAFETY: every pointer is to a value above, alive until the call returns; the
                // answer is copied out and freed with webauthn.dll's own function.
                let answer = unsafe {
                    WebAuthNAuthenticatorMakeCredential(
                        window(),
                        &raw const rp,
                        &raw const user,
                        &raw const params,
                        &raw const data,
                        Some(&raw const options),
                    )
                };
                match answer {
                    Ok(a) if !a.is_null() => {
                        // SAFETY: a non-null attestation from webauthn.dll, freed below.
                        let created = unsafe {
                            let r = &*a;
                            let c = Created {
                                credential_id: copy(r.pbCredentialId, r.cbCredentialId),
                                client_data_json: json.clone(),
                                attestation_object: copy(
                                    r.pbAttestationObject,
                                    r.cbAttestationObject,
                                ),
                            };
                            WebAuthNFreeCredentialAttestation(Some(a));
                            c
                        };
                        done(Ok(created));
                    }
                    Ok(_) => done(Err(Failure::NotAllowed)),
                    Err(e) => done(Err(failure(&e))),
                }
            });
        // A failed spawn drops `done`: the app's wait ends as "unavailable".
        drop(spawned);
    }

    pub(crate) fn assert(rp_id: &str, challenge: &[u8], allow: &[Vec<u8>], done: AssertDone) {
        let (rp_id, mut allow) = (rp_id.to_owned(), allow.to_vec());
        let mut json = client_data("webauthn.get", challenge, &rp_id);
        let _ = std::thread::Builder::new()
            .name("songbird-passkey".into())
            .spawn(move || {
                let rp_w = HSTRING::from(&*rp_id);
                let mut list: Vec<WEBAUTHN_CREDENTIAL> = allow
                    .iter_mut()
                    .map(|id| WEBAUTHN_CREDENTIAL {
                        dwVersion: WEBAUTHN_CREDENTIAL_CURRENT_VERSION,
                        cbId: u32::try_from(id.len()).unwrap_or(0),
                        pbId: id.as_mut_ptr(),
                        pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
                    })
                    .collect();
                let data = WEBAUTHN_CLIENT_DATA {
                    dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
                    cbClientDataJSON: u32::try_from(json.len()).unwrap_or(0),
                    pbClientDataJSON: json.as_mut_ptr(),
                    pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
                };
                let options = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS {
                    dwVersion: WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_1,
                    dwTimeoutMilliseconds: TIMEOUT_MS,
                    CredentialList: WEBAUTHN_CREDENTIALS {
                        cCredentials: u32::try_from(list.len()).unwrap_or(0),
                        pCredentials: list.as_mut_ptr(),
                    },
                    dwAuthenticatorAttachment: WEBAUTHN_AUTHENTICATOR_ATTACHMENT_ANY,
                    dwUserVerificationRequirement: WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED,
                    ..Default::default()
                };
                // SAFETY: as for making one: our values outlive the call; the answer is copied and
                // freed with webauthn.dll's function.
                let answer = unsafe {
                    WebAuthNAuthenticatorGetAssertion(
                        window(),
                        &rp_w,
                        &raw const data,
                        Some(&raw const options),
                    )
                };
                match answer {
                    Ok(a) if !a.is_null() => {
                        // SAFETY: a non-null assertion from webauthn.dll, freed below.
                        let assertion = unsafe {
                            let r: &WEBAUTHN_ASSERTION = &*a;
                            let out = Assertion {
                                credential_id: copy(r.Credential.pbId, r.Credential.cbId),
                                authenticator_data: copy(
                                    r.pbAuthenticatorData,
                                    r.cbAuthenticatorData,
                                ),
                                client_data_json: json.clone(),
                                signature: copy(r.pbSignature, r.cbSignature),
                            };
                            WebAuthNFreeAssertion(a);
                            out
                        };
                        done(Ok(assertion));
                    }
                    Ok(_) => done(Err(Failure::NotAllowed)),
                    Err(e) => done(Err(failure(&e))),
                }
            });
    }
}

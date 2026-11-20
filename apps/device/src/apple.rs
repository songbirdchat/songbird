//! macOS and iOS: the Keychain and LocalAuthentication (D-0172).
//!
//! `unsafe` here: Objective-C messages to `LAContext` (objc2 marks every framework method
//! unsafe); each block says why it holds.

#![allow(
    unsafe_code,
    reason = "D-0172: LocalAuthentication through objc2; each block says why it holds"
)]

use std::sync::Mutex;

use block2::RcBlock;
use objc2::{rc::Retained, runtime::Bool};
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LABiometryType, LAContext, LAPolicy};
#[cfg(target_os = "ios")]
use security_framework::access_control::{ProtectionMode, SecAccessControl};
use security_framework::passwords::{
    PasswordOptions, generic_password, set_generic_password_options,
};
use zeroize::Zeroizing;

use crate::{
    biometrics::{Done, Failure, Kind},
    secure::{Error, fresh},
};

const SERVICE: &str = "chat.songbird.app";
const ACCOUNT: &str = "vault";
/// `errSecItemNotFound`.
const NOT_FOUND: i32 = -25300;

fn options() -> PasswordOptions {
    PasswordOptions::new_generic_password(SERVICE, ACCOUNT)
}

pub(crate) fn vault_key() -> Result<Zeroizing<[u8; 32]>, Error> {
    match generic_password(options()) {
        Ok(v) => {
            let v = Zeroizing::new(v);
            let k: [u8; 32] = v.as_slice().try_into().map_err(|_| Error::Refused)?;
            Ok(Zeroizing::new(k))
        }
        Err(e) if e.code() == NOT_FOUND => {
            let k = fresh()?;
            let mut o = options();
            // Never synced to iCloud Keychain.
            o.set_access_synchronized(Some(false));
            // iOS: this device only, readable once unlocked after a restart, never in a device
            // backup. macOS keeps it in the login keychain (local, unlocked with the session):
            // the data-protection keychain needs a signing entitlement a development build
            // doesn't have.
            #[cfg(target_os = "ios")]
            if let Ok(ac) = SecAccessControl::create_with_protection(
                Some(ProtectionMode::AccessibleAfterFirstUnlockThisDeviceOnly),
                0,
            ) {
                o.set_access_control(ac);
            }
            set_generic_password_options(&k[..], o).map_err(|_| Error::Unavailable)?;
            Ok(k)
        }
        Err(_) => Err(Error::Unavailable),
    }
}

pub(crate) fn biometrics() -> Option<Kind> {
    // SAFETY: a fresh context, used on this thread only; `canEvaluatePolicy` has no side effect.
    let ctx = unsafe { LAContext::new() };
    // SAFETY: as above.
    unsafe { ctx.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthenticationWithBiometrics) }
        .ok()?;
    // SAFETY: as above; read after `canEvaluatePolicy`, as the documentation asks.
    match unsafe { ctx.biometryType() } {
        LABiometryType::FaceID => Some(Kind::FaceId),
        LABiometryType::TouchID => Some(Kind::TouchId),
        LABiometryType::OpticID => Some(Kind::OpticId),
        _ => None,
    }
}

pub(crate) fn authenticate(reason: &str, done: Done) {
    // SAFETY: a fresh context.
    let ctx = unsafe { LAContext::new() };
    // The system offers its own "Enter passcode" fallback otherwise; the app's passcode is the
    // fallback here, so the button is hidden (an empty title).
    // SAFETY: setting a property on our own context.
    unsafe { ctx.setLocalizedFallbackTitle(Some(&NSString::from_str(""))) };
    // The reply holds the context: it must live until the reply, and the framework holds the
    // reply until then.
    let keep: Retained<LAContext> = ctx.clone();
    let done = Mutex::new(Some(done));
    let reply = RcBlock::new(move |ok: Bool, err: *mut NSError| {
        let _alive = &keep;
        let result = if ok.as_bool() {
            Ok(())
        } else {
            // SAFETY: the framework passes a valid error or null, alive for this call.
            let code = unsafe { err.as_ref() }.map_or(0, NSError::code);
            Err(match code {
                -1 => Failure::NotRecognised,
                -2 | -3 | -4 | -9 => Failure::Cancelled,
                _ => Failure::Unavailable,
            })
        };
        if let Some(d) = done.lock().ok().and_then(|mut d| d.take()) {
            d(result);
        }
    });
    let reason = if reason.is_empty() {
        "unlock Songbird"
    } else {
        reason
    };
    // SAFETY: the reply keeps the context alive until it runs; the reason is non-empty, as
    // the method requires; the block is `Send`-safe (it only touches a Mutex and a `Send`
    // callback).
    unsafe {
        ctx.evaluatePolicy_localizedReason_reply(
            LAPolicy::DeviceOwnerAuthenticationWithBiometrics,
            &NSString::from_str(reason),
            &reply,
        );
    }
}

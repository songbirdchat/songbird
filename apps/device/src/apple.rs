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
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let ctx = unsafe { LAContext::new() };
    // SAFETY: as above.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe { ctx.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthenticationWithBiometrics) }
        .ok()?;
    // SAFETY: as above; read after `canEvaluatePolicy`, as the documentation asks.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    match unsafe { ctx.biometryType() } {
        LABiometryType::FaceID => Some(Kind::FaceId),
        LABiometryType::TouchID => Some(Kind::TouchId),
        LABiometryType::OpticID => Some(Kind::OpticId),
        _ => None,
    }
}

pub(crate) fn authenticate(reason: &str, done: Done) {
    // SAFETY: a fresh context.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let ctx = unsafe { LAContext::new() };
    // The system offers its own "Enter passcode" fallback otherwise; the app's passcode is the
    // fallback here, so the button is hidden (an empty title).
    // SAFETY: setting a property on our own context.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
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
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
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
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe {
        ctx.evaluatePolicy_localizedReason_reply(
            LAPolicy::DeviceOwnerAuthenticationWithBiometrics,
            &NSString::from_str(reason),
            &reply,
        );
    }
}

/// Runs `f` on the main thread, where AppKit and UIKit take calls. NSH runs the app's code on
/// its engine thread, so a call from the app is queued on the main queue; one from a system
/// callback, already on the main thread, runs at once.
pub(crate) fn on_main(f: impl FnOnce(objc2::MainThreadMarker) + Send + 'static) {
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        return f(mtm);
    }
    dispatch2::DispatchQueue::main().exec_async(move || {
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            f(mtm);
        }
    });
}

/// macOS: every window of the app out of screen capture, or back in.
#[cfg(target_os = "macos")]
pub(crate) fn protect_windows(on: bool) {
    use objc2_app_kit::{NSApplication, NSWindowSharingType};
    on_main(move |mtm| {
        let sharing = if on {
            NSWindowSharingType::None
        } else {
            NSWindowSharingType::ReadOnly
        };
        let windows = NSApplication::sharedApplication(mtm).windows();
        for i in 0..windows.count() {
            windows.objectAtIndex(i).setSharingType(sharing);
        }
    });
}

/// iOS: whether the main screen is recorded, mirrored or shared.
#[cfg(target_os = "ios")]
#[allow(
    deprecated,
    reason = "UIScreen.mainScreen is still the screen the app shows on"
)]
pub(crate) fn captured() -> bool {
    use objc2_ui_kit::UIScreen;
    use std::sync::atomic::{AtomicBool, Ordering};
    // UIKit answers on the main thread only: the answer is read there and kept, and the app
    // (which asks every moment it shows) sees it at its next look.
    static CAPTURED: AtomicBool = AtomicBool::new(false);
    on_main(|mtm| CAPTURED.store(UIScreen::mainScreen(mtm).isCaptured(), Ordering::Relaxed));
    CAPTURED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------- photos (D-0174)

/// iOS: add-only access, then one asset from a temporary file (moved into the library).
#[cfg(target_os = "ios")]
pub(crate) fn save_media(
    bytes: &[u8],
    kind: crate::photos::Kind,
    name: &str,
    done: crate::photos::Done,
) {
    use objc2_foundation::{NSString, NSURL};
    use objc2_photos::{
        PHAccessLevel, PHAssetCreationRequest, PHAssetResourceCreationOptions, PHAssetResourceType,
        PHAuthorizationStatus, PHPhotoLibrary,
    };

    use crate::photos::{Failure, Kind};

    // iOS: the app's own sandboxed tmp (no other process can reach it); Photos moves the file out and it is removed after (D-0185).
    // nosemgrep: rust.lang.security.temp-dir.temp-dir
    let file = std::env::temp_dir().join(format!(
        "songbird-save-{}-{}",
        std::process::id(),
        crate::photos::safe_name(name)
    ));
    if std::fs::write(&file, bytes).is_err() {
        done(Err(Failure::Failed));
        return;
    }
    let done = std::sync::Arc::new(Mutex::new(Some(done)));
    let finish = move |r: Result<(), Failure>| {
        if let Some(d) = done.lock().ok().and_then(|mut d| d.take()) {
            d(r);
        }
    };
    let path = file.to_string_lossy().into_owned();
    let handler = RcBlock::new(move |status: PHAuthorizationStatus| {
        if status != PHAuthorizationStatus::Authorized && status != PHAuthorizationStatus::Limited {
            let _ = std::fs::remove_file(&path);
            finish(Err(Failure::Denied));
            return;
        }
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
        let resource = match kind {
            Kind::Photo => PHAssetResourceType::Photo,
            Kind::Video => PHAssetResourceType::Video,
        };
        let change = RcBlock::new(move || {
            // SAFETY: inside the library's change block, as `creationRequestForAsset` requires.
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            let request = unsafe { PHAssetCreationRequest::creationRequestForAsset() };
            // SAFETY: a fresh options object; the file is moved into the library (none left).
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            let options = unsafe { PHAssetResourceCreationOptions::new() };
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            unsafe {
                options.setShouldMoveFile(true);
                request.addResourceWithType_fileURL_options(resource, &url, Some(&options));
            }
        });
        let path2 = path.clone();
        let finish2 = finish.clone();
        let completion = RcBlock::new(move |ok: Bool, _err: *mut NSError| {
            let _ = std::fs::remove_file(&path2);
            finish2(if ok.as_bool() {
                Ok(())
            } else {
                Err(Failure::Failed)
            });
        });
        // SAFETY: the shared library; the blocks live until the library calls them.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        unsafe {
            PHPhotoLibrary::sharedPhotoLibrary()
                .performChanges_completionHandler(RcBlock::as_ptr(&change), Some(&completion));
        }
    });
    // SAFETY: a class method; the handler is called once, on a queue of the system's.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe {
        PHPhotoLibrary::requestAuthorizationForAccessLevel_handler(
            PHAccessLevel::AddOnly,
            &handler,
        );
    }
}

// ---------------------------------------------------------------- call audio (D-0176)

/// iOS: the call's sound to the loudspeaker, or back to the route WebRTC's session chose.
#[cfg(target_os = "ios")]
pub(crate) fn speaker(on: bool) {
    use objc2_avf_audio::{AVAudioSession, AVAudioSessionPortOverride};
    let port = if on {
        AVAudioSessionPortOverride::Speaker
    } else {
        AVAudioSessionPortOverride::None
    };
    // SAFETY: the app's shared audio session; overriding the output port has no other effect.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let _ = unsafe { AVAudioSession::sharedInstance().overrideOutputAudioPort_error(port) };
}

// ---------------------------------------------------------------- PushKit and CallKit (D-0176)

#[cfg(target_os = "ios")]
pub(crate) mod callkit {
    use std::cell::RefCell;

    use block2::DynBlock;
    use objc2::{
        AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject, ProtocolObject},
    };
    use objc2_avf_audio::AVAudioSession;
    use objc2_call_kit::{
        CXAnswerCallAction, CXCallEndedReason, CXCallUpdate, CXEndCallAction, CXHandle,
        CXHandleType, CXProvider, CXProviderConfiguration, CXProviderDelegate,
        CXSetMutedCallAction,
    };
    use objc2_foundation::{
        NSError, NSNumber, NSObject, NSObjectProtocol, NSSet, NSString, NSUUID,
    };
    use objc2_push_kit::{
        PKPushCredentials, PKPushPayload, PKPushRegistry, PKPushRegistryDelegate, PKPushType,
        PKPushTypeVoIP,
    };

    use crate::callkit::{Ended, Event, OnEvent};

    pub(crate) struct Ivars {
        on: OnEvent,
    }

    thread_local! {
        static PROVIDER: RefCell<Option<Retained<CXProvider>>> = const { RefCell::new(None) };
        static KEEP: RefCell<Vec<Retained<AnyObject>>> = const { RefCell::new(Vec::new()) };
    }

    /// WebRTC's `RTCAudioSession` (in libwebrtc's Objective-C SDK), when linked.
    fn rtc_session() -> Option<Retained<AnyObject>> {
        let cls = AnyClass::get(c"RTCAudioSession")?;
        // SAFETY: a class method returning the shared instance.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let s: Option<Retained<AnyObject>> = unsafe { msg_send![cls, sharedInstance] };
        s
    }

    /// WebRTC's audio waits for CallKit (`true`), or runs as usual.
    fn manual_audio(on: bool) {
        if let Some(s) = rtc_session() {
            // SAFETY: documented RTCAudioSession properties.
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            unsafe {
                let _: () = msg_send![&*s, setUseManualAudio: on];
                let _: () = msg_send![&*s, setIsAudioEnabled: !on];
            }
        }
    }

    define_class!(
        // SAFETY: plain NSObject subclasses; both delegates are called on the main queue.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "SongbirdCallDelegate"]
        #[ivars = Ivars]
        pub(crate) struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl PKPushRegistryDelegate for Delegate {
            #[unsafe(method(pushRegistry:didUpdatePushCredentials:forType:))]
            fn credentials(&self, _r: &PKPushRegistry, creds: &PKPushCredentials, _t: &PKPushType) {
                // SAFETY: reading the token PushKit handed over.
                let token = unsafe { creds.token() }.to_vec();
                (self.ivars().on)(Event::Token(token));
            }

            #[unsafe(method(pushRegistry:didReceiveIncomingPushWithPayload:forType:withCompletionHandler:))]
            fn incoming(
                &self,
                _r: &PKPushRegistry,
                _payload: &PKPushPayload,
                _t: &PKPushType,
                completion: &DynBlock<dyn Fn()>,
            ) {
                let mut id = [0u8; 16];
                let _ = crate::random(&mut id);
                // The UUID must be a valid v4 for CallKit.
                id[6] = (id[6] & 0x0f) | 0x40;
                id[8] = (id[8] & 0x3f) | 0x80;
                report_incoming(id, "Songbird", false, completion);
                (self.ivars().on)(Event::Ring(id));
            }
        }

        unsafe impl CXProviderDelegate for Delegate {
            #[unsafe(method(providerDidReset:))]
            fn reset(&self, _p: &CXProvider) {
                manual_audio(false);
            }

            #[unsafe(method(provider:performAnswerCallAction:))]
            fn answer(&self, _p: &CXProvider, action: &CXAnswerCallAction) {
                // SAFETY: reading and completing the action CallKit handed over.
                let id = unsafe { action.callUUID() }.as_bytes();
                unsafe { action.fulfill() };
                (self.ivars().on)(Event::Answer(id));
            }

            #[unsafe(method(provider:performEndCallAction:))]
            fn end(&self, _p: &CXProvider, action: &CXEndCallAction) {
                // SAFETY: as above.
                let id = unsafe { action.callUUID() }.as_bytes();
                unsafe { action.fulfill() };
                (self.ivars().on)(Event::End(id));
            }

            #[unsafe(method(provider:performSetMutedCallAction:))]
            fn mute(&self, _p: &CXProvider, action: &CXSetMutedCallAction) {
                // SAFETY: as above.
                let (id, muted) = unsafe { (action.callUUID().as_bytes(), action.isMuted()) };
                unsafe { action.fulfill() };
                (self.ivars().on)(Event::Mute(id, muted));
            }

            #[unsafe(method(provider:didActivateAudioSession:))]
            fn activated(&self, _p: &CXProvider, session: &AVAudioSession) {
                if let Some(s) = rtc_session() {
                    // SAFETY: documented RTCAudioSession calls, with the session CallKit gave.
                    unsafe {
                        let _: () = msg_send![&*s, audioSessionDidActivate: session];
                        let _: () = msg_send![&*s, setIsAudioEnabled: true];
                    }
                }
            }

            #[unsafe(method(provider:didDeactivateAudioSession:))]
            fn deactivated(&self, _p: &CXProvider, session: &AVAudioSession) {
                if let Some(s) = rtc_session() {
                    // SAFETY: as above.
                    unsafe {
                        let _: () = msg_send![&*s, audioSessionDidDeactivate: session];
                        let _: () = msg_send![&*s, setIsAudioEnabled: false];
                    }
                }
            }
        }
    );

    fn call_update(caller: &str, video: bool) -> Retained<CXCallUpdate> {
        // SAFETY: fresh objects, configured before use.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        unsafe {
            let u = CXCallUpdate::new();
            let handle = CXHandle::initWithType_value(
                CXHandle::alloc(),
                CXHandleType::Generic,
                &NSString::from_str(caller),
            );
            u.setRemoteHandle(Some(&handle));
            u.setLocalizedCallerName(Some(&NSString::from_str(caller)));
            u.setHasVideo(video);
            u
        }
    }

    fn report_incoming(id: [u8; 16], caller: &str, video: bool, completion: &DynBlock<dyn Fn()>) {
        manual_audio(true);
        let update = call_update(caller, video);
        let done = block2::RcBlock::new(|_err: *mut NSError| {});
        PROVIDER.with_borrow(|p| {
            if let Some(p) = p {
                // SAFETY: our provider, on the main queue; the block lives for the call.
                // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
                unsafe {
                    p.reportNewIncomingCallWithUUID_update_completion(
                        &NSUUID::from_bytes(id),
                        &update,
                        &done,
                    );
                }
            }
        });
        // PushKit's completion: the call is reported.
        completion.call(());
    }

    pub(crate) fn install(on: OnEvent) -> bool {
        super::on_main(move |mtm| install_on_main(mtm, on));
        true
    }

    fn install_on_main(mtm: MainThreadMarker, on: OnEvent) {
        let delegate: Retained<Delegate> = {
            let this = Delegate::alloc(mtm).set_ivars(Ivars { on });
            // SAFETY: NSObject's designated initializer.
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            unsafe { msg_send![super(this), init] }
        };
        // SAFETY: CallKit and PushKit set up on the main thread, their delegate kept for the
        // life of the app (KEEP), the main queue for both.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        unsafe {
            let config = CXProviderConfiguration::new();
            config.setSupportsVideo(true);
            config.setMaximumCallGroups(1);
            config.setMaximumCallsPerCallGroup(1);
            config.setIncludesCallsInRecents(false);
            config.setSupportedHandleTypes(&NSSet::from_slice(&[&*NSNumber::new_isize(
                CXHandleType::Generic.0,
            )]));
            let provider = CXProvider::initWithConfiguration(CXProvider::alloc(), &config);
            provider.setDelegate_queue(
                Some(ProtocolObject::from_ref(&*delegate)),
                Some(dispatch2::DispatchQueue::main()),
            );
            PROVIDER.with_borrow_mut(|p| *p = Some(provider));
            let registry = PKPushRegistry::initWithQueue(
                PKPushRegistry::alloc(),
                Some(dispatch2::DispatchQueue::main()),
            );
            registry.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            registry.setDesiredPushTypes(Some(&NSSet::from_slice(&[PKPushTypeVoIP])));
            KEEP.with_borrow_mut(|k| {
                k.push(Retained::into_super(Retained::into_super(registry)));
                k.push(Retained::into_super(delegate).into());
            });
        }
    }

    pub(crate) fn update(id: [u8; 16], caller: &str, video: bool) {
        let caller = caller.to_owned();
        super::on_main(move |_| {
            let u = call_update(&caller, video);
            PROVIDER.with_borrow(|p| {
                if let Some(p) = p {
                    // SAFETY: our provider on the main thread.
                    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
                    unsafe { p.reportCallWithUUID_updated(&NSUUID::from_bytes(id), &u) };
                }
            });
        });
    }

    pub(crate) fn ended(id: [u8; 16], why: Ended) {
        let reason = match why {
            Ended::Failed => CXCallEndedReason::Failed,
            Ended::Remote => CXCallEndedReason::RemoteEnded,
            Ended::Unanswered => CXCallEndedReason::Unanswered,
            Ended::AnsweredElsewhere => CXCallEndedReason::AnsweredElsewhere,
            Ended::DeclinedElsewhere => CXCallEndedReason::DeclinedElsewhere,
        };
        super::on_main(move |_| {
            PROVIDER.with_borrow(|p| {
                if let Some(p) = p {
                    // SAFETY: as above.
                    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
                    unsafe {
                        p.reportCallWithUUID_endedAtDate_reason(
                            &NSUUID::from_bytes(id),
                            None,
                            reason,
                        );
                    }
                }
            });
            manual_audio(false);
        });
    }
}

// ---------------------------------------------------------------- passkeys (D-0177)

pub(crate) mod passkeys {
    use std::cell::RefCell;

    use objc2::{
        AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
        rc::Retained, runtime::AnyObject,
    };
    use objc2_authentication_services::{
        ASAuthorizationController, ASAuthorizationPlatformPublicKeyCredentialDescriptor,
        ASAuthorizationPlatformPublicKeyCredentialProvider, ASAuthorizationRequest,
    };
    use objc2_foundation::{NSArray, NSData, NSError, NSObject, NSString};

    use crate::passkeys::{AssertDone, Assertion, CreateDone, Created, Failure};

    enum Done {
        Create(CreateDone),
        Assert(AssertDone),
    }

    pub(crate) struct Ivars {
        done: RefCell<Option<Done>>,
    }

    thread_local! {
        static LIVE: RefCell<Vec<(Retained<Delegate>, Retained<ASAuthorizationController>)>> =
            const { RefCell::new(Vec::new()) };
    }

    fn data(obj: &AnyObject, sel: objc2::runtime::Sel) -> Vec<u8> {
        // SAFETY: the credential's documented `NSData` properties, looked up by selector.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let d: Option<Retained<NSData>> = unsafe { msg_send![obj, performSelector: sel] };
        d.map(|d| d.to_vec()).unwrap_or_default()
    }

    define_class!(
        // SAFETY: a plain NSObject; the controller calls it on the main thread.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "SongbirdPasskeyDelegate"]
        #[ivars = Ivars]
        pub(crate) struct Delegate;

        impl Delegate {
            #[unsafe(method(authorizationController:didCompleteWithAuthorization:))]
            fn done_ok(&self, _c: &ASAuthorizationController, auth: &AnyObject) {
                // SAFETY: `credential` of the ASAuthorization handed over.
                let cred: Retained<AnyObject> = unsafe { msg_send![auth, credential] };
                let id = data(&cred, objc2::sel!(credentialID));
                let client = data(&cred, objc2::sel!(rawClientDataJSON));
                match self.ivars().done.borrow_mut().take() {
                    Some(Done::Create(d)) => d(Ok(Created {
                        credential_id: id,
                        client_data_json: client,
                        attestation_object: data(&cred, objc2::sel!(rawAttestationObject)),
                    })),
                    Some(Done::Assert(d)) => d(Ok(Assertion {
                        credential_id: id,
                        authenticator_data: data(&cred, objc2::sel!(rawAuthenticatorData)),
                        client_data_json: client,
                        signature: data(&cred, objc2::sel!(signature)),
                    })),
                    None => {}
                }
            }

            #[unsafe(method(authorizationController:didCompleteWithError:))]
            fn done_err(&self, _c: &ASAuthorizationController, err: &NSError) {
                // ASAuthorizationErrorCanceled is 1001.
                let f = if err.code() == 1001 {
                    Failure::Cancelled
                } else {
                    Failure::NotAllowed
                };
                match self.ivars().done.borrow_mut().take() {
                    Some(Done::Create(d)) => d(Err(f)),
                    Some(Done::Assert(d)) => d(Err(f)),
                    None => {}
                }
            }

            #[unsafe(method_id(presentationAnchorForAuthorizationController:))]
            fn anchor(&self, _c: &ASAuthorizationController) -> Retained<AnyObject> {
                window()
            }
        }
    );

    /// The window the system's sheet shows over.
    fn window() -> Retained<AnyObject> {
        #[cfg(target_os = "ios")]
        {
            let mtm = MainThreadMarker::new().expect("main thread");
            let app = objc2_ui_kit::UIApplication::sharedApplication(mtm);
            // SAFETY: the app's windows; the first is the one NSH shows.
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            let windows: Retained<NSArray<AnyObject>> = unsafe { msg_send![&*app, windows] };
            windows.firstObject().expect("a window")
        }
        #[cfg(target_os = "macos")]
        {
            let mtm = MainThreadMarker::new().expect("main thread");
            let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
            let w = app.keyWindow().or_else(|| app.mainWindow()).or_else(|| {
                let ws = app.windows();
                (ws.count() > 0).then(|| ws.objectAtIndex(0))
            });
            Retained::into_super(Retained::into_super(Retained::into_super(
                w.expect("a window"),
            )))
        }
    }

    fn run(mtm: MainThreadMarker, request: Retained<ASAuthorizationRequest>, done: Done) {
        let delegate: Retained<Delegate> = {
            let this = Delegate::alloc(mtm).set_ivars(Ivars {
                done: RefCell::new(Some(done)),
            });
            // SAFETY: NSObject's designated initializer.
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            unsafe { msg_send![super(this), init] }
        };
        // SAFETY: a controller for one request, its delegate and presentation provider ours,
        // both kept alive (LIVE) until the next request.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        unsafe {
            let controller = ASAuthorizationController::initWithAuthorizationRequests(
                ASAuthorizationController::alloc(),
                &NSArray::from_retained_slice(&[request]),
            );
            let _: () = msg_send![&*controller, setDelegate: &*delegate];
            let _: () = msg_send![&*controller, setPresentationContextProvider: &*delegate];
            controller.performRequests();
            LIVE.with_borrow_mut(|l| {
                l.retain(|(d, _)| d.ivars().done.borrow().is_some());
                l.push((delegate, controller));
            });
        }
    }

    fn provider(rp_id: &str) -> Retained<ASAuthorizationPlatformPublicKeyCredentialProvider> {
        // SAFETY: a provider for the relying party.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        unsafe {
            ASAuthorizationPlatformPublicKeyCredentialProvider::initWithRelyingPartyIdentifier(
                ASAuthorizationPlatformPublicKeyCredentialProvider::alloc(),
                &NSString::from_str(rp_id),
            )
        }
    }

    pub(crate) fn create(
        rp_id: &str,
        user_id: &[u8],
        name: &str,
        challenge: &[u8],
        done: CreateDone,
    ) {
        let (rp_id, user_id, name, challenge) = (
            rp_id.to_owned(),
            user_id.to_vec(),
            name.to_owned(),
            challenge.to_vec(),
        );
        super::on_main(move |mtm| {
            // SAFETY: a registration request from our provider.
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            let req = unsafe {
                provider(&rp_id).createCredentialRegistrationRequestWithChallenge_name_userID(
                    &NSData::with_bytes(&challenge),
                    &NSString::from_str(&name),
                    &NSData::with_bytes(&user_id),
                )
            };
            run(mtm, Retained::into_super(req), Done::Create(done));
        });
    }

    pub(crate) fn assert(rp_id: &str, challenge: &[u8], allow: &[Vec<u8>], done: AssertDone) {
        let (rp_id, challenge, allow) = (rp_id.to_owned(), challenge.to_vec(), allow.to_vec());
        super::on_main(move |mtm| assert_on_main(mtm, &rp_id, &challenge, &allow, done));
    }

    fn assert_on_main(
        mtm: MainThreadMarker,
        rp_id: &str,
        challenge: &[u8],
        allow: &[Vec<u8>],
        done: AssertDone,
    ) {
        // SAFETY: an assertion request from our provider, its allowed credentials set before use.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let req = unsafe {
            let r = provider(rp_id)
                .createCredentialAssertionRequestWithChallenge(&NSData::with_bytes(challenge));
            if !allow.is_empty() {
                let list: Vec<_> = allow
                    .iter()
                    .map(|id| {
                        ASAuthorizationPlatformPublicKeyCredentialDescriptor::initWithCredentialID(
                            ASAuthorizationPlatformPublicKeyCredentialDescriptor::alloc(),
                            &NSData::with_bytes(id),
                        )
                    })
                    .collect();
                r.setAllowedCredentials(&NSArray::from_retained_slice(&list));
            }
            r
        };
        run(mtm, Retained::into_super(req), Done::Assert(done));
    }
}

// ---------------------------------------------------------------- StoreKit 2 (D-0178)

pub(crate) mod store {
    use std::{
        collections::HashMap,
        ffi::{CStr, CString, c_char},
        sync::{
            Mutex,
            atomic::{AtomicU64, Ordering},
        },
    };

    use crate::store::{Done, Failure, Proof};

    type Callback = unsafe extern "C" fn(u64, i32, *const c_char);

    unsafe extern "C" {
        fn sb_store_buy(product: *const c_char, id: u64, done: Callback);
        fn sb_store_current(id: u64, done: Callback);
    }

    static WAITING: Mutex<Option<HashMap<u64, Done>>> = Mutex::new(None);
    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn park(done: Done) -> u64 {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        WAITING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_or_insert_with(HashMap::new)
            .insert(id, done);
        id
    }

    unsafe extern "C" fn answered(id: u64, code: i32, jws: *const c_char) {
        let done = WAITING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
            .and_then(|w| w.remove(&id));
        let Some(done) = done else { return };
        let r = match code {
            0 if !jws.is_null() => {
                // SAFETY: the Swift side passes a NUL-terminated string alive for this call.
                // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
                let s = unsafe { CStr::from_ptr(jws) }
                    .to_string_lossy()
                    .into_owned();
                Ok(Proof::Apple(s))
            }
            1 => Err(Failure::Cancelled),
            2 => Err(Failure::NotFound),
            _ => Err(Failure::Failed),
        };
        done(r);
    }

    pub(crate) fn buy(product: &str, done: Done) {
        let Ok(p) = CString::new(product) else {
            done(Err(Failure::NotFound));
            return;
        };
        let id = park(done);
        // SAFETY: `p` outlives the call (the Swift side copies it at once); the callback is a
        // plain C function.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        unsafe { sb_store_buy(p.as_ptr(), id, answered) };
    }

    pub(crate) fn current(done: Done) {
        let id = park(done);
        // SAFETY: as above.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        unsafe { sb_store_current(id, answered) };
    }
}

// ---------------------------------------------------------------- the app's icon (D-0179)

/// iOS: the alternate icons in the app's asset catalog.
#[cfg(target_os = "ios")]
pub(crate) mod icon {
    use std::sync::{Mutex, PoisonError};

    use objc2::MainThreadMarker;
    use objc2_foundation::{NSError, NSString};
    use objc2_ui_kit::UIApplication;

    /// Whether the bundle declares alternate icons (`alternate-icons.sh` adds them). Read from
    /// Info.plist, which any thread may: NSH builds screens off the main thread.
    pub(crate) fn supported() -> bool {
        use objc2_foundation::{NSBundle, NSDictionary, ns_string};
        NSBundle::mainBundle()
            .objectForInfoDictionaryKey(ns_string!("CFBundleIcons"))
            .and_then(|icons| icons.downcast::<NSDictionary>().ok())
            .is_some_and(|icons| {
                icons
                    .objectForKey(ns_string!("CFBundleAlternateIcons"))
                    .is_some()
            })
    }

    /// Changes the icon on the main thread, where UIKit takes it.
    pub(crate) fn set(name: Option<&str>, done: crate::icon::Done) {
        let name = name.map(str::to_owned);
        super::on_main(move |mtm| change(mtm, name.as_deref(), done));
    }

    fn change(mtm: MainThreadMarker, name: Option<&str>, done: crate::icon::Done) {
        let app = UIApplication::sharedApplication(mtm);
        if !app.supportsAlternateIcons() {
            return done(false);
        }
        let done = Mutex::new(Some(done));
        let reply = block2::RcBlock::new(move |error: *mut NSError| {
            if let Some(done) = done.lock().unwrap_or_else(PoisonError::into_inner).take() {
                done(error.is_null());
            }
        });
        let name = name.map(NSString::from_str);
        app.setAlternateIconName_completionHandler(name.as_deref(), Some(&reply));
    }
}

/// macOS: the Dock's icon while the app runs.
#[cfg(target_os = "macos")]
pub(crate) mod icon {
    use objc2::AnyThread as _;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    pub(crate) fn set(png: &'static [u8], done: crate::icon::Done) {
        super::on_main(move |mtm| {
            let Some(image) = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(png))
            else {
                return done(false);
            };
            // SAFETY: the setter is unsafe only because it may take `None`; this passes an image.
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
            done(true);
        });
    }
}

// ---------------------------------------------------------------- screen sharing (D-0180)

/// iOS: the screen from Songbird's ReplayKit broadcast extension (`apps/ios/ScreenShare`).
/// The app listens on a socket in the app group's container (only Songbird's processes reach
/// it) and opens the system's broadcast sheet with the extension chosen; once the person
/// starts it, the extension connects and sends each frame as NV12:
/// `"SBS1"`, width, height, rotation (u32, little-endian), then the luma rows and the `CbCr`
/// rows, packed.
#[cfg(target_os = "ios")]
pub(crate) mod share {
    use std::{
        cell::RefCell,
        io::Read,
        net::Shutdown,
        os::unix::{
            fs::PermissionsExt,
            net::{UnixListener, UnixStream},
        },
        path::PathBuf,
        sync::{
            Mutex, PoisonError,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    use objc2::{
        msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject},
    };
    use objc2_foundation::{NSArray, NSFileManager, NSString};

    use crate::share::{Frame, OnEnded, OnFrame, OnStarted, Pixels};

    #[link(name = "ReplayKit", kind = "framework")]
    unsafe extern "C" {}

    const GROUP: &str = "group.chat.songbird.app";
    const EXTENSION: &str = "chat.songbird.app.screen";
    /// How long the system's sheet may wait for the person.
    const WAIT: Duration = Duration::from_secs(120);
    /// The largest frame taken (the extension scales to 1280 on the long side).
    const MAX_PIXELS: u32 = 1280 * 1280;

    static STREAM: Mutex<Option<UnixStream>> = Mutex::new(None);
    static CANCEL: AtomicBool = AtomicBool::new(false);

    thread_local! {
        /// The picker the sheet came from (main thread), removed at the next start or stop.
        static PICKER: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    }

    fn socket_path() -> Option<PathBuf> {
        let url = NSFileManager::defaultManager()
            .containerURLForSecurityApplicationGroupIdentifier(&NSString::from_str(GROUP))?;
        Some(PathBuf::from(url.path()?.to_string()).join("screen.sock"))
    }

    pub(crate) fn start(on_frame: OnFrame, started: OnStarted, ended: OnEnded) {
        let Some(path) = socket_path() else {
            return started(false);
        };
        let _ = std::fs::remove_file(&path);
        let Ok(listener) = UnixListener::bind(&path) else {
            return started(false);
        };
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        CANCEL.store(false, Ordering::Relaxed);
        let spawned = std::thread::Builder::new()
            .name("songbird-screen".into())
            .spawn(move || {
                let stream = accept(&listener);
                let _ = std::fs::remove_file(&path);
                let Some(stream) = stream else {
                    return started(false);
                };
                let reader = stream.try_clone();
                *STREAM.lock().unwrap_or_else(PoisonError::into_inner) = Some(stream);
                started(true);
                if let Ok(reader) = reader {
                    receive(reader, &on_frame);
                }
                // Ended by the extension (the person stopped it) unless the app took the stream.
                let ours = STREAM.lock().unwrap_or_else(PoisonError::into_inner).take();
                if ours.is_some() {
                    ended();
                }
                super::on_main(|_| remove_picker());
            });
        if spawned.is_err() {
            return;
        }
        super::on_main(|_| show_picker());
    }

    pub(crate) fn stop() {
        CANCEL.store(true, Ordering::Relaxed);
        if let Some(s) = STREAM.lock().unwrap_or_else(PoisonError::into_inner).take() {
            // The extension's next write fails and it ends the broadcast.
            let _ = s.shutdown(Shutdown::Both);
        }
        super::on_main(|_| remove_picker());
    }

    fn accept(listener: &UnixListener) -> Option<UnixStream> {
        listener.set_nonblocking(true).ok()?;
        let until = Instant::now() + WAIT;
        while Instant::now() < until && !CANCEL.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((s, _)) => {
                    s.set_nonblocking(false).ok()?;
                    return Some(s);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(_) => return None,
            }
        }
        None
    }

    fn receive(mut s: UnixStream, on_frame: &OnFrame) {
        let mut header = [0u8; 16];
        let mut pixels = Vec::new();
        loop {
            if s.read_exact(&mut header).is_err() || &header[..4] != b"SBS1" {
                return;
            }
            let word = |i: usize| {
                u32::from_le_bytes([header[i], header[i + 1], header[i + 2], header[i + 3]])
            };
            let (width, height, rotation) = (word(4), word(8), word(12));
            if width == 0
                || height == 0
                || width % 2 != 0
                || height % 2 != 0
                || width.saturating_mul(height) > MAX_PIXELS
            {
                return;
            }
            let luma = (width * height) as usize;
            pixels.resize(luma + luma / 2, 0);
            if s.read_exact(&mut pixels).is_err() {
                return;
            }
            let (y, uv) = pixels.split_at(luma);
            on_frame(Frame {
                width,
                height,
                rotation: u16::try_from(rotation % 360).unwrap_or(0),
                pixels: Pixels::Nv12 {
                    luma: y,
                    luma_stride: width,
                    chroma: uv,
                    chroma_stride: width,
                },
            });
        }
    }

    /// The system's broadcast sheet, with Songbird's extension chosen: an invisible
    /// `RPSystemBroadcastPickerView` in the window, its button pressed.
    fn show_picker() {
        remove_picker();
        let Some(cls) = AnyClass::get(c"RPSystemBroadcastPickerView") else {
            return;
        };
        // SAFETY: ReplayKit's picker view, made and configured on the main thread; the
        // window is the app's own; the button is the picker's, pressed as a tap would.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        unsafe {
            let picker: Retained<AnyObject> = msg_send![cls, new];
            let _: () = msg_send![&*picker, setPreferredExtension: &*NSString::from_str(EXTENSION)];
            let _: () = msg_send![&*picker, setShowsMicrophoneButton: false];
            let app: Retained<AnyObject> = msg_send![
                AnyClass::get(c"UIApplication").expect("UIKit"),
                sharedApplication
            ];
            let windows: Retained<NSArray<AnyObject>> = msg_send![&*app, windows];
            if let Some(window) = windows.firstObject() {
                let _: () = msg_send![&*window, addSubview: &*picker];
            }
            let subviews: Retained<NSArray<AnyObject>> = msg_send![&*picker, subviews];
            let button = AnyClass::get(c"UIButton").expect("UIKit");
            for view in &subviews {
                let is_button: bool = msg_send![&*view, isKindOfClass: button];
                if is_button {
                    // UIControlEventTouchUpInside.
                    let _: () = msg_send![&*view, sendActionsForControlEvents: 1u64 << 6];
                }
            }
            PICKER.with_borrow_mut(|p| *p = Some(picker));
        }
    }

    fn remove_picker() {
        if let Some(p) = PICKER.with_borrow_mut(Option::take) {
            // SAFETY: our picker view, on the main thread.
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            let _: () = unsafe { msg_send![&*p, removeFromSuperview] };
        }
    }
}

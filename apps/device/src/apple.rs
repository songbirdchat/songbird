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

/// macOS: every window of the app out of screen capture, or back in.
#[cfg(target_os = "macos")]
pub(crate) fn protect_windows(on: bool) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSWindowSharingType};
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let sharing = if on {
        NSWindowSharingType::None
    } else {
        NSWindowSharingType::ReadOnly
    };
    let windows = NSApplication::sharedApplication(mtm).windows();
    for i in 0..windows.count() {
        windows.objectAtIndex(i).setSharingType(sharing);
    }
}

/// iOS: whether the main screen is recorded, mirrored or shared.
#[cfg(target_os = "ios")]
#[allow(
    deprecated,
    reason = "UIScreen.mainScreen is still the screen the app shows on"
)]
pub(crate) fn captured() -> bool {
    use objc2::MainThreadMarker;
    use objc2_ui_kit::UIScreen;
    MainThreadMarker::new().is_some_and(|mtm| UIScreen::mainScreen(mtm).isCaptured())
}

// ---------------------------------------------------------------- location (D-0174)

mod locate {
    use std::cell::RefCell;

    use objc2::{
        DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
        runtime::ProtocolObject,
    };
    use objc2_core_location::{
        CLAuthorizationStatus, CLLocation, CLLocationManager, CLLocationManagerDelegate,
        kCLLocationAccuracyHundredMeters,
    };
    use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol};

    use crate::location::{Done, Failure, Position};

    pub(crate) struct Ivars {
        done: RefCell<Option<Done>>,
        manager: RefCell<Option<Retained<CLLocationManager>>>,
        requested: RefCell<bool>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; the delegate is main-thread-only,
        // as CoreLocation calls it on the thread the manager was made on (the main thread).
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "SongbirdLocationDelegate"]
        #[ivars = Ivars]
        pub(crate) struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl CLLocationManagerDelegate for Delegate {
            #[unsafe(method(locationManager:didUpdateLocations:))]
            fn did_update(&self, _manager: &CLLocationManager, locations: &NSArray<CLLocation>) {
                let Some(last) = locations.lastObject() else {
                    return;
                };
                // SAFETY: reading two properties of a location CoreLocation handed over.
                let (c, acc) = unsafe { (last.coordinate(), last.horizontalAccuracy()) };
                self.finish(Ok(Position {
                    latitude: c.latitude,
                    longitude: c.longitude,
                    accuracy_m: acc,
                }));
            }

            #[unsafe(method(locationManager:didFailWithError:))]
            fn did_fail(&self, _manager: &CLLocationManager, error: &NSError) {
                // kCLErrorLocationUnknown (0) is transient: CoreLocation keeps trying.
                match error.code() {
                    0 => {}
                    1 => self.finish(Err(Failure::Denied)),
                    _ => self.finish(Err(Failure::Unavailable)),
                }
            }

            #[unsafe(method(locationManagerDidChangeAuthorization:))]
            fn did_change(&self, manager: &CLLocationManager) {
                self.follow(manager);
            }
        }
    );

    impl Delegate {
        fn new(mtm: MainThreadMarker, done: Done) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(Ivars {
                done: RefCell::new(Some(done)),
                manager: RefCell::new(None),
                requested: RefCell::new(false),
            });
            // SAFETY: NSObject's designated initializer.
            unsafe { msg_send![super(this), init] }
        }

        /// Acts on the permission as it stands: ask, refuse, or take a fix.
        fn follow(&self, manager: &CLLocationManager) {
            // SAFETY: a property of the manager this delegate serves, on its thread.
            let status = unsafe { manager.authorizationStatus() };
            match status {
                CLAuthorizationStatus::NotDetermined => {
                    // SAFETY: as above; the answer comes back through `did_change`.
                    unsafe { manager.requestWhenInUseAuthorization() };
                }
                CLAuthorizationStatus::Denied | CLAuthorizationStatus::Restricted => {
                    self.finish(Err(Failure::Denied));
                }
                _ => {
                    if !self.ivars().requested.replace(true) {
                        // SAFETY: as above; one fix, delivered to `did_update` or `did_fail`.
                        unsafe { manager.requestLocation() };
                    }
                }
            }
        }

        fn finish(&self, r: Result<Position, Failure>) {
            if let Some(done) = self.ivars().done.borrow_mut().take() {
                done(r);
            }
            if let Some(m) = self.ivars().manager.borrow_mut().take() {
                // SAFETY: detaching ourselves from the manager we made.
                unsafe { m.setDelegate(None) };
            }
            // Released at the next request, not here: this method runs on it.
        }
    }

    thread_local! {
        /// Delegates with a request out: CoreLocation holds its delegate weakly.
        static LIVE: RefCell<Vec<Retained<Delegate>>> = const { RefCell::new(Vec::new()) };
    }

    pub(crate) fn current(done: Done) {
        let Some(mtm) = MainThreadMarker::new() else {
            done(Err(Failure::Unavailable));
            return;
        };
        let delegate = Delegate::new(mtm, done);
        // SAFETY: a fresh manager on the main thread, configured before use; the delegate
        // outlives it (kept in LIVE until it finishes).
        let manager = unsafe { CLLocationManager::new() };
        unsafe {
            manager.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            manager.setDesiredAccuracy(kCLLocationAccuracyHundredMeters);
        }
        delegate.ivars().manager.replace(Some(manager.clone()));
        LIVE.with_borrow_mut(|l| {
            l.retain(|d| d.ivars().done.borrow().is_some());
            l.push(delegate.clone());
        });
        delegate.follow(&manager);
    }
}

pub(crate) fn location(done: crate::location::Done) {
    locate::current(done);
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
            let request = unsafe { PHAssetCreationRequest::creationRequestForAsset() };
            // SAFETY: a fresh options object; the file is moved into the library (none left).
            let options = unsafe { PHAssetResourceCreationOptions::new() };
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
        unsafe {
            PHPhotoLibrary::sharedPhotoLibrary()
                .performChanges_completionHandler(RcBlock::as_ptr(&change), Some(&completion));
        }
    });
    // SAFETY: a class method; the handler is called once, on a queue of the system's.
    unsafe {
        PHPhotoLibrary::requestAuthorizationForAccessLevel_handler(
            PHAccessLevel::AddOnly,
            &handler,
        );
    }
}

// ---------------------------------------------------------------- push (D-0175)

#[cfg(target_os = "ios")]
static ON_TOKEN: std::sync::OnceLock<crate::push::OnToken> = std::sync::OnceLock::new();

/// `application:didRegisterForRemoteNotificationsWithDeviceToken:`, added to NSH's app delegate.
#[cfg(target_os = "ios")]
unsafe extern "C-unwind" fn did_register(
    _this: *mut objc2::runtime::AnyObject,
    _cmd: objc2::runtime::Sel,
    _app: *mut objc2::runtime::AnyObject,
    token: *mut objc2_foundation::NSData,
) {
    // SAFETY: UIKit passes the token, alive for this call.
    let Some(token) = (unsafe { token.as_ref() }) else {
        return;
    };
    let bytes = token.to_vec();
    if let Some(on) = ON_TOKEN.get() {
        on(crate::push::Token::Apns(bytes));
    }
}

/// `application:didFailToRegisterForRemoteNotificationsWithError:`: no token (no entitlement, a
/// simulator without push). Nothing to do; the next start asks again.
#[cfg(target_os = "ios")]
unsafe extern "C-unwind" fn did_fail(
    _this: *mut objc2::runtime::AnyObject,
    _cmd: objc2::runtime::Sel,
    _app: *mut objc2::runtime::AnyObject,
    _error: *mut NSError,
) {
}

#[cfg(target_os = "ios")]
pub(crate) fn register_push(on_token: crate::push::OnToken) -> bool {
    use objc2::{MainThreadMarker, ffi, sel};
    use objc2_ui_kit::UIApplication;
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let _ = ON_TOKEN.set(on_token);
    let app = UIApplication::sharedApplication(mtm);
    // SAFETY: reading the app's delegate property on the main thread.
    let Some(delegate) = (unsafe { app.delegate() }) else {
        return false;
    };
    let obj: &objc2::runtime::AnyObject = delegate.as_ref();
    // SAFETY: the delegate is a live object; its class lives as long as the process.
    let cls = unsafe { ffi::object_getClass(obj) }.cast_mut();
    if cls.is_null() {
        return false;
    }
    // SAFETY: adding (never replacing: `class_addMethod` refuses a selector the class already
    // has) two methods with the exact signatures UIKit calls them with: `v@:@@`.
    unsafe {
        let types = c"v@:@@".as_ptr();
        let _ = ffi::class_addMethod(
            cls,
            sel!(application:didRegisterForRemoteNotificationsWithDeviceToken:),
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(
                    *mut objc2::runtime::AnyObject,
                    objc2::runtime::Sel,
                    *mut objc2::runtime::AnyObject,
                    *mut objc2_foundation::NSData,
                ),
                objc2::runtime::Imp,
            >(did_register),
            types,
        );
        let _ = ffi::class_addMethod(
            cls,
            sel!(application:didFailToRegisterForRemoteNotificationsWithError:),
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(
                    *mut objc2::runtime::AnyObject,
                    objc2::runtime::Sel,
                    *mut objc2::runtime::AnyObject,
                    *mut NSError,
                ),
                objc2::runtime::Imp,
            >(did_fail),
            types,
        );
    }
    app.registerForRemoteNotifications();
    true
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
        let s: Option<Retained<AnyObject>> = unsafe { msg_send![cls, sharedInstance] };
        s
    }

    /// WebRTC's audio waits for CallKit (`true`), or runs as usual.
    fn manual_audio(on: bool) {
        if let Some(s) = rtc_session() {
            // SAFETY: documented RTCAudioSession properties.
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
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let delegate: Retained<Delegate> = {
            let this = Delegate::alloc(mtm).set_ivars(Ivars { on });
            // SAFETY: NSObject's designated initializer.
            unsafe { msg_send![super(this), init] }
        };
        // SAFETY: CallKit and PushKit set up on the main thread, their delegate kept for the
        // life of the app (KEEP), the main queue for both.
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
        true
    }

    pub(crate) fn update(id: [u8; 16], caller: &str, video: bool) {
        let u = call_update(caller, video);
        PROVIDER.with_borrow(|p| {
            if let Some(p) = p {
                // SAFETY: our provider on the main thread.
                unsafe { p.reportCallWithUUID_updated(&NSUUID::from_bytes(id), &u) };
            }
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
        PROVIDER.with_borrow(|p| {
            if let Some(p) = p {
                // SAFETY: as above.
                unsafe {
                    p.reportCallWithUUID_endedAtDate_reason(&NSUUID::from_bytes(id), None, reason);
                }
            }
        });
        manual_audio(false);
    }
}

//! macOS and iOS: AVFoundation (D-0110, M2).
//!
//! - Access is `AVCaptureDevice`'s authorization for video; the prompt is the system's, with
//!   the purpose string from `nsh.json` (`ios.infoPlist`, `macos.infoPlist`).
//! - A capture is an `AVCaptureSession` with one input and the outputs its purpose needs:
//!   `AVCaptureMetadataOutput` for QR codes (Apple's on-device decoder), a video data output
//!   for call frames, a photo output for stills. It starts and stops on a serial queue of its
//!   own (both block), and reports through the [`Sink`].
//! - The preview is the native component `songbird/camera`: an `AVCaptureVideoPreviewLayer`
//!   on the session its `session` prop names.
//!
//! `unsafe` here is calls into AVFoundation, Core Media and Core Video through `objc2`'s
//! bindings (every one `unsafe` because the bindings cannot prove the framework's contracts),
//! the reads of locked pixel buffers, and `Send` for the session handles. Each block says why
//! it holds.

#![allow(
    unsafe_code,
    reason = "D-0110 §5, reviewed in D-0116: AVFoundation, Core Media and Core Video through objc2"
)]

use std::{
    collections::HashMap,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::{
    AllocAnyThread, DefinedClass, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceDiscoverySession,
    AVCaptureDeviceInput, AVCaptureDevicePosition, AVCaptureDeviceType,
    AVCaptureDeviceTypeBuiltInWideAngleCamera, AVCaptureMetadataOutput,
    AVCaptureMetadataOutputObjectsDelegate, AVCaptureOutput, AVCapturePhoto,
    AVCapturePhotoCaptureDelegate, AVCapturePhotoOutput, AVCapturePhotoSettings, AVCaptureSession,
    AVCaptureSessionPresetHigh, AVCaptureSessionPresetPhoto, AVCaptureTorchMode,
    AVCaptureVideoDataOutput, AVCaptureVideoDataOutputSampleBufferDelegate,
    AVCaptureVideoPreviewLayer, AVLayerVideoGravityResizeAspectFill, AVMediaTypeVideo,
    AVMetadataMachineReadableCodeObject, AVMetadataObject, AVMetadataObjectTypeQRCode,
};
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType,
    CVPixelBufferGetPlaneCount, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSError, NSNumber, NSString};

use crate::{
    Access, AccessCallback, Backend, Capture, CaptureError, Config, Facing, Frame, Lens, Pixels,
    Purpose, Rotation, Sink, Still, StillCallback, Unavailable, jpeg,
};

// ---------------------------------------------------------------- the backend

/// AVFoundation's camera.
#[derive(Debug, Default)]
pub struct AppleCamera;

fn video() -> &'static NSString {
    // SAFETY: a constant AVFoundation exports.
    unsafe { AVMediaTypeVideo }.expect("AVMediaTypeVideo") // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
}

impl Backend for AppleCamera {
    fn access(&self) -> Access {
        // SAFETY: a class method with a valid media type; it never prompts.
        match unsafe { AVCaptureDevice::authorizationStatusForMediaType(video()) } {
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            AVAuthorizationStatus::Authorized => Access::Granted,
            AVAuthorizationStatus::Denied => Access::Denied,
            AVAuthorizationStatus::Restricted => Access::Restricted,
            _ => Access::NotAsked,
        }
    }

    fn request_access(&self, done: AccessCallback) {
        let done = Mutex::new(Some(done));
        let block = RcBlock::new(move |granted: Bool| {
            if let Some(done) = done.lock().unwrap_or_else(PoisonError::into_inner).take() {
                done(if granted.as_bool() {
                    Access::Granted
                } else {
                    Access::Denied
                });
            }
        });
        // SAFETY: a valid media type; AVFoundation copies the block and calls it once, on any
        // thread, which `done` allows (`Send`).
        unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(video(), &block) }; // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    }

    fn lenses(&self) -> Vec<Lens> {
        devices()
            .iter()
            .map(|d| {
                // SAFETY: plain property reads of a device AVFoundation listed.
                let (id, name, position) = unsafe {
                    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
                    (
                        d.uniqueID().to_string(),
                        d.localizedName().to_string(),
                        d.position(),
                    )
                };
                let facing = match position {
                    AVCaptureDevicePosition::Front => Facing::Front,
                    AVCaptureDevicePosition::Back => Facing::Back,
                    // Macs: the built-in camera faces the person; others are external.
                    _ if cfg!(target_os = "macos") && is_builtin(d) => Facing::Front,
                    _ => Facing::External,
                };
                Lens { id, name, facing }
            })
            .collect()
    }

    fn open(&self, config: Config, sink: Sink) -> Result<Box<dyn Capture>, Unavailable> {
        if self.access() != Access::Granted {
            return Err(Unavailable::Failed("no access".into()));
        }
        let capture = AppleCapture::open(&config, sink)?;
        Ok(Box::new(capture))
    }

    fn settings_url(&self) -> Option<&'static str> {
        if cfg!(target_os = "ios") {
            // UIApplicationOpenSettingsURLString: the app's own page in Settings.
            Some("app-settings:")
        } else {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Camera")
        }
    }
}

/// The video devices: the built-in cameras, and on macOS external ones and Continuity Camera.
fn devices() -> Vec<Retained<AVCaptureDevice>> {
    // SAFETY: constants AVFoundation exports, on the OS versions NSH supports (macOS 14+ for
    // the external and Continuity types; iOS uses only the built-in type, there since iOS 10).
    let types: Vec<&AVCaptureDeviceType> = unsafe {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        #[cfg(target_os = "macos")]
        {
            vec![
                AVCaptureDeviceTypeBuiltInWideAngleCamera,
                objc2_av_foundation::AVCaptureDeviceTypeExternal,
                objc2_av_foundation::AVCaptureDeviceTypeContinuityCamera,
            ]
        }
        #[cfg(target_os = "ios")]
        {
            vec![AVCaptureDeviceTypeBuiltInWideAngleCamera]
        }
    };
    let types = NSArray::from_slice(&types);
    // SAFETY: a discovery for video devices at any position; it only lists.
    let session = unsafe {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
            &types,
            Some(video()),
            AVCaptureDevicePosition::Unspecified,
        )
    };
    // SAFETY: a property read.
    unsafe { session.devices() }.to_vec() // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
}

#[cfg(target_os = "macos")]
fn is_builtin(d: &AVCaptureDevice) -> bool {
    // SAFETY: a property read; the constant is AVFoundation's.
    unsafe { *d.deviceType() == *AVCaptureDeviceTypeBuiltInWideAngleCamera } // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
}

#[cfg(not(target_os = "macos"))]
fn is_builtin(_: &AVCaptureDevice) -> bool {
    true
}

// ---------------------------------------------------------------- sessions

/// Sessions by preview id, for the preview component (main thread) to find.
static SESSIONS: Mutex<Option<HashMap<u64, SendSession>>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// An `AVCaptureSession` handle that may cross threads.
#[derive(Clone)]
struct SendSession(Retained<AVCaptureSession>);
// SAFETY: AVCaptureSession is documented as usable from any thread: configuration and
// start/stop are serialized internally (Apple runs them on a queue of the app's choosing); the
// handle is a reference count, which is atomic.
unsafe impl Send for SendSession {} // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage

fn sessions<R>(f: impl FnOnce(&mut HashMap<u64, SendSession>) -> R) -> R {
    let mut map = SESSIONS.lock().unwrap_or_else(PoisonError::into_inner);
    f(map.get_or_insert_with(HashMap::new))
}

/// A running AVFoundation capture.
struct AppleCapture {
    id: u64,
    session: Retained<AVCaptureSession>,
    queue: DispatchRetained<DispatchQueue>,
    device: Retained<AVCaptureDevice>,
    input: Retained<AVCaptureDeviceInput>,
    photo: Option<Retained<AVCapturePhotoOutput>>,
    /// Kept alive while the outputs hold it weakly.
    _delegate: Option<Retained<Delegate>>,
    /// Photo delegates until they are called (the photo output does not keep them).
    pending: Vec<Retained<PhotoDelegate>>,
    /// The session's interruption and runtime-error observers, removed when it stops.
    observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    purpose: Purpose,
    sink: Sink,
}

// SAFETY: every field is an Objective-C object AVFoundation documents as usable from any
// thread (sessions, devices, inputs, outputs, dispatch queues), or our own delegate, whose
// ivars are `Send + Sync`. The capture is used from one thread at a time (`&mut self`).
unsafe impl Send for AppleCapture {} // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage

impl AppleCapture {
    fn open(config: &Config, sink: Sink) -> Result<Self, Unavailable> {
        let id_ns = NSString::from_str(&config.lens);
        // SAFETY: a class method with a valid string.
        let device =
            unsafe { AVCaptureDevice::deviceWithUniqueID(&id_ns) }.ok_or(Unavailable::NoCamera)?; // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        // SAFETY: a device AVFoundation returned; errors come back as `Err`.
        let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&device) } // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            .map_err(|e| unavailable_from(&e))?;
        // SAFETY: a new session, configured before it runs.
        let session = unsafe { AVCaptureSession::new() }; // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let queue = DispatchQueue::new("chat.songbird.camera", None);
        let mut photo = None;
        let mut delegate = None;
        // SAFETY: configuration of a session that is not running, with objects made here.
        unsafe {
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            session.beginConfiguration();
            let preset = if config.purpose == Purpose::Photo {
                AVCaptureSessionPresetPhoto
            } else {
                AVCaptureSessionPresetHigh
            };
            if session.canSetSessionPreset(preset) {
                session.setSessionPreset(preset);
            }
            if !session.canAddInput(&input) {
                session.commitConfiguration();
                return Err(Unavailable::InUse);
            }
            session.addInput(&input);
            match config.purpose {
                Purpose::ScanCode => {
                    let out = AVCaptureMetadataOutput::new();
                    if !session.canAddOutput(&out) {
                        session.commitConfiguration();
                        return Err(Unavailable::Failed("metadata output".into()));
                    }
                    session.addOutput(&out);
                    // Types are only available once the output is in the session, and asking
                    // for one that is not raises an exception: check first.
                    let qr = AVMetadataObjectTypeQRCode;
                    if out.availableMetadataObjectTypes().containsObject(qr) {
                        let d = Delegate::new(sink.clone(), false);
                        out.setMetadataObjectTypes(Some(&NSArray::from_slice(&[qr])));
                        out.setMetadataObjectsDelegate_queue(
                            Some(ProtocolObject::from_ref(&*d)),
                            Some(&queue),
                        );
                        delegate = Some(d);
                    } else {
                        session.removeOutput(&out);
                        let d = Delegate::new(sink.clone(), true);
                        add_frames(&session, &d, &queue)?;
                        delegate = Some(d);
                    }
                }
                Purpose::Call => {
                    let d = Delegate::new(sink.clone(), false);
                    add_frames(&session, &d, &queue)?;
                    delegate = Some(d);
                }
                Purpose::Photo => {
                    let out = AVCapturePhotoOutput::new();
                    if !session.canAddOutput(&out) {
                        session.commitConfiguration();
                        return Err(Unavailable::Failed("photo output".into()));
                    }
                    session.addOutput(&out);
                    photo = Some(out);
                }
                // Movie recording comes with M5; a video session previews until then.
                Purpose::Video => {}
            }
            session.commitConfiguration();
        }
        let observers = observe(&session, &sink, &config.lens);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        sessions(|m| m.insert(id, SendSession(session.clone())));
        let starting = SendSession(session.clone());
        let lens = config.lens.clone();
        let started = sink.clone();
        // `startRunning` blocks until the camera runs: off the caller's thread.
        queue.exec_async(move || {
            let s = starting;
            // SAFETY: starting a configured session on its own serial queue.
            unsafe { s.0.startRunning() }; // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            // SAFETY: a property read.
            if unsafe { s.0.isRunning() } {
                // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
                started.running(&lens);
            } else {
                started.unavailable(Unavailable::InUse);
            }
        });
        Ok(AppleCapture {
            id,
            session,
            queue,
            device,
            input,
            photo,
            _delegate: delegate,
            pending: Vec::new(),
            observers,
            purpose: config.purpose,
            sink,
        })
    }
}

impl Capture for AppleCapture {
    fn set_lens(&mut self, lens: &str) {
        let id = NSString::from_str(lens);
        // SAFETY: as in `open`.
        let Some(device) = (unsafe { AVCaptureDevice::deviceWithUniqueID(&id) }) else {
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            return;
        };
        // SAFETY: as in `open`.
        let Ok(input) = (unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&device) })
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        else {
            return;
        };
        // SAFETY: swapping the input of a running session inside a configuration block, which
        // AVFoundation applies atomically.
        unsafe {
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            self.session.beginConfiguration();
            self.session.removeInput(&self.input);
            if self.session.canAddInput(&input) {
                self.session.addInput(&input);
                self.device = device;
                self.input = input;
                self.sink.running(lens);
            } else {
                self.session.addInput(&self.input);
            }
            self.session.commitConfiguration();
        }
    }

    fn set_torch(&mut self, on: bool) -> bool {
        let mode = if on {
            AVCaptureTorchMode::On
        } else {
            AVCaptureTorchMode::Off
        };
        // SAFETY: torch changes under the device's configuration lock, as documented.
        unsafe {
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            if !self.device.hasTorch() || !self.device.isTorchModeSupported(mode) {
                return false;
            }
            if self.device.lockForConfiguration().is_err() {
                return false;
            }
            self.device.setTorchMode(mode);
            self.device.unlockForConfiguration();
        }
        true
    }

    fn capture_photo(&mut self, done: StillCallback) {
        let Some(photo) = (self.purpose == Purpose::Photo)
            .then_some(())
            .and(self.photo.as_ref())
        else {
            return done(Err(CaptureError::WrongPurpose));
        };
        // Asking for a photo with no live video connection (a webcam unplugged, the camera
        // taken by another app) raises an Objective-C exception: check first (D-0116).
        // SAFETY: property reads of the output and its connection.
        let live = unsafe {
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            photo
                .connectionWithMediaType(video())
                .is_some_and(|c| c.isActive() && c.isEnabled())
        };
        if !live {
            return done(Err(CaptureError::NotRunning));
        }
        self.pending.retain(|d| !d.is_done());
        let delegate = PhotoDelegate::new(done);
        // SAFETY: JPEG settings and our delegate, which `pending` keeps alive until it is
        // called.
        unsafe {
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            let settings = AVCapturePhotoSettings::photoSettingsWithFormat(Some(&jpeg_format()));
            photo
                .capturePhotoWithSettings_delegate(&settings, ProtocolObject::from_ref(&*delegate));
        }
        self.pending.push(delegate);
    }

    fn preview_id(&self) -> u64 {
        self.id
    }
}

/// Reports the session's losses and returns: an interruption (another app took the camera, a
/// phone call, Split View, a covered window) and a runtime error as `Unavailable`, the end of
/// an interruption as running again (D-0116).
fn observe(
    session: &AVCaptureSession,
    sink: &Sink,
    lens: &str,
) -> Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>> {
    let center = objc2_foundation::NSNotificationCenter::defaultCenter();
    let mut out = Vec::new();
    let mut add = |name: &objc2_foundation::NSNotificationName, report: Box<dyn Fn() + 'static>| {
        let block =
            RcBlock::new(move |_: std::ptr::NonNull<objc2_foundation::NSNotification>| report());
        // SAFETY: an observer for this session's notifications only, delivered on the posting
        // thread; the block owns what it reports into and is removed with the capture.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(name),
                Some(session),
                None,
                &block,
            )
        }; // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        out.push(token);
    };
    // SAFETY: constants AVFoundation exports.
    let (lost, back, failed) = unsafe {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        (
            objc2_av_foundation::AVCaptureSessionWasInterruptedNotification,
            objc2_av_foundation::AVCaptureSessionInterruptionEndedNotification,
            objc2_av_foundation::AVCaptureSessionRuntimeErrorNotification,
        )
    };
    let s = sink.clone();
    add(
        lost,
        Box::new(move || s.unavailable(Unavailable::Interrupted)),
    );
    let (s, lens) = (sink.clone(), lens.to_owned());
    add(back, Box::new(move || s.running(&lens)));
    let s = sink.clone();
    add(
        failed,
        Box::new(move || s.unavailable(Unavailable::Failed("AVFoundation runtime error".into()))),
    );
    out
}

impl Drop for AppleCapture {
    fn drop(&mut self) {
        let center = objc2_foundation::NSNotificationCenter::defaultCenter();
        for token in self.observers.drain(..) {
            // SAFETY: removing an observer this capture added.
            unsafe { center.removeObserver(token.as_ref()) }; // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        }
        sessions(|m| m.remove(&self.id));
        let s = SendSession(self.session.clone());
        // `stopRunning` blocks: on the session's queue, after anything queued before it.
        self.queue.exec_async(move || {
            let s = s;
            // SAFETY: stopping on the session's own queue.
            unsafe { s.0.stopRunning() }; // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        });
    }
}

/// Adds an NV12 video data output reporting to `delegate`, inside a configuration block.
///
/// # Safety
///
/// `session` is being configured (between `beginConfiguration` and `commitConfiguration`).
unsafe fn add_frames(
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    session: &AVCaptureSession,
    delegate: &Delegate,
    queue: &DispatchQueue,
) -> Result<(), Unavailable> {
    // SAFETY: the caller's configuration block; objects made here.
    unsafe {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let out = AVCaptureVideoDataOutput::new();
        out.setAlwaysDiscardsLateVideoFrames(true);
        out.setVideoSettings(Some(&nv12_settings()));
        if !session.canAddOutput(&out) {
            session.commitConfiguration();
            return Err(Unavailable::Failed("video output".into()));
        }
        session.addOutput(&out);
        out.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(delegate)), Some(queue));
    }
    Ok(())
}

fn unavailable_from(e: &NSError) -> Unavailable {
    // AVErrorApplicationIsNotAuthorizedToUseDevice (-11852) and device-in-use (-11804 on
    // macOS, "AVErrorDeviceInUseByAnotherApplication") are worth their own screens.
    match e.code() {
        -11852 => Unavailable::SystemOff,
        -11804 | -11818 => Unavailable::InUse,
        -11814 => Unavailable::NoCamera,
        code => Unavailable::Failed(format!("AVFoundation {code}")),
    }
}

/// `{kCVPixelBufferPixelFormatTypeKey: NV12 full range}`.
fn nv12_settings() -> Retained<NSDictionary<NSString, AnyObject>> {
    // SAFETY: a constant Core Video exports; it is toll-free bridged to NSString.
    let key: &NSString = unsafe {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        &*std::ptr::from_ref(objc2_core_video::kCVPixelBufferPixelFormatTypeKey).cast::<NSString>()
    };
    let value = NSNumber::new_u32(kCVPixelFormatType_420YpCbCr8BiPlanarFullRange);
    NSDictionary::from_slices(
        &[key],
        &[&*Retained::into_super(Retained::into_super(value))],
    )
}

/// `{AVVideoCodecKey: AVVideoCodecTypeJPEG}`.
fn jpeg_format() -> Retained<NSDictionary<NSString, AnyObject>> {
    // SAFETY: constants AVFoundation exports.
    let (key, jpeg) = unsafe {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        (
            objc2_av_foundation::AVVideoCodecKey.expect("AVVideoCodecKey"),
            objc2_av_foundation::AVVideoCodecTypeJPEG.expect("AVVideoCodecTypeJPEG"),
        )
    };
    let value: &AnyObject = jpeg;
    NSDictionary::from_slices(&[key], &[value])
}

// ---------------------------------------------------------------- delegates

/// What the session's delegate reports into.
struct DelegateIvars {
    sink: Sink,
    /// For a scanning session whose device offers no QR metadata (some Macs): frames go to
    /// the on-device `rqrr` decoder instead.
    scanner: Option<crate::qr::Scanner>,
}

define_class!(
    /// Receives decoded codes and video frames on the session's queue.
    #[unsafe(super(NSObject))]
    #[name = "SongbirdCameraDelegate"]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {} // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage

    unsafe impl AVCaptureMetadataOutputObjectsDelegate for Delegate {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        #[unsafe(method(captureOutput:didOutputMetadataObjects:fromConnection:))]
        fn did_output_metadata(
            &self,
            _output: &AVCaptureMetadataOutput,
            objects: &NSArray<AVMetadataObject>,
            _connection: &AVCaptureConnection,
        ) {
            for object in objects {
                let Ok(code) = object.downcast::<AVMetadataMachineReadableCodeObject>() else {
                    continue;
                };
                // SAFETY: a property read of an object AVFoundation handed us.
                if let Some(text) = unsafe { code.stringValue() } {
                    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
                    self.ivars().sink.code(&text.to_string());
                }
            }
        }
    }

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for Delegate {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn did_output_sample(
            &self,
            _output: &AVCaptureOutput,
            sample: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            if let Some(frame) = frame_of(sample) {
                match &self.ivars().scanner {
                    Some(scanner) => scanner.offer(frame),
                    None => self.ivars().sink.frame(frame),
                }
            }
        }
    }
);

impl Delegate {
    fn new(sink: Sink, scan_frames: bool) -> Retained<Self> {
        let scanner = scan_frames.then(|| crate::qr::Scanner::start(sink.clone()));
        let this = Self::alloc().set_ivars(DelegateIvars { sink, scanner });
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] } // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    }
}

/// Holds the callback of one photo until AVFoundation reports it.
struct PhotoIvars {
    done: Mutex<Option<StillCallback>>,
}

define_class!(
    /// Receives one processed photo.
    #[unsafe(super(NSObject))]
    #[name = "SongbirdPhotoDelegate"]
    #[ivars = PhotoIvars]
    struct PhotoDelegate;

    unsafe impl NSObjectProtocol for PhotoDelegate {} // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage

    unsafe impl AVCapturePhotoCaptureDelegate for PhotoDelegate {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        #[unsafe(method(captureOutput:didFinishProcessingPhoto:error:))]
        fn did_finish(
            &self,
            _output: &AVCapturePhotoOutput,
            photo: &AVCapturePhoto,
            error: Option<&NSError>,
        ) {
            let Some(done) = self
                .ivars()
                .done
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
            else {
                return;
            };
            if let Some(e) = error {
                return done(Err(CaptureError::Failed(format!(
                    "AVFoundation {}",
                    e.code()
                ))));
            }
            // SAFETY: a method of the photo AVFoundation handed us.
            let Some(data) = (unsafe { photo.fileDataRepresentation() }) else {
                // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
                return done(Err(CaptureError::Failed("no photo data".into())));
            };
            done(still_of(&data));
        }
    }
);

impl PhotoDelegate {
    fn is_done(&self) -> bool {
        self.ivars()
            .done
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_none()
    }

    fn new(done: StillCallback) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PhotoIvars {
            done: Mutex::new(Some(done)),
        });
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] } // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    }
}

fn still_of(data: &NSData) -> Result<Still, CaptureError> {
    let stripped =
        jpeg::strip(&data.to_vec()).map_err(|e| CaptureError::Failed(format!("{e:?}")))?;
    let (width, height) = stripped.upright_size();
    Ok(Still {
        bytes: stripped.bytes,
        mime: "image/jpeg",
        width,
        height,
    })
}

/// A copy of an NV12 sample's planes, or `None` for anything else.
fn frame_of(sample: &CMSampleBuffer) -> Option<Frame> {
    // SAFETY: the sample is valid for this call; the image buffer is retained.
    let image = unsafe { sample.image_buffer() }?; // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    // SAFETY: a video data output configured for NV12 hands CVPixelBuffers.
    let buffer: &CVPixelBuffer = &image;
    if CVPixelBufferGetPixelFormatType(buffer) != kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
        || CVPixelBufferGetPlaneCount(buffer) != 2
    {
        return None;
    }
    // SAFETY: CMTime is plain data.
    let pts = unsafe { sample.presentation_time_stamp() }; // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let timestamp_us = if pts.timescale > 0 {
        u64::try_from(i128::from(pts.value) * 1_000_000 / i128::from(pts.timescale)).unwrap_or(0)
    } else {
        0
    };
    let lock = CVPixelBufferLockFlags::ReadOnly;
    // SAFETY: locking for reading; unlocked below on every path.
    if unsafe { CVPixelBufferLockBaseAddress(buffer, lock) } != 0 {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        return None;
    }
    let copy = |plane: usize| -> Option<(Vec<u8>, usize)> {
        let base = CVPixelBufferGetBaseAddressOfPlane(buffer, plane).cast::<u8>();
        let stride = CVPixelBufferGetBytesPerRowOfPlane(buffer, plane);
        let rows = CVPixelBufferGetHeightOfPlane(buffer, plane);
        if base.is_null() {
            return None;
        }
        let len = stride.checked_mul(rows)?;
        // SAFETY: the buffer is locked, and Core Video promises `bytes per row × height`
        // readable bytes at the plane's base address while it is.
        Some((
            unsafe { std::slice::from_raw_parts(base, len) }.to_vec(), // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            stride,
        ))
    };
    let planes = copy(0).zip(copy(1));
    // SAFETY: the lock taken above.
    unsafe { CVPixelBufferUnlockBaseAddress(buffer, lock) }; // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let ((y, y_stride), (uv, uv_stride)) = planes?;
    let frame = Frame {
        width: u32::try_from(CVPixelBufferGetWidth(buffer)).ok()?,
        height: u32::try_from(CVPixelBufferGetHeight(buffer)).ok()?,
        rotation: Rotation::Deg0,
        timestamp_us,
        pixels: Pixels::Nv12 {
            y,
            y_stride,
            uv,
            uv_stride,
        },
    };
    frame.is_consistent().then_some(frame)
}

// ---------------------------------------------------------------- the preview

/// The session a preview component's `session` prop names.
fn session_for(prop: Option<&serde_json::Value>) -> Option<Retained<AVCaptureSession>> {
    let id = prop?.as_u64()?;
    sessions(|m| m.get(&id).map(|s| s.0.clone()))
}

#[cfg(target_os = "macos")]
mod preview {
    use nsh_platform_macos::{ComponentContext, Components, NativeComponent};
    use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained};
    use objc2_app_kit::NSView;
    use objc2_foundation::NSRect;

    use super::{AVCaptureVideoPreviewLayer, AVLayerVideoGravityResizeAspectFill, session_for};

    struct Preview {
        view: Retained<NSView>,
        layer: Option<Retained<AVCaptureVideoPreviewLayer>>,
    }

    impl NativeComponent for Preview {
        fn view(&self) -> Retained<NSView> {
            self.view.clone()
        }

        fn set_prop(&mut self, name: &str, value: Option<&serde_json::Value>) {
            if name != "session" {
                return;
            }
            let Some(session) = session_for(value) else {
                self.view.setLayer(None);
                self.layer = None;
                return;
            };
            // SAFETY: a preview layer on a session, made and attached on the main thread,
            // where NSH calls components.
            unsafe {
                // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
                let layer = AVCaptureVideoPreviewLayer::layerWithSession(&session);
                if let Some(fill) = AVLayerVideoGravityResizeAspectFill {
                    layer.setVideoGravity(fill);
                }
                self.view.setLayer(Some(&layer));
                self.view.setWantsLayer(true);
                self.layer = Some(layer);
            }
        }
    }

    /// Registers `songbird/camera`.
    ///
    /// # Panics
    ///
    /// When NSH makes a component off the main thread (it never does).
    pub fn register(components: &Components) {
        components.register(
            "songbird/camera",
            std::rc::Rc::new(|_cx: ComponentContext| {
                let mtm = MainThreadMarker::new().expect("components are made on the main thread");
                let view = NSView::initWithFrame(NSView::alloc(mtm), NSRect::ZERO);
                Box::new(Preview { view, layer: None }) as Box<dyn NativeComponent>
            }),
        );
    }
}

#[cfg(target_os = "ios")]
mod preview {
    use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
    use objc2::{
        ClassType, MainThreadMarker, MainThreadOnly, define_class, rc::Retained, runtime::AnyClass,
    };
    use objc2_core_foundation::CGRect;
    use objc2_ui_kit::UIView;

    use super::{
        AVCaptureVideoPreviewLayer, AVLayerVideoGravityResizeAspectFill, NSObject,
        NSObjectProtocol, msg_send, session_for,
    };

    define_class!(
        /// A view whose layer is an `AVCaptureVideoPreviewLayer`, so it follows the view's size.
        #[unsafe(super(UIView, objc2_ui_kit::UIResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "SongbirdCameraPreviewView"]
        struct PreviewView;

        unsafe impl NSObjectProtocol for PreviewView {} // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage

        impl PreviewView {
            #[unsafe(method(layerClass))]
            fn layer_class() -> &'static AnyClass {
                AVCaptureVideoPreviewLayer::class()
            }
        }
    );

    /// The view, and the session id its layer shows.
    struct Preview(Retained<PreviewView>, Option<u64>);

    impl Preview {
        fn layer(&self) -> Retained<AVCaptureVideoPreviewLayer> {
            let layer = self.0.layer();
            // SAFETY: `layerClass` makes the view's layer an AVCaptureVideoPreviewLayer.
            unsafe { Retained::cast_unchecked(layer) } // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        }
    }

    impl NativeComponent for Preview {
        fn view(&self) -> &UIView {
            &self.0
        }

        fn set_prop(&mut self, name: &str, value: Option<&serde_json::Value>) {
            if name != "session" {
                return;
            }
            // The same session again (props are sent again as the screen around the view
            // changes) is left alone: iOS aborts the app when a preview layer attached to a
            // session is attached again (found on an iPhone, D-0183). Another session is
            // attached only after the old one is let go.
            let id = value.and_then(serde_json::Value::as_u64);
            if id == self.1 {
                return;
            }
            let session = session_for(value);
            // SAFETY: setting the layer's session on the main thread, where NSH calls
            // components.
            unsafe {
                // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
                let layer = self.layer();
                if self.1.is_some() {
                    layer.setSession(None);
                }
                layer.setSession(session.as_deref());
                if let Some(fill) = AVLayerVideoGravityResizeAspectFill {
                    layer.setVideoGravity(fill);
                }
            }
            self.1 = session.is_some().then_some(id).flatten();
        }
    }

    /// Registers `songbird/camera`.
    ///
    /// # Panics
    ///
    /// When NSH makes a component off the main thread (it never does).
    pub fn register(components: &Components) {
        components.register(
            "songbird/camera",
            std::rc::Rc::new(|_cx: ComponentContext| {
                let mtm = MainThreadMarker::new().expect("components are made on the main thread");
                // SAFETY: UIView's designated initializer.
                let view: Retained<PreviewView> =
                    unsafe { msg_send![PreviewView::alloc(mtm), initWithFrame: CGRect::ZERO] }; // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
                Box::new(Preview(view, None)) as Box<dyn NativeComponent>
            }),
        );
    }
}

pub use preview::register;

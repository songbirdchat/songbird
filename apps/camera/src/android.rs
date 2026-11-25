//! Android: CameraX in Songbird's Gradle host (`apps/android`, D-0113), reached over JNI.
//!
//! The Java side (`chat.songbird.camera.CameraBridge`) owns CameraX and the permission; this
//! side implements [`Backend`] by calling its static methods, and receives its results through
//! the `native*` methods below. QR codes are decoded here, on the device, with `rqrr` (no ML
//! Kit, no Play Services): the Y plane of each analysis frame goes to a [`Scanner`].
//! The preview and call video are NSH components this side makes ([`register`], D-0188).
//!
//! `unsafe` here: the exported JNI entry points (`#[unsafe(no_mangle)]`), and reading the direct
//! byte buffers CameraX hands over, which are valid for the duration of the call (Java closes
//! the image after it returns). Each block says why it holds.

#![allow(
    unsafe_code,
    reason = "D-0110 §5, reviewed in D-0116: JNI entry points and CameraX direct buffers"
)]

use std::{
    collections::HashMap,
    sync::{
        Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use jni::{
    JNIEnv, JavaVM,
    objects::{GlobalRef, JByteArray, JByteBuffer, JClass, JString, JValue, JValueOwned},
    sys::{jint, jlong},
};

use crate::{
    Access, AccessCallback, Backend, Capture, CaptureError, Config, Frame, Lens, Pixels, Purpose,
    Rotation, Sink, Still, StillCallback, Unavailable, jpeg, qr::Scanner,
};

/// The longest frame side accepted from Java; larger frames are dropped.
const MAX_SIDE: u32 = 4096;

#[derive(Default)]
struct State {
    vm: Option<JavaVM>,
    bridge: Option<GlobalRef>,
    asking: Vec<AccessCallback>,
    sessions: HashMap<u64, Session>,
}

struct Session {
    sink: Sink,
    scanner: Option<Scanner>,
    purpose: Purpose,
    photos: Vec<StillCallback>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn state() -> MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(state().get_or_insert_with(State::default))
}

/// The JavaVM and the bridge class, without holding the state lock while Java runs.
fn host() -> Option<(JavaVM, GlobalRef)> {
    let (vm, bridge) = with(|s| Some((s.vm.as_ref()?.get_java_vm_pointer(), s.bridge.clone()?)))?;
    // SAFETY: the pointer is of the JavaVM Java handed us in `nativeInit`, which lives as long
    // as the process; `from_raw` makes a handle that does not own it.
    let vm = unsafe { JavaVM::from_raw(vm) }.ok()?; // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    Some((vm, bridge))
}

/// Calls the static `CameraBridge.<name>` and reads its result with `read`; `None` when the
/// host is not attached or Java threw.
fn call<R>(
    name: &str,
    sig: &str,
    args: impl for<'l> FnOnce(&mut JNIEnv<'l>) -> Option<Vec<JValueOwned<'l>>>,
    read: impl FnOnce(&mut JNIEnv<'_>, JValueOwned<'_>) -> Option<R>,
) -> Option<R> {
    let (vm, bridge) = host()?;
    let mut env = vm.attach_current_thread_permanently().ok()?;
    env.with_local_frame(16, |env| -> Result<Option<R>, jni::errors::Error> {
        let Some(args) = args(env) else {
            return Ok(None);
        };
        let args: Vec<JValue<'_, '_>> = args.iter().map(JValueOwned::borrow).collect();
        let class: &JClass<'_> = bridge.as_obj().into();
        let out = env.call_static_method(class, name, sig, &args);
        if env.exception_check()? {
            env.exception_clear()?;
            return Ok(None);
        }
        Ok(out.ok().and_then(|v| read(env, v)))
    })
    .ok()
    .flatten()
}

/// Calls a `void` method; whether it ran.
fn call_void(
    name: &str,
    sig: &str,
    args: impl for<'l> FnOnce(&mut JNIEnv<'l>) -> Option<Vec<JValueOwned<'l>>>,
) -> bool {
    call(name, sig, args, |_, _| Some(())).is_some()
}

#[allow(clippy::unnecessary_wraps)] // the shape `call` takes
fn no_args<'l>(_: &mut JNIEnv<'l>) -> Option<Vec<JValueOwned<'l>>> {
    Some(Vec::new())
}

fn long<'l>(id: u64) -> JValueOwned<'l> {
    JValueOwned::Long(i64::try_from(id).unwrap_or(i64::MAX))
}

/// `CameraBridge.lenses()`, parsed: `id \t facing \t name` per line.
fn lenses_from_java() -> Vec<Lens> {
    let text = call("lenses", "()Ljava/lang/String;", no_args, |env, v| {
        let s = JString::from(v.l().ok()?);
        env.get_string(&s).ok().map(String::from)
    });
    crate::Lens::parse_lines(&text.unwrap_or_default())
}

/// CameraX's camera on Android.
#[derive(Debug, Default)]
pub struct AndroidCamera;

impl Backend for AndroidCamera {
    fn access(&self) -> Access {
        match call("access", "()I", no_args, |_, v| v.i().ok()) {
            Some(1) => Access::Granted,
            Some(2) => Access::Denied,
            Some(0) => Access::NotAsked,
            // Not attached, or device management turned it off.
            _ => Access::Restricted,
        }
    }

    fn request_access(&self, done: AccessCallback) {
        with(|s| s.asking.push(done));
        if !call_void("requestAccess", "()V", no_args) {
            answer(Access::Denied);
        }
    }

    fn lenses(&self) -> Vec<Lens> {
        lenses_from_java()
    }

    fn open(&self, config: Config, sink: Sink) -> Result<Box<dyn Capture>, Unavailable> {
        if self.access() != Access::Granted {
            return Err(Unavailable::Failed("no access".into()));
        }
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let scanner = (config.purpose == Purpose::ScanCode).then(|| Scanner::start(sink.clone()));
        with(|s| {
            s.sessions.insert(
                id,
                Session {
                    sink,
                    scanner,
                    purpose: config.purpose,
                    photos: Vec::new(),
                },
            )
        });
        let purpose = match config.purpose {
            Purpose::ScanCode => 0,
            Purpose::Photo => 1,
            Purpose::Video => 2,
            Purpose::Call => 3,
        };
        let lens = config.lens.clone();
        let ok = call_void("open", "(JILjava/lang/String;)V", move |env| {
            let lens = env.new_string(&lens).ok()?;
            Some(vec![
                long(id),
                JValueOwned::Int(purpose),
                JValueOwned::Object(lens.into()),
            ])
        });
        if !ok {
            with(|s| s.sessions.remove(&id));
            return Err(Unavailable::Failed("the host is not attached".into()));
        }
        Ok(Box::new(AndroidCapture { id }))
    }

    fn settings_url(&self) -> Option<&'static str> {
        // `ACTION_APPLICATION_DETAILS_SETTINGS` for the app, which NSH's `open_url` maps from
        // this scheme on Android.
        Some("package:chat.songbird.app")
    }
}

fn id_from_jlong(id: jlong) -> Option<u64> {
    u64::try_from(id).ok()
}

struct AndroidCapture {
    id: u64,
}

impl Capture for AndroidCapture {
    fn set_lens(&mut self, lens: &str) {
        let (id, lens) = (self.id, lens.to_owned());
        call_void("setLens", "(JLjava/lang/String;)V", move |env| {
            let lens = env.new_string(&lens).ok()?;
            Some(vec![long(id), JValueOwned::Object(lens.into())])
        });
    }

    fn set_torch(&mut self, on: bool) -> bool {
        let id = self.id;
        call(
            "setTorch",
            "(JZ)Z",
            move |_| Some(vec![long(id), JValueOwned::Bool(u8::from(on))]),
            |_, v| v.z().ok(),
        )
        .unwrap_or(false)
    }

    fn capture_photo(&mut self, done: StillCallback) {
        let ok = with(|s| match s.sessions.get_mut(&self.id) {
            Some(session) if session.purpose == Purpose::Photo => {
                session.photos.push(done);
                Ok(())
            }
            _ => Err(done),
        });
        match ok {
            Ok(()) => {
                let id = self.id;
                if !call_void("takePhoto", "(J)V", move |_| Some(vec![long(id)])) {
                    photo_done(self.id, None);
                }
            }
            Err(done) => done(Err(CaptureError::WrongPurpose)),
        }
    }

    fn preview_id(&self) -> u64 {
        self.id
    }
}

impl Drop for AndroidCapture {
    fn drop(&mut self) {
        // The scanner thread ends after its current frame, unwaited (D-0116).
        with(|s| s.sessions.remove(&self.id));
        let id = self.id;
        call_void("close", "(J)V", move |_| Some(vec![long(id)]));
    }
}

fn answer(access: Access) {
    let waiting = with(|s| std::mem::take(&mut s.asking));
    for done in waiting {
        done(access);
    }
}

fn photo_done(id: u64, jpeg_bytes: Option<Vec<u8>>) {
    let Some(done) = with(|s| {
        s.sessions
            .get_mut(&id)
            .and_then(|x| (!x.photos.is_empty()).then(|| x.photos.remove(0)))
    }) else {
        return;
    };
    done(match jpeg_bytes {
        None => Err(CaptureError::Failed("CameraX took no photo".into())),
        Some(bytes) => jpeg::strip(&bytes)
            .map(|s| {
                let (width, height) = s.upright_size();
                Still {
                    bytes: s.bytes,
                    mime: "image/jpeg",
                    width,
                    height,
                }
            })
            .map_err(|e| CaptureError::Failed(format!("{e:?}"))),
    });
}

fn sink_of(id: u64) -> Option<Sink> {
    with(|s| s.sessions.get(&id).map(|x| x.sink.clone()))
}

/// The bytes of a direct buffer, valid while the Java call that handed it is running.
///
/// # Safety
///
/// `buffer` is a direct `ByteBuffer` Java keeps alive and unchanged for the current call.
unsafe fn direct<'a>(env: &JNIEnv<'_>, buffer: &JByteBuffer<'_>) -> Option<&'a [u8]> {
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let ptr = env.get_direct_buffer_address(buffer).ok()?;
    let len = env.get_direct_buffer_capacity(buffer).ok()?;
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the caller's contract; JNI gives the buffer's address and capacity.
    Some(unsafe { std::slice::from_raw_parts(ptr, len) }) // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
}

// ---------------------------------------------------------------- called by Java

/// `CameraBridge.nativeInit()`: the activity is up.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_camera_CameraBridge_nativeInit(
    env: JNIEnv<'_>,
    class: JClass<'_>,
) {
    let Ok(vm) = env.get_java_vm() else { return };
    let Ok(bridge) = env.new_global_ref(class) else {
        return;
    };
    with(|s| {
        s.vm = Some(vm);
        s.bridge = Some(bridge);
    });
}

/// `CameraBridge.nativeAccess(int)`: the permission dialog answered.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_camera_CameraBridge_nativeAccess(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    access: jint,
) {
    answer(match access {
        1 => Access::Granted,
        3 => Access::Restricted,
        0 => Access::NotAsked,
        _ => Access::Denied,
    });
}

/// `CameraBridge.nativeRunning(long, String)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_camera_CameraBridge_nativeRunning(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
    lens: JString<'_>,
) {
    let Some(sink) = id_from_jlong(id).and_then(sink_of) else {
        return;
    };
    let lens: String = env.get_string(&lens).map(Into::into).unwrap_or_default();
    sink.running(&lens);
}

/// `CameraBridge.nativeUnavailable(long, int)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_camera_CameraBridge_nativeUnavailable(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
    why: jint,
) {
    let Some(sink) = id_from_jlong(id).and_then(sink_of) else {
        return;
    };
    sink.unavailable(match why {
        0 => Unavailable::NoCamera,
        1 => Unavailable::InUse,
        2 => Unavailable::Interrupted,
        3 => Unavailable::SystemOff,
        code => Unavailable::Failed(format!("CameraX {code}")),
    });
}

/// `CameraBridge.nativeFrame(...)`: an analysis frame, valid during the call.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments, clippy::many_single_char_names)] // Java's signature
pub extern "system" fn Java_chat_songbird_camera_CameraBridge_nativeFrame(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
    y: JByteBuffer<'_>,
    y_stride: jint,
    u: JByteBuffer<'_>,
    v: JByteBuffer<'_>,
    uv_stride: jint,
    uv_pixel: jint,
    width: jint,
    height: jint,
    rotation: jint,
    timestamp_nanos: jlong,
) {
    let Some(id) = id_from_jlong(id) else { return };
    let (Ok(w), Ok(h)) = (u32::try_from(width), u32::try_from(height)) else {
        return;
    };
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return;
    }
    let Some((sink, scanning)) = with(|s| {
        s.sessions
            .get(&id)
            .map(|x| (x.sink.clone(), x.scanner.is_some()))
    }) else {
        return;
    };
    let (Ok(ys), Ok(uvs), Ok(uvp)) = (
        usize::try_from(y_stride),
        usize::try_from(uv_stride),
        usize::try_from(uv_pixel),
    ) else {
        return;
    };
    // SAFETY: CameraX's planes, direct buffers Java keeps open until this call returns.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let Some(y) = (unsafe { direct(&env, &y) }) else {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        return;
    };
    let rotation = match rotation {
        90 => Rotation::Deg90,
        180 => Rotation::Deg180,
        270 => Rotation::Deg270,
        _ => Rotation::Deg0,
    };
    let timestamp_us = u64::try_from(timestamp_nanos / 1000).unwrap_or(0);
    let (wu, hu) = (w as usize, h as usize);
    if scanning {
        // Scanning needs the luma only.
        let Some(luma) = crate::frame::luma_rows(y, ys, wu, hu) else {
            return;
        };
        let frame = Frame {
            width: w,
            height: h,
            rotation,
            timestamp_us,
            pixels: Pixels::Grey {
                data: luma,
                stride: wu,
            },
        };
        with(|s| {
            if let Some(scanner) = s.sessions.get(&id).and_then(|x| x.scanner.as_ref()) {
                scanner.offer(frame);
            }
        });
        return;
    }
    // SAFETY: as above.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let (Some(u), Some(v)) = (unsafe { direct(&env, &u) }, unsafe { direct(&env, &v) }) else {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        return;
    };
    let Some(pixels) = crate::frame::nv12_from_yuv(y, ys, u, v, uvs, uvp, wu, hu) else {
        return;
    };
    let frame = Frame {
        width: w,
        height: h,
        rotation,
        timestamp_us,
        pixels,
    };
    if frame.is_consistent() {
        sink.frame(frame);
    }
}

/// `CameraBridge.nativePhoto(long, byte[])`: a JPEG, or null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_camera_CameraBridge_nativePhoto(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
    jpeg_bytes: JByteArray<'_>,
) {
    let Some(id) = id_from_jlong(id) else { return };
    let bytes = if jpeg_bytes.is_null() {
        None
    } else {
        let len = env
            .get_array_length(&jpeg_bytes)
            .ok()
            .and_then(|n| usize::try_from(n).ok());
        len.filter(|&n| n <= jpeg::MAX_LEN)
            .and_then(|_| env.convert_byte_array(&jpeg_bytes).ok())
    };
    photo_done(id, bytes);
}

// ---------------------------------------------------------------- call video (D-0183)

/// A frame of a call's video for `CameraBridge.videoFrame`: the pixels as a direct buffer,
/// copied into the feed's bitmap before the call returns.
pub(crate) fn video_frame(feed: u64, width: u32, height: u32, rgba: &[u8]) {
    let (Ok(feed), Ok(w), Ok(h)) = (
        i64::try_from(feed),
        i32::try_from(width),
        i32::try_from(height),
    ) else {
        return;
    };
    let ptr = rgba.as_ptr().cast_mut();
    let len = rgba.len();
    call_void("videoFrame", "(JIILjava/nio/ByteBuffer;)V", |env| {
        // SAFETY: the buffer views `rgba`, which outlives this call; Java only reads it, and
        // copies it into a bitmap before returning.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let buf = unsafe { env.new_direct_byte_buffer(ptr, len) }.ok()?;
        Some(vec![
            JValueOwned::Long(feed),
            JValueOwned::Int(w),
            JValueOwned::Int(h),
            JValueOwned::Object(buf.into()),
        ])
    });
}

/// `CameraBridge.videoClear(feed)`.
pub(crate) fn video_clear(feed: u64) {
    let Ok(feed) = i64::try_from(feed) else {
        return;
    };
    call_void("videoClear", "(J)V", |_| {
        Some(vec![JValueOwned::Long(feed)])
    });
}

// ------------------------------------------------------------ the native components (D-0188)

/// Registers `songbird/camera` (CameraX's preview of a session, its `session` prop) and
/// `songbird/video` (a call participant's frames, its `feed` prop) with NSH's own activity:
/// NSH 0.7 lets an app's crate make Android components (`#[native_components(android)]`), so
/// Songbird has no activity of its own to register them in.
pub fn register(components: &nsh::Components) {
    components.register(
        "songbird/camera",
        std::rc::Rc::new(|cx| {
            JavaView::make(
                &cx,
                "chat/songbird/camera/CameraPreview",
                "session",
                "setSession",
            )
        }),
    );
    components.register(
        "songbird/video",
        std::rc::Rc::new(|cx| {
            JavaView::make(
                &cx,
                "chat/songbird/camera/VideoFrameView",
                "feed",
                "setFeed",
            )
        }),
    );
}

/// A component whose Java object (`view()`, `<setter>(long)`, `destroy()`) does the work.
struct JavaView {
    /// The Java object; `None` when it couldn't be made (then `view` is an empty `View`).
    object: Option<GlobalRef>,
    view: GlobalRef,
    /// The one prop, a number, and the Java method taking it.
    prop: &'static str,
    setter: &'static str,
}

impl JavaView {
    fn make(
        cx: &nsh::ComponentContext,
        class: &'static str,
        prop: &'static str,
        setter: &'static str,
    ) -> Box<dyn nsh::NativeComponent> {
        let context = cx.android_context().clone();
        // Called on the UI thread from NSH's Java, so the app's class loader finds `class`.
        let made = cx.with_env(|env| {
            let object = env.new_object(
                class,
                "(Landroid/content/Context;)V",
                &[JValue::Object(context.as_obj())],
            )?;
            let view = env
                .call_method(&object, "view", "()Landroid/view/View;", &[])?
                .l()?;
            Ok((env.new_global_ref(&object)?, env.new_global_ref(&view)?))
        });
        let (object, view) = match made {
            Ok((object, view)) => (Some(object), view),
            Err(e) => {
                log::error!("{class}: {e}");
                // An empty view in its place: the screen around it says the camera is out.
                let empty = cx.with_env(|env| {
                    let view = env.new_object(
                        "android/view/View",
                        "(Landroid/content/Context;)V",
                        &[JValue::Object(context.as_obj())],
                    )?;
                    env.new_global_ref(&view)
                });
                match empty {
                    Ok(view) => (None, view),
                    // NSH catches this and puts an empty view there itself.
                    Err(e) => panic!("{class}: not even an empty view: {e}"),
                }
            }
        };
        Box::new(Self {
            object,
            view,
            prop,
            setter,
        })
    }
}

impl nsh::NativeComponent for JavaView {
    fn view(&self) -> &GlobalRef {
        &self.view
    }

    fn set_prop(&mut self, env: &mut JNIEnv<'_>, name: &str, value: Option<&serde_json::Value>) {
        if name != self.prop {
            return;
        }
        let Some(object) = &self.object else {
            return;
        };
        // Ids are whole numbers; none (or not a number) is -1: no session, no feed.
        let id = value.and_then(serde_json::Value::as_i64).unwrap_or(-1);
        if env
            .call_method(object, self.setter, "(J)V", &[JValue::Long(id)])
            .is_err()
        {
            let _ = env.exception_clear();
        }
    }

    fn destroy(&mut self, env: &mut JNIEnv<'_>) {
        if let Some(object) = self.object.take()
            && env.call_method(&object, "destroy", "()V", &[]).is_err()
        {
            let _ = env.exception_clear();
        }
    }
}

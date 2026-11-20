//! Android: the Keystore and BiometricPrompt in Songbird's Gradle host (`apps/android`,
//! `chat.songbird.device.DeviceBridge`), reached over JNI (D-0172).
//!
//! `unsafe` here: the exported JNI entry points (`#[unsafe(no_mangle)]`) and rebuilding the
//! JavaVM handle Java gave in `nativeInit`. Each block says why it holds.

#![allow(
    unsafe_code,
    reason = "D-0172: JNI entry points; each block says why it holds"
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
    objects::{GlobalRef, JByteArray, JClass, JValue, JValueOwned},
    sys::{jint, jlong},
};
use zeroize::Zeroizing;

use crate::{
    biometrics::{Done, Failure, Kind},
    secure::{Error, Keep, fresh},
};

#[derive(Default)]
struct State {
    vm: Option<JavaVM>,
    bridge: Option<GlobalRef>,
    prompts: HashMap<u64, Done>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

fn state() -> MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(state().get_or_insert_with(State::default))
}

fn host() -> Option<(JavaVM, GlobalRef)> {
    let (vm, bridge) = with(|s| Some((s.vm.as_ref()?.get_java_vm_pointer(), s.bridge.clone()?)))?;
    // SAFETY: the pointer is of the JavaVM Java handed over in `nativeInit`, which lives as long
    // as the process; `from_raw` makes a handle that does not own it.
    let vm = unsafe { JavaVM::from_raw(vm) }.ok()?; // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    Some((vm, bridge))
}

/// Calls the static `DeviceBridge.<name>` and reads its result; `None` when the host isn't
/// attached or Java threw.
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

fn bytes_call(name: &str, input: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    call(
        name,
        "([B)[B",
        |env| {
            Some(vec![JValueOwned::Object(
                env.byte_array_from_slice(input).ok()?.into(),
            )])
        },
        |env, v| {
            let arr = JByteArray::from(v.l().ok()?);
            if arr.is_null() {
                return None;
            }
            env.convert_byte_array(&arr).ok().map(Zeroizing::new)
        },
    )
}

pub(crate) fn vault_key(keep: &dyn Keep) -> Result<Zeroizing<[u8; 32]>, Error> {
    if host().is_none() {
        return Err(Error::Unavailable);
    }
    if let Some(wrapped) = keep.load() {
        let k = bytes_call("unwrapKey", &wrapped).ok_or(Error::Refused)?;
        let k: [u8; 32] = k.as_slice().try_into().map_err(|_| Error::Refused)?;
        return Ok(Zeroizing::new(k));
    }
    let k = fresh()?;
    let wrapped = bytes_call("wrapKey", &k[..]).ok_or(Error::Unavailable)?;
    keep.save(&wrapped);
    Ok(k)
}

pub(crate) fn biometrics() -> Option<Kind> {
    let ok = call("biometrics", "()Z", |_| Some(Vec::new()), |_, v| v.z().ok())?;
    ok.then_some(Kind::Android)
}

pub(crate) fn authenticate(reason: &str, done: Done) {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    with(|s| s.prompts.insert(id, done));
    let started = call(
        "authenticate",
        "(Ljava/lang/String;J)V",
        |env| {
            Some(vec![
                JValueOwned::Object(env.new_string(reason).ok()?.into()),
                JValueOwned::Long(i64::try_from(id).unwrap_or(i64::MAX)),
            ])
        },
        |_, _| Some(()),
    );
    if started.is_none()
        && let Some(d) = with(|s| s.prompts.remove(&id))
    {
        d(Err(Failure::Unavailable));
    }
}

/// `DeviceBridge.nativeInit()`: the activity is up.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_device_DeviceBridge_nativeInit(
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

/// `DeviceBridge.nativeAuthResult(id, code)`: 0 matched, 1 cancelled, 2 not recognised,
/// anything else unavailable.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_device_DeviceBridge_nativeAuthResult(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
    code: jint,
) {
    let Ok(id) = u64::try_from(id) else { return };
    let Some(done) = with(|s| s.prompts.remove(&id)) else {
        return;
    };
    done(match code {
        0 => Ok(()),
        1 => Err(Failure::Cancelled),
        2 => Err(Failure::NotRecognised),
        _ => Err(Failure::Unavailable),
    });
}

fn call_bool(name: &str, on: bool) {
    let _ = call(
        name,
        "(Z)V",
        |_| Some(vec![JValueOwned::Bool(u8::from(on))]),
        |_, _| Some(()),
    );
}

/// `FLAG_SECURE` on the activity's window.
pub(crate) fn set_secure(on: bool) {
    call_bool("setSecure", on);
}

/// A blank recents thumbnail (Android 13+).
pub(crate) fn hide_in_recents(on: bool) {
    call_bool("hideInRecents", on);
}

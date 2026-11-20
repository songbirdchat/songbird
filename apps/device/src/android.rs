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

// ---------------------------------------------------------------- location (D-0174)

static LOCATING: Mutex<Vec<(u64, crate::location::Done)>> = Mutex::new(Vec::new());

pub(crate) fn location(done: crate::location::Done) {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    LOCATING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((id, done));
    let started = call(
        "locate",
        "(J)V",
        |_| {
            Some(vec![JValueOwned::Long(
                i64::try_from(id).unwrap_or(i64::MAX),
            )])
        },
        |_, _| Some(()),
    );
    if started.is_none() {
        finish_location(id, Err(crate::location::Failure::Unavailable));
    }
}

fn finish_location(id: u64, r: Result<crate::location::Position, crate::location::Failure>) {
    let done = {
        let mut l = LOCATING.lock().unwrap_or_else(PoisonError::into_inner);
        l.iter().position(|(i, _)| *i == id).map(|i| l.remove(i).1)
    };
    if let Some(d) = done {
        d(r);
    }
}

/// `DeviceBridge.nativeLocation(id, code, lat, lon, accuracy)`: code 0 a position, 1 denied,
/// anything else unavailable.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_device_DeviceBridge_nativeLocation(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
    code: jint,
    lat: jni::sys::jdouble,
    lon: jni::sys::jdouble,
    accuracy: jni::sys::jdouble,
) {
    use crate::location::{Failure, Position};
    let Ok(id) = u64::try_from(id) else { return };
    finish_location(
        id,
        match code {
            0 => Ok(Position {
                latitude: lat,
                longitude: lon,
                accuracy_m: accuracy,
            }),
            1 => Err(Failure::Denied),
            _ => Err(Failure::Unavailable),
        },
    );
}

// ---------------------------------------------------------------- photos (D-0174)

pub(crate) fn save_media(
    bytes: &[u8],
    kind: crate::photos::Kind,
    name: &str,
    mime: &str,
    done: crate::photos::Done,
) {
    let name = crate::photos::safe_name(name);
    let ok = call(
        "saveMedia",
        "([BLjava/lang/String;Ljava/lang/String;Z)Z",
        |env| {
            Some(vec![
                JValueOwned::Object(env.byte_array_from_slice(bytes).ok()?.into()),
                JValueOwned::Object(env.new_string(&name).ok()?.into()),
                JValueOwned::Object(env.new_string(mime).ok()?.into()),
                JValueOwned::Bool(u8::from(kind == crate::photos::Kind::Video)),
            ])
        },
        |_, v| v.z().ok(),
    );
    done(if ok == Some(true) {
        Ok(())
    } else {
        Err(crate::photos::Failure::Failed)
    });
}

// ---------------------------------------------------------------- call audio (D-0176)

static MIC: Mutex<Vec<crate::audio::Done>> = Mutex::new(Vec::new());

pub(crate) fn microphone(done: crate::audio::Done) {
    let granted = call("microphone", "()I", |_| Some(Vec::new()), |_, v| v.i().ok());
    match granted {
        // 1: granted already; 0: asked now, the answer comes to `nativeMicrophone`.
        Some(1) => done(true),
        Some(0) => MIC
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(done),
        _ => done(false),
    }
}

/// `DeviceBridge.nativeMicrophone(granted)`: the permission dialog answered.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_device_DeviceBridge_nativeMicrophone(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    granted: jni::sys::jboolean,
) {
    let waiting = std::mem::take(&mut *MIC.lock().unwrap_or_else(PoisonError::into_inner));
    for d in waiting {
        d(granted != 0);
    }
}

pub(crate) fn audio_mode(call: bool) {
    call_bool("audioMode", call);
}

pub(crate) fn speaker(on: bool) {
    call_bool("speaker", on);
}

// ---------------------------------------------------------------- push (D-0176)

static ON_TOKEN: std::sync::OnceLock<crate::push::OnToken> = std::sync::OnceLock::new();

pub(crate) fn register_push(on_token: crate::push::OnToken) -> bool {
    let _ = ON_TOKEN.set(on_token);
    call("registerPush", "()V", |_| Some(Vec::new()), |_, _| Some(())).is_some()
}

/// `DeviceBridge.nativePushEndpoint(url, p256dh, auth)`: the distributor's endpoint.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_device_DeviceBridge_nativePushEndpoint(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    url: jni::objects::JString<'_>,
    p256dh: JByteArray<'_>,
    auth: JByteArray<'_>,
) {
    let Ok(url) = env.get_string(&url).map(String::from) else {
        return;
    };
    let (Ok(p), Ok(a)) = (
        env.convert_byte_array(&p256dh),
        env.convert_byte_array(&auth),
    ) else {
        return;
    };
    let (Ok(p256dh), Ok(auth)) = (<[u8; 65]>::try_from(p), <[u8; 16]>::try_from(a)) else {
        return;
    };
    if let Some(on) = ON_TOKEN.get() {
        on(crate::push::Token::WebPush {
            endpoint: url,
            p256dh,
            auth,
        });
    }
}

// ---------------------------------------------------------------- passkeys (D-0177)

fn b64url(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut s = String::with_capacity(b.len() * 4 / 3 + 2);
    for chunk in b.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, &x)| acc | (u32::from(x) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            let idx = usize::try_from((n >> (18 - 6 * i)) & 63).unwrap_or(0);
            s.push(char::from(A.get(idx).copied().unwrap_or(b'A')));
        }
    }
    s
}

fn unb64url(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).ok()?);
        }
    }
    Some(out)
}

pub(crate) fn passkeys_available() -> bool {
    call(
        "passkeysAvailable",
        "()Z",
        |_| Some(Vec::new()),
        |_, v| v.z().ok(),
    ) == Some(true)
}

enum PasskeyDone {
    Create(crate::passkeys::CreateDone),
    Assert(crate::passkeys::AssertDone),
}

static PASSKEYS: Mutex<Vec<(u64, PasskeyDone)>> = Mutex::new(Vec::new());

fn passkey_call(method: &str, json: &str, done: PasskeyDone) {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    PASSKEYS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((id, done));
    let started = call(
        method,
        "(Ljava/lang/String;J)V",
        |env| {
            Some(vec![
                JValueOwned::Object(env.new_string(json).ok()?.into()),
                JValueOwned::Long(i64::try_from(id).unwrap_or(i64::MAX)),
            ])
        },
        |_, _| Some(()),
    );
    if started.is_none() {
        finish_passkey(id, 3, "");
    }
}

pub(crate) fn passkey_create(
    rp_id: &str,
    user_id: &[u8],
    name: &str,
    challenge: &[u8],
    done: crate::passkeys::CreateDone,
) {
    let json = serde_json::json!({
        "rp": { "id": rp_id, "name": "Songbird" },
        "user": { "id": b64url(user_id), "name": name, "displayName": name },
        "challenge": b64url(challenge),
        "pubKeyCredParams": [
            { "type": "public-key", "alg": -7 },
            { "type": "public-key", "alg": -8 }
        ],
        "timeout": 120_000,
        "attestation": "none",
        "authenticatorSelection": { "residentKey": "required", "userVerification": "required" }
    });
    passkey_call(
        "createPasskey",
        &json.to_string(),
        PasskeyDone::Create(done),
    );
}

pub(crate) fn passkey_assert(
    rp_id: &str,
    challenge: &[u8],
    allow: &[Vec<u8>],
    done: crate::passkeys::AssertDone,
) {
    let json = serde_json::json!({
        "rpId": rp_id,
        "challenge": b64url(challenge),
        "allowCredentials": allow
            .iter()
            .map(|id| serde_json::json!({ "type": "public-key", "id": b64url(id) }))
            .collect::<Vec<_>>(),
        "userVerification": "required",
        "timeout": 120_000
    });
    passkey_call("getPasskey", &json.to_string(), PasskeyDone::Assert(done));
}

/// code 0 a response (WebAuthn JSON), 1 cancelled, 2 not allowed, else unavailable.
fn finish_passkey(id: u64, code: i32, json: &str) {
    use crate::passkeys::{Assertion, Created, Failure};
    let done = {
        let mut l = PASSKEYS.lock().unwrap_or_else(PoisonError::into_inner);
        l.iter().position(|(i, _)| *i == id).map(|i| l.remove(i).1)
    };
    let Some(done) = done else { return };
    let fail = match code {
        0 => None,
        1 => Some(Failure::Cancelled),
        2 => Some(Failure::NotAllowed),
        _ => Some(Failure::Unavailable),
    };
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or(serde_json::Value::Null);
    let field = |path: &str| {
        v.pointer(path)
            .and_then(serde_json::Value::as_str)
            .and_then(unb64url)
    };
    match done {
        PasskeyDone::Create(d) => d(match fail {
            Some(f) => Err(f),
            None => match (
                field("/rawId"),
                field("/response/clientDataJSON"),
                field("/response/attestationObject"),
            ) {
                (Some(credential_id), Some(client_data_json), Some(attestation_object)) => {
                    Ok(Created {
                        credential_id,
                        client_data_json,
                        attestation_object,
                    })
                }
                _ => Err(Failure::Unavailable),
            },
        }),
        PasskeyDone::Assert(d) => d(match fail {
            Some(f) => Err(f),
            None => match (
                field("/rawId"),
                field("/response/authenticatorData"),
                field("/response/clientDataJSON"),
                field("/response/signature"),
            ) {
                (
                    Some(credential_id),
                    Some(authenticator_data),
                    Some(client_data_json),
                    Some(signature),
                ) => Ok(Assertion {
                    credential_id,
                    authenticator_data,
                    client_data_json,
                    signature,
                }),
                _ => Err(Failure::Unavailable),
            },
        }),
    }
}

/// `DeviceBridge.nativePasskey(id, code, json)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_device_DeviceBridge_nativePasskey(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
    code: jint,
    json: jni::objects::JString<'_>,
) {
    let json = env.get_string(&json).map(String::from).unwrap_or_default();
    if let Ok(id) = u64::try_from(id) {
        finish_passkey(id, code, &json);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64url_round_trips() {
        for n in 0..40u8 {
            let b: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(super::unb64url(&super::b64url(&b)).unwrap(), b);
        }
    }
}

// ---------------------------------------------------------------- the store (D-0178)

static STORE: Mutex<Vec<(u64, crate::store::Done)>> = Mutex::new(Vec::new());

fn store_call(method: &str, product: Option<&str>, done: crate::store::Done) {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    STORE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((id, done));
    let long = JValueOwned::Long(i64::try_from(id).unwrap_or(i64::MAX));
    let started = match product {
        Some(p) => call(
            method,
            "(Ljava/lang/String;J)V",
            |env| {
                Some(vec![
                    JValueOwned::Object(env.new_string(p).ok()?.into()),
                    long,
                ])
            },
            |_, _| Some(()),
        ),
        None => call(method, "(J)V", |_| Some(vec![long]), |_, _| Some(())),
    };
    if started.is_none() {
        finish_store(id, 3, "");
    }
}

fn finish_store(id: u64, code: i32, token: &str) {
    use crate::store::{Failure, Proof};
    let done = {
        let mut l = STORE.lock().unwrap_or_else(PoisonError::into_inner);
        l.iter().position(|(i, _)| *i == id).map(|i| l.remove(i).1)
    };
    if let Some(done) = done {
        done(match code {
            0 if !token.is_empty() => Ok(Proof::Google(token.to_owned())),
            1 => Err(Failure::Cancelled),
            2 => Err(Failure::NotFound),
            _ => Err(Failure::Failed),
        });
    }
}

pub(crate) fn store_buy(product: &str, done: crate::store::Done) {
    store_call("storeBuy", Some(product), done);
}

pub(crate) fn store_current(done: crate::store::Done) {
    store_call("storeCurrent", None, done);
}

/// `DeviceBridge.nativeStore(id, code, token)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_device_DeviceBridge_nativeStore(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    id: jlong,
    code: jint,
    token: jni::objects::JString<'_>,
) {
    let token = env.get_string(&token).map(String::from).unwrap_or_default();
    if let Ok(id) = u64::try_from(id) {
        finish_store(id, code, &token);
    }
}

// ---------------------------------------------------------------- the app's icon (D-0179)

/// Enables the launcher alias `chat.songbird.app.Icon<name>` and disables the others.
pub(crate) fn set_icon(name: &str) -> bool {
    call(
        "setIcon",
        "(Ljava/lang/String;)Z",
        |env| Some(vec![JValueOwned::Object(env.new_string(name).ok()?.into())]),
        |_, v| v.z().ok(),
    ) == Some(true)
}

// ---------------------------------------------------------------- screen sharing (D-0180)

struct Share {
    on_frame: crate::share::OnFrame,
    // Behind their own locks: taken once, from any thread, while frames read `on_frame`.
    started: Mutex<Option<crate::share::OnStarted>>,
    ended: Mutex<Option<crate::share::OnEnded>>,
}

static SHARE: std::sync::RwLock<Option<Share>> = std::sync::RwLock::new(None);

pub(crate) fn share_start(
    on_frame: crate::share::OnFrame,
    started: crate::share::OnStarted,
    ended: crate::share::OnEnded,
) {
    *SHARE.write().unwrap_or_else(PoisonError::into_inner) = Some(Share {
        on_frame,
        started: Mutex::new(Some(started)),
        ended: Mutex::new(Some(ended)),
    });
    if call(
        "startScreenShare",
        "()Z",
        |_| Some(Vec::new()),
        |_, v| v.z().ok(),
    ) != Some(true)
    {
        share_started(false);
    }
}

pub(crate) fn share_stop() {
    // Stopped here: no "ended" for the app, which asked.
    if let Some(s) = SHARE
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        s.ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }
    let _ = call(
        "stopScreenShare",
        "()V",
        |_| Some(Vec::new()),
        |_, _| Some(()),
    );
    *SHARE.write().unwrap_or_else(PoisonError::into_inner) = None;
}

fn share_started(ok: bool) {
    let started = SHARE
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|s| {
            s.started
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
        });
    if let Some(started) = started {
        started(ok);
    }
    if !ok {
        *SHARE.write().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// `DeviceBridge.nativeScreenStarted(ok)`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_device_DeviceBridge_nativeScreenStarted(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    ok: jni::sys::jboolean,
) {
    share_started(ok != 0);
}

/// `DeviceBridge.nativeScreenEnded()`: the person stopped it (the notification, the system).
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_device_DeviceBridge_nativeScreenEnded(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
) {
    let ended = SHARE
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .and_then(|s| s.ended.into_inner().unwrap_or_else(PoisonError::into_inner));
    if let Some(ended) = ended {
        ended();
    }
}

/// `DeviceBridge.nativeScreenFrame(pixels, width, height, rowStride, rotation)`: one RGBA
/// frame in a direct buffer, read in place.
#[unsafe(no_mangle)]
pub extern "system" fn Java_chat_songbird_device_DeviceBridge_nativeScreenFrame(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    pixels: jni::objects::JByteBuffer<'_>,
    width: jint,
    height: jint,
    stride: jint,
    rotation: jint,
) {
    let (Ok(ptr), Ok(len)) = (
        env.get_direct_buffer_address(&pixels),
        env.get_direct_buffer_capacity(&pixels),
    ) else {
        return;
    };
    if ptr.is_null() {
        return;
    }
    // SAFETY: the buffer is the Image plane Java holds open until this call returns; `len` is
    // its capacity.
    let data = unsafe { std::slice::from_raw_parts(ptr.cast_const(), len) };
    let (Ok(width), Ok(height), Ok(stride), Ok(rotation)) = (
        u32::try_from(width),
        u32::try_from(height),
        u32::try_from(stride),
        u16::try_from(rotation),
    ) else {
        return;
    };
    if let Some(s) = SHARE
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        (s.on_frame)(crate::share::Frame {
            width,
            height,
            rotation,
            pixels: crate::share::Pixels::Rgba { data, stride },
        });
    }
}

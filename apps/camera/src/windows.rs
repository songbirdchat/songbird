//! Windows: Media Foundation (D-0110, M4; D-0114).
//!
//! - Access is the Windows privacy settings' (Settings › Privacy & security › Camera): the
//!   switches for the whole device (machine policy: `Restricted`), for the user, and for desktop
//!   apps, read from the capability consent store. Windows shows no prompt to desktop apps.
//! - Lenses come from `MFEnumDeviceSources`; a capture activates one, reads NV12 through a
//!   source reader on a thread of its own, and [`crate::desktop`] routes the frames.
//! - The preview component draws the newest image with Direct2D (Windows components draw into
//!   the window's context) and asks for a redraw when a new one comes.
//!
//! `unsafe` here: COM calls into Media Foundation, the registry and Direct2D through the
//! `windows` crate, and reading a locked media buffer. Each block says why it holds.

#![allow(
    unsafe_code,
    reason = "D-0110 §5, reviewed in D-0116: Media Foundation, the registry and Direct2D through COM"
)]

use std::sync::Arc;

use windows::{
    Win32::{
        Foundation::{E_ACCESSDENIED, ERROR_SUCCESS},
        Media::MediaFoundation::{
            IMFActivate, IMFAttributes, IMFMediaSource, IMFSample,
            MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_MT_DEFAULT_STRIDE,
            MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
            MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
            MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_ERROR, MF_VERSION, MFCreateAttributes,
            MFCreateMediaType, MFCreateSourceReaderFromMediaSource, MFEnumDeviceSources,
            MFMediaType_Video, MFSTARTUP_FULL, MFStartup, MFVideoFormat_NV12,
        },
        System::{
            Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree},
            Registry::{HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
        },
    },
    core::{HSTRING, PWSTR},
};

use crate::{
    Access, AccessCallback, Backend, Capture, Config, Facing, Frame, Lens, Pixels, Rotation, Sink,
    Unavailable,
    desktop::{Opener, Pump, ThreadCapture},
};

/// Media Foundation's camera.
#[derive(Debug, Default)]
pub struct WindowsCamera;

const CONSENT: &str =
    r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam";

/// The consent store's `Value` (`Allow` or `Deny`) under `key`, if set.
fn consent(root: HKEY, key: &str) -> Option<String> {
    let mut buf = [0u16; 16];
    let mut len = u32::try_from(std::mem::size_of_val(&buf)).ok()?;
    // SAFETY: a registry read into a buffer whose size in bytes we pass; Windows writes at most
    // that many and NUL-terminates.
    let status = unsafe {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        RegGetValueW(
            root,
            &HSTRING::from(key),
            &HSTRING::from("Value"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&raw mut len),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let chars = (len as usize / 2).min(buf.len());
    Some(
        String::from_utf16_lossy(&buf[..chars])
            .trim_end_matches('\0')
            .to_owned(),
    )
}

/// Each thread that calls Media Foundation joins the multithreaded apartment and starts it
/// once; both are reference counted, so repeating them is harmless.
fn mf() -> windows::core::Result<()> {
    // SAFETY: COM and Media Foundation start-up, as documented, with no arguments we own.
    unsafe {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL)
    }
}

/// The video capture devices, as activation objects.
fn devices() -> windows::core::Result<Vec<IMFActivate>> {
    mf()?;
    // SAFETY: Media Foundation calls with attributes made here; the array it allocates is read
    // within its count, each element's reference taken over once, then the array freed.
    unsafe {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&raw mut attrs, 1)?;
        let attrs = attrs.ok_or(windows::core::Error::from(E_ACCESSDENIED))?;
        attrs.SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )?;
        let mut array: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        MFEnumDeviceSources(&attrs, &raw mut array, &raw mut count)?;
        let mut out = Vec::new();
        if !array.is_null() {
            for i in 0..count as usize {
                if let Some(device) = std::ptr::read(array.add(i)) {
                    out.push(device);
                }
            }
            CoTaskMemFree(Some(array.cast()));
        }
        Ok(out)
    }
}

/// A device's string attribute.
fn string(device: &IMFActivate, key: &windows::core::GUID) -> Option<String> {
    let mut value = PWSTR::null();
    let mut len = 0u32;
    // SAFETY: Media Foundation allocates the string; we copy it and free it.
    unsafe {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        device
            .GetAllocatedString(key, &raw mut value, &raw mut len)
            .ok()?;
        let text = value.to_string().ok();
        CoTaskMemFree(Some(value.0.cast()));
        text
    }
}

impl Backend for WindowsCamera {
    fn access(&self) -> Access {
        let path = CONSENT.to_owned();
        if consent(HKEY_LOCAL_MACHINE, &path).as_deref() == Some("Deny") {
            return Access::Restricted;
        }
        let user = consent(HKEY_CURRENT_USER, &path);
        let desktop = consent(HKEY_CURRENT_USER, &format!(r"{path}\NonPackaged"));
        if user.as_deref() == Some("Deny") || desktop.as_deref() == Some("Deny") {
            Access::Denied
        } else {
            Access::Granted
        }
    }

    fn request_access(&self, done: AccessCallback) {
        // Windows has no prompt for desktop apps: the answer is the settings'.
        done(self.access());
    }

    fn lenses(&self) -> Vec<Lens> {
        devices()
            .unwrap_or_default()
            .iter()
            .filter_map(|d| {
                let id = string(d, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK)?;
                let name = string(d, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)
                    .unwrap_or_else(|| "Camera".into());
                Some(Lens {
                    id,
                    name,
                    facing: Facing::External,
                })
            })
            .take(16)
            .collect()
    }

    fn open(&self, config: Config, sink: Sink) -> Result<Box<dyn Capture>, Unavailable> {
        if self.access() != Access::Granted {
            return Err(Unavailable::SystemOff);
        }
        let open: Opener = Arc::new(|lens: &str, pump: Pump| {
            let lens = lens.to_owned();
            std::thread::Builder::new()
                .name("songbird-mf".into())
                .spawn(move || read(&lens, pump))
        });
        Ok(Box::new(ThreadCapture::start(
            config.purpose,
            &config.lens,
            sink,
            open,
        )?))
    }

    fn settings_url(&self) -> Option<&'static str> {
        Some("ms-settings:privacy-webcam")
    }
}

fn unavailable_of(e: &windows::core::Error) -> Unavailable {
    // HRESULTs as their bits: E_ACCESSDENIED is the privacy switch; the hardware-busy ones
    // another app.
    match u32::from_ne_bytes(e.code().0.to_ne_bytes()) {
        0x8007_0005 => Unavailable::SystemOff,
        0xC00D_3704 | 0xC00D_3EA2 | 0xC00D_3EA3 | 0x8007_00AA => Unavailable::InUse,
        0xC00D_36D5 | 0x8007_0002 => Unavailable::NoCamera,
        code => Unavailable::Failed(format!("Media Foundation {code:#010x}")),
    }
}

/// Reads `lens` (a symbolic link) until the pump says stop.
fn read(lens: &str, mut pump: Pump) {
    if let Err(e) = read_inner(lens, &mut pump) {
        pump.unavailable(unavailable_of(&e));
    }
}

fn read_inner(lens: &str, pump: &mut Pump) -> windows::core::Result<()> {
    let device = devices()?.into_iter().find(|d| {
        string(d, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK).as_deref() == Some(lens)
    });
    let Some(device) = device else {
        pump.unavailable(Unavailable::NoCamera);
        return Ok(());
    };
    // The stream "index" is a negative constant passed as its bits (0xFFFFFFFC).
    let stream = u32::from_ne_bytes(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0.to_ne_bytes());
    // SAFETY: Media Foundation's documented sequence: activate the source, make a reader with
    // video processing (so it converts to NV12), choose NV12, read samples; every object is
    // made here and released when dropped.
    let (reader, width, height, stride) = unsafe {
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let source: IMFMediaSource = device.ActivateObject()?;
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&raw mut attrs, 1)?;
        let attrs = attrs.ok_or(windows::core::Error::from(E_ACCESSDENIED))?;
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        let reader = MFCreateSourceReaderFromMediaSource(&source, &attrs)?;
        let wanted = MFCreateMediaType()?;
        wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        reader.SetCurrentMediaType(stream, None, &wanted)?;
        let current = reader.GetCurrentMediaType(stream)?;
        let size = current.GetUINT64(&MF_MT_FRAME_SIZE)?;
        let (width, height) = ((size >> 32) as u32, (size & 0xFFFF_FFFF) as u32);
        // A negative default stride means bottom-up rows; NV12 is top-down in practice.
        let stride = current.GetUINT32(&MF_MT_DEFAULT_STRIDE).map_or(width, |s| {
            i32::from_ne_bytes(s.to_ne_bytes()).unsigned_abs()
        });
        (reader, width, height, stride)
    };
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        pump.unavailable(Unavailable::Failed("frame size".into()));
        return Ok(());
    }
    let (h, ys) = (height as usize, (stride as usize).max(width as usize));
    while !pump.stopped() {
        let mut flags = 0u32;
        let mut timestamp = 0i64;
        let mut sample: Option<IMFSample> = None;
        // SAFETY: a synchronous read into out-parameters made here.
        unsafe {
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            reader.ReadSample(
                stream,
                0,
                None,
                Some(&raw mut flags),
                Some(&raw mut timestamp),
                Some(&raw mut sample),
            )?;
        }
        let ended = u32::from_ne_bytes(
            (MF_SOURCE_READERF_ERROR.0 | MF_SOURCE_READERF_ENDOFSTREAM.0).to_ne_bytes(),
        );
        if flags & ended != 0 {
            pump.unavailable(Unavailable::Interrupted);
            return Ok(());
        }
        let Some(sample) = sample else { continue };
        pump.running(lens);
        // SAFETY: the buffer is locked while we copy at most its current length from it.
        let bytes = unsafe {
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut data = std::ptr::null_mut();
            let mut len = 0u32;
            buffer.Lock(&raw mut data, None, Some(&raw mut len))?;
            let copy = if data.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(data, len as usize).to_vec()
            };
            buffer.Unlock()?;
            copy
        };
        // NV12: `ys × rows` of luma (rows may be padded to an even or 16-aligned height), then
        // half as many rows of chroma.
        let rows = bytes
            .len()
            .checked_mul(2)
            .map_or(0, |n| n / 3 / ys.max(1))
            .max(h);
        let Some(uv_start) = ys.checked_mul(rows) else {
            continue;
        };
        let (Some(y), Some(uv)) = (bytes.get(..ys * h), bytes.get(uv_start..)) else {
            continue;
        };
        let frame = Frame {
            width,
            height,
            rotation: Rotation::Deg0,
            timestamp_us: u64::try_from(timestamp / 10).unwrap_or(0),
            pixels: Pixels::Nv12 {
                y: y.to_vec(),
                y_stride: ys,
                uv: uv.to_vec(),
                uv_stride: ys,
            },
        };
        pump.frame(frame);
    }
    Ok(())
}

// ---------------------------------------------------------------- the preview

mod preview {
    use std::{cell::Cell, rc::Rc, sync::Arc};

    use nsh_platform_windows::{ComponentContext, Components, NativeComponent};
    use windows::Win32::Graphics::{
        Direct2D::{
            Common::{D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT},
            D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1, D2D1_INTERPOLATION_MODE_LINEAR,
            ID2D1DeviceContext,
        },
        Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
    };

    use crate::desktop::{latest, watch};

    struct Preview {
        cx: Arc<ComponentContext>,
        session: Rc<Cell<u64>>,
    }

    impl NativeComponent for Preview {
        #[allow(clippy::cast_precision_loss)] // preview images are at most 640 a side
        fn draw(&self, dc: &ID2D1DeviceContext, size: (f32, f32)) {
            let Some((_, image)) = latest(self.session.get()) else {
                return;
            };
            let (iw, ih) = (image.width as f32, image.height as f32);
            // Cover: the image's centre, cropped to the view's aspect.
            let scale = (size.0 / iw).max(size.1 / ih);
            let (cw, ch) = (size.0 / scale, size.1 / scale);
            let src = D2D_RECT_F {
                left: (iw - cw) / 2.0,
                top: (ih - ch) / 2.0,
                right: f32::midpoint(iw, cw),
                bottom: f32::midpoint(ih, ch),
            };
            let dest = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: size.0,
                bottom: size.1,
            };
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
                colorContext: std::mem::ManuallyDrop::new(None),
            };
            // SAFETY: Direct2D calls on the context NSH draws with; the pixels outlive the call
            // that copies them into the bitmap.
            unsafe {
                // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
                if let Ok(bitmap) = dc.CreateBitmap(
                    D2D_SIZE_U {
                        width: image.width,
                        height: image.height,
                    },
                    Some(image.bgra.as_ptr().cast()),
                    image.width * 4,
                    &raw const props,
                ) {
                    dc.DrawBitmap(
                        &bitmap,
                        Some(&raw const dest),
                        1.0,
                        D2D1_INTERPOLATION_MODE_LINEAR,
                        Some(&raw const src),
                        None,
                    );
                }
            }
        }

        fn set_prop(&mut self, name: &str, value: Option<&serde_json::Value>) {
            if name != "session" {
                return;
            }
            let id = value.and_then(serde_json::Value::as_u64).unwrap_or(0);
            self.session.set(id);
            if id != 0 {
                let cx = self.cx.clone();
                // ComponentContext is Send + Sync: the capture thread asks for the redraw.
                watch(id, Arc::new(move || cx.redraw()));
            }
            self.cx.redraw();
        }
    }

    /// Registers `songbird/camera`.
    pub fn register(components: &Components) {
        components.register(
            "songbird/camera",
            Rc::new(|cx: ComponentContext| {
                Box::new(Preview {
                    cx: Arc::new(cx),
                    session: Rc::new(Cell::new(0)),
                }) as Box<dyn NativeComponent>
            }),
        );
    }
}

pub use preview::register;

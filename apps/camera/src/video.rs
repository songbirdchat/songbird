//! Another participant's video, drawn natively on phones (D-0183): the decoded frames go from
//! the call's decoder thread straight to the native view `songbird/video`, never through the
//! UI engine. As NSH images, each frame was encoded to PNG and decoded again, and loaded
//! asynchronously: on a phone that lagged and blinked.
//!
//! - **iOS**: a `UIView` whose layer shows a `CGImage` made from the pixels (aspect fill).
//! - **Android**: `chat.songbird.camera.VideoFrameView`, a view drawing a reused `Bitmap`.
//! - **Desktops** draw pixel buffers directly already: [`supported`] is false and the app keeps
//!   its images.
//!
//! A feed is one participant's video in one call ([`feed_id`]); a view shows the feed its
//! `feed` prop names, and the latest frame at once when it appears.

/// Whether this platform draws call video with `songbird/video`.
#[must_use]
pub fn supported() -> bool {
    cfg!(any(target_os = "ios", target_os = "android"))
}

/// A participant identity's feed number (32 bits, so it travels exactly as a JSON number).
#[must_use]
pub fn feed_id(identity: &str) -> u64 {
    // FNV-1a.
    let mut h: u32 = 0x811c_9dc5;
    for b in identity.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    u64::from(h)
}

/// The latest frame of `feed`: `width * height` RGBA pixels (R, G, B, A bytes), upright. From
/// any thread; a frame that arrives before the last was drawn replaces it.
#[allow(
    unused_variables,
    clippy::needless_pass_by_value,
    reason = "platforms without the view; iOS keeps the pixels"
)]
pub fn show(feed: u64, width: u32, height: u32, rgba: Vec<u8>) {
    let Some(len) = usize::try_from(u64::from(width) * u64::from(height) * 4).ok() else {
        return;
    };
    let whole = width > 0 && height > 0 && rgba.len() >= len;
    #[cfg(target_os = "ios")]
    if whole {
        ios::show(feed, width, height, rgba);
    }
    #[cfg(target_os = "android")]
    if whole {
        crate::android::video_frame(feed, width, height, &rgba);
    }
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    let _ = whole;
}

/// `feed` ended (they left, or turned their camera off): its views go blank.
#[allow(unused_variables, reason = "platforms without the view")]
pub fn clear(feed: u64) {
    #[cfg(target_os = "ios")]
    ios::clear(feed);
    #[cfg(target_os = "android")]
    crate::android::video_clear(feed);
}

#[cfg(target_os = "ios")]
mod ios {
    #![allow(unsafe_code, reason = "D-0183: CoreGraphics and CALayer through objc2")]

    use std::{
        cell::RefCell,
        collections::{HashMap, HashSet},
        sync::{LazyLock, Mutex, PoisonError},
    };

    use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
    use objc2::{MainThreadMarker, MainThreadOnly, msg_send, rc::Retained, runtime::AnyObject};
    use objc2_core_foundation::{CFData, CFRetained, CGRect};
    use objc2_core_graphics::{
        CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
        CGImageAlphaInfo,
    };
    use objc2_quartz_core::kCAGravityResizeAspectFill;
    use objc2_ui_kit::UIView;

    /// A frame: width, height, RGBA pixels.
    type Pixels = (u32, u32, Vec<u8>);

    /// Frames waiting for the main thread, by feed (the newest only).
    static LATEST: LazyLock<Mutex<HashMap<u64, Pixels>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    /// Feeds with a draw already queued on the main thread.
    static QUEUED: LazyLock<Mutex<HashSet<u64>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

    thread_local! {
        /// Main thread: each feed's views and its latest image.
        static VIEWS: RefCell<HashMap<u64, Vec<Retained<UIView>>>> = RefCell::new(HashMap::new());
        static IMAGES: RefCell<HashMap<u64, CFRetained<CGImage>>> = RefCell::new(HashMap::new());
    }

    pub(super) fn show(feed: u64, width: u32, height: u32, rgba: Vec<u8>) {
        LATEST
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(feed, (width, height, rgba));
        if QUEUED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(feed)
        {
            dispatch2::DispatchQueue::main().exec_async(move || draw(feed));
        }
    }

    pub(super) fn clear(feed: u64) {
        LATEST
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&feed);
        dispatch2::DispatchQueue::main().exec_async(move || {
            IMAGES.with_borrow_mut(|i| i.remove(&feed));
            VIEWS.with_borrow(|v| {
                for view in v.get(&feed).into_iter().flatten() {
                    set_contents(view, None);
                }
            });
        });
    }

    /// On the main thread: the feed's newest frame as an image, on its views.
    fn draw(feed: u64) {
        QUEUED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&feed);
        let Some((w, h, px)) = LATEST
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&feed)
        else {
            return;
        };
        let Some(image) = image(w, h, &px) else {
            return;
        };
        VIEWS.with_borrow(|v| {
            for view in v.get(&feed).into_iter().flatten() {
                set_contents(view, Some(&image));
            }
        });
        IMAGES.with_borrow_mut(|i| i.insert(feed, image));
    }

    fn image(width: u32, height: u32, px: &[u8]) -> Option<CFRetained<CGImage>> {
        let (w, h) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?);
        // SAFETY: `px` holds `px.len()` bytes; CFData copies them.
        let data = unsafe { CFData::new(None, px.as_ptr(), isize::try_from(px.len()).ok()?) }?;
        let provider = CGDataProvider::with_cf_data(Some(&data))?;
        let space = CGColorSpace::new_device_rgb()?;
        // SAFETY: the layout matches `px`: 8 bits a component, 4 bytes a pixel (the fourth
        // unused), `w * 4` bytes a row, rows of `h`; no decode array.
        unsafe {
            CGImage::new(
                w,
                h,
                8,
                32,
                w * 4,
                Some(&space),
                CGBitmapInfo(CGImageAlphaInfo::NoneSkipLast.0),
                Some(&provider),
                std::ptr::null(),
                false,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
        }
    }

    fn set_contents(view: &UIView, image: Option<&CGImage>) {
        let layer = view.layer();
        // SAFETY: a CGImage is the object a layer's `contents` takes; set on the main thread.
        unsafe {
            let obj = image.map(|i| &*std::ptr::from_ref(i).cast::<AnyObject>());
            layer.setContents(obj);
        }
    }

    struct VideoView {
        view: Retained<UIView>,
        feed: Option<u64>,
    }

    impl VideoView {
        fn unregister(&mut self) {
            if let Some(feed) = self.feed.take() {
                VIEWS.with_borrow_mut(|v| {
                    if let Some(list) = v.get_mut(&feed) {
                        list.retain(|x| {
                            !std::ptr::eq(Retained::as_ptr(x), Retained::as_ptr(&self.view))
                        });
                        if list.is_empty() {
                            v.remove(&feed);
                        }
                    }
                });
            }
        }
    }

    impl NativeComponent for VideoView {
        fn view(&self) -> &UIView {
            &self.view
        }

        fn set_prop(&mut self, name: &str, value: Option<&serde_json::Value>) {
            if name != "feed" {
                return;
            }
            self.unregister();
            let Some(feed) = value.and_then(serde_json::Value::as_u64) else {
                set_contents(&self.view, None);
                return;
            };
            self.feed = Some(feed);
            VIEWS.with_borrow_mut(|v| v.entry(feed).or_default().push(self.view.clone()));
            IMAGES.with_borrow(|i| set_contents(&self.view, i.get(&feed).map(|x| &**x)));
        }
    }

    impl Drop for VideoView {
        fn drop(&mut self) {
            self.unregister();
        }
    }

    /// Registers `songbird/video`.
    ///
    /// # Panics
    ///
    /// When NSH makes a component off the main thread (it never does).
    pub fn register(components: &Components) {
        components.register(
            "songbird/video",
            std::rc::Rc::new(|_cx: ComponentContext| {
                let mtm = MainThreadMarker::new().expect("components are made on the main thread");
                // SAFETY: UIView's designated initializer.
                let view: Retained<UIView> =
                    unsafe { msg_send![UIView::alloc(mtm), initWithFrame: CGRect::ZERO] };
                view.setClipsToBounds(true);
                let layer = view.layer();
                // SAFETY: a constant CoreAnimation exports.
                layer.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
                Box::new(VideoView { view, feed: None }) as Box<dyn NativeComponent>
            }),
        );
    }
}

#[cfg(target_os = "ios")]
pub use ios::register;

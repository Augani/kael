//! Compatibility bridge from the retained screen-capture API to getDisplayMedia.

use crate::{
    DevicePixels, ForegroundExecutor, PlatformScreenCaptureFrame, ScreenCaptureFrame,
    ScreenCaptureSource, ScreenCaptureStream, SourceMetadata,
    media_capture::{
        CaptureConfig, CaptureDeviceKind, CaptureFrame, CaptureSession, FrameCallback, PixelFormat,
        default_capture_manager,
    },
    size,
};
use anyhow::{Result, anyhow};
use futures::channel::oneshot;
use parking_lot::Mutex;
use std::{rc::Rc, sync::Arc};
use wasm_bindgen::{JsCast as _, JsValue};

const BROWSER_DISPLAY_PICKER_ID: u64 = 0x6b61_656c_7765_6201;

/// Browser display capture is available only when getDisplayMedia is exposed.
pub(super) fn is_supported() -> bool {
    let Ok(media_devices) = web_sys::window()
        .ok_or_else(|| anyhow!("browser Window is unavailable"))
        .and_then(|window| {
            window
                .navigator()
                .media_devices()
                .map_err(|error| anyhow!("{error:?}"))
        })
    else {
        return false;
    };

    js_sys::Reflect::get(
        media_devices.as_ref(),
        &JsValue::from_str("getDisplayMedia"),
    )
    .ok()
    .is_some_and(|value| value.dyn_ref::<js_sys::Function>().is_some())
}

/// Return a synthetic source because the browser owns the exact screen/window
/// picker and intentionally does not permit pre-enumeration.
pub(super) fn sources() -> oneshot::Receiver<Result<Vec<Rc<dyn ScreenCaptureSource>>>> {
    let (sender, receiver) = oneshot::channel();
    let result = if is_supported() {
        let metadata = Arc::new(Mutex::new(unknown_picker_metadata()));
        Ok(vec![
            Rc::new(BrowserScreenCaptureSource { metadata }) as Rc<dyn ScreenCaptureSource>
        ])
    } else {
        Err(anyhow!(
            "browser display capture is unavailable; getDisplayMedia requires a secure, supported browser context"
        ))
    };
    let _ = sender.send(result);
    receiver
}

struct BrowserScreenCaptureSource {
    metadata: Arc<Mutex<SourceMetadata>>,
}

impl ScreenCaptureSource for BrowserScreenCaptureSource {
    fn metadata(&self) -> Result<SourceMetadata> {
        Ok(self.metadata.lock().clone())
    }

    fn stream(
        &self,
        _foreground_executor: &ForegroundExecutor,
        frame_callback: Box<dyn Fn(ScreenCaptureFrame) + Send>,
    ) -> oneshot::Receiver<Result<Box<dyn ScreenCaptureStream>>> {
        let (sender, receiver) = oneshot::channel();

        // This work intentionally remains synchronous through `session.start`:
        // getDisplayMedia must be invoked in the same trusted user-activation
        // call stack that requested the stream. The browser picker and video
        // setup continue asynchronously inside the capture backend.
        let result = (|| {
            let manager = default_capture_manager();
            let config = CaptureConfig::new("browser-display-picker", CaptureDeviceKind::Screen);
            let mut session = manager.create_session(&config)?;
            let metadata = Arc::clone(&self.metadata);
            let callback = Arc::new(Mutex::new(frame_callback));
            let capture_callback: FrameCallback = Arc::new(move |frame| {
                let CaptureFrame::Video {
                    width,
                    height,
                    format,
                    data,
                    timestamp_ms,
                } = frame
                else {
                    return;
                };
                if format != PixelFormat::Rgba32 {
                    return;
                }

                metadata.lock().resolution =
                    size(DevicePixels::from(width), DevicePixels::from(height));
                (callback.lock())(ScreenCaptureFrame(PlatformScreenCaptureFrame {
                    width,
                    height,
                    rgba: data,
                    timestamp_ms,
                }));
            });
            session.start(config, capture_callback)?;
            Ok(Box::new(BrowserScreenCaptureStream {
                session,
                metadata: Arc::clone(&self.metadata),
            }) as Box<dyn ScreenCaptureStream>)
        })();

        let _ = sender.send(result);
        receiver
    }
}

struct BrowserScreenCaptureStream {
    session: Box<dyn CaptureSession>,
    metadata: Arc<Mutex<SourceMetadata>>,
}

impl ScreenCaptureStream for BrowserScreenCaptureStream {
    fn metadata(&self) -> Result<SourceMetadata> {
        Ok(self.metadata.lock().clone())
    }
}

impl Drop for BrowserScreenCaptureStream {
    fn drop(&mut self) {
        let _ = self.session.stop();
    }
}

fn unknown_picker_metadata() -> SourceMetadata {
    SourceMetadata {
        id: BROWSER_DISPLAY_PICKER_ID,
        label: Some("Browser display/window picker".into()),
        is_main: None,
        // The selected surface and its resolution are privacy-protected until
        // the user chooses one. This is updated before each delivered frame.
        resolution: size(DevicePixels(0), DevicePixels(0)),
    }
}

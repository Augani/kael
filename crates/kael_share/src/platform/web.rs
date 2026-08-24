use std::{future::Future, pin::Pin, sync::Arc};

use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
use wasm_bindgen::{JsCast as _, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{File, FilePropertyBag, Navigator, ShareData};

use crate::{
    PlatformShareSupport, ReceiverCallback, ShareError, ShareFileType, ShareOperationResult,
    ShareResult, ShareSheet, platform::PlatformShareReceiver,
};

type BrowserFuture<'a> = Pin<Box<dyn Future<Output = ShareOperationResult<ShareResult>> + 'a>>;

#[derive(Clone)]
struct BrowserFile {
    name: String,
    mime_type: String,
    bytes: Arc<[u8]>,
}

#[derive(Default)]
struct BrowserSharePayload {
    title: Option<String>,
    text: Option<String>,
    url: Option<String>,
    files: Vec<BrowserFile>,
}

trait BrowserShareDriver {
    fn available(&self) -> bool;
    fn user_activation_active(&self) -> bool;
    fn can_share(&self, payload: &BrowserSharePayload) -> ShareOperationResult<bool>;
    fn share<'a>(&'a self, payload: &'a BrowserSharePayload) -> BrowserFuture<'a>;
}

struct NavigatorShareDriver;

impl BrowserShareDriver for NavigatorShareDriver {
    fn available(&self) -> bool {
        navigator().is_some_and(|navigator| has_function(navigator.as_ref(), "share"))
    }

    fn user_activation_active(&self) -> bool {
        let Some(navigator) = navigator() else {
            return false;
        };
        Reflect::get(navigator.as_ref(), &JsValue::from_str("userActivation"))
            .ok()
            .and_then(|activation| {
                Reflect::get(&activation, &JsValue::from_str("isActive"))
                    .ok()
                    .and_then(|active| active.as_bool())
            })
            .unwrap_or(false)
    }

    fn can_share(&self, payload: &BrowserSharePayload) -> ShareOperationResult<bool> {
        let navigator = navigator().ok_or(ShareError::Unavailable)?;
        if payload.files.is_empty() && !has_function(navigator.as_ref(), "canShare") {
            return Ok(true);
        }
        if !has_function(navigator.as_ref(), "canShare") {
            return Ok(false);
        }
        let data = share_data(payload)?;
        let function = Reflect::get(navigator.as_ref(), &JsValue::from_str("canShare"))
            .map_err(|_| ShareError::Unavailable)?
            .dyn_into::<Function>()
            .map_err(|_| ShareError::Unavailable)?;
        function
            .call1(navigator.as_ref(), data.as_ref())
            .map_err(map_js_error)?
            .as_bool()
            .ok_or_else(|| ShareError::Platform("navigator.canShare returned a non-boolean".into()))
    }

    fn share<'a>(&'a self, payload: &'a BrowserSharePayload) -> BrowserFuture<'a> {
        Box::pin(async move {
            let navigator = navigator().ok_or(ShareError::Unavailable)?;
            let data = share_data(payload)?;
            let function = Reflect::get(navigator.as_ref(), &JsValue::from_str("share"))
                .map_err(|_| ShareError::Unavailable)?
                .dyn_into::<Function>()
                .map_err(|_| ShareError::Unavailable)?;
            let result = function
                .call1(navigator.as_ref(), data.as_ref())
                .map_err(map_js_error)?;
            let promise = result
                .dyn_into::<Promise>()
                .map_err(|_| ShareError::Platform("navigator.share returned no Promise".into()))?;
            JsFuture::from(promise).await.map_err(map_js_error)?;
            Ok(ShareResult::Completed {
                activity_type: "web-share".into(),
            })
        })
    }
}

pub(crate) async fn show(sheet: &ShareSheet) -> ShareOperationResult<ShareResult> {
    show_with_driver(sheet, &NavigatorShareDriver).await
}

async fn show_with_driver(
    sheet: &ShareSheet,
    driver: &dyn BrowserShareDriver,
) -> ShareOperationResult<ShareResult> {
    if !sheet.excluded().is_empty() {
        return Err(ShareError::UnsupportedPayload(
            "the Web Share API cannot exclude individual destinations".into(),
        ));
    }
    let payload = BrowserSharePayload::from_sheet(sheet)?;
    if !driver.available() {
        return Err(ShareError::Unavailable);
    }
    if !driver.user_activation_active() {
        return Err(ShareError::UserActivationRequired);
    }
    if !driver.can_share(&payload)? {
        return Err(ShareError::UnsupportedPayload(
            "navigator.canShare rejected the payload or file types".into(),
        ));
    }
    driver.share(&payload).await
}

pub(crate) fn register_receiver(
    _file_types: &[ShareFileType],
    _callback: ReceiverCallback,
) -> anyhow::Result<PlatformShareReceiver> {
    Err(anyhow::anyhow!(
        "browser share-target registration belongs to the installed PWA manifest and service worker"
    ))
}

pub(crate) fn support() -> PlatformShareSupport {
    let navigator = navigator();
    let system_picker = navigator
        .as_ref()
        .is_some_and(|navigator| has_function(navigator.as_ref(), "share"));
    let memory_files = system_picker
        && navigator
            .as_ref()
            .is_some_and(|navigator| has_function(navigator.as_ref(), "canShare"));
    PlatformShareSupport {
        mail: false,
        messages: false,
        airdrop: false,
        clipboard: false,
        social: false,
        print: false,
        receiver_registration: false,
        system_picker,
        memory_files,
        requires_user_activation: true,
    }
}

impl BrowserSharePayload {
    fn from_sheet(sheet: &ShareSheet) -> ShareOperationResult<Self> {
        if sheet.items().iter().any(|item| !item.files.is_empty()) {
            return Err(ShareError::UnsupportedPayload(
                "native file paths cannot cross the browser sandbox; use ShareFile bytes".into(),
            ));
        }
        let (text, url) = sheet.browser_text_and_url();
        let mut files = Vec::new();
        for (item_index, item) in sheet.items().iter().enumerate() {
            files.extend(item.memory_files.iter().map(|file| BrowserFile {
                name: file.name().to_string(),
                mime_type: file.mime_type().to_string(),
                bytes: Arc::from(file.bytes()),
            }));
            if let Some(image) = item.image.as_ref() {
                let name = image
                    .suggested_name()
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        format!(
                            "kael-share-image-{item_index}.{}",
                            image_extension(image.mime_type())
                        )
                    });
                files.push(BrowserFile {
                    name,
                    mime_type: image.mime_type().to_string(),
                    bytes: Arc::from(image.bytes()),
                });
            }
        }
        Ok(Self {
            title: sheet.first_subject().map(str::to_string),
            text,
            url,
            files,
        })
    }
}

fn navigator() -> Option<Navigator> {
    web_sys::window().map(|window| window.navigator())
}

fn has_function(target: &JsValue, name: &str) -> bool {
    Reflect::get(target, &JsValue::from_str(name)).is_ok_and(|value| value.is_function())
}

fn share_data(payload: &BrowserSharePayload) -> ShareOperationResult<ShareData> {
    let data = ShareData::new();
    if let Some(title) = payload.title.as_deref() {
        data.set_title(title);
    }
    if let Some(text) = payload.text.as_deref() {
        data.set_text(text);
    }
    if let Some(url) = payload.url.as_deref() {
        data.set_url(url);
    }
    if !payload.files.is_empty() {
        let files = Array::new();
        for file in &payload.files {
            let bits = Array::new();
            bits.push(&Uint8Array::from(file.bytes.as_ref()));
            let options = FilePropertyBag::new();
            options.set_type(&file.mime_type);
            let file =
                File::new_with_u8_array_sequence_and_options(bits.as_ref(), &file.name, &options)
                    .map_err(map_js_error)?;
            files.push(file.as_ref());
        }
        data.set_files(files.as_ref());
    }
    Ok(data)
}

fn map_js_error(error: JsValue) -> ShareError {
    let name = Reflect::get(&error, &JsValue::from_str("name"))
        .ok()
        .and_then(|name| name.as_string())
        .unwrap_or_default();
    match name.as_str() {
        "AbortError" => ShareError::Cancelled,
        "NotAllowedError" | "SecurityError" => ShareError::PermissionDenied,
        "TypeError" | "DataError" => {
            ShareError::UnsupportedPayload("the browser rejected the Web Share payload".into())
        }
        _ => ShareError::Platform("the browser share Promise rejected".into()),
    }
}

fn image_extension(mime_type: &str) -> &'static str {
    match mime_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/tiff" => "tiff",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

    use crate::{ShareError, ShareFile, ShareResult, ShareSheet};

    use super::{BrowserFuture, BrowserShareDriver, BrowserSharePayload, show_with_driver};

    wasm_bindgen_test_configure!(run_in_browser);

    struct MockDriver {
        available: bool,
        active: bool,
        can_share: bool,
        result: Result<ShareResult, ShareError>,
        calls: Rc<Cell<usize>>,
    }

    impl BrowserShareDriver for MockDriver {
        fn available(&self) -> bool {
            self.available
        }

        fn user_activation_active(&self) -> bool {
            self.active
        }

        fn can_share(&self, _payload: &BrowserSharePayload) -> Result<bool, ShareError> {
            Ok(self.can_share)
        }

        fn share<'a>(&'a self, payload: &'a BrowserSharePayload) -> BrowserFuture<'a> {
            self.calls.set(self.calls.get() + 1);
            let result = self.result.clone();
            Box::pin(async move {
                assert_eq!(payload.files.len(), 1);
                result
            })
        }
    }

    fn driver(result: Result<ShareResult, ShareError>) -> MockDriver {
        MockDriver {
            available: true,
            active: true,
            can_share: true,
            result,
            calls: Rc::new(Cell::new(0)),
        }
    }

    fn sheet() -> ShareSheet {
        ShareSheet::builder()
            .text("portable")
            .memory_file(ShareFile::new("proof.txt", "text/plain", b"kael".to_vec()))
            .build_checked()
            .unwrap()
    }

    #[wasm_bindgen_test(async)]
    async fn injected_driver_proves_share_without_opening_os_ui() {
        let driver = driver(Ok(ShareResult::Completed {
            activity_type: "test-share".into(),
        }));
        let result = show_with_driver(&sheet(), &driver).await.unwrap();
        assert!(result.is_completed());
        assert_eq!(driver.calls.get(), 1);
    }

    #[wasm_bindgen_test(async)]
    async fn unavailable_activation_and_cancel_are_typed() {
        let mut driver = driver(Err(ShareError::Cancelled));
        driver.available = false;
        assert_eq!(
            show_with_driver(&sheet(), &driver).await.unwrap_err(),
            ShareError::Unavailable
        );

        driver.available = true;
        driver.active = false;
        assert_eq!(
            show_with_driver(&sheet(), &driver).await.unwrap_err(),
            ShareError::UserActivationRequired
        );

        driver.active = true;
        assert_eq!(
            show_with_driver(&sheet(), &driver).await.unwrap_err(),
            ShareError::Cancelled
        );
    }
}

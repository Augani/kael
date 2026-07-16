use std::sync::LazyLock;

use anyhow::{Result, anyhow};
use collections::{FxHashMap, FxHashSet};
use itertools::Itertools;
use windows::Win32::{
    Foundation::{GlobalFree, HANDLE, HGLOBAL},
    System::{
        DataExchange::{
            CloseClipboard, CountClipboardFormats, EmptyClipboard, EnumClipboardFormats,
            GetClipboardData, GetClipboardFormatNameW, IsClipboardFormatAvailable, OpenClipboard,
            RegisterClipboardFormatW, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        Ole::{CF_HDROP, CF_UNICODETEXT},
    },
    UI::Shell::{DragQueryFileW, HDROP},
};
use windows_core::PCWSTR;

use crate::{ClipboardEntry, ClipboardItem, ClipboardString, Image, ImageFormat, hash};

// https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-dragqueryfilew
const DRAGDROP_GET_FILES_COUNT: u32 = 0xFFFFFFFF;
const MAX_CLIPBOARD_DATA_BYTES: usize = 256 * 1024 * 1024;
const MAX_CLIPBOARD_FILES: u32 = 4096;
const MAX_CLIPBOARD_PATH_UNITS: usize = 32_767;
const MAX_CLIPBOARD_IMAGE_DIMENSION: u32 = 16_384;

// Clipboard formats
static CLIPBOARD_HASH_FORMAT: LazyLock<u32> =
    LazyLock::new(|| register_clipboard_format(windows::core::w!("GPUI internal text hash")));
static CLIPBOARD_METADATA_FORMAT: LazyLock<u32> =
    LazyLock::new(|| register_clipboard_format(windows::core::w!("GPUI internal metadata")));
static CLIPBOARD_SVG_FORMAT: LazyLock<u32> =
    LazyLock::new(|| register_clipboard_format(windows::core::w!("image/svg+xml")));
static CLIPBOARD_GIF_FORMAT: LazyLock<u32> =
    LazyLock::new(|| register_clipboard_format(windows::core::w!("GIF")));
static CLIPBOARD_PNG_FORMAT: LazyLock<u32> =
    LazyLock::new(|| register_clipboard_format(windows::core::w!("PNG")));
static CLIPBOARD_JPG_FORMAT: LazyLock<u32> =
    LazyLock::new(|| register_clipboard_format(windows::core::w!("JFIF")));

// Helper maps and sets
static FORMATS_MAP: LazyLock<FxHashMap<u32, ClipboardFormatType>> = LazyLock::new(|| {
    let mut formats_map = FxHashMap::default();
    formats_map.insert(CF_UNICODETEXT.0 as u32, ClipboardFormatType::Text);
    for format in [
        *CLIPBOARD_PNG_FORMAT,
        *CLIPBOARD_GIF_FORMAT,
        *CLIPBOARD_JPG_FORMAT,
        *CLIPBOARD_SVG_FORMAT,
    ] {
        if format != 0 {
            formats_map.insert(format, ClipboardFormatType::Image);
        }
    }
    formats_map.insert(CF_HDROP.0 as u32, ClipboardFormatType::Files);
    formats_map
});
static FORMATS_SET: LazyLock<FxHashSet<u32>> = LazyLock::new(|| {
    let mut formats_map = FxHashSet::default();
    formats_map.insert(CF_UNICODETEXT.0 as u32);
    for format in [
        *CLIPBOARD_PNG_FORMAT,
        *CLIPBOARD_GIF_FORMAT,
        *CLIPBOARD_JPG_FORMAT,
        *CLIPBOARD_SVG_FORMAT,
    ] {
        if format != 0 {
            formats_map.insert(format);
        }
    }
    formats_map.insert(CF_HDROP.0 as u32);
    formats_map
});
static IMAGE_FORMATS_MAP: LazyLock<FxHashMap<u32, ImageFormat>> = LazyLock::new(|| {
    let mut formats_map = FxHashMap::default();
    for (format, image_format) in [
        (*CLIPBOARD_PNG_FORMAT, ImageFormat::Png),
        (*CLIPBOARD_GIF_FORMAT, ImageFormat::Gif),
        (*CLIPBOARD_JPG_FORMAT, ImageFormat::Jpeg),
        (*CLIPBOARD_SVG_FORMAT, ImageFormat::Svg),
    ] {
        if format != 0 {
            formats_map.insert(format, image_format);
        }
    }
    formats_map
});

#[derive(Debug, Clone, Copy)]
enum ClipboardFormatType {
    Text,
    Image,
    Files,
}

pub(crate) fn write_to_clipboard(item: ClipboardItem) {
    with_clipboard(|| write_to_clipboard_inner(item));
}

pub(crate) fn clear_clipboard() {
    with_clipboard(|| -> windows_core::Result<()> {
        unsafe {
            EmptyClipboard()?;
            Ok(())
        }
    });
}

pub(crate) fn read_from_clipboard() -> Option<ClipboardItem> {
    with_clipboard(|| {
        with_best_match_format(|item_format| match format_to_type(item_format)? {
            ClipboardFormatType::Text => read_string_from_clipboard(),
            ClipboardFormatType::Image => read_image_from_clipboard(item_format),
            ClipboardFormatType::Files => read_files_from_clipboard(),
        })
    })
    .flatten()
}

pub(crate) fn with_file_names<F>(hdrop: HDROP, mut f: F)
where
    F: FnMut(String),
{
    let file_count = unsafe { DragQueryFileW(hdrop, DRAGDROP_GET_FILES_COUNT, None) };
    if file_count > MAX_CLIPBOARD_FILES {
        log::warn!(
            "clipboard file list contains {file_count} entries; reading the first {MAX_CLIPBOARD_FILES}"
        );
    }
    for file_index in 0..file_count.min(MAX_CLIPBOARD_FILES) {
        let filename_length = unsafe { DragQueryFileW(hdrop, file_index, None) } as usize;
        if filename_length == 0 || filename_length > MAX_CLIPBOARD_PATH_UNITS {
            log::error!("clipboard file path has invalid UTF-16 length {filename_length}");
            continue;
        }
        let mut buffer = vec![0u16; filename_length + 1];
        let ret = unsafe { DragQueryFileW(hdrop, file_index, Some(buffer.as_mut_slice())) };
        if ret == 0 {
            log::error!("unable to read file name of dragged file");
            continue;
        }
        match String::from_utf16(&buffer[0..filename_length]) {
            Ok(file_name) => f(file_name),
            Err(e) => {
                log::error!("dragged file name is not UTF-16: {}", e)
            }
        }
    }
}

fn with_clipboard<F, T>(f: F) -> Option<T>
where
    F: FnOnce() -> T,
{
    struct ClipboardGuard;

    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            if let Err(error) = unsafe { CloseClipboard() } {
                log::error!("failed to close clipboard: {error}");
            }
        }
    }

    match unsafe { OpenClipboard(None) } {
        Ok(()) => {
            let _guard = ClipboardGuard;
            Some(f())
        }
        Err(e) => {
            log::error!("Failed to open clipboard: {e}",);
            None
        }
    }
}

fn register_clipboard_format(format: PCWSTR) -> u32 {
    let ret = unsafe { RegisterClipboardFormatW(format) };
    if ret == 0 {
        log::error!(
            "failed to register clipboard format: {}",
            std::io::Error::last_os_error()
        );
    }
    ret
}

#[inline]
fn format_to_type(item_format: u32) -> Option<&'static ClipboardFormatType> {
    FORMATS_MAP.get(&item_format)
}

// Write all entries simultaneously so receiving applications can choose the preferred format.
fn write_to_clipboard_inner(item: ClipboardItem) -> Result<()> {
    unsafe {
        EmptyClipboard()?;
    }
    if item.entries().is_empty() {
        // Writing an empty list of entries just clears the clipboard.
        return Ok(());
    }
    for entry in item.entries() {
        match entry {
            ClipboardEntry::String(string) => {
                write_string_to_clipboard(string)?;
            }
            ClipboardEntry::Image(image) => {
                write_image_to_clipboard(image)?;
            }
        }
    }
    Ok(())
}

fn write_string_to_clipboard(item: &ClipboardString) -> Result<()> {
    let encode_wide = item.text.encode_utf16().chain(Some(0)).collect_vec();
    set_data_to_clipboard(&encode_wide, CF_UNICODETEXT.0 as u32)?;

    if let Some(metadata) = item.metadata.as_ref() {
        let hash_result = {
            let hash = ClipboardString::text_hash(&item.text);
            hash.to_ne_bytes()
        };
        let encode_wide =
            unsafe { std::slice::from_raw_parts(hash_result.as_ptr().cast::<u16>(), 4) };
        if *CLIPBOARD_HASH_FORMAT != 0 {
            set_data_to_clipboard(encode_wide, *CLIPBOARD_HASH_FORMAT)?;
        }

        let metadata_wide = metadata.encode_utf16().chain(Some(0)).collect_vec();
        if *CLIPBOARD_METADATA_FORMAT != 0 {
            set_data_to_clipboard(&metadata_wide, *CLIPBOARD_METADATA_FORMAT)?;
        }
    }
    Ok(())
}

fn set_data_to_clipboard<T>(data: &[T], format: u32) -> Result<()> {
    anyhow::ensure!(format != 0, "clipboard format is unavailable");
    anyhow::ensure!(!data.is_empty(), "clipboard data cannot be empty");
    let byte_len = std::mem::size_of_val(data);
    anyhow::ensure!(
        byte_len <= MAX_CLIPBOARD_DATA_BYTES,
        "clipboard data exceeds {MAX_CLIPBOARD_DATA_BYTES} bytes"
    );
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE, byte_len)?;
        let handle = GlobalLock(global);
        if handle.is_null() {
            let _ = GlobalFree(Some(global));
            return Err(anyhow!(
                "failed to lock allocated clipboard memory: {}",
                std::io::Error::last_os_error()
            ));
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), handle as _, data.len());
        let _ = GlobalUnlock(global);
        if let Err(error) = SetClipboardData(format, Some(HANDLE(global.0))) {
            let _ = GlobalFree(Some(global));
            return Err(error.into());
        }
    }
    Ok(())
}

// Here writing PNG to the clipboard to better support other apps. For more info, please ref to
// the PR.
fn write_image_to_clipboard(item: &Image) -> Result<()> {
    match item.format {
        ImageFormat::Svg => set_data_to_clipboard(item.bytes(), *CLIPBOARD_SVG_FORMAT)?,
        ImageFormat::Gif => {
            if *CLIPBOARD_GIF_FORMAT != 0 {
                set_data_to_clipboard(item.bytes(), *CLIPBOARD_GIF_FORMAT)?;
            }
            let png_bytes = convert_image_to_png_format(item.bytes(), ImageFormat::Gif)?;
            set_data_to_clipboard(&png_bytes, *CLIPBOARD_PNG_FORMAT)?;
        }
        ImageFormat::Png => {
            set_data_to_clipboard(item.bytes(), *CLIPBOARD_PNG_FORMAT)?;
        }
        ImageFormat::Jpeg => {
            if *CLIPBOARD_JPG_FORMAT != 0 {
                set_data_to_clipboard(item.bytes(), *CLIPBOARD_JPG_FORMAT)?;
            }
            let png_bytes = convert_image_to_png_format(item.bytes(), ImageFormat::Jpeg)?;
            set_data_to_clipboard(&png_bytes, *CLIPBOARD_PNG_FORMAT)?;
        }
        other => {
            log::warn!(
                "Clipboard unsupported image format: {:?}, convert to PNG instead.",
                item.format
            );
            let png_bytes = convert_image_to_png_format(item.bytes(), other)?;
            set_data_to_clipboard(&png_bytes, *CLIPBOARD_PNG_FORMAT)?;
        }
    }
    Ok(())
}

fn convert_image_to_png_format(bytes: &[u8], image_format: ImageFormat) -> Result<Vec<u8>> {
    anyhow::ensure!(
        bytes.len() <= MAX_CLIPBOARD_DATA_BYTES,
        "clipboard image exceeds {MAX_CLIPBOARD_DATA_BYTES} bytes"
    );
    let mut reader = image::ImageReader::with_format(
        std::io::Cursor::new(bytes),
        decoder_image_format(image_format)?,
    );
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_CLIPBOARD_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_CLIPBOARD_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_CLIPBOARD_DATA_BYTES as u64);
    reader.limits(limits);
    let image = reader.decode()?;
    let mut output_buf = Vec::new();
    image.write_to(
        &mut std::io::Cursor::new(&mut output_buf),
        image::ImageFormat::Png,
    )?;
    anyhow::ensure!(
        output_buf.len() <= MAX_CLIPBOARD_DATA_BYTES,
        "encoded clipboard image exceeds {MAX_CLIPBOARD_DATA_BYTES} bytes"
    );
    Ok(output_buf)
}

// Here, we enumerate all formats on the clipboard and find the first one that we can process.
// The reason we don't use `GetPriorityClipboardFormat` is that it sometimes returns the
// wrong format.
// For instance, when copying a JPEG image from  Microsoft Word, there may be several formats
// on the clipboard: Jpeg, Png, Svg.
// If we use `GetPriorityClipboardFormat`, it will return Svg, which is not what we want.
fn with_best_match_format<F>(f: F) -> Option<ClipboardItem>
where
    F: Fn(u32) -> Option<ClipboardEntry>,
{
    let count = unsafe { CountClipboardFormats() };
    let mut clipboard_format = 0;
    for _ in 0..count {
        clipboard_format = unsafe { EnumClipboardFormats(clipboard_format) };
        let Some(item_format) = FORMATS_SET.get(&clipboard_format) else {
            continue;
        };
        if let Some(entry) = f(*item_format) {
            return Some(ClipboardItem {
                entries: vec![entry],
            });
        }
    }
    // log the formats that we don't support yet.
    {
        clipboard_format = 0;
        for _ in 0..count {
            clipboard_format = unsafe { EnumClipboardFormats(clipboard_format) };
            let mut buffer = [0u16; 64];
            unsafe { GetClipboardFormatNameW(clipboard_format, &mut buffer) };
            let format_name = String::from_utf16_lossy(&buffer);
            log::warn!(
                "Try to paste with unsupported clipboard format: {}, {}.",
                clipboard_format,
                format_name
            );
        }
    }
    None
}

fn read_string_from_clipboard() -> Option<ClipboardEntry> {
    let text = with_clipboard_data(CF_UNICODETEXT.0 as u32, read_wide_string)??;
    let Some(hash) = read_hash_from_clipboard() else {
        return Some(ClipboardEntry::String(ClipboardString::new(text)));
    };
    let Some(metadata) = read_metadata_from_clipboard() else {
        return Some(ClipboardEntry::String(ClipboardString::new(text)));
    };
    if hash == ClipboardString::text_hash(&text) {
        Some(ClipboardEntry::String(ClipboardString {
            text,
            metadata: Some(metadata),
        }))
    } else {
        Some(ClipboardEntry::String(ClipboardString::new(text)))
    }
}

fn read_hash_from_clipboard() -> Option<u64> {
    let format = *CLIPBOARD_HASH_FORMAT;
    if format == 0 || unsafe { IsClipboardFormatAvailable(format).is_err() } {
        return None;
    }
    with_clipboard_data(format, |data_ptr, size| {
        if size < 8 {
            return None;
        }
        let hash_bytes: [u8; 8] = unsafe {
            std::slice::from_raw_parts(data_ptr.cast::<u8>(), 8)
                .try_into()
                .ok()
        }?;
        Some(u64::from_ne_bytes(hash_bytes))
    })?
}

fn read_metadata_from_clipboard() -> Option<String> {
    let format = *CLIPBOARD_METADATA_FORMAT;
    if format == 0 {
        return None;
    }
    unsafe { IsClipboardFormatAvailable(format).ok()? };
    with_clipboard_data(format, read_wide_string)?
}

fn read_image_from_clipboard(format: u32) -> Option<ClipboardEntry> {
    let image_format = format_number_to_image_format(format)?;
    read_image_for_type(format, *image_format)
}

#[inline]
fn format_number_to_image_format(format_number: u32) -> Option<&'static ImageFormat> {
    IMAGE_FORMATS_MAP.get(&format_number)
}

fn read_image_for_type(format_number: u32, format: ImageFormat) -> Option<ClipboardEntry> {
    let (bytes, id) = with_clipboard_data(format_number, |data_ptr, size| {
        let bytes = unsafe { std::slice::from_raw_parts(data_ptr as *mut u8 as _, size).to_vec() };
        let id = hash(&bytes);
        (bytes, id)
    })?;
    Some(ClipboardEntry::Image(Image { format, bytes, id }))
}

fn read_files_from_clipboard() -> Option<ClipboardEntry> {
    let text = with_clipboard_data(CF_HDROP.0 as u32, |data_ptr, _size| {
        let hdrop = HDROP(data_ptr);
        let mut filenames = Vec::new();
        with_file_names(hdrop, |file_name| {
            filenames.push(file_name);
        });
        filenames.join("\n")
    })?;
    Some(ClipboardEntry::String(ClipboardString {
        text,
        metadata: None,
    }))
}

struct ClipboardGlobalLock(HGLOBAL);

impl Drop for ClipboardGlobalLock {
    fn drop(&mut self) {
        unsafe {
            GlobalUnlock(self.0).ok();
        }
    }
}

fn with_clipboard_data<F, R>(format: u32, f: F) -> Option<R>
where
    F: FnOnce(*mut std::ffi::c_void, usize) -> R,
{
    let global = HGLOBAL(unsafe { GetClipboardData(format).ok() }?.0);
    let size = unsafe { GlobalSize(global) };
    if size == 0 || size > MAX_CLIPBOARD_DATA_BYTES {
        return None;
    }
    let data_ptr = unsafe { GlobalLock(global) };
    if data_ptr.is_null() {
        return None;
    }
    let _lock = ClipboardGlobalLock(global);
    Some(f(data_ptr, size))
}

fn read_wide_string(data_ptr: *mut std::ffi::c_void, size: usize) -> Option<String> {
    if size < std::mem::size_of::<u16>() {
        return None;
    }
    let units = unsafe {
        std::slice::from_raw_parts(data_ptr.cast::<u16>(), size / std::mem::size_of::<u16>())
    };
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    Some(String::from_utf16_lossy(&units[..end]))
}

fn decoder_image_format(value: ImageFormat) -> Result<image::ImageFormat> {
    Ok(match value {
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Jpeg => image::ImageFormat::Jpeg,
        ImageFormat::Webp => image::ImageFormat::WebP,
        ImageFormat::Gif => image::ImageFormat::Gif,
        ImageFormat::Bmp => image::ImageFormat::Bmp,
        ImageFormat::Tiff => image::ImageFormat::Tiff,
        ImageFormat::Svg => return Err(anyhow!("SVG clipboard images cannot be raster-decoded")),
    })
}

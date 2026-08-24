#![deny(unsafe_op_in_unsafe_fn)]

use crate::{
    PlatformPrintJob, PrintOrientation,
    platform::print_pdf::{PrintPageRaster, render_print_job_pages},
};
use anyhow::{Context as _, Result, anyhow, ensure};
use std::{mem, ptr};
use windows::{
    Win32::{
        Foundation::{GlobalFree, HGLOBAL, HWND},
        Graphics::{
            Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDCW, DEVMODEW, DIB_RGB_COLORS,
                DM_IN_BUFFER, DM_ORIENTATION, DM_OUT_BUFFER, DM_PAPERLENGTH, DM_PAPERSIZE,
                DM_PAPERWIDTH, DMORIENT_LANDSCAPE, DMORIENT_PORTRAIT, DeleteDC, GetDeviceCaps, HDC,
                PHYSICALHEIGHT, PHYSICALOFFSETX, PHYSICALOFFSETY, PHYSICALWIDTH, SRCCOPY,
                StretchDIBits,
            },
            Printing::{
                ClosePrinter, DocumentPropertiesW, GetDefaultPrinterW, OpenPrinterW, PRINTER_HANDLE,
            },
        },
        Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage},
        System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        UI::Controls::Dialogs::{
            CommDlgExtendedError, PD_NOPAGENUMS, PD_NOSELECTION, PD_RETURNDC,
            PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGW, PrintDlgW,
        },
    },
    core::{PCWSTR, PWSTR},
};

const POINTS_PER_INCH: f64 = 72.0;
const TENTHS_OF_MILLIMETER_PER_INCH: f64 = 254.0;

struct PrinterDc(HDC);

impl Drop for PrinterDc {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = DeleteDC(self.0);
            }
        }
    }
}

struct PrinterHandle(PRINTER_HANDLE);

impl Drop for PrinterHandle {
    fn drop(&mut self) {
        if !self.0.Value.is_null() {
            unsafe {
                let _ = ClosePrinter(self.0);
            }
        }
    }
}

struct DevModeBuffer {
    words: Vec<usize>,
    byte_len: usize,
}

impl DevModeBuffer {
    fn new(byte_len: usize) -> Result<Self> {
        ensure!(
            byte_len >= mem::size_of::<DEVMODEW>() && byte_len <= 1024 * 1024,
            "Windows printer DEVMODE length is invalid"
        );
        let word_size = mem::size_of::<usize>();
        let word_count = byte_len
            .checked_add(word_size - 1)
            .map(|bytes| bytes / word_size)
            .context("Windows printer DEVMODE allocation overflowed")?;
        let mut words = Vec::new();
        words
            .try_reserve_exact(word_count)
            .context("allocating Windows printer DEVMODE")?;
        words.resize(word_count, 0);
        Ok(Self { words, byte_len })
    }

    fn as_ptr(&self) -> *const DEVMODEW {
        self.words.as_ptr().cast()
    }

    fn as_mut_ptr(&mut self) -> *mut DEVMODEW {
        self.words.as_mut_ptr().cast()
    }
}

pub(super) fn print_job(owner: HWND, job: PlatformPrintJob, show_dialog: bool) -> Result<()> {
    let dc = if show_dialog {
        print_dialog_dc(owner, &job)?
    } else {
        silent_printer_dc(owner, &job)?
    };
    let pages = render_print_job_pages(&job)?;
    spool_rasters(dc.0, job.title.as_ref(), &pages)
}

fn default_printer_name() -> Result<Vec<u16>> {
    let mut length = 0u32;
    unsafe {
        let _ = GetDefaultPrinterW(None, &mut length);
    }
    ensure!(
        length > 1 && length <= 32_768,
        "Windows has no usable default printer"
    );
    let mut name = vec![0u16; length as usize];
    unsafe { GetDefaultPrinterW(Some(PWSTR(name.as_mut_ptr())), &mut length) }
        .ok()
        .context("reading the Windows default printer")?;
    ensure!(
        length > 1 && (length as usize) <= name.len(),
        "Windows returned an invalid default-printer name length"
    );
    name.truncate(length as usize);
    if name.last().copied() != Some(0) {
        name.push(0);
    }
    Ok(name)
}

fn configured_default_devmode(
    owner: HWND,
    job: &PlatformPrintJob,
) -> Result<(Vec<u16>, DevModeBuffer)> {
    let printer_name = default_printer_name()?;
    let mut raw_printer = PRINTER_HANDLE::default();
    unsafe { OpenPrinterW(PCWSTR(printer_name.as_ptr()), &mut raw_printer, None) }
        .context("opening the Windows default printer")?;
    let printer = PrinterHandle(raw_printer);
    let byte_len = unsafe {
        DocumentPropertiesW(
            Some(owner),
            printer.0,
            PCWSTR(printer_name.as_ptr()),
            None,
            None,
            0,
        )
    };
    ensure!(
        byte_len > 0,
        "Windows printer rejected its DEVMODE size query"
    );
    let mut devmode = DevModeBuffer::new(byte_len as usize)?;
    let initialized = unsafe {
        DocumentPropertiesW(
            Some(owner),
            printer.0,
            PCWSTR(printer_name.as_ptr()),
            Some(devmode.as_mut_ptr()),
            None,
            DM_OUT_BUFFER.0,
        )
    };
    ensure!(
        initialized == 1,
        "Windows printer failed to initialize DEVMODE"
    );

    let paper_tenths_mm = |points: f32| -> Result<i16> {
        let value = f64::from(points) * TENTHS_OF_MILLIMETER_PER_INCH / POINTS_PER_INCH;
        ensure!(
            value.is_finite() && value >= 1.0 && value <= f64::from(i16::MAX),
            "Windows print paper dimensions are outside DEVMODE limits"
        );
        Ok(value.round() as i16)
    };
    unsafe {
        let devmode_ref = &mut *devmode.as_mut_ptr();
        // Custom dmPaperWidth/dmPaperLength are ignored by some drivers when
        // the initialized DEVMODE still advertises a named dmPaperSize.
        devmode_ref.dmFields &= !DM_PAPERSIZE;
        devmode_ref.dmFields |= DM_ORIENTATION | DM_PAPERWIDTH | DM_PAPERLENGTH;
        let paper = &mut devmode_ref.Anonymous1.Anonymous1;
        paper.dmOrientation = match job.orientation {
            PrintOrientation::Portrait => DMORIENT_PORTRAIT as i16,
            PrintOrientation::Landscape => DMORIENT_LANDSCAPE as i16,
        };
        paper.dmPaperSize = 0;
        paper.dmPaperWidth = paper_tenths_mm(job.page_size.width.0)?;
        paper.dmPaperLength = paper_tenths_mm(job.page_size.height.0)?;
    }
    let validated = unsafe {
        DocumentPropertiesW(
            Some(owner),
            printer.0,
            PCWSTR(printer_name.as_ptr()),
            Some(devmode.as_mut_ptr()),
            Some(devmode.as_ptr()),
            (DM_IN_BUFFER | DM_OUT_BUFFER).0,
        )
    };
    ensure!(
        validated == 1,
        "Windows printer rejected the requested paper size or orientation"
    );
    Ok((printer_name, devmode))
}

fn silent_printer_dc(owner: HWND, job: &PlatformPrintJob) -> Result<PrinterDc> {
    let (printer_name, devmode) = configured_default_devmode(owner, job)?;
    let dc = unsafe {
        CreateDCW(
            PCWSTR::from_raw(windows::core::w!("WINSPOOL").as_ptr()),
            PCWSTR(printer_name.as_ptr()),
            PCWSTR::null(),
            Some(devmode.as_ptr()),
        )
    };
    ensure!(
        !dc.is_invalid(),
        "Windows failed to create a default printer DC"
    );
    Ok(PrinterDc(dc))
}

fn global_devmode(devmode: &DevModeBuffer) -> Result<HGLOBAL> {
    let global = unsafe { GlobalAlloc(GMEM_MOVEABLE, devmode.byte_len) }
        .context("allocating Windows print-dialog DEVMODE")?;
    let destination = unsafe { GlobalLock(global) };
    if destination.is_null() {
        unsafe {
            let _ = GlobalFree(Some(global));
        }
        return Err(anyhow!("locking Windows print-dialog DEVMODE failed"));
    }
    unsafe {
        ptr::copy_nonoverlapping(
            devmode.as_ptr().cast::<u8>(),
            destination.cast::<u8>(),
            devmode.byte_len,
        );
        let _ = GlobalUnlock(global);
    }
    Ok(global)
}

fn free_dialog_globals(dialog: &mut PRINTDLGW) {
    unsafe {
        if !dialog.hDevMode.is_invalid() {
            let _ = GlobalFree(Some(mem::take(&mut dialog.hDevMode)));
        }
        if !dialog.hDevNames.is_invalid() {
            let _ = GlobalFree(Some(mem::take(&mut dialog.hDevNames)));
        }
    }
}

fn print_dialog_dc(owner: HWND, job: &PlatformPrintJob) -> Result<PrinterDc> {
    let (_, devmode) = configured_default_devmode(owner, job)?;
    let mut dialog = PRINTDLGW {
        lStructSize: mem::size_of::<PRINTDLGW>() as u32,
        hwndOwner: owner,
        hDevMode: global_devmode(&devmode)?,
        Flags: PD_RETURNDC | PD_NOPAGENUMS | PD_NOSELECTION | PD_USEDEVMODECOPIESANDCOLLATE,
        nMinPage: 1,
        nMaxPage: u16::try_from(job.pages.len()).unwrap_or(u16::MAX),
        nFromPage: 1,
        nToPage: u16::try_from(job.pages.len()).unwrap_or(u16::MAX),
        nCopies: 1,
        ..Default::default()
    };
    let accepted = unsafe { PrintDlgW(&mut dialog) }.as_bool();
    if !accepted {
        let code = unsafe { CommDlgExtendedError() };
        free_dialog_globals(&mut dialog);
        if code.0 == 0 {
            return Err(anyhow!("Windows print dialog was cancelled"));
        }
        return Err(anyhow!(
            "Windows print dialog failed with common-dialog error {:#x}",
            code.0
        ));
    }
    let dc = mem::take(&mut dialog.hDC);
    free_dialog_globals(&mut dialog);
    ensure!(
        !dc.is_invalid(),
        "Windows print dialog returned no printer DC"
    );
    Ok(PrinterDc(dc))
}

fn spool_rasters(dc: HDC, title: &str, pages: &[PrintPageRaster]) -> Result<()> {
    ensure!(!pages.is_empty(), "Windows print job has no rendered pages");
    let mut title_wide: Vec<u16> = title.encode_utf16().collect();
    ensure!(
        !title_wide.contains(&0),
        "Windows print job title contains an embedded NUL"
    );
    title_wide.push(0);
    let document_info = DOCINFOW {
        cbSize: mem::size_of::<DOCINFOW>() as i32,
        lpszDocName: PCWSTR(title_wide.as_ptr()),
        ..Default::default()
    };
    let started = unsafe { StartDocW(dc, &document_info) };
    ensure!(started > 0, "Windows printer rejected StartDoc");

    let result = (|| -> Result<()> {
        for (page_index, page) in pages.iter().enumerate() {
            ensure!(
                unsafe { StartPage(dc) } > 0,
                "Windows printer rejected StartPage for page {}",
                page_index + 1
            );
            draw_page_raster(dc, page)
                .with_context(|| format!("drawing Windows print page {}", page_index + 1))?;
            ensure!(
                unsafe { EndPage(dc) } > 0,
                "Windows printer rejected EndPage for page {}",
                page_index + 1
            );
        }
        ensure!(unsafe { EndDoc(dc) } > 0, "Windows printer rejected EndDoc");
        Ok(())
    })();
    if result.is_err() {
        unsafe {
            let _ = AbortDoc(dc);
        }
    }
    result
}

fn draw_page_raster(dc: HDC, page: &PrintPageRaster) -> Result<()> {
    let expected = usize::try_from(page.width)
        .ok()
        .and_then(|width| width.checked_mul(page.height as usize))
        .and_then(|pixels| pixels.checked_mul(4))
        .context("Windows print raster byte count overflowed")?;
    ensure!(
        page.bgra.len() == expected,
        "Windows print raster byte length is invalid"
    );
    let physical_width = unsafe { GetDeviceCaps(Some(dc), PHYSICALWIDTH) };
    let physical_height = unsafe { GetDeviceCaps(Some(dc), PHYSICALHEIGHT) };
    let offset_x = unsafe { GetDeviceCaps(Some(dc), PHYSICALOFFSETX) };
    let offset_y = unsafe { GetDeviceCaps(Some(dc), PHYSICALOFFSETY) };
    ensure!(
        physical_width > 0 && physical_height > 0 && offset_x >= 0 && offset_y >= 0,
        "Windows printer returned invalid physical-page metrics"
    );
    let bitmap = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: i32::try_from(page.width).context("Windows print raster width exceeds i32")?,
            // Negative height defines the supplied BGRA rows as top-down.
            biHeight: -i32::try_from(page.height)
                .context("Windows print raster height exceeds i32")?,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: u32::try_from(page.bgra.len()).unwrap_or(0),
            ..Default::default()
        },
        ..Default::default()
    };
    let lines = unsafe {
        StretchDIBits(
            dc,
            -offset_x,
            -offset_y,
            physical_width,
            physical_height,
            0,
            0,
            i32::try_from(page.width).context("Windows print source width exceeds i32")?,
            i32::try_from(page.height).context("Windows print source height exceeds i32")?,
            Some(page.bgra.as_ptr().cast()),
            &bitmap,
            DIB_RGB_COLORS,
            SRCCOPY,
        )
    };
    ensure!(lines > 0, "Windows printer rejected the page raster");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devmode_buffer_is_aligned_and_bounded() {
        let mut buffer = DevModeBuffer::new(mem::size_of::<DEVMODEW>() + 37).unwrap();
        assert_eq!(
            (buffer.as_mut_ptr() as usize) % mem::align_of::<DEVMODEW>(),
            0
        );
        assert!(DevModeBuffer::new(mem::size_of::<DEVMODEW>() - 1).is_err());
        assert!(DevModeBuffer::new(2 * 1024 * 1024).is_err());
    }
}

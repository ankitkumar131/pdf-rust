//! GDI printing for Windows.
//!
//! After the standard print dialog, every page is rendered by the PDF worker at
//! the printer's resolution and blitted into the printer device context with
//! `StretchDIBits`. Nothing is spooled through an external application, so the
//! reader can safely be the default PDF handler.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use core::ffi::c_void;
use crate::ui::Bitmap;

type Hwnd = *mut c_void;
type Hdc = *mut c_void;
type Hinstance = *mut c_void;
type Hglobal = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct PrintDialogW {
    struct_size: u32,
    owner: Hwnd,
    dev_mode: Hglobal,
    dev_names: Hglobal,
    hdc: Hdc,
    flags: u32,
    from_page: u16,
    to_page: u16,
    min_page: u16,
    max_page: u16,
    copies: u16,
    instance: Hinstance,
    cust_data: isize,
    print_hook: *mut c_void,
    setup_hook: *mut c_void,
    print_template: *const u16,
    setup_template: *const u16,
    print_template_handle: Hglobal,
    setup_template_handle: Hglobal,
}

#[repr(C)]
#[derive(Default)]
struct DocInfoW {
    cb_size: i32,
    doc_name: *const u16,
    output: *const u16,
    datatype: *const u16,
    fw_type: u32,
}

#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_pels: i32,
    y_pels: i32,
    clr_used: u32,
    clr_important: u32,
}

#[repr(C)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colors: [u32; 3],
}

const PD_ALLPAGES: u32 = 0x0000_0000;
const PD_RETURNDC: u32 = 0x0000_0100;
const PD_NOSELECTION: u32 = 0x0000_0004;
const PD_NOPAGENUMS: u32 = 0x0000_0008;
const PD_USEDEVMODECOPIESANDCOLLATE: u32 = 0x0004_0000;
const LOGPIXELSX: i32 = 88;
const HORZRES: i32 = 8;
const DIB_RGB_COLORS: u32 = 0;
const SRCCOPY: u32 = 0x00CC_0020;

#[link(name = "comdlg32")]
extern "system" {
    fn PrintDlgW(dialog: *mut PrintDialogW) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn StartDocW(hdc: Hdc, info: *const DocInfoW) -> i32;
    fn EndDoc(hdc: Hdc) -> i32;
    fn StartPage(hdc: Hdc) -> i32;
    fn EndPage(hdc: Hdc) -> i32;
    fn AbortDoc(hdc: Hdc) -> i32;
    fn DeleteDC(hdc: Hdc) -> i32;
    fn GetDeviceCaps(hdc: Hdc, index: i32) -> i32;
    fn StretchDIBits(
        hdc: Hdc,
        x_dest: i32,
        y_dest: i32,
        w_dest: i32,
        h_dest: i32,
        x_src: i32,
        y_src: i32,
        w_src: i32,
        h_src: i32,
        bits: *const u8,
        info: *const BitmapInfo,
        usage: u32,
        rop: u32,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalFree(mem: Hglobal) -> Hglobal;
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A print job writing pages to the printer device context.
pub struct Job {
    hdc: Hdc,
    dev_mode: Hglobal,
    dev_names: Hglobal,
    scale: f32,
    page_width: i32,
    started: bool,
    pages: u32,
}

impl Job {
    /// Shows the print dialog and opens a print job.
    pub fn start(_path: &std::path::Path, title: &str) -> Result<Job, String> {
        let mut dialog = PrintDialogW {
            struct_size: std::mem::size_of::<PrintDialogW>() as u32,
            flags: PD_ALLPAGES
                | PD_RETURNDC
                | PD_NOSELECTION
                | PD_NOPAGENUMS
                | PD_USEDEVMODECOPIESANDCOLLATE,
            ..Default::default()
        };
        // SAFETY: standard print dialog call; the dialog allocates DEVNAMES/DEVMODE.
        let ok = unsafe { PrintDlgW(&mut dialog) };
        if ok == 0 {
            return Err("printing was cancelled".to_string());
        }
        if dialog.hdc.is_null() {
            if !dialog.dev_mode.is_null() {
                unsafe {
                    GlobalFree(dialog.dev_mode);
                }
            }
            if !dialog.dev_names.is_null() {
                unsafe {
                    GlobalFree(dialog.dev_names);
                }
            }
            return Err("the printer did not provide a device context".to_string());
        }
        // SAFETY: device caps on a printer DC.
        let (dpi, page_width) = unsafe {
            (
                GetDeviceCaps(dialog.hdc, LOGPIXELSX).max(72),
                GetDeviceCaps(dialog.hdc, HORZRES).max(1),
            )
        };
        let title_wide = wide(title);
        let info = DocInfoW {
            cb_size: std::mem::size_of::<DocInfoW>() as i32,
            doc_name: title_wide.as_ptr(),
            ..Default::default()
        };
        // SAFETY: the DOCINFOW is valid for the call.
        let started = unsafe { StartDocW(dialog.hdc, &info) } > 0;
        if !started {
            unsafe {
                DeleteDC(dialog.hdc);
            }
            return Err("StartDocW failed".to_string());
        }
        Ok(Job {
            hdc: dialog.hdc,
            dev_mode: dialog.dev_mode,
            dev_names: dialog.dev_names,
            scale: dpi as f32 / 72.0,
            page_width,
            started: true,
            pages: 0,
        })
    }

    pub fn needs_pages(&self) -> bool {
        true
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Width of the printable area in device pixels (used to cap the render size).
    pub fn printable_width(&self) -> i32 {
        self.page_width
    }

    pub fn page(&mut self, bitmap: &Bitmap) -> Result<(), String> {
        if bitmap.width == 0 || bitmap.height == 0 {
            return Ok(());
        }
        let info = BitmapInfo {
            header: BitmapInfoHeader {
                size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                width: bitmap.width as i32,
                height: -(bitmap.height as i32),
                planes: 1,
                bit_count: 32,
                compression: 0,
                size_image: (bitmap.width * bitmap.height * 4) as u32,
                x_pels: 0,
                y_pels: 0,
                clr_used: 0,
                clr_important: 0,
            },
            colors: [0; 3],
        };
        // SAFETY: printer DC is open and the bitmap header describes the pixels.
        unsafe {
            if StartPage(self.hdc) <= 0 {
                return Err("StartPage failed".to_string());
            }
            let drawn = StretchDIBits(
                self.hdc,
                0,
                0,
                bitmap.width as i32,
                bitmap.height as i32,
                0,
                0,
                bitmap.width as i32,
                bitmap.height as i32,
                bitmap.px.as_ptr() as *const u8,
                &info,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            if EndPage(self.hdc) <= 0 || drawn <= 0 {
                return Err("drawing the page failed".to_string());
            }
        }
        self.pages += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Result<String, String> {
        // SAFETY: completing the device context opened by start().
        let pages = self.pages;
        let result = unsafe {
            let ok = EndDoc(self.hdc);
            DeleteDC(self.hdc);
            if !self.dev_mode.is_null() {
                GlobalFree(self.dev_mode);
            }
            if !self.dev_names.is_null() {
                GlobalFree(self.dev_names);
            }
            self.started = false;
            ok
        };
        if result <= 0 {
            return Err("EndDoc failed".to_string());
        }
        Ok(format!(
            "Sent {} page(s) to the printer",
            pages.max(1)
        ))
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if self.started {
            // SAFETY: abort an unfinished job so the spooler does not keep it.
            unsafe {
                AbortDoc(self.hdc);
                DeleteDC(self.hdc);
                if !self.dev_mode.is_null() {
                    GlobalFree(self.dev_mode);
                    self.dev_mode = std::ptr::null_mut();
                }
                if !self.dev_names.is_null() {
                    GlobalFree(self.dev_names);
                    self.dev_names = std::ptr::null_mut();
                }
                self.started = false;
            }
        }
    }
}

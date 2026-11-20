//! Win32 shell (Windows).
//!
//! The executable links against `user32`, `gdi32`, `shell32` and `comdlg32`
//! directly — they are part of every supported Windows version — and uses only
//! documented APIs.
//!
//! The frame is blitted with `StretchDIBits` from the same `0xAARRGGBB` buffer
//! the X11 shell feeds to `XPutImage`, so both platforms render identical pixels.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use core::ffi::c_void;

use crate::app::App;
use crate::geom::{Point, Size};
use crate::ui::{Bitmap, Font, Input, Key, KeyEvent};

pub mod printing;

type Hwnd = *mut c_void;
type Hinstance = *mut c_void;
type Hicon = *mut c_void;
type Hcursor = *mut c_void;
type Hbrush = *mut c_void;
type Hdc = *mut c_void;
type Hmenu = *mut c_void;
type Lparam = isize;
type Wparam = usize;
type Lresult = isize;
type Bool = i32;

#[repr(C)]
#[derive(Clone, Copy)]
struct Point32 {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect32 {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct Msg {
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    time: u32,
    pt: Point32,
    private: u32,
}

#[repr(C)]
struct WndClassExW {
    cb_size: u32,
    style: u32,
    wnd_proc: extern "system" fn(Hwnd, u32, Wparam, Lparam) -> Lresult,
    cls_extra: i32,
    wnd_extra: i32,
    instance: Hinstance,
    icon: Hicon,
    cursor: Hcursor,
    background: Hbrush,
    menu_name: *const u16,
    class_name: *const u16,
    icon_small: Hicon,
}

#[repr(C)]
struct PaintStruct {
    hdc: Hdc,
    erase: Bool,
    paint: Rect32,
    restore: Bool,
    inc_update: Bool,
    reserved: [u8; 32],
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

#[repr(C)]
struct MinMaxInfo {
    reserved: Point32,
    max_size: Point32,
    max_position: Point32,
    min_track_size: Point32,
    max_track_size: Point32,
}

#[repr(C)]
struct TrackMouseEventData {
    size: u32,
    flags: u32,
    hwnd_track: Hwnd,
    hover_time: u32,
}

const WM_DESTROY: u32 = 0x0002;
const WM_SIZE: u32 = 0x0005;
const WM_PAINT: u32 = 0x000F;
const WM_CLOSE: u32 = 0x0010;
const WM_QUIT: u32 = 0x0012;
const WM_KEYDOWN: u32 = 0x0100;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_CHAR: u32 = 0x0102;
const WM_TIMER: u32 = 0x0113;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const WM_MOUSEWHEEL: u32 = 0x020A;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_DROPFILES: u32 = 0x0233;
const WM_GETMINMAXINFO: u32 = 0x0024;
const WM_SETTINGCHANGE: u32 = 0x001A;
const WM_DPICHANGED: u32 = 0x02E0;

const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
const CW_USEDEFAULT: i32 = 0x8000_0000u32 as i32;
const SW_SHOW: i32 = 5;
const PM_REMOVE: u32 = 0x0001;
const CS_DBLCLKS: u32 = 0x0008;
const IDC_ARROW: *const u16 = 32512 as *const u16;
const COLOR_WINDOW: usize = 5;
const TME_LEAVE: u32 = 0x0000_0002;
const DIB_RGB_COLORS: u32 = 0;
const SRCCOPY: u32 = 0x00CC_0020;
const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12;
const GWLP_USERDATA: i32 = -21;

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Hinstance;
    fn GetLastError() -> u32;
}

#[link(name = "user32")]
extern "system" {
    fn RegisterClassExW(class: *const WndClassExW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: Hmenu,
        instance: Hinstance,
        param: *mut c_void,
    ) -> Hwnd;
    fn DefWindowProcW(hwnd: Hwnd, msg: u32, wparam: Wparam, lparam: Lparam) -> Lresult;
    fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
    fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> Bool;
    fn ShowWindow(hwnd: Hwnd, cmd: i32) -> Bool;
    fn UpdateWindow(hwnd: Hwnd) -> Bool;
    fn PeekMessageW(msg: *mut Msg, hwnd: Hwnd, min: u32, max: u32, remove: u32) -> Bool;
    fn TranslateMessage(msg: *const Msg) -> Bool;
    fn DispatchMessageW(msg: *const Msg) -> Lresult;
    fn PostQuitMessage(code: i32);
    fn DestroyWindow(hwnd: Hwnd) -> Bool;
    fn InvalidateRect(hwnd: Hwnd, rect: *const Rect32, erase: Bool) -> Bool;
    fn BeginPaint(hwnd: Hwnd, paint: *mut PaintStruct) -> Hdc;
    fn EndPaint(hwnd: Hwnd, paint: *const PaintStruct) -> Bool;
    fn LoadCursorW(instance: Hinstance, name: *const u16) -> Hcursor;
    fn GetKeyState(key: i32) -> i16;
    fn SetTimer(hwnd: Hwnd, id: usize, elapse: u32, proc: *mut c_void) -> usize;
    fn KillTimer(hwnd: Hwnd, id: usize) -> Bool;
    fn AdjustWindowRectEx(rect: *mut Rect32, style: u32, menu: Bool, ex_style: u32) -> Bool;
    fn SetProcessDPIAware() -> Bool;
    fn TrackMouseEvent(data: *mut TrackMouseEventData) -> Bool;
}

#[link(name = "gdi32")]
extern "system" {
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

#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteW(
        hwnd: Hwnd,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> isize;
    fn DragAcceptFiles(hwnd: Hwnd, accept: Bool);
    fn DragQueryFileW(drop: Hdrop, index: u32, file: *mut u16, length: u32) -> u32;
    fn DragFinish(drop: Hdrop);
}

type Hdrop = *mut c_void;

/// Opens a URL with the user's default handler.
pub fn shell_execute(url: &str) -> Result<(), String> {
    let op = wide("open");
    let file = wide(url);
    // SAFETY: NUL-terminated UTF-16 strings, valid for the call.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOW,
        )
    };
    // ShellExecuteW returns a value > 32 on success.
    if result as isize > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecuteW failed with code {}", result))
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn keysym(vk: usize) -> Option<Key> {
    Some(match vk as i32 {
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x0D => Key::Enter,
        0x1B => Key::Escape,
        0x20 => Key::Char(' '),
        0x23 => Key::End,
        0x24 => Key::Home,
        0x25 => Key::Left,
        0x26 => Key::Up,
        0x27 => Key::Right,
        0x28 => Key::Down,
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x2E => Key::Delete,
        0x30..=0x39 => Key::Char((b'0' + (vk as u8 - 0x30)) as char),
        0x41..=0x5A => Key::Char((b'a' + (vk as u8 - 0x41)) as char),
        0x70..=0x7B => match vk - 0x70 {
            0 => Key::F1,
            1 => Key::F2,
            2 => Key::F3,
            3 => Key::F4,
            4 => Key::F5,
            5 => Key::F6,
            6 => Key::F7,
            7 => Key::F8,
            8 => Key::F9,
            9 => Key::F10,
            10 => Key::F11,
            _ => Key::F12,
        },
        // OEM keys that map to commands: +, -, =, 0, comma.
        0x6B => Key::Char('+'),
        0x6D => Key::Char('-'),
        0x6E => Key::Char('.'),
        0xBC => Key::Char(','),
        0xBB => Key::Char('='),
        _ => return None,
    })
}

/// Mutable state shared with the window procedure.
struct Shell {
    app: App,
    font: Font,
    bmp: Bitmap,
    input: Input,
    quit: bool,
    title: String,
}

static SHELL: std::sync::atomic::AtomicPtr<Shell> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

fn shell_ptr() -> Option<&'static mut Shell> {
    let ptr = SHELL.load(std::sync::atomic::Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        // SAFETY: the pointer is installed for the message loop's lifetime and
        // only used from the thread that owns the window.
        Some(unsafe { &mut *ptr })
    }
}

fn modifiers(input: &mut Input) {
    // SAFETY: GetKeyState is a pure query.
    unsafe {
        input.shift = GetKeyState(VK_SHIFT) < 0;
        input.ctrl = GetKeyState(VK_CONTROL) < 0;
        input.alt = GetKeyState(VK_MENU) < 0;
    }
}

extern "system" fn wnd_proc(hwnd: Hwnd, msg: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
    let Some(shell) = shell_ptr() else {
        // SAFETY: default handling for messages before the shell is installed.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    match msg {
        WM_PAINT => {
            let mut paint = PaintStruct {
                hdc: std::ptr::null_mut(),
                erase: 0,
                paint: Rect32::default(),
                restore: 0,
                inc_update: 0,
                reserved: [0; 32],
            };
            // SAFETY: BeginPaint/EndPaint bracket the drawing.
            unsafe {
                let hdc = BeginPaint(hwnd, &mut paint);
                if !hdc.is_null() {
                    present(shell, hdc);
                }
                EndPaint(hwnd, &paint);
            }
            0
        }
        WM_SIZE => {
            let width = (lparam & 0xFFFF) as i32;
            let height = ((lparam >> 16) & 0xFFFF) as i32;
            if width > 0 && height > 0 {
                shell.app.window = Size::new(width as f32, height as f32);
                shell.bmp = Bitmap::new(width as u32, height as u32);
                shell.app.redraw = true;
            }
            0
        }
        WM_GETMINMAXINFO => {
            // SAFETY: lparam points at a MINMAXINFO for the duration of the call.
            let info = unsafe { &mut *(lparam as *mut MinMaxInfo) };
            info.min_track_size = Point32 { x: 640, y: 420 };
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let repeat = (lparam & (1 << 30)) != 0;
            modifiers(&mut shell.input);
            if let Some(key) = keysym(wparam) {
                shell
                    .input
                    .keys
                    .push(KeyEvent { key, repeat });
                if shell.input.ctrl && matches!(key, Key::Char('v') | Key::Char('V')) {
                    if let Some(text) = clipboard_read() {
                        shell.input.paste = Some(text);
                    }
                }
            }
            0
        }
        WM_CHAR => {
            modifiers(&mut shell.input);
            if !shell.input.ctrl && !shell.input.alt {
                if let Some(ch) = char::from_u32(wparam as u32) {
                    if !ch.is_control() {
                        shell.input.text.push(ch);
                    }
                }
            }
            0
        }
        WM_MOUSEMOVE => {
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            shell.input.mouse = Point::new(x, y);
            modifiers(&mut shell.input);
            let mut track = TrackMouseEventData {
                size: std::mem::size_of::<TrackMouseEventData>() as u32,
                flags: TME_LEAVE,
                hwnd_track: hwnd,
                hover_time: 0,
            };
            // SAFETY: the struct is valid for the call.
            unsafe {
                TrackMouseEvent(&mut track);
            }
            0
        }
        WM_MOUSELEAVE => {
            shell.input.mouse = Point::new(-100.0, -100.0);
            0
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            shell.input.mouse = Point::new(x, y);
            shell.input.down = true;
            shell.input.pressed = true;
            shell.input.double_click = msg == WM_LBUTTONDBLCLK;
            modifiers(&mut shell.input);
            0
        }
        WM_LBUTTONUP => {
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            shell.input.mouse = Point::new(x, y);
            shell.input.down = false;
            shell.input.released = true;
            0
        }
        WM_RBUTTONDOWN => {
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            shell.input.mouse = Point::new(x, y);
            shell.input.right_pressed = true;
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam >> 16) & 0xFFFF) as i16 as f32 / 120.0;
            shell.input.wheel = delta;
            modifiers(&mut shell.input);
            0
        }
        WM_DROPFILES => {
            let drop = wparam as Hdrop;
            let mut buf = vec![0u16; 260];
            // SAFETY: wparam is an HDROP; the buffer is large enough for a path.
            unsafe {
                if DragQueryFileW(drop, 0, buf.as_mut_ptr(), buf.len() as u32) > 0 {
                    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                    let path = String::from_utf16_lossy(&buf[..len]);
                    DragFinish(drop);
                    shell.app.open_path(std::path::PathBuf::from(path), None);
                } else {
                    DragFinish(drop);
                }
            }
            0
        }
        WM_TIMER => {
            shell.app.redraw = true;
            0
        }
        WM_SETTINGCHANGE => {
            shell.app.redraw = true;
            0
        }
        WM_DPICHANGED => 0,
        WM_CLOSE => {
            shell.app.save_all();
            shell.quit = true;
            // SAFETY: destroys the window this shell created.
            unsafe {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            // SAFETY: posts the quit that ends the message loop.
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => {
            // SAFETY: default processing for everything else.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}

fn present(shell: &mut Shell, hdc: Hdc) {
    let bmp = &shell.bmp;
    if bmp.width == 0 || bmp.height == 0 {
        return;
    }
    // Top-down 32bpp DIB whose byte layout matches 0xAARRGGBB little endian.
    let info = BitmapInfo {
        header: BitmapInfoHeader {
            size: std::mem::size_of::<BitmapInfoHeader>() as u32,
            width: bmp.width as i32,
            height: -(bmp.height as i32),
            planes: 1,
            bit_count: 32,
            compression: 0,
            size_image: (bmp.width * bmp.height * 4) as u32,
            x_pels: 0,
            y_pels: 0,
            clr_used: 0,
            clr_important: 0,
        },
        colors: [0; 3],
    };
    // SAFETY: the pixel buffer and header describe the same bitmap.
    unsafe {
        StretchDIBits(
            hdc,
            0,
            0,
            bmp.width as i32,
            bmp.height as i32,
            0,
            0,
            bmp.width as i32,
            bmp.height as i32,
            bmp.px.as_ptr() as *const u8,
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}

/// Reads text from the Windows clipboard.
fn clipboard_read() -> Option<String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalLock(mem: *mut c_void) -> *mut u16;
        fn GlobalUnlock(mem: *mut c_void) -> Bool;
    }
    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(hwnd: Hwnd) -> Bool;
        fn CloseClipboard() -> Bool;
        fn GetClipboardData(format: u32) -> *mut c_void;
        fn IsClipboardFormatAvailable(format: u32) -> Bool;
    }
    const CF_UNICODETEXT: u32 = 13;
    // SAFETY: standard clipboard sequence; the handle is closed on every path.
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return None;
        }
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let handle = GetClipboardData(CF_UNICODETEXT);
        let mut out = None;
        if !handle.is_null() {
            let ptr = GlobalLock(handle);
            if !ptr.is_null() {
                let mut len = 0usize;
                while *ptr.add(len) != 0 {
                    len += 1;
                    if len > 1 << 22 {
                        break;
                    }
                }
                let slice = std::slice::from_raw_parts(ptr, len);
                out = Some(String::from_utf16_lossy(slice));
                GlobalUnlock(handle);
            }
        }
        CloseClipboard();
        out
    }
}

/// Writes text to the Windows clipboard.
fn clipboard_write(text: &str) -> Result<(), String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
        fn GlobalLock(mem: *mut c_void) -> *mut u16;
        fn GlobalUnlock(mem: *mut c_void) -> Bool;
    }
    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(hwnd: Hwnd) -> Bool;
        fn CloseClipboard() -> Bool;
        fn EmptyClipboard() -> Bool;
        fn SetClipboardData(format: u32, mem: *mut c_void) -> *mut c_void;
    }
    const CF_UNICODETEXT: u32 = 13;
    const GMEM_MOVEABLE: u32 = 0x0002;
    let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = units.len() * 2;
    // SAFETY: standard clipboard sequence with a moveable global block.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err("OpenClipboard failed".to_string());
        }
        EmptyClipboard();
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if mem.is_null() {
            CloseClipboard();
            return Err("GlobalAlloc failed".to_string());
        }
        let ptr = GlobalLock(mem);
        if ptr.is_null() {
            CloseClipboard();
            return Err("GlobalLock failed".to_string());
        }
        std::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
        GlobalUnlock(mem);
        if SetClipboardData(CF_UNICODETEXT, mem).is_null() {
            CloseClipboard();
            return Err("SetClipboardData failed".to_string());
        }
        CloseClipboard();
    }
    Ok(())
}

/// Runs the reader on Windows until the user quits.
pub fn run(app: App, font: Font) -> Result<(), String> {
    // SAFETY: DPI awareness is process-wide and optional.
    unsafe {
        SetProcessDPIAware();
    }
    let width = app.window.w.max(640.0) as i32;
    let height = app.window.h.max(420.0) as i32;
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let class_name = wide("PdfRustWindowClass");
    let class = WndClassExW {
        cb_size: std::mem::size_of::<WndClassExW>() as u32,
        style: CS_DBLCLKS,
        wnd_proc,
        cls_extra: 0,
        wnd_extra: 0,
        instance,
        icon: std::ptr::null_mut(),
        // SAFETY: IDC_ARROW is a predefined cursor resource.
        cursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) },
        background: COLOR_WINDOW as Hbrush,
        menu_name: std::ptr::null(),
        class_name: class_name.as_ptr(),
        icon_small: std::ptr::null_mut(),
    };
    // SAFETY: the class structure is valid for the call.
    let atom = unsafe { RegisterClassExW(&class) };
    if atom == 0 {
        return Err(format!(
            "RegisterClassExW failed (error {})",
            unsafe { GetLastError() }
        ));
    }
    let mut rect = Rect32 {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    // SAFETY: adjusts the outer size so the client area matches the request.
    unsafe {
        AdjustWindowRectEx(&mut rect, WS_OVERLAPPEDWINDOW, 0, 0);
    }
    let title = wide(&crate::platform::window_title(None));
    // SAFETY: standard CreateWindowExW call with a registered class.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            rect.right - rect.left,
            rect.bottom - rect.top,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut(),
        )
    };
    if hwnd.is_null() {
        return Err(format!(
            "CreateWindowExW failed (error {})",
            unsafe { GetLastError() }
        ));
    }
    let mut shell = Box::new(Shell {
        app,
        font,
        bmp: Bitmap::new(width as u32, height as u32),
        input: Input::default(),
        quit: false,
        title: String::new(),
    });
    SHELL.store(&mut *shell as *mut Shell, std::sync::atomic::Ordering::Release);
    // SAFETY: show the window and start the 16 ms animation timer.
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        SetTimer(hwnd, 1, 16, std::ptr::null_mut());
        DragAcceptFiles(hwnd, 1);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut *shell as *mut Shell as isize);
    }

    let mut last_frame = std::time::Instant::now();
    let mut msg: Msg = unsafe { std::mem::zeroed() };
    loop {
        let mut had_message = false;
        // SAFETY: standard message pump; DispatchMessageW drives wnd_proc.
        unsafe {
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                had_message = true;
                if msg.message == WM_QUIT {
                    shell.quit = true;
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if shell.quit {
            break;
        }
        let now = std::time::Instant::now();
        shell.app.tick(shell.app.now());
        let animate = shell.app.wants_frames();
        let idle = if animate {
            std::time::Duration::from_millis(16)
        } else {
            std::time::Duration::from_millis(120)
        };
        if had_message || shell.app.redraw || now.duration_since(last_frame) > idle {
            let bmp = &mut shell.bmp;
            let font = &shell.font;
            let input = shell.input.clone();
            shell.app.frame(&input, bmp, font);
            shell.input.clear_transient();
            let title = crate::platform::window_title(shell.app.active_doc().map(|d| d.title.as_str()));
            if title != shell.title {
                shell.title = title.clone();
                let wide_title = wide(&title);
                // SAFETY: window handle is live; string is NUL-terminated.
                unsafe {
                    SetWindowTextW(hwnd, wide_title.as_ptr());
                }
            }
            if let Some(text) = shell.app.clipboard_out.take() {
                let _ = clipboard_write(&text);
            }
            // SAFETY: repaint through WM_PAINT so the DIB blit happens once.
            unsafe {
                InvalidateRect(hwnd, std::ptr::null(), 0);
                UpdateWindow(hwnd);
            }
            last_frame = std::time::Instant::now();
        } else {
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        // Full-screen toggling is handled by the window manager on X11; here the
        // window is resized to the monitor and the style stripped on request.
        if shell.app.quit {
            shell.quit = true;
        }
    }
    shell.app.save_all();
    // SAFETY: teardown of the timer and the shell pointer.
    unsafe {
        KillTimer(hwnd, 1);
        if !hwnd.is_null() {
            DestroyWindow(hwnd);
        }
    }
    SHELL.store(std::ptr::null_mut(), std::sync::atomic::Ordering::Release);
    drop(shell);
    Ok(())
}

/// Extracts the first document path from an "open with" invocation.
pub fn document_from_args() -> Option<std::path::PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy().into_owned();
        if text.starts_with('-') {
            continue;
        }
        let path = std::path::PathBuf::from(text);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_keys_map_to_command_keys() {
        assert_eq!(keysym(0x25), Some(Key::Left));
        assert_eq!(keysym(0x41), Some(Key::Char('a')));
        assert_eq!(keysym(0x70), Some(Key::F1));
        assert_eq!(keysym(0x7B), Some(Key::F12));
        assert_eq!(keysym(0x01), None);
    }

    #[test]
    fn modifiers_read_from_key_state() {
        let mut input = Input::default();
        modifiers(&mut input);
        // No assertion on the values: they reflect the real keyboard state, but
        // calling it must not crash or block.
        let _ = (input.ctrl, input.shift, input.alt);
    }
}

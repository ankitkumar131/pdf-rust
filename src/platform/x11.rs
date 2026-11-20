//! X11 shell (Linux).
//!
//! Xlib is loaded at run time, so the binary has no link-time dependency on it
//! and reports a clean error when no display is available. The shell is thin:
//! it owns the window, translates events into [`Input`] and blits the finished
//! frame with `XPutImage`.
//!
//! The CLIPBOARD selection is implemented in full (owner + requester), so copy
//! and paste work with every other X11 application.

#![allow(non_snake_case)]

use core::ffi::{c_char, c_int, c_long, c_short, c_uint, c_ulong, c_void};

use crate::app::App;
use crate::platform::dynload::Library;
use crate::ui::{Bitmap, Font, Input, Key};

type Display = c_void;
type Window = c_ulong;
type Atom = c_ulong;
type Gc = *mut c_void;
type Visual = c_void;
type XImagePtr = *mut XImage;

const KEY_PRESS: c_int = 2;
const KEY_RELEASE: c_int = 3;
const BUTTON_PRESS: c_int = 4;
const BUTTON_RELEASE: c_int = 5;
const MOTION_NOTIFY: c_int = 6;
const EXPOSE: c_int = 12;
const CONFIGURE_NOTIFY: c_int = 22;
const SELECTION_CLEAR: c_int = 29;
const SELECTION_REQUEST: c_int = 30;
const SELECTION_NOTIFY: c_int = 31;
const CLIENT_MESSAGE: c_int = 33;

const SHIFT_MASK: c_uint = 1;
const CONTROL_MASK: c_uint = 4;
const MOD1_MASK: c_uint = 8;

const EVENT_MASK: c_long = (1 << 0)      // KeyPress
    | (1 << 1)                            // KeyRelease
    | (1 << 2)                            // ButtonPress
    | (1 << 3)                            // ButtonRelease
    | (1 << 6)                            // PointerMotion
    | (1 << 15)                           // Exposure
    | (1 << 17)                           // StructureNotify
    | (1 << 21); // FocusChange

const INPUT_OUTPUT: c_uint = 1;
const PROP_MODE_REPLACE: c_int = 0;

/// Partial `XImage` definition: we only touch the leading fields. The trailing
/// function table stays opaque.
#[repr(C)]
pub struct XImage {
    pub width: c_int,
    pub height: c_int,
    pub xoffset: c_int,
    pub format: c_int,
    pub data: *mut c_char,
    pub byte_order: c_int,
    pub bitmap_unit: c_int,
    pub bitmap_bit_order: c_int,
    pub bitmap_pad: c_int,
    pub depth: c_int,
    pub bytes_per_line: c_int,
    pub bits_per_pixel: c_int,
    pub red_mask: c_ulong,
    pub green_mask: c_ulong,
    pub blue_mask: c_ulong,
    obdata: *mut c_void,
    f: [*mut c_void; 6],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XKeyEvent {
    kind: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    window: Window,
    root: Window,
    subwindow: Window,
    time: c_ulong,
    x: c_int,
    y: c_int,
    x_root: c_int,
    y_root: c_int,
    state: c_uint,
    keycode: c_uint,
    same_screen: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XButtonEvent {
    kind: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    window: Window,
    root: Window,
    subwindow: Window,
    time: c_ulong,
    x: c_int,
    y: c_int,
    x_root: c_int,
    y_root: c_int,
    state: c_uint,
    button: c_uint,
    same_screen: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XMotionEvent {
    kind: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    window: Window,
    root: Window,
    subwindow: Window,
    time: c_ulong,
    x: c_int,
    y: c_int,
    x_root: c_int,
    y_root: c_int,
    state: c_uint,
    is_hint: c_char,
    same_screen: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XConfigureEvent {
    kind: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    event: Window,
    window: Window,
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
    border_width: c_int,
    above: Window,
    override_redirect: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XClientMessageEvent {
    kind: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    window: Window,
    message_type: Atom,
    format: c_int,
    data: [c_long; 5],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XSelectionRequestEvent {
    kind: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    owner: Window,
    requestor: Window,
    selection: Atom,
    target: Atom,
    property: Atom,
    time: c_ulong,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XSelectionEvent {
    kind: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    requestor: Window,
    selection: Atom,
    target: Atom,
    property: Atom,
    time: c_ulong,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XSelectionClearEvent {
    kind: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    window: Window,
    selection: Atom,
    time: c_ulong,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XSelectionNotifyEvent {
    kind: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut Display,
    requestor: Window,
    selection: Atom,
    target: Atom,
    property: Atom,
    time: c_ulong,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XSizeHints {
    flags: c_long,
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
    min_width: c_int,
    min_height: c_int,
    max_width: c_int,
    max_height: c_int,
    width_inc: c_int,
    height_inc: c_int,
    min_aspect_x: c_int,
    min_aspect_y: c_int,
    max_aspect_x: c_int,
    max_aspect_y: c_int,
    base_width: c_int,
    base_height: c_int,
    win_gravity: c_int,
}

#[repr(C)]
struct XClassHint {
    res_name: *mut c_char,
    res_class: *mut c_char,
}

#[repr(C, align(8))]
pub struct XEvent {
    data: [u64; 24],
}

impl XEvent {
    fn kind(&self) -> c_int {
        self.data[0] as c_int
    }

    fn as_key(&self) -> &XKeyEvent {
        // SAFETY: XKeyEvent is the layout Xlib uses for key events.
        unsafe { &*(self as *const XEvent as *const XKeyEvent) }
    }

    fn as_button(&self) -> &XButtonEvent {
        unsafe { &*(self as *const XEvent as *const XButtonEvent) }
    }

    fn as_motion(&self) -> &XMotionEvent {
        unsafe { &*(self as *const XEvent as *const XMotionEvent) }
    }

    fn as_configure(&self) -> &XConfigureEvent {
        unsafe { &*(self as *const XEvent as *const XConfigureEvent) }
    }

    fn as_client(&self) -> &XClientMessageEvent {
        unsafe { &*(self as *const XEvent as *const XClientMessageEvent) }
    }

    fn as_selection_request(&self) -> &XSelectionRequestEvent {
        unsafe { &*(self as *const XEvent as *const XSelectionRequestEvent) }
    }

    fn as_selection_clear(&self) -> &XSelectionClearEvent {
        unsafe { &*(self as *const XEvent as *const XSelectionClearEvent) }
    }

    fn as_selection_notify(&self) -> &XSelectionNotifyEvent {
        unsafe { &*(self as *const XEvent as *const XSelectionNotifyEvent) }
    }
}

macro_rules! declare_xlib {
    ($( fn $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) -> $ret:ty ; )*) => {
        struct Xlib {
            $( $name: unsafe extern "C" fn($($ty),*) -> $ret, )*
        }

        impl Xlib {
            // Forwarding wrappers so call sites can use ordinary method syntax.
            $(
                #[allow(clippy::too_many_arguments)]
                unsafe fn $name(&self, $($arg: $ty),*) -> $ret {
                    (self.$name)($($arg),*)
                }
            )*
        }

        impl Xlib {
            fn load() -> Result<Xlib, String> {
                let lib = Library::open(&[
                    "libX11.so.6",
                    "libX11.so",
                    "libX11.so.6.4.0",
                ])
                .map_err(|e| format!("X11 is not available: {}", e))?;
                Ok(Xlib {
                    $( $name: {
                        // SAFETY: the symbol has the documented Xlib signature.
                        unsafe { lib.get(concat!(stringify!($name), "\0")) }
                            .ok_or_else(|| format!("Xlib is missing {}", stringify!($name)))?
                    }, )*
                })
            }
        }
    };
}

declare_xlib! {
    fn XOpenDisplay(name: *const c_char) -> *mut Display;
    fn XCloseDisplay(dpy: *mut Display) -> c_int;
    fn XDefaultScreen(dpy: *mut Display) -> c_int;
    fn XRootWindow(dpy: *mut Display, screen: c_int) -> Window;
    fn XDefaultDepth(dpy: *mut Display, screen: c_int) -> c_int;
    fn XDefaultVisual(dpy: *mut Display, screen: c_int) -> *mut Visual;
    fn XConnectionNumber(dpy: *mut Display) -> c_int;
    fn XCreateWindow(
        dpy: *mut Display,
        parent: Window,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        border: c_uint,
        depth: c_int,
        class: c_uint,
        visual: *mut Visual,
        valuemask: c_ulong,
        attributes: *mut c_void,
    ) -> Window;
    fn XDestroyWindow(dpy: *mut Display, window: Window) -> c_int;
    fn XMapWindow(dpy: *mut Display, window: Window) -> c_int;
    fn XSelectInput(dpy: *mut Display, window: Window, mask: c_long) -> c_int;
    fn XSetWindowBackground(dpy: *mut Display, window: Window, pixel: c_ulong) -> c_int;
    fn XStoreName(dpy: *mut Display, window: Window, name: *const c_char) -> c_int;
    fn XInternAtom(dpy: *mut Display, name: *const c_char, only_if_exists: c_int) -> Atom;
    fn XChangeProperty(
        dpy: *mut Display,
        window: Window,
        property: Atom,
        kind: Atom,
        format: c_int,
        mode: c_int,
        data: *const u8,
        elements: c_int,
    ) -> c_int;
    fn XSetWMProtocols(dpy: *mut Display, window: Window, protocols: *mut Atom, count: c_int) -> c_int;
    fn XSetWMNormalHints(dpy: *mut Display, window: Window, hints: *mut XSizeHints) -> ();
    fn XSetClassHint(dpy: *mut Display, window: Window, hint: *mut XClassHint) -> c_int;
    fn XResizeWindow(dpy: *mut Display, window: Window, width: c_uint, height: c_uint) -> c_int;
    fn XFlush(dpy: *mut Display) -> c_int;
    fn XSync(dpy: *mut Display, discard: c_int) -> c_int;
    fn XPending(dpy: *mut Display) -> c_int;
    fn XNextEvent(dpy: *mut Display, event: *mut XEvent) -> c_int;
    fn XCreateGC(dpy: *mut Display, window: Window, valuemask: c_ulong, values: *mut c_void) -> Gc;
    fn XFreeGC(dpy: *mut Display, gc: Gc) -> c_int;
    fn XCreateImage(
        dpy: *mut Display,
        visual: *mut Visual,
        depth: c_uint,
        format: c_int,
        offset: c_int,
        data: *mut c_char,
        width: c_uint,
        height: c_uint,
        bitmap_pad: c_int,
        bytes_per_line: c_int,
    ) -> XImagePtr;
    fn XDestroyImage(image: XImagePtr) -> c_int;
    fn XPutImage(
        dpy: *mut Display,
        window: Window,
        gc: Gc,
        image: XImagePtr,
        src_x: c_int,
        src_y: c_int,
        dest_x: c_int,
        dest_y: c_int,
        width: c_uint,
        height: c_uint,
    ) -> c_int;
    fn XLookupKeysym(event: *mut XKeyEvent, index: c_int) -> c_ulong;
    fn XLookupString(
        event: *mut XKeyEvent,
        buffer: *mut c_char,
        bytes: c_int,
        keysym: *mut c_ulong,
        status: *mut c_void,
    ) -> c_int;
    fn XSetSelectionOwner(dpy: *mut Display, selection: Atom, owner: Window, time: c_ulong) -> c_int;
    fn XGetSelectionOwner(dpy: *mut Display, selection: Atom) -> Window;
    fn XConvertSelection(
        dpy: *mut Display,
        selection: Atom,
        target: Atom,
        property: Atom,
        requestor: Window,
        time: c_ulong,
    ) -> c_int;
    fn XSendEvent(dpy: *mut Display, window: Window, propagate: c_int, mask: c_long, event: *mut XEvent) -> c_int;
    fn XGetWindowProperty(
        dpy: *mut Display,
        window: Window,
        property: Atom,
        offset: c_long,
        length: c_long,
        delete: c_int,
        req_type: Atom,
        actual_type: *mut Atom,
        actual_format: *mut c_int,
        items: *mut c_ulong,
        bytes_after: *mut c_ulong,
        data: *mut *mut u8,
    ) -> c_int;
    fn XFree(data: *mut c_void) -> c_int;
    fn XSetErrorHandler(handler: extern "C" fn(*mut Display, *mut c_void) -> c_int) -> *mut c_void;
    fn XSetInputFocus(dpy: *mut Display, window: Window, revert_to: c_int, time: c_ulong) -> c_int;
}

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: c_short,
    revents: c_short,
}

extern "C" {
    fn poll(fds: *mut PollFd, count: c_ulong, timeout: c_int) -> c_int;
}

const POLLIN: c_short = 1;

extern "C" fn ignore_x_error(_dpy: *mut Display, _event: *mut c_void) -> c_int {
    0
}

fn keysym_to_key(keysym: c_ulong) -> Option<Key> {
    // Printable ASCII maps 1:1 onto its code point.
    if (0x20..=0x7e).contains(&keysym) {
        return Some(Key::Char(char::from_u32(keysym as u32)?));
    }
    Some(match keysym {
        0xff08 => Key::Backspace,
        0xff09 => Key::Tab,
        0xff0d | 0xff8d => Key::Enter,
        0xff1b => Key::Escape,
        0xffff => Key::Delete,
        0xff50 => Key::Home,
        0xff51 => Key::Left,
        0xff52 => Key::Up,
        0xff53 => Key::Right,
        0xff54 => Key::Down,
        0xff55 => Key::PageUp,
        0xff56 => Key::PageDown,
        0xff57 => Key::End,
        0x0020 => Key::Space,
        0xffbe => Key::F1,
        0xffbf => Key::F2,
        0xffc0 => Key::F3,
        0xffc1 => Key::F4,
        0xffc2 => Key::F5,
        0xffc3 => Key::F6,
        0xffc4 => Key::F7,
        0xffc5 => Key::F8,
        0xffc6 => Key::F9,
        0xffc7 => Key::F10,
        0xffc8 => Key::F11,
        0xffc9 => Key::F12,
        _ => return None,
    })
}

struct Shell {
    x: Xlib,
    dpy: *mut Display,
    window: Window,
    gc: Gc,
    screen: c_int,
    depth: c_int,
    visual: *mut Visual,
    wm_delete: Atom,
    net_wm_name: Atom,
    utf8_string: Atom,
    clipboard: Atom,
    targets: Atom,
    sel_property: Atom,
    pixels: Vec<u32>,
    image: XImagePtr,
    clipboard_text: String,
    pending_paste: bool,
    last_title: String,
    last_click: (std::time::Instant, f32, f32),
}

impl Shell {
    fn open(width: u32, height: u32, title: &str) -> Result<Shell, String> {
        let x = Xlib::load()?;
        // SAFETY: every call below follows the Xlib contract; pointers come from
        // Xlib itself except the image buffer, which we own and outlive.
        unsafe {
            x.XSetErrorHandler(ignore_x_error);
            let dpy = x.XOpenDisplay(std::ptr::null());
            if dpy.is_null() {
                return Err(
                    "cannot open an X11 display (is DISPLAY set, or is a Wayland session running \
                     without XWayland?)"
                        .to_string(),
                );
            }
            let screen = x.XDefaultScreen(dpy);
            let root = x.XRootWindow(dpy, screen);
            let depth = x.XDefaultDepth(dpy, screen);
            let visual = x.XDefaultVisual(dpy, screen);
            let window = x.XCreateWindow(
                dpy,
                root,
                80,
                60,
                width,
                height,
                0,
                depth,
                INPUT_OUTPUT,
                visual,
                0,
                std::ptr::null_mut(),
            );
            if window == 0 {
                x.XCloseDisplay(dpy);
                return Err("XCreateWindow failed".to_string());
            }
            x.XSelectInput(dpy, window, EVENT_MASK);
            x.XSetWindowBackground(dpy, window, 0x0020_2020);
            let wm_delete = atom(&x, dpy, "WM_DELETE_WINDOW");
            let net_wm_name = atom(&x, dpy, "_NET_WM_NAME");
            let utf8_string = atom(&x, dpy, "UTF8_STRING");
            let clipboard = atom(&x, dpy, "CLIPBOARD");
            let targets = atom(&x, dpy, "TARGETS");
            let sel_property = atom(&x, dpy, "PDFRUST_SELECTION");
            let mut protocols = [wm_delete];
            x.XSetWMProtocols(dpy, window, protocols.as_mut_ptr(), 1);
            let mut hints = XSizeHints {
                flags: 2 | 8 | 16, // USSize | PSize | PMinSize
                x: 0,
                y: 0,
                width: width as c_int,
                height: height as c_int,
                min_width: 640,
                min_height: 420,
                max_width: 0,
                max_height: 0,
                width_inc: 0,
                height_inc: 0,
                min_aspect_x: 0,
                min_aspect_y: 0,
                max_aspect_x: 0,
                max_aspect_y: 0,
                base_width: 0,
                base_height: 0,
                win_gravity: 0,
            };
            x.XSetWMNormalHints(dpy, window, &mut hints);
            let class_name = std::ffi::CString::new("pdf-rust").unwrap();
            let class_class = std::ffi::CString::new("PdfRust").unwrap();
            let mut class_hint = XClassHint {
                res_name: class_name.as_ptr() as *mut c_char,
                res_class: class_class.as_ptr() as *mut c_char,
            };
            x.XSetClassHint(dpy, window, &mut class_hint);
            let gc = x.XCreateGC(dpy, window, 0, std::ptr::null_mut());
            x.XMapWindow(dpy, window);
            x.XFlush(dpy);
            let mut shell = Shell {
                x,
                dpy,
                window,
                gc,
                screen,
                depth,
                visual,
                wm_delete,
                net_wm_name,
                utf8_string,
                clipboard,
                targets,
                sel_property,
                pixels: vec![0; (width * height) as usize],
                image: std::ptr::null_mut(),
                clipboard_text: String::new(),
                pending_paste: false,
                last_title: String::new(),
                last_click: (std::time::Instant::now(), -100.0, -100.0),
            };
            shell.create_image(width, height)?;
            shell.set_title(title);
            Ok(shell)
        }
    }

    fn create_image(&mut self, width: u32, height: u32) -> Result<(), String> {
        // SAFETY: Xlib reads the buffer during XPutImage; it lives as long as
        // the image, and the image is destroyed before the buffer.
        unsafe {
            if !self.image.is_null() {
                (*self.image).data = std::ptr::null_mut();
                self.x.XDestroyImage(self.image);
                self.image = std::ptr::null_mut();
            }
            self.pixels = vec![0; (width * height) as usize];
            let image = self.x.XCreateImage(
                self.dpy,
                self.visual,
                self.depth as c_uint,
                2, // ZPixmap
                0,
                self.pixels.as_mut_ptr() as *mut c_char,
                width,
                height,
                32,
                0,
            );
            if image.is_null() {
                return Err("XCreateImage failed".to_string());
            }
            // The server's byte order is ours on every supported target; make it
            // explicit so a big-endian server cannot misinterpret the pixels.
            (*image).byte_order = if cfg!(target_endian = "little") { 0 } else { 1 };
            (*image).bitmap_pad = 32;
            self.image = image;
            Ok(())
        }
    }

    fn set_title(&mut self, title: &str) {
        if title == self.last_title {
            return;
        }
        self.last_title = title.to_string();
        let c = match std::ffi::CString::new(title) {
            Ok(c) => c,
            Err(_) => return,
        };
        // SAFETY: valid NUL-terminated strings and atoms from this shell.
        unsafe {
            self.x.XStoreName(self.dpy, self.window, c.as_ptr());
            self.x.XChangeProperty(
                self.dpy,
                self.window,
                self.net_wm_name,
                self.utf8_string,
                8,
                PROP_MODE_REPLACE,
                c.as_ptr() as *const u8,
                title.len() as c_int,
            );
            self.x.XFlush(self.dpy);
        }
    }

    fn present(&mut self, bmp: &Bitmap) {
        if bmp.width == 0 || bmp.height == 0 {
            return;
        }
        if self.image.is_null()
            || self.pixels.len() != (bmp.width * bmp.height) as usize
        {
            if self.create_image(bmp.width, bmp.height).is_err() {
                return;
            }
        }
        let len = self.pixels.len();
        self.pixels.copy_from_slice(&bmp.px[..len]);
        // SAFETY: image and pixel buffer match the bitmap dimensions.
        unsafe {
            self.x.XPutImage(
                self.dpy,
                self.window,
                self.gc,
                self.image,
                0,
                0,
                0,
                0,
                bmp.width,
                bmp.height,
            );
            self.x.XFlush(self.dpy);
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let _ = self.create_image(width, height);
    }

    fn set_clipboard(&mut self, text: &str) {
        self.clipboard_text = text.to_string();
        // SAFETY: window and atoms belong to this display.
        unsafe {
            self.x
                .XSetSelectionOwner(self.dpy, self.clipboard, self.window, 0);
            self.x.XFlush(self.dpy);
        }
    }

    fn request_paste(&mut self) {
        // SAFETY: selection atoms belong to this display.
        unsafe {
            self.x.XConvertSelection(
                self.dpy,
                self.clipboard,
                self.utf8_string,
                self.sel_property,
                self.window,
                0,
            );
            self.x.XFlush(self.dpy);
        }
    }

    fn serve_selection(&mut self, event: &XSelectionRequestEvent) {
        // SAFETY: all handles come from the event or this shell.
        unsafe {
            let property = if event.property == 0 {
                event.target
            } else {
                event.property
            };
            let mut ok = false;
            if event.target == self.targets {
                let atoms = [self.utf8_string];
                self.x.XChangeProperty(
                    self.dpy,
                    event.requestor,
                    property,
                    4, // XA_ATOM
                    32,
                    PROP_MODE_REPLACE,
                    atoms.as_ptr() as *const u8,
                    1,
                );
                ok = true;
            } else if event.target == self.utf8_string {
                let text = self.clipboard_text.clone();
                self.x.XChangeProperty(
                    self.dpy,
                    event.requestor,
                    property,
                    self.utf8_string,
                    8,
                    PROP_MODE_REPLACE,
                    text.as_ptr(),
                    text.len() as c_int,
                );
                ok = true;
            }
            let mut reply = XEvent { data: [0; 24] };
            {
                let ev = &mut *(&mut reply as *mut XEvent as *mut XSelectionEvent);
                ev.kind = SELECTION_NOTIFY;
                ev.serial = 0;
                ev.send_event = 1;
                ev.display = self.dpy;
                ev.requestor = event.requestor;
                ev.selection = event.selection;
                ev.target = event.target;
                ev.property = if ok { property } else { 0 };
                ev.time = event.time;
            }
            self.x
                .XSendEvent(self.dpy, event.requestor, 0, 0, &mut reply);
            self.x.XFlush(self.dpy);
        }
    }

    fn read_selection(&mut self, property: Atom) {
        if property == 0 {
            return;
        }
        // SAFETY: property was set by the selection owner for this window.
        unsafe {
            let mut actual_type: Atom = 0;
            let mut format: c_int = 0;
            let mut items: c_ulong = 0;
            let mut after: c_ulong = 0;
            let mut data: *mut u8 = std::ptr::null_mut();
            let status = self.x.XGetWindowProperty(
                self.dpy,
                self.window,
                property,
                0,
                1 << 20,
                1, // delete
                0, // AnyPropertyType
                &mut actual_type,
                &mut format,
                &mut items,
                &mut after,
                &mut data,
            );
            if status == 0 && !data.is_null() && format == 8 {
                let bytes = std::slice::from_raw_parts(data, items as usize);
                self.clipboard_text = String::from_utf8_lossy(bytes).into_owned();
                self.pending_paste = true;
                self.x.XFree(data as *mut c_void);
            } else if !data.is_null() {
                self.x.XFree(data as *mut c_void);
            }
            self.x.XFlush(self.dpy);
        }
    }

    fn handle(&mut self, event: &XEvent, input: &mut Input, app: &mut App) {
        match event.kind() {
            KEY_PRESS | KEY_RELEASE => {
                let ev = *event.as_key();
                let pressed = event.kind() == KEY_PRESS;
                let keycode = ev.keycode as u32;
                let repeat = key_repeat(keycode, pressed);
                if !pressed {
                    return;
                }
                let mut keysym: c_ulong = 0;
                let mut buf = [0i8; 16];
                let mut ev_copy = ev;
                // SAFETY: the event is a key event owned by this frame.
                let n = unsafe {
                    self.x.XLookupString(
                        &mut ev_copy,
                        buf.as_mut_ptr(),
                        buf.len() as c_int,
                        &mut keysym,
                        std::ptr::null_mut(),
                    )
                };
                input.ctrl = ev.state & CONTROL_MASK != 0;
                input.shift = ev.state & SHIFT_MASK != 0;
                input.alt = ev.state & MOD1_MASK != 0;
                let key = match keysym_to_key(keysym) {
                    Some(key) => key,
                    None => return,
                };
                input.keys.push(crate::ui::KeyEvent { key, repeat });
                if n > 0 && !input.ctrl && !input.alt {
                    let text = unsafe {
                        let bytes = std::slice::from_raw_parts(buf.as_ptr() as *const u8, n as usize);
                        String::from_utf8_lossy(bytes).into_owned()
                    };
                    for ch in text.chars() {
                        if !ch.is_control() {
                            input.text.push(ch);
                        }
                    }
                }
                // Paste is a platform responsibility.
                if input.ctrl && matches!(key, Key::Char('v') | Key::Char('V')) {
                    self.request_paste();
                }
                let _ = app;
            }
            BUTTON_PRESS | BUTTON_RELEASE => {
                let ev = *event.as_button();
                let pressed = event.kind() == BUTTON_PRESS;
                let x = ev.x as f32;
                let y = ev.y as f32;
                input.mouse = crate::geom::Point::new(x, y);
                match ev.button {
                    1 => {
                        input.down = pressed;
                        if pressed {
                            input.pressed = true;
                            let now = std::time::Instant::now();
                            let (last_time, lx, ly) = self.last_click;
                            if now.duration_since(last_time).as_secs_f32() < 0.4
                                && (lx - x).abs() < 6.0
                                && (ly - y).abs() < 6.0
                            {
                                input.double_click = true;
                            }
                            self.last_click = (now, x, y);
                        } else {
                            input.released = true;
                        }
                    }
                    3 => {
                        if pressed {
                            input.right_pressed = true;
                        }
                    }
                    4 | 5 => {
                        if pressed {
                            input.wheel = if ev.button == 4 { 1.0 } else { -1.0 };
                        }
                    }
                    _ => {}
                }
                input.shift = ev.state & SHIFT_MASK != 0;
                input.ctrl = ev.state & CONTROL_MASK != 0;
                input.alt = ev.state & MOD1_MASK != 0;
            }
            MOTION_NOTIFY => {
                let ev = *event.as_motion();
                input.mouse = crate::geom::Point::new(ev.x as f32, ev.y as f32);
                input.shift = ev.state & SHIFT_MASK != 0;
                input.ctrl = ev.state & CONTROL_MASK != 0;
                input.alt = ev.state & MOD1_MASK != 0;
            }
            CONFIGURE_NOTIFY => {
                let ev = *event.as_configure();
                if ev.width > 0 && ev.height > 0 {
                    app.window = crate::geom::Size::new(ev.width as f32, ev.height as f32);
                    self.resize(ev.width as u32, ev.height as u32);
                    app.redraw = true;
                }
            }
            EXPOSE => {
                app.redraw = true;
            }
            CLIENT_MESSAGE => {
                let ev = *event.as_client();
                if ev.message_type == self.wm_delete {
                    app.save_all();
                    app.quit = true;
                }
            }
            SELECTION_REQUEST => {
                let ev = *event.as_selection_request();
                self.serve_selection(&ev);
            }
            SELECTION_CLEAR => {
                let _ev = *event.as_selection_clear();
                self.clipboard_text.clear();
            }
            SELECTION_NOTIFY => {
                let ev = *event.as_selection_notify();
                self.read_selection(ev.property);
            }
            _ => {}
        }
    }
}

/// Tracks key repeats: X11 sends KeyRelease/KeyPress pairs while a key is held,
/// so a KeyPress for a keycode that is already down is an auto-repeat.
fn key_repeat(keycode: u32, pressed: bool) -> bool {
    thread_local! {
        static HELD: std::cell::RefCell<std::collections::HashSet<u32>> =
            std::cell::RefCell::new(std::collections::HashSet::new());
    }
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        if pressed {
            !held.insert(keycode)
        } else {
            held.remove(&keycode);
            false
        }
    })
}

fn atom(x: &Xlib, dpy: *mut Display, name: &str) -> Atom {
    let c = match std::ffi::CString::new(name) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    // SAFETY: valid NUL-terminated name.
    unsafe { x.XInternAtom(dpy, c.as_ptr(), 0) }
}

/// Runs the reader on X11 until the user quits.
pub fn run(mut app: App, font: Font) -> Result<(), String> {
    let width = app.window.w.max(640.0) as u32;
    let height = app.window.h.max(420.0) as u32;
    let mut shell = Shell::open(
        width,
        height,
        &crate::platform::window_title(app.active_doc().map(|d| d.title.as_str())),
    )?;
    let mut bmp = Bitmap::new(width, height);
    let mut input = Input::default();
    let mut last_draw = std::time::Instant::now();
    let mut forced_draw = true;

    loop {
        let mut frame_needed = forced_draw || app.redraw;
        forced_draw = false;

        // SAFETY: display handle is valid for the shell's lifetime.
        while unsafe { shell.x.XPending(shell.dpy) } > 0 {
            let mut event = XEvent { data: [0; 24] };
            // SAFETY: XPending reported an event.
            if unsafe { shell.x.XNextEvent(shell.dpy, &mut event) } == 0 {
                shell.handle(&event, &mut input, &mut app);
                frame_needed = true;
            }
        }

        input.time = app.now();
        app.tick(app.now());
        if shell.pending_paste {
            shell.pending_paste = false;
            input.paste = Some(shell.clipboard_text.clone());
            frame_needed = true;
        }

        let animating = app.wants_frames();
        let idle_limit = if animating { 0.016 } else { 0.25 };
        if frame_needed || animating || last_draw.elapsed().as_secs_f32() > idle_limit {
            app.frame(&input, &mut bmp, &font);
            shell.present(&bmp);
            shell.set_title(&crate::platform::window_title(
                app.active_doc().map(|d| d.title.as_str()),
            ));
            if let Some(text) = app.clipboard_out.take() {
                shell.set_clipboard(&text);
            }
            input.clear_transient();
            last_draw = std::time::Instant::now();
        }
        if app.quit {
            break;
        }

        // Wait for the next event (or the next animation tick) without burning CPU.
        let fd = unsafe { shell.x.XConnectionNumber(shell.dpy) };
        let mut fds = PollFd {
            fd,
            events: POLLIN,
            revents: 0,
        };
        let timeout = if animating { 16 } else { 60 };
        // SAFETY: single pollfd on the stack.
        unsafe {
            poll(&mut fds, 1, timeout);
        }
    }

    app.save_all();
    // SAFETY: teardown of objects created by this shell.
    unsafe {
        if !shell.image.is_null() {
            (*shell.image).data = std::ptr::null_mut();
            shell.x.XDestroyImage(shell.image);
        }
        shell.x.XFreeGC(shell.dpy, shell.gc);
        shell.x.XDestroyWindow(shell.dpy, shell.window);
        shell.x.XSync(shell.dpy, 0);
        shell.x.XCloseDisplay(shell.dpy);
    }
    let _ = shell.screen;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_keysyms_map_to_characters() {
        assert_eq!(keysym_to_key(0x61), Some(Key::Char('a')));
        assert_eq!(keysym_to_key(0x20), Some(Key::Char(' ')));
        assert_eq!(keysym_to_key(0x2b), Some(Key::Char('+')));
    }

    #[test]
    fn special_keysyms_map_to_navigation() {
        assert_eq!(keysym_to_key(0xff51), Some(Key::Left));
        assert_eq!(keysym_to_key(0xff55), Some(Key::PageUp));
        assert_eq!(keysym_to_key(0xffc8), Some(Key::F11));
        assert_eq!(keysym_to_key(0x1234), None);
    }

    #[test]
    fn event_kinds_are_read_from_the_first_word() {
        let mut event = XEvent { data: [0; 24] };
        event.data[0] = KEY_PRESS as u64;
        assert_eq!(event.kind(), KEY_PRESS);
    }
}

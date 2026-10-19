//! The tray icon on Windows: an icon in the taskbar's notification area.
//!
//! It runs on a thread of its own, with a hidden window that receives its
//! clicks; its menu is drawn by Windows. If Explorer restarts, the icon is
//! put back.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NIN_SELECT, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GWLP_USERDATA, GetCursorPos, GetMessageW, GetSystemMetrics,
    GetWindowLongPtrW, HICON, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTCOLOR, LoadIconW, LoadImageW,
    MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SM_CXSMICON, SM_CYSMICON, SetForegroundWindow, SetWindowLongPtrW,
    TPM_BOTTOMALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_NULL,
    WNDCLASSW, WS_OVERLAPPED,
};
use windows::core::{HSTRING, PCWSTR, w};

use super::{EventHook, TrayState};
use crate::media::MediaEvent;

/// What the icon's clicks arrive as.
const CLICKED: u32 = WM_APP + 1;
/// The app has a new [`TrayState`] to show.
const CHANGED: u32 = WM_APP + 2;
const ICON_ID: u32 = 1;
/// A keyboard selection (Space or Enter): `NIN_SELECT | NINF_KEY`.
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;

pub struct Tray {
    /// The hidden window, as a number so it can cross threads.
    window: usize,
    /// The notification area shows the icon.
    shown: Arc<AtomicBool>,
    shared: Arc<Mutex<TrayState>>,
    state: TrayState,
}

impl Tray {
    /// Puts the icon in the notification area. None if Windows refuses it.
    pub fn start(on_event: EventHook) -> Option<Self> {
        let shown = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(TrayState::default()));
        let (ready, started) = mpsc::channel();
        let (on_tray, state) = (Arc::clone(&shown), Arc::clone(&shared));
        let spawned = std::thread::Builder::new()
            .name("spotless-tray".into())
            .spawn(move || {
                let context = Context {
                    on_event,
                    shown: on_tray,
                    state,
                    taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
                    icon: icon(),
                };
                run(context, ready)
            });
        if let Err(error) = spawned {
            log::warn!("no tray icon: {error}");
            return None;
        }
        match started.recv() {
            Ok(Ok(window)) => Some(Self {
                window,
                shown,
                shared,
                state: TrayState::default(),
            }),
            Ok(Err(error)) => {
                log::warn!("no tray icon: {error}");
                None
            }
            Err(_) => None,
        }
    }

    /// The notification area shows the icon, so a hidden window can be
    /// found again.
    pub fn shown(&self) -> bool {
        self.shown.load(Ordering::SeqCst)
    }

    pub fn update(&mut self, state: TrayState) {
        if state == self.state {
            return;
        }
        self.state = state.clone();
        *self.shared.lock().unwrap_or_else(|p| p.into_inner()) = state;
        let _ = unsafe { PostMessageW(Some(self.hwnd()), CHANGED, WPARAM(0), LPARAM(0)) };
    }

    fn hwnd(&self) -> HWND {
        HWND(self.window as *mut _)
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let _ = unsafe { PostMessageW(Some(self.hwnd()), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }
}

/// What the hidden window's messages need, owned by the tray's thread.
struct Context {
    on_event: EventHook,
    shown: Arc<AtomicBool>,
    state: Arc<Mutex<TrayState>>,
    /// Sent to every window when Explorer (re)starts.
    taskbar_created: u32,
    icon: HICON,
}

impl Context {
    fn state(&self) -> TrayState {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// The icon's details; `flags` says which ones count.
    fn data(&self, window: HWND) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: ICON_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: CLICKED,
            hIcon: self.icon,
            ..Default::default()
        };
        let state = self.state();
        let tip = if state.now_playing.is_empty() {
            "Spotless".to_string()
        } else {
            format!("Spotless\n{}", state.now_playing)
        };
        // The tooltip holds 127 characters and the final nul.
        for (slot, unit) in data.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
            *slot = unit;
        }
        data
    }

    fn add(&self, window: HWND) {
        let mut data = self.data(window);
        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool();
        if added {
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) };
        }
        self.shown.store(added, Ordering::SeqCst);
    }

    fn menu(&self, window: HWND, x: i32, y: i32) {
        let state = self.state();
        let window_item = if state.window_hidden {
            ("Show Spotless", MediaEvent::Raise)
        } else {
            ("Hide Spotless", MediaEvent::ToggleWindow)
        };
        let items = [
            Some((
                if state.playing { "Pause" } else { "Play" },
                MediaEvent::PlayPause,
            )),
            Some(("Previous", MediaEvent::Previous)),
            Some(("Next", MediaEvent::Next)),
            None,
            Some(window_item),
            None,
            Some(("Quit", MediaEvent::Quit)),
        ];
        let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
            return;
        };
        for (i, item) in items.iter().enumerate() {
            // Command 0 means nothing was chosen, so they count from 1.
            let _ = match item {
                Some((label, _)) => unsafe {
                    AppendMenuW(menu, MF_STRING, i + 1, &HSTRING::from(*label))
                },
                None => unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()) },
            };
        }
        // Without the window in front, the menu wouldn't close when the
        // user clicks elsewhere.
        let chosen = unsafe {
            let _ = SetForegroundWindow(window);
            let chosen = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                x,
                y,
                None,
                window,
                None,
            );
            let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            chosen.0 as usize
        };
        if let Some(Some((_, event))) = chosen.checked_sub(1).and_then(|i| items.get(i)) {
            (self.on_event)(*event);
        }
    }
}

/// The tray's thread: makes the hidden window and the icon, tells `ready`
/// the window, then handles its messages until the Tray is dropped.
fn run(context: Context, ready: mpsc::Sender<Result<usize, String>>) {
    let window = match create_window() {
        Ok(window) => window,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    context.add(window);
    if !context.shown.load(Ordering::SeqCst) {
        let _ = unsafe { DestroyWindow(window) };
        let _ = ready.send(Err("the notification area refused the icon".into()));
        return;
    }
    // The window procedure finds the context here; it lives as long as
    // this thread, which outlives the window.
    let context = Box::new(context);
    unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, &*context as *const Context as isize) };
    let _ = ready.send(Ok(window.0 as usize));

    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    drop(context);
}

fn create_window() -> windows::core::Result<HWND> {
    let instance = unsafe { GetModuleHandleW(None)? };
    let class = w!("SpotlessTray");
    let registered = WNDCLASSW {
        lpfnWndProc: Some(window_procedure),
        hInstance: instance.into(),
        lpszClassName: class,
        ..Default::default()
    };
    unsafe { RegisterClassW(&registered) };
    // An ordinary top-level window, never shown: a message-only one
    // wouldn't hear that Explorer restarted.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("Spotless tray"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
}

/// The icon build.rs puts in spotless.exe, at the notification area's
/// size; Windows' generic one if there is none (the examples).
fn icon() -> HICON {
    let size = unsafe { (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)) };
    let own = unsafe {
        GetModuleHandleW(None).and_then(|instance| {
            LoadImageW(
                Some(instance.into()),
                // MAKEINTRESOURCE(1): the ID winresource gives the icon.
                PCWSTR(std::ptr::without_provenance(1)),
                IMAGE_ICON,
                size.0,
                size.1,
                LR_DEFAULTCOLOR,
            )
        })
    };
    match own {
        Ok(handle) => HICON(handle.0),
        Err(_) => unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default(),
    }
}

extern "system" fn window_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let context = unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) } as *const Context;
    // SAFETY: set by `run` before any message that reaches here needs it,
    // and freed only after the window is gone.
    let Some(context) = (unsafe { context.as_ref() }) else {
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    };
    match message {
        CLICKED => {
            // NOTIFYICON_VERSION_4: the event in the low word, and the
            // menu's anchor in wparam.
            match (lparam.0 as u32) & 0xffff {
                NIN_SELECT | NIN_KEYSELECT => (context.on_event)(MediaEvent::ToggleWindow),
                WM_CONTEXTMENU => {
                    let x = (wparam.0 & 0xffff) as i16 as i32;
                    let y = ((wparam.0 >> 16) & 0xffff) as i16 as i32;
                    let (x, y) = if x == 0 && y == 0 {
                        let mut point = POINT::default();
                        let _ = unsafe { GetCursorPos(&mut point) };
                        (point.x, point.y)
                    } else {
                        (x, y)
                    };
                    context.menu(window, x, y);
                }
                _ => {}
            }
            LRESULT(0)
        }
        CHANGED => {
            let data = context.data(window);
            let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = unsafe { DestroyWindow(window) };
            LRESULT(0)
        }
        WM_DESTROY => {
            let data = context.data(window);
            let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
            context.shown.store(false, Ordering::SeqCst);
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ if message == context.taskbar_created && message != 0 => {
            context.add(window);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

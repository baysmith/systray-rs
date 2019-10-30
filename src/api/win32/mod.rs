use crate::{SystrayError, SystrayEvent};
use std;
use std::cell::RefCell;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::mpsc::{channel, Sender};
use std::thread;
use winapi::ctypes::{c_int, c_ulong, c_ushort, c_void};
use winapi::shared::basetsd::ULONG_PTR;
use winapi::shared::guiddef::GUID;
use winapi::shared::minwindef::{DWORD, HINSTANCE, LPARAM, LRESULT, PBYTE, TRUE, UINT, WPARAM};
use winapi::shared::windef::{HBITMAP, HBRUSH, HICON, HMENU, HWND, POINT, RECT};
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::libloaderapi::GetModuleHandleA;
use winapi::um::shellapi::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use winapi::um::wingdi::{CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, SelectObject};
use winapi::um::winnt::LPCWSTR;
use winapi::um::winuser::{
    CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
    DispatchMessageW, DrawIconEx, FillRect, GetCursorPos, GetDC, GetMenuItemID, GetMessageW,
    InsertMenuItemW, LoadCursorW, LoadIconW, LoadImageW, LookupIconIdFromDirectoryEx, PostMessageW,
    PostQuitMessage, RegisterClassW, ReleaseDC, SetForegroundWindow, SetMenuInfo, TrackPopupMenu,
    TranslateMessage, CW_USEDEFAULT, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTCOLOR, LR_LOADFROMFILE,
    MENUINFO, MENUITEMINFOW, MFT_SEPARATOR, MFT_STRING, MIIM_BITMAP, MIIM_FTYPE, MIIM_ID,
    MIIM_STATE, MIIM_STRING, MIIM_SUBMENU, MIM_APPLYTOSUBMENUS, MIM_STYLE, MNS_NOTIFYBYPOS, MSG,
    TPM_BOTTOMALIGN, TPM_LEFTALIGN, WM_DESTROY, WM_LBUTTONUP, WM_MENUCOMMAND, WM_QUIT,
    WM_RBUTTONUP, WM_USER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

// Got this idea from glutin. Yay open source! Boo stupid winproc! Even more boo
// doing SetLongPtr tho.
thread_local!(static WININFO_STASH: RefCell<Option<WindowsLoopData>> = RefCell::new(None));

fn to_wstring(str: &str) -> Vec<u16> {
    OsStr::new(str)
        .encode_wide()
        .chain(Some(0).into_iter())
        .collect::<Vec<_>>()
}

#[derive(Clone)]
struct WindowInfo {
    pub hwnd: HWND,
    pub hinstance: HINSTANCE,
    pub hmenu: HMENU,
}

unsafe impl Send for WindowInfo {}
unsafe impl Sync for WindowInfo {}

#[derive(Clone)]
struct WindowsLoopData {
    pub info: WindowInfo,
    pub tx: Sender<SystrayEvent>,
}

unsafe fn get_win_os_error(msg: &str) -> SystrayError {
    SystrayError::OsError(format!("{}: {}", &msg, GetLastError()))
}

unsafe extern "system" fn window_proc(
    h_wnd: HWND,
    msg: UINT,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if msg == WM_MENUCOMMAND {
        WININFO_STASH.with(|stash| {
            let stash = stash.borrow();
            let stash = stash.as_ref();
            if let Some(stash) = stash {
                let hmenu = if l_param == stash.info.hmenu as isize {
                    0
                } else {
                    l_param
                };
                let item_id = GetMenuItemID(l_param as HMENU, w_param as i32) as i32;
                if item_id != -1 {
                    stash
                        .tx
                        .send(SystrayEvent {
                            menu_id: hmenu as u64,
                            item_id: item_id as u32,
                        })
                        .ok();
                }
            }
        });
    }

    if msg == WM_USER + 1 && (l_param as UINT == WM_LBUTTONUP || l_param as UINT == WM_RBUTTONUP) {
        let mut p = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut p as *mut POINT) == 0 {
            return 1;
        }
        SetForegroundWindow(h_wnd);
        WININFO_STASH.with(|stash| {
            let stash = stash.borrow();
            let stash = stash.as_ref();
            if let Some(stash) = stash {
                TrackPopupMenu(
                    stash.info.hmenu,
                    0,
                    p.x,
                    p.y,
                    (TPM_BOTTOMALIGN | TPM_LEFTALIGN) as i32,
                    h_wnd,
                    std::ptr::null_mut(),
                );
            }
        });
    }
    if msg == WM_DESTROY {
        PostQuitMessage(0);
    }
    DefWindowProcW(h_wnd, msg, w_param, l_param)
}

fn get_nid_struct(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as DWORD,
        hWnd: hwnd,
        uID: 0x1 as UINT,
        uFlags: 0 as UINT,
        uCallbackMessage: 0 as UINT,
        hIcon: 0 as HICON,
        szTip: [0 as u16; 128],
        dwState: 0 as DWORD,
        dwStateMask: 0 as DWORD,
        szInfo: [0 as u16; 256],
        u: unsafe { std::mem::zeroed() },
        szInfoTitle: [0 as u16; 64],
        dwInfoFlags: 0 as UINT,
        guidItem: GUID {
            Data1: 0 as c_ulong,
            Data2: 0 as c_ushort,
            Data3: 0 as c_ushort,
            Data4: [0; 8],
        },
        hBalloonIcon: 0 as HICON,
    }
}

fn get_menu_item_struct() -> MENUITEMINFOW {
    MENUITEMINFOW {
        cbSize: std::mem::size_of::<MENUITEMINFOW>() as UINT,
        fMask: 0 as UINT,
        fType: 0 as UINT,
        fState: 0 as UINT,
        wID: 0 as UINT,
        hSubMenu: 0 as HMENU,
        hbmpChecked: 0 as HBITMAP,
        hbmpUnchecked: 0 as HBITMAP,
        dwItemData: 0 as ULONG_PTR,
        dwTypeData: std::ptr::null_mut(),
        cch: 0 as u32,
        hbmpItem: 0 as HBITMAP,
    }
}

unsafe fn init_window() -> Result<WindowInfo, SystrayError> {
    let class_name = to_wstring("my_window");
    let hinstance: HINSTANCE = GetModuleHandleA(std::ptr::null_mut());
    let wnd = WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: 0 as HINSTANCE,
        hIcon: LoadIconW(0 as HINSTANCE, IDI_APPLICATION),
        hCursor: LoadCursorW(0 as HINSTANCE, IDI_APPLICATION),
        hbrBackground: 16 as HBRUSH,
        lpszMenuName: 0 as LPCWSTR,
        lpszClassName: class_name.as_ptr(),
    };
    if RegisterClassW(&wnd) == 0 {
        return Err(get_win_os_error("Error creating window class"));
    }
    let hwnd = CreateWindowExW(
        0,
        class_name.as_ptr(),
        to_wstring("rust_systray_window").as_ptr(),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        0,
        CW_USEDEFAULT,
        0,
        0 as HWND,
        0 as HMENU,
        0 as HINSTANCE,
        std::ptr::null_mut(),
    );
    if hwnd.is_null() {
        return Err(get_win_os_error("Error creating window"));
    }
    let mut nid = get_nid_struct(hwnd);
    nid.uID = 0x1;
    nid.uFlags = NIF_MESSAGE;
    nid.uCallbackMessage = WM_USER + 1;
    if Shell_NotifyIconW(NIM_ADD, &mut nid as *mut NOTIFYICONDATAW) == 0 {
        return Err(get_win_os_error("Error adding menu icon"));
    }
    // Setup menu
    let hmenu = CreatePopupMenu();
    let m = MENUINFO {
        cbSize: std::mem::size_of::<MENUINFO>() as DWORD,
        fMask: MIM_APPLYTOSUBMENUS | MIM_STYLE,
        dwStyle: MNS_NOTIFYBYPOS,
        cyMax: 0 as UINT,
        hbrBack: 0 as HBRUSH,
        dwContextHelpID: 0 as DWORD,
        dwMenuData: 0 as ULONG_PTR,
    };
    if SetMenuInfo(hmenu, &m as *const MENUINFO) == 0 {
        return Err(get_win_os_error("Error setting up menu"));
    }

    Ok(WindowInfo {
        hwnd,
        hmenu,
        hinstance,
    })
}

unsafe fn run_loop() {
    debug!("Running windows loop");
    // Run message loop
    let mut msg = MSG {
        hwnd: 0 as HWND,
        message: 0 as UINT,
        wParam: 0 as WPARAM,
        lParam: 0 as LPARAM,
        time: 0 as DWORD,
        pt: POINT { x: 0, y: 0 },
    };
    loop {
        GetMessageW(&mut msg, 0 as HWND, 0, 0);
        if msg.message == WM_QUIT {
            break;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    debug!("Leaving windows run loop");
}

pub struct Window {
    info: WindowInfo,
    windows_loop: Option<thread::JoinHandle<()>>,
}

impl Window {
    pub fn new(event_tx: Sender<SystrayEvent>) -> Result<Window, SystrayError> {
        let (tx, rx) = channel();
        let windows_loop = thread::spawn(move || {
            unsafe {
                let i = init_window();
                let k;
                match i {
                    Ok(j) => {
                        tx.send(Ok(j.clone())).ok();
                        k = j;
                    }
                    Err(e) => {
                        // If creation didn't work, return out of the thread.
                        tx.send(Err(e)).ok();
                        return;
                    }
                };
                WININFO_STASH.with(|stash| {
                    let data = WindowsLoopData {
                        info: k,
                        tx: event_tx,
                    };
                    (*stash.borrow_mut()) = Some(data);
                });
                run_loop();
            }
        });
        let info = match rx.recv().unwrap() {
            Ok(i) => i,
            Err(e) => {
                return Err(e);
            }
        };
        let w = Window {
            info,
            windows_loop: Some(windows_loop),
        };
        Ok(w)
    }

    pub fn quit(&mut self) {
        unsafe {
            PostMessageW(self.info.hwnd, WM_DESTROY, 0 as WPARAM, 0 as LPARAM);
        }
        if let Some(t) = self.windows_loop.take() {
            t.join().ok();
        }
    }

    pub fn set_tooltip(&self, tooltip: &str) -> Result<(), SystrayError> {
        // Add Tooltip
        debug!("Setting tooltip to {}", tooltip);
        let tt = to_wstring(tooltip);
        let mut nid = get_nid_struct(self.info.hwnd);
        for (i, c) in tt.iter().take(128).enumerate() {
            nid.szTip[i] = *c;
        }
        nid.uFlags = NIF_TIP;
        unsafe {
            if Shell_NotifyIconW(NIM_MODIFY, &mut nid as *mut NOTIFYICONDATAW) == 0 {
                return Err(get_win_os_error("Error setting tooltip"));
            }
        }
        Ok(())
    }

    pub fn add_menu_entry(
        &self,
        submenu: u64,
        menu_idx: u32,
        item_idx: u32,
        item_name: &str,
        icon_file: Option<&str>,
    ) -> Result<(), SystrayError> {
        let mut st = to_wstring(item_name);
        let mut item = get_menu_item_struct();
        item.fMask = MIIM_FTYPE | MIIM_STRING | MIIM_ID | MIIM_STATE;
        item.fType = MFT_STRING;
        item.wID = menu_idx;
        item.dwTypeData = st.as_mut_ptr();
        item.cch = (item_name.len() * 2) as u32;
        if let Some(icon_file) = icon_file {
            item.fMask |= MIIM_BITMAP;
            item.hbmpItem = self.load_icon_as_bitmap(icon_file)?;
        }
        let hmenu = if submenu == 0 {
            self.info.hmenu
        } else {
            submenu as HMENU
        };
        unsafe {
            if InsertMenuItemW(hmenu, item_idx, 1, &item as *const MENUITEMINFOW) == 0 {
                return Err(get_win_os_error("Error inserting menu item"));
            }
        }
        Ok(())
    }

    fn new_submenu(&self) -> Result<HMENU, SystrayError> {
        let hmenu = unsafe { CreatePopupMenu() };
        let m = MENUINFO {
            cbSize: std::mem::size_of::<MENUINFO>() as DWORD,
            fMask: MIM_APPLYTOSUBMENUS | MIM_STYLE,
            dwStyle: MNS_NOTIFYBYPOS,
            cyMax: 0 as UINT,
            hbrBack: 0 as HBRUSH,
            dwContextHelpID: 0 as DWORD,
            dwMenuData: 0 as ULONG_PTR,
        };
        unsafe {
            if SetMenuInfo(hmenu, &m as *const MENUINFO) == 0 {
                return Err(get_win_os_error("Error setting up menu"));
            }
        }
        Ok(hmenu)
    }

    pub fn add_menu_group(
        &self,
        submenu: u64,
        menu_idx: u32,
        item_idx: u32,
        item_name: &str,
        icon_file: Option<&str>,
    ) -> Result<u64, SystrayError> {
        let mut st = to_wstring(item_name);
        let mut item = get_menu_item_struct();
        item.fMask = MIIM_FTYPE | MIIM_SUBMENU | MIIM_ID | MIIM_STATE | MIIM_STRING;
        item.fType = MFT_STRING;
        item.wID = menu_idx;
        item.hSubMenu = self.new_submenu()?;
        item.dwTypeData = st.as_mut_ptr();
        item.cch = (item_name.len() * 2) as u32;
        if let Some(icon_file) = icon_file {
            item.fMask |= MIIM_BITMAP;
            item.hbmpItem = self.load_icon_as_bitmap(icon_file)?;
        }
        let hmenu = if submenu == 0 {
            self.info.hmenu
        } else {
            submenu as HMENU
        };
        unsafe {
            if InsertMenuItemW(hmenu, item_idx, 1, &item as *const MENUITEMINFOW) == 0 {
                return Err(get_win_os_error("Error inserting menu item"));
            }
        }
        Ok(item.hSubMenu as u64)
    }

    pub fn add_menu_separator(
        &self,
        submenu: u64,
        menu_idx: u32,
        item_idx: u32,
    ) -> Result<(), SystrayError> {
        let mut item = get_menu_item_struct();
        item.fMask = MIIM_FTYPE;
        item.fType = MFT_SEPARATOR;
        item.wID = menu_idx;
        let hmenu = if submenu == 0 {
            self.info.hmenu
        } else {
            submenu as HMENU
        };
        unsafe {
            if InsertMenuItemW(hmenu, item_idx, 1, &item as *const MENUITEMINFOW) == 0 {
                return Err(get_win_os_error("Error inserting separator"));
            }
        }
        Ok(())
    }

    fn set_icon(&self, icon: HICON) -> Result<(), SystrayError> {
        unsafe {
            let mut nid = get_nid_struct(self.info.hwnd);
            nid.uFlags = NIF_ICON;
            nid.hIcon = icon;
            if Shell_NotifyIconW(NIM_MODIFY, &mut nid as *mut NOTIFYICONDATAW) == 0 {
                return Err(get_win_os_error("Error setting icon"));
            }
        }
        Ok(())
    }

    pub fn set_icon_from_resource(&self, resource_name: &str) -> Result<(), SystrayError> {
        let icon;
        unsafe {
            icon = LoadImageW(
                self.info.hinstance,
                to_wstring(&resource_name).as_ptr(),
                IMAGE_ICON,
                64,
                64,
                0,
            ) as HICON;
            if icon == std::ptr::null_mut() as HICON {
                return Err(get_win_os_error("Error setting icon from resource"));
            }
        }
        self.set_icon(icon)
    }

    fn icon_to_bitmap(&self, hicon: HICON, size: i32) -> Result<HBITMAP, SystrayError> {
        let hresultbmp;
        unsafe {
            let hdc = GetDC(std::ptr::null_mut() as HWND);
            let hmemdc = CreateCompatibleDC(hdc);
            let hmembmp = CreateCompatibleBitmap(hdc, size, size);
            let horgbmp = SelectObject(hmemdc, hmembmp as *mut c_void);
            const DI_NORMAL: UINT = 0x0003;
            let rect = RECT {
                left: 0,
                top: 0,
                right: size,
                bottom: size,
            };
            let prect: *const RECT = &rect;
            FillRect(hmemdc, prect, 16 as HBRUSH);

            DrawIconEx(
                hmemdc,
                0,
                0,
                hicon,
                size as c_int,
                size as c_int,
                0,
                std::ptr::null_mut() as HBRUSH,
                DI_NORMAL,
            );

            hresultbmp = hmembmp;
            SelectObject(hmemdc, horgbmp);
            DeleteDC(hmemdc);
            ReleaseDC(std::ptr::null_mut() as HWND, hdc);
            DestroyIcon(hicon);
        }
        Ok(hresultbmp)
    }

    fn load_icon_as_bitmap(&self, icon_file: &str) -> Result<HBITMAP, SystrayError> {
        let wstr_icon_file = to_wstring(&icon_file);
        let hbitmap;
        const ICON_SIZE: i32 = 16;
        unsafe {
            let hicon = LoadImageW(
                std::ptr::null_mut() as HINSTANCE,
                wstr_icon_file.as_ptr(),
                IMAGE_ICON,
                ICON_SIZE,
                ICON_SIZE,
                LR_LOADFROMFILE,
            ) as HICON;
            if hicon == std::ptr::null_mut() as HICON {
                return Err(get_win_os_error(&format!(
                    "Error loading icon from file {}",
                    icon_file
                )));
            }
            hbitmap = self.icon_to_bitmap(hicon, ICON_SIZE)?;
        }
        Ok(hbitmap)
    }

    pub fn set_icon_from_file(&self, icon_file: &str) -> Result<(), SystrayError> {
        let wstr_icon_file = to_wstring(&icon_file);
        let hicon;
        unsafe {
            hicon = LoadImageW(
                std::ptr::null_mut() as HINSTANCE,
                wstr_icon_file.as_ptr(),
                IMAGE_ICON,
                64,
                64,
                LR_LOADFROMFILE,
            ) as HICON;
            if hicon == std::ptr::null_mut() as HICON {
                return Err(get_win_os_error("Error setting icon from file"));
            }
        }
        self.set_icon(hicon)
    }

    pub fn set_icon_from_buffer(
        &self,
        buffer: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), SystrayError> {
        let offset = unsafe {
            LookupIconIdFromDirectoryEx(
                buffer.as_ptr() as PBYTE,
                TRUE,
                width as i32,
                height as i32,
                LR_DEFAULTCOLOR,
            )
        };

        if offset != 0 {
            let icon_data = &buffer[offset as usize..];
            let hicon = unsafe {
                CreateIconFromResourceEx(
                    icon_data.as_ptr() as PBYTE,
                    0,
                    TRUE,
                    0x30000,
                    width as i32,
                    height as i32,
                    LR_DEFAULTCOLOR,
                )
            };

            if hicon == std::ptr::null_mut() as HICON {
                return Err(unsafe { get_win_os_error("Cannot load icon from the buffer") });
            }

            self.set_icon(hicon)
        } else {
            Err(unsafe { get_win_os_error("Error setting icon from buffer") })
        }
    }

    pub fn shutdown(&self) -> Result<(), SystrayError> {
        unsafe {
            let mut nid = get_nid_struct(self.info.hwnd);
            nid.uFlags = NIF_ICON;
            if Shell_NotifyIconW(NIM_DELETE, &mut nid as *mut NOTIFYICONDATAW) == 0 {
                return Err(get_win_os_error("Error deleting icon from menu"));
            }
        }
        Ok(())
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        self.shutdown().ok();
    }
}

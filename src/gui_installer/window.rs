use super::{
    InstallerWindowState, WizardMode, INST_HEIGHT_DIP, INST_WIDTH_DIP, TIMER_ANIM, TIMER_CARET,
    WM_APP_INSTALL_EVENT,
};
use crate::d2d::D2DRenderer;
use crate::win32_ops::{schedule_self_delete, to_wide};
use std::env;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetMessageW,
    GetSystemMetrics, GetWindowLongPtrW, KillTimer, LoadCursorW, LoadIconW, PostQuitMessage,
    RegisterClassW, SendMessageW, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, CREATESTRUCTW, CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA,
    IDC_ARROW, ICON_BIG, ICON_SMALL, MSG, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE, SWP_NOZORDER,
    SW_SHOW, WM_CHAR, WM_CREATE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SETICON, WM_SIZE,
    WM_TIMER, WNDCLASSW, WS_EX_APPWINDOW, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
};

pub fn run_installer_or_uninstaller_gui(mode: WizardMode) -> Result<(), String> {
    let renderer = D2DRenderer::new()?;
    let state = Box::new(match mode {
        WizardMode::Installer {
            installer_exe,
            manifest,
        } => InstallerWindowState::new_installer(renderer, installer_exe, manifest),
        WizardMode::Uninstaller {
            uninstaller_exe,
            receipt,
        } => InstallerWindowState::new_uninstaller(renderer, uninstaller_exe, receipt),
    });

    unsafe {
        let h_instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class_name = to_wide("NyanSetupInstallerWndClass");

        let app_icon = LoadIconW(h_instance, PCWSTR(1 as *const u16)).unwrap_or_default();

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DROPSHADOW,
            lpfnWndProc: Some(installer_wnd_proc),
            hInstance: h_instance.into(),
            hIcon: app_icon,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let sys_dpi = GetDpiForSystem().max(96);
        let scale = (sys_dpi as f32) / 96.0;
        let phys_w = (INST_WIDTH_DIP * scale).round() as i32;
        let phys_h = (INST_HEIGHT_DIP * scale).round() as i32;

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let pos_x = ((screen_w - phys_w) / 2).max(10);
        let pos_y = ((screen_h - phys_h) / 2).max(10);

        let title = to_wide("Nyan Setup 安装程序");
        let state_ptr = Box::into_raw(state);

        // Pure borderless WS_POPUP window: NO WS_CAPTION, NO WS_THICKFRAME
        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP | WS_SYSMENU | WS_MINIMIZEBOX,
            pos_x,
            pos_y,
            phys_w,
            phys_h,
            None,
            None,
            h_instance,
            Some(state_ptr as *const _),
        )
        .map_err(|e| format!("创建安装向导窗口失败: {}", e))?;

        if !app_icon.is_invalid() {
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                WPARAM(ICON_BIG as usize),
                LPARAM(app_icon.0 as isize),
            );
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                WPARAM(ICON_SMALL as usize),
                LPARAM(app_icon.0 as isize),
            );
        }

        let corner_pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner_pref as *const _ as *const _,
            std::mem::size_of_val(&corner_pref) as u32,
        );

        let _ = SetTimer(hwnd, TIMER_CARET, 530, None);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetFocus(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    let _ = env::current_dir();
    Ok(())
}

unsafe extern "system" fn installer_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CREATE {
        let cs = &*(lparam.0 as *const CREATESTRUCTW);
        let state_ptr = cs.lpCreateParams as *mut InstallerWindowState;
        if !state_ptr.is_null() {
            (*state_ptr).hwnd = hwnd;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
        }
        return LRESULT(0);
    }

    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut InstallerWindowState;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *state_ptr;

    let map_mouse = |state: &InstallerWindowState, lparam: LPARAM| -> (f32, f32) {
        let x_px = (lparam.0 & 0xFFFF) as i16 as f32;
        let y_px = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let cw = (rc.right - rc.left).max(1) as f32;
        let ch = (rc.bottom - rc.top).max(1) as f32;
        (x_px * (state.width_dip / cw), y_px * (state.height_dip / ch))
    };

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_SIZE => {
            let width_px = (lparam.0 & 0xFFFF) as u32;
            let height_px = ((lparam.0 >> 16) & 0xFFFF) as u32;
            if width_px > 0 && height_px > 0 {
                let dpi = GetDpiForWindow(hwnd).max(96);
                state.width_dip = INST_WIDTH_DIP;
                state.height_dip = INST_HEIGHT_DIP;
                state.renderer.resize(width_px, height_px, dpi);
                state.invalidate();
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let dpi = (wparam.0 & 0xFFFF) as u32;
            let suggested = &*(lparam.0 as *const RECT);
            let new_w = (suggested.right - suggested.left).max(1);
            let new_h = (suggested.bottom - suggested.top).max(1);
            let _ = SetWindowPos(
                hwnd,
                None,
                suggested.left,
                suggested.top,
                new_w,
                new_h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            state.renderer.resize(new_w as u32, new_h as u32, dpi);
            state.invalidate();
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (mx, my) = map_mouse(state, lparam);
            let lbutton_down = (wparam.0 & 0x0001) != 0;
            state.on_mouse_move(mx, my, lbutton_down);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (mx, my) = map_mouse(state, lparam);
            state.on_lbutton_down(mx, my);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            state.text_input.is_dragging = false;
            if state.dragging_log_hscroll.is_some() || state.dragging_log_vscroll.is_some() {
                state.dragging_log_hscroll = None;
                state.dragging_log_vscroll = None;
                let _ = ReleaseCapture();
                state.invalidate();
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
            state.on_mouse_wheel(delta);
            LRESULT(0)
        }
        WM_CHAR => {
            if let Some(ch) = char::from_u32(wparam.0 as u32) {
                state.on_char(ch);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            state.on_keydown(wparam.0 as u16);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_CARET {
                if state.path_focused {
                    state.caret_visible = !state.caret_visible;
                    state.invalidate();
                }
            } else if wparam.0 == TIMER_ANIM {
                state.ripples.retain_mut(|r| r.step());
                if state.is_working {
                    state.poll_install_events();
                    let diff = state.target_progress - state.progress;
                    if diff.abs() > 0.002 {
                        state.progress += diff * 0.22;
                    }
                }
                if state.ripples.is_empty() && !state.is_working {
                    let _ = KillTimer(hwnd, TIMER_ANIM);
                }
                state.invalidate();
            }
            LRESULT(0)
        }
        WM_APP_INSTALL_EVENT => {
            state.poll_install_events();
            state.invalidate();
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            state.draw();
            state.renderer.blit_to_hdc(ps.hdc);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            if state.uninstall_done {
                if let WizardMode::Uninstaller {
                    ref uninstaller_exe,
                    ref receipt,
                } = state.mode
                {
                    schedule_self_delete(uninstaller_exe, Path::new(&receipt.install_dir));
                }
            }
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(state_ptr));
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// Zemo — Phase 0 spike: share visibility.
//
// Question this answers: does a transparent, always-on-top, WS_EX_LAYERED overlay
// window actually appear in a *monitor* share in Teams and Zoom?
//
// Klipp proves the technique works for FFmpeg gdigrab (a display-capture path).
// Teams and Zoom use different backends (DXGI Desktop Duplication /
// Windows.Graphics.Capture), so the premise of the whole product is unverified
// until a second participant on a real call confirms they can see the overlay.
//
// Secondary question: does SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)
// reliably hide the toolbar window from that same share? (PRD R4.2)
//
// This is throwaway code. It is deliberately one file.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize};

#[cfg(windows)]
use windows::Win32::Foundation::HWND;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowDisplayAffinity, SetWindowLongPtrW, GWL_EXSTYLE,
    WDA_EXCLUDEFROMCAPTURE, WDA_NONE, WS_EX_LAYERED, WS_EX_TRANSPARENT,
};

// ─────────────────────────────────────────────────────────────────────────────
// Win32 / WebView2 helpers — ported from Klipp's src-tauri/src/commands/overlay.rs
// ─────────────────────────────────────────────────────────────────────────────

/// WebView2 paints an opaque background by default, which Tauri's
/// `transparent: true` alone does not override. Reach through to
/// `ICoreWebView2Controller2` and set DefaultBackgroundColor to alpha 0.
///
/// Must be re-applied after any resize — a resize reverts it.
#[cfg(windows)]
fn apply_webview_transparency(window: &tauri::WebviewWindow) -> Result<(), tauri::Error> {
    window.with_webview(|webview: tauri::webview::PlatformWebview| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2Controller2, COREWEBVIEW2_COLOR,
        };
        use windows::core::Interface;

        let controller = webview.controller();
        let controller2: ICoreWebView2Controller2 = match controller.cast() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[spike] ICoreWebView2Controller2 cast failed: {e}");
                return;
            }
        };

        let transparent = COREWEBVIEW2_COLOR { A: 0, R: 0, G: 0, B: 0 };
        if let Err(e) = controller2.SetDefaultBackgroundColor(transparent) {
            eprintln!("[spike] SetDefaultBackgroundColor failed: {e}");
        }
    })
}

/// Toggle click-through WITHOUT losing per-pixel transparency.
///
/// Tauri's `set_ignore_cursor_events` strips WS_EX_LAYERED along with
/// WS_EX_TRANSPARENT, which kills WebView2's transparency. Flip only
/// WS_EX_TRANSPARENT and force WS_EX_LAYERED to stay on.
#[cfg(windows)]
fn set_click_through_preserving_layered(
    window: &tauri::WebviewWindow,
    click_through: bool,
) -> Result<(), String> {
    let hwnd_raw = window.hwnd().map_err(|e| e.to_string())?;
    let hwnd = HWND(hwnd_raw.0 as *mut _);
    let layered = WS_EX_LAYERED.0 as isize;
    let transparent = WS_EX_TRANSPARENT.0 as isize;

    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let next = if click_through {
            (current | layered) | transparent
        } else {
            (current | layered) & !transparent
        };
        if next != current {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
        }
    }
    Ok(())
}

/// Apply or clear WDA_EXCLUDEFROMCAPTURE (PRD R4.2).
///
/// WDA_EXCLUDEFROMCAPTURE needs Windows 10 2004 (build 19041)+. On older builds
/// the call fails and the window stays visible to capture — surfaced to the UI
/// rather than swallowed, because silently appearing in someone's share is the
/// exact failure we care about.
#[cfg(windows)]
fn set_capture_excluded(window: &tauri::WebviewWindow, excluded: bool) -> Result<(), String> {
    let hwnd_raw = window.hwnd().map_err(|e| e.to_string())?;
    let hwnd = HWND(hwnd_raw.0 as *mut _);
    let affinity = if excluded { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };

    unsafe {
        SetWindowDisplayAffinity(hwnd, affinity)
            .map_err(|e| format!("SetWindowDisplayAffinity failed: {e}"))
    }
}

#[cfg(not(windows))]
fn apply_webview_transparency(_w: &tauri::WebviewWindow) -> Result<(), tauri::Error> {
    Ok(())
}
#[cfg(not(windows))]
fn set_click_through_preserving_layered(_w: &tauri::WebviewWindow, _c: bool) -> Result<(), String> {
    Ok(())
}
#[cfg(not(windows))]
fn set_capture_excluded(_w: &tauri::WebviewWindow, _e: bool) -> Result<(), String> {
    Err("not supported on this platform".into())
}

fn window_or_err(app: &tauri::AppHandle, label: &str) -> Result<tauri::WebviewWindow, String> {
    app.get_webview_window(label)
        .ok_or_else(|| format!("no window labelled '{label}'"))
}

// ─────────────────────────────────────────────────────────────────────────────
// Commands driven by the control panel
// ─────────────────────────────────────────────────────────────────────────────

/// Toggle capture exclusion on either window.
///
/// Excluding the OVERLAY is the positive control for test A: if the overlay
/// vanishes from the viewer's screen when this is on and returns when it's off,
/// the share is demonstrably capturing our window, and a "can't see it" result
/// is a real negative rather than a misconfigured call.
#[tauri::command]
fn set_exclusion(app: tauri::AppHandle, target: String, excluded: bool) -> Result<(), String> {
    let window = window_or_err(&app, &target)?;
    set_capture_excluded(&window, excluded)
}

/// Make the overlay interactive (drawable) or click-through. Not strictly needed
/// to answer the spike question, but it exercises the toggle that Phase 1 depends
/// on, and proves transparency survives it.
#[tauri::command]
fn set_overlay_interactive(app: tauri::AppHandle, interactive: bool) -> Result<(), String> {
    let overlay = window_or_err(&app, "overlay")?;
    set_click_through_preserving_layered(&overlay, !interactive)?;
    #[cfg(windows)]
    apply_webview_transparency(&overlay).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn set_overlay_visible(app: tauri::AppHandle, visible: bool) -> Result<(), String> {
    let overlay = window_or_err(&app, "overlay")?;
    if visible {
        overlay.show().map_err(|e| e.to_string())?;
    } else {
        overlay.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Ask the overlay to repaint in a different accent colour, so the tester can
/// confirm over voice that what the viewer sees is live and not a stale frame.
#[tauri::command]
fn set_overlay_color(app: tauri::AppHandle, color: String) -> Result<(), String> {
    app.emit_to("overlay", "set-color", color)
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct Diagnostics {
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: u32,
    monitor_height: u32,
    scale_factor: f64,
    os_version: String,
    exclusion_supported: bool,
}

#[tauri::command]
fn diagnostics(app: tauri::AppHandle) -> Result<Diagnostics, String> {
    let overlay = window_or_err(&app, "overlay")?;
    let monitor = overlay
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("no primary monitor")?;

    // Probe support by actually attempting the call rather than version-sniffing,
    // then restore the window's real state.
    let exclusion_supported = set_capture_excluded(&overlay, true).is_ok();
    let _ = set_capture_excluded(&overlay, false);

    Ok(Diagnostics {
        monitor_x: monitor.position().x,
        monitor_y: monitor.position().y,
        monitor_width: monitor.size().width,
        monitor_height: monitor.size().height,
        scale_factor: monitor.scale_factor(),
        os_version: os_version(),
        exclusion_supported,
    })
}

fn os_version() -> String {
    #[cfg(windows)]
    {
        use std::process::Command;
        // `cmd /c ver` is the least ceremonious way to get the real build number;
        // GetVersionExW lies without an app manifest.
        if let Ok(out) = Command::new("cmd").args(["/c", "ver"]).output() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    "unknown".into()
}

#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

// ─────────────────────────────────────────────────────────────────────────────

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            set_exclusion,
            set_overlay_interactive,
            set_overlay_visible,
            set_overlay_color,
            diagnostics,
            quit
        ])
        .setup(|app| {
            let overlay = app
                .get_webview_window("overlay")
                .expect("overlay window missing from tauri.conf.json");

            // Size the overlay to the full primary monitor in PHYSICAL pixels.
            // Physical throughout keeps one coordinate contract end to end, which
            // is what Klipp learned the hard way on mixed-DPI setups.
            if let Ok(Some(monitor)) = overlay.primary_monitor() {
                let pos = monitor.position();
                let size = monitor.size();

                overlay.set_position(PhysicalPosition::new(pos.x, pos.y))?;
                overlay.set_size(PhysicalSize::new(size.width, size.height))?;

                // Frameless Windows windows carry invisible chrome (~8px on some
                // sides). set_position sets the OUTER rect but WebView2 renders in
                // the INNER one, so measure the delta and compensate — otherwise
                // the overlay's content is offset from the monitor it's covering.
                if let (Ok(outer), Ok(inner)) = (overlay.outer_position(), overlay.inner_position())
                {
                    let dx = inner.x - outer.x;
                    let dy = inner.y - outer.y;
                    if dx != 0 || dy != 0 {
                        eprintln!("[spike] compensating window chrome: dx={dx} dy={dy}");
                        overlay.set_position(PhysicalPosition::new(pos.x - dx, pos.y - dy))?;
                    }
                }
            }

            // Transparency AFTER the resize — a resize reverts the WebView2
            // background colour.
            #[cfg(windows)]
            if let Err(e) = apply_webview_transparency(&overlay) {
                eprintln!("[spike] transparency setup failed: {e}");
            }

            // Click-through, and explicitly NOT capture-excluded: the overlay must
            // be visible to the share. This is the whole point.
            if let Err(e) = set_click_through_preserving_layered(&overlay, true) {
                eprintln!("[spike] click-through setup failed: {e}");
            }
            let _ = set_capture_excluded(&overlay, false);

            overlay.show()?;

            // The control panel IS capture-excluded (PRD R4.2) — viewers should
            // never see the presenter's controls.
            if let Some(control) = app.get_webview_window("control") {
                match set_capture_excluded(&control, true) {
                    Ok(()) => eprintln!("[spike] control panel excluded from capture"),
                    Err(e) => eprintln!("[spike] control panel exclusion FAILED: {e}"),
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running zemo spike");
}


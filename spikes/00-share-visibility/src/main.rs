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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

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
// Overlay geometry — invisible window chrome
// ─────────────────────────────────────────────────────────────────────────────
//
// On Windows a frameless window still has a non-client area: an invisible
// resize border, typically ~8px left/right/bottom and 0 on top. `set_position`
// positions the OUTER rect (border included) but WebView2 renders inside the
// INNER (client) rect. Ask for the window at (0,0) and the content can land at
// (8,0) — the overlay is then shifted off the monitor edge, and for Zemo that
// means annotations land a few pixels away from where the user pointed.
//
// The amount varies by Windows version, DPI and theme, so it must be MEASURED
// per machine rather than assumed. Measure inner - outer, then shift the outer
// window back by that delta so the inner rect lands exactly on the monitor.

/// Fit the overlay to the primary monitor. Returns the measured chrome offset
/// `(dx, dy)` — `(0, 0)` means this machine has no offset.
fn fit_overlay_to_monitor(overlay: &tauri::WebviewWindow) -> Result<(i32, i32), String> {
    let monitor = overlay
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("no primary monitor")?;
    let pos = monitor.position();
    let size = monitor.size();

    // Physical pixels throughout — one coordinate contract end to end.
    overlay
        .set_position(PhysicalPosition::new(pos.x, pos.y))
        .map_err(|e| e.to_string())?;
    overlay
        .set_size(PhysicalSize::new(size.width, size.height))
        .map_err(|e| e.to_string())?;

    let outer = overlay.outer_position().map_err(|e| e.to_string())?;
    let inner = overlay.inner_position().map_err(|e| e.to_string())?;
    let dx = inner.x - outer.x;
    let dy = inner.y - outer.y;

    if dx != 0 || dy != 0 {
        overlay
            .set_position(PhysicalPosition::new(pos.x - dx, pos.y - dy))
            .map_err(|e| e.to_string())?;
    }

    // A resize or reposition reverts WebView2's background colour, so
    // transparency has to be re-applied after fitting, never before.
    #[cfg(windows)]
    apply_webview_transparency(overlay).map_err(|e| e.to_string())?;

    Ok((dx, dy))
}

#[derive(serde::Serialize)]
struct GeometryCheck {
    monitor: [i32; 4],
    inner: [i32; 4],
    chrome_dx: i32,
    chrome_dy: i32,
    /// Per-edge error in physical pixels; 0 means the overlay edge sits exactly
    /// on the monitor edge. Positive = overlay edge is inside the monitor.
    gap_left: i32,
    gap_top: i32,
    gap_right: i32,
    gap_bottom: i32,
    ok: bool,
}

/// Verify the overlay's *rendered* area actually covers the monitor, rather than
/// trusting that the compensation worked. This is what the tester reads before
/// going on a call — a silently misaligned overlay would invalidate the spike.
#[tauri::command]
fn geometry_check(app: tauri::AppHandle) -> Result<GeometryCheck, String> {
    let overlay = window_or_err(&app, "overlay")?;
    let monitor = overlay
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("no primary monitor")?;
    let mpos = monitor.position();
    let msize = monitor.size();

    let outer = overlay.outer_position().map_err(|e| e.to_string())?;
    let inner = overlay.inner_position().map_err(|e| e.to_string())?;
    let isize_ = overlay.inner_size().map_err(|e| e.to_string())?;

    let gap_left = inner.x - mpos.x;
    let gap_top = inner.y - mpos.y;
    let gap_right = (mpos.x + msize.width as i32) - (inner.x + isize_.width as i32);
    let gap_bottom = (mpos.y + msize.height as i32) - (inner.y + isize_.height as i32);

    Ok(GeometryCheck {
        monitor: [mpos.x, mpos.y, msize.width as i32, msize.height as i32],
        inner: [inner.x, inner.y, isize_.width as i32, isize_.height as i32],
        chrome_dx: inner.x - outer.x,
        chrome_dy: inner.y - outer.y,
        gap_left,
        gap_top,
        gap_right,
        gap_bottom,
        ok: gap_left == 0 && gap_top == 0 && gap_right == 0 && gap_bottom == 0,
    })
}

/// Re-run the fit. Gives a tester whose geometry check failed something to try
/// before giving up — a second pass measures chrome against the already-shifted
/// window and usually converges.
#[tauri::command]
fn refit_overlay(app: tauri::AppHandle) -> Result<(i32, i32), String> {
    let overlay = window_or_err(&app, "overlay")?;
    fit_overlay_to_monitor(&overlay)
}

// ─────────────────────────────────────────────────────────────────────────────
// Pass-through <-> draw mode
// ─────────────────────────────────────────────────────────────────────────────
//
// The core loop: draw something, drop back to pass-through to actually click
// the app you're demoing, then come back and draw again — without the marks you
// already made disappearing (PRD R5.4). Arming is a global hotkey because once
// you're clicking other apps, hunting for a control panel defeats the point.

static ARMED: AtomicBool = AtomicBool::new(false);
/// Tool to restore when re-arming, so the loop doesn't force a re-pick each time.
static TOOL: Mutex<String> = Mutex::new(String::new());

fn current_tool() -> String {
    let t = TOOL.lock().map(|g| g.clone()).unwrap_or_default();
    if t.is_empty() { "arrow".to_string() } else { t }
}

/// Single place that flips the mode, so the Win32 state, the overlay and the
/// control panel can never disagree about which mode we're in.
fn apply_mode(app: &tauri::AppHandle, armed: bool) -> Result<(), String> {
    let overlay = window_or_err(app, "overlay")?;

    set_click_through_preserving_layered(&overlay, !armed)?;
    #[cfg(windows)]
    apply_webview_transparency(&overlay).map_err(|e| e.to_string())?;

    if armed {
        // Needed for the overlay to receive Esc and pointer events.
        let _ = overlay.set_focus();
    }
    ARMED.store(armed, Ordering::Relaxed);

    let tool = if armed { current_tool() } else { "none".to_string() };
    app.emit_to("overlay", "set-tool", tool.clone())
        .map_err(|e| e.to_string())?;
    app.emit_to("control", "mode-changed", (armed, tool))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Pick a tool. Selecting one arms draw mode; "none" returns to pass-through.
#[tauri::command]
fn set_tool(app: tauri::AppHandle, tool: String) -> Result<(), String> {
    if tool != "none" {
        *TOOL.lock().map_err(|e| e.to_string())? = tool;
        apply_mode(&app, true)
    } else {
        apply_mode(&app, false)
    }
}

#[tauri::command]
fn toggle_mode(app: tauri::AppHandle) -> Result<(), String> {
    let armed = ARMED.load(Ordering::Relaxed);
    apply_mode(&app, !armed)
}

#[tauri::command]
fn clear_shapes(app: tauri::AppHandle) -> Result<(), String> {
    app.emit_to("overlay", "clear-shapes", ())
        .map_err(|e| e.to_string())
}

/// Remove a single mark. `which` is "newest" (undo-style) or "oldest" (drain
/// the queue from the front) — useful when you've built up a sequence of marks
/// and want to retire them in the order you drew them.
#[tauri::command]
fn remove_shape(app: tauri::AppHandle, which: String) -> Result<(), String> {
    app.emit_to("overlay", "remove-shape", which)
        .map_err(|e| e.to_string())
}

/// The overlay owns the shape list, so it reports the count back for the panel
/// to display. Routed through Rust so the panel doesn't need event-emit perms.
#[tauri::command]
fn report_count(app: tauri::AppHandle, count: usize) -> Result<(), String> {
    app.emit_to("control", "shape-count", count)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_stroke(app: tauri::AppHandle, width: u32) -> Result<(), String> {
    app.emit_to("overlay", "set-stroke", width)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_test_graphics(app: tauri::AppHandle, visible: bool) -> Result<(), String> {
    app.emit_to("overlay", "test-graphics", visible)
        .map_err(|e| e.to_string())
}

/// Panic reset (PRD R5.7): back to pass-through, marks gone, clean desktop.
#[tauri::command]
fn panic_reset(app: tauri::AppHandle) -> Result<(), String> {
    clear_shapes(app.clone())?;
    apply_mode(&app, false)
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
    // Ctrl+Alt+* avoids the usual conferencing-app bindings (Teams Ctrl+Shift+M/O/E,
    // Zoom Alt+A/V/S). Caveat for later: Windows synthesizes Ctrl+Alt from AltGr
    // on non-US layouts, which is why the real app makes these remappable.
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, ev| {
                    // Fire on press only; the release event would toggle straight back.
                    if ev.state() != ShortcutState::Pressed {
                        return;
                    }
                    // Every binding is Ctrl+Alt+<key>, so matching the key code is
                    // enough — and it avoids the handler owning Shortcut values that
                    // setup() would also need to borrow.
                    let result = match shortcut.key {
                        Code::KeyZ => toggle_mode(app.clone()),
                        Code::KeyR if shortcut.mods.contains(Modifiers::SHIFT) => {
                            panic_reset(app.clone())
                        }
                        Code::KeyA => clear_shapes(app.clone()),
                        Code::Backspace => remove_shape(app.clone(), "newest".into()),
                        Code::Home => remove_shape(app.clone(), "oldest".into()),
                        // Shift is required so a stray Ctrl+Alt+Q can't kill the
                        // app mid-presentation. AltGr on non-US layouts synthesizes
                        // Ctrl+Alt, which makes a two-modifier quit genuinely risky.
                        Code::KeyQ if shortcut.mods.contains(Modifiers::SHIFT) => {
                            app.exit(0);
                            Ok(())
                        }
                        _ => Ok(()),
                    };
                    if let Err(e) = result {
                        eprintln!("[spike] shortcut handler failed: {e}");
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            set_exclusion,
            set_overlay_interactive,
            set_overlay_visible,
            set_overlay_color,
            set_tool,
            toggle_mode,
            clear_shapes,
            remove_shape,
            report_count,
            set_stroke,
            set_test_graphics,
            panic_reset,
            geometry_check,
            refit_overlay,
            diagnostics,
            quit
        ])
        .setup(|app| {
            let overlay = app
                .get_webview_window("overlay")
                .expect("overlay window missing from tauri.conf.json");

            // Size and position the overlay to exactly cover the primary monitor,
            // compensating for invisible window chrome. See fit_overlay_to_monitor.
            match fit_overlay_to_monitor(&overlay) {
                Ok((dx, dy)) if dx != 0 || dy != 0 => {
                    eprintln!("[spike] compensated invisible window chrome: dx={dx} dy={dy}")
                }
                Ok(_) => eprintln!("[spike] no window chrome offset on this machine"),
                Err(e) => eprintln!("[spike] overlay fit FAILED: {e}"),
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

            // Rebuilt rather than captured: the handler closure above already owns
            // the originals, and Shortcut is cheap to construct.
            let ctrl_alt = Some(Modifiers::CONTROL | Modifiers::ALT);
            for (sc, name) in [
                (Shortcut::new(ctrl_alt, Code::KeyZ), "Ctrl+Alt+Z      toggle draw"),
                (Shortcut::new(ctrl_alt, Code::KeyA), "Ctrl+Alt+A      clear all"),
                (Shortcut::new(ctrl_alt, Code::Backspace), "Ctrl+Alt+Bksp   remove newest"),
                (Shortcut::new(ctrl_alt, Code::Home), "Ctrl+Alt+Home   remove oldest"),
                (
                    Shortcut::new(
                        Some(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT),
                        Code::KeyR,
                    ),
                    "Ctrl+Alt+Shift+R  reset",
                ),
                (
                    Shortcut::new(
                        Some(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SHIFT),
                        Code::KeyQ,
                    ),
                    "Ctrl+Alt+Shift+Q  quit",
                ),
            ] {
                // A failure here means another app owns the combo — surfaced rather
                // than swallowed, since the loop is unusable without the hotkey.
                match app.global_shortcut().register(sc) {
                    Ok(()) => eprintln!("[spike] registered  {name}"),
                    Err(e) => eprintln!("[spike] UNAVAILABLE  {name}  ({e})"),
                }
            }

            // The control panel IS capture-excluded (PRD R4.2) — viewers should
            // never see the presenter's controls.
            if let Some(control) = app.get_webview_window("control") {
                // Escape hatch: exclusion also blocks ordinary screenshot tools, so
                // there is no way to capture the panel for a bug report. Setting
                // ZEMO_SPIKE_NO_EXCLUDE=1 leaves it capturable.
                if std::env::var("ZEMO_SPIKE_NO_EXCLUDE").is_ok() {
                    eprintln!("[spike] ZEMO_SPIKE_NO_EXCLUDE set — control panel left capturable");
                } else {
                    match set_capture_excluded(&control, true) {
                        Ok(()) => eprintln!("[spike] control panel excluded from capture"),
                        Err(e) => eprintln!("[spike] control panel exclusion FAILED: {e}"),
                    }
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running zemo spike");
}


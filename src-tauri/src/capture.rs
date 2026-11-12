//! One-key screen capture: grab the screen (overlay marks included), write a
//! PNG, and put it on the clipboard.
//!
//! GDI `BitBlt` is used rather than the fancier WGC path because it is
//! synchronous, dependency-free, and — critically — it honours
//! `WDA_EXCLUDEFROMCAPTURE`. The control panel and the cheat sheet set that
//! affinity, so they are absent from the shot for free: you capture what the
//! call sees, not your own controls.
//!
//! Both outputs happen on one keystroke. A save dialog mid-presentation
//! defeats the purpose, and the clipboard copy is what lets you paste straight
//! into the call's chat without touching a file at all.

use std::path::PathBuf;

use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS,
    HBITMAP, HGDIOBJ, SRCCOPY,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

/// CF_DIB. Not exposed as a typed constant in the crate's clipboard module.
const CF_DIB: u32 = 8;

pub struct Shot {
    pub width: i32,
    pub height: i32,
    /// Bottom-up BGRA, the layout both the clipboard and `GetDIBits` want.
    pub bgra_bottom_up: Vec<u8>,
}

/// Capture the primary monitor.
pub fn grab() -> Result<Shot, String> {
    unsafe {
        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);
        if width <= 0 || height <= 0 {
            return Err("could not read screen size".into());
        }

        let screen_dc = GetDC(None);
        if screen_dc.is_invalid() {
            return Err("GetDC failed".into());
        }
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
        let previous = SelectObject(mem_dc, HGDIOBJ(bitmap.0));

        // CAPTUREBLT is what pulls in layered windows — without it the overlay,
        // the entire point of the shot, would be missing.
        let blt = BitBlt(mem_dc, 0, 0, width, height, Some(screen_dc), 0, 0, SRCCOPY | CAPTUREBLT);

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Positive height = bottom-up rows, which is what CF_DIB expects.
                biHeight: height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut buffer = vec![0u8; (width as usize) * (height as usize) * 4];
        let scanlines = GetDIBits(
            mem_dc,
            bitmap,
            0,
            height as u32,
            Some(buffer.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        );

        // Unwind GDI state before reporting any failure.
        SelectObject(mem_dc, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);

        blt.map_err(|e| format!("BitBlt failed: {e}"))?;
        if scanlines == 0 {
            return Err("GetDIBits returned no scanlines".into());
        }

        Ok(Shot { width, height, bgra_bottom_up: buffer })
    }
}

/// Default save location: `%USERPROFILE%\Pictures\Zemo\zemo-YYYYMMDD-HHMMSS.png`.
pub fn default_path() -> Result<PathBuf, String> {
    let home = std::env::var("USERPROFILE").map_err(|_| "USERPROFILE not set".to_string())?;
    let dir = PathBuf::from(home).join("Pictures").join("Zemo");
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;

    // GetLocalTime avoids pulling in a date crate for one filename.
    let t = unsafe { GetLocalTime() };
    let name = format!(
        "zemo-{:04}{:02}{:02}-{:02}{:02}{:02}.png",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    );
    Ok(dir.join(name))
}

pub fn save_png(shot: &Shot, path: &PathBuf) -> Result<(), String> {
    let (w, h) = (shot.width as usize, shot.height as usize);
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        // Flip: the buffer is bottom-up, PNG wants top-down.
        let src_row = (h - 1 - y) * w * 4;
        for x in 0..w {
            let s = src_row + x * 4;
            let d = (y * w + x) * 4;
            rgba[d] = shot.bgra_bottom_up[s + 2];
            rgba[d + 1] = shot.bgra_bottom_up[s + 1];
            rgba[d + 2] = shot.bgra_bottom_up[s];
            rgba[d + 3] = 255;
        }
    }
    image::save_buffer(path, &rgba, w as u32, h as u32, image::ColorType::Rgba8)
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Put the shot on the clipboard as CF_DIB, so it can be pasted into a chat.
pub fn copy_to_clipboard(shot: &Shot) -> Result<(), String> {
    unsafe {
        let header_size = std::mem::size_of::<BITMAPINFOHEADER>();
        let total = header_size + shot.bgra_bottom_up.len();

        let handle = GlobalAlloc(GMEM_MOVEABLE, total).map_err(|e| format!("GlobalAlloc: {e}"))?;
        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            return Err("GlobalLock failed".into());
        }

        let header = BITMAPINFOHEADER {
            biSize: header_size as u32,
            biWidth: shot.width,
            biHeight: shot.height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: shot.bgra_bottom_up.len() as u32,
            ..Default::default()
        };
        std::ptr::copy_nonoverlapping(&header as *const _ as *const u8, ptr as *mut u8, header_size);
        std::ptr::copy_nonoverlapping(
            shot.bgra_bottom_up.as_ptr(),
            (ptr as *mut u8).add(header_size),
            shot.bgra_bottom_up.len(),
        );
        let _ = GlobalUnlock(handle);

        OpenClipboard(Some(HWND::default())).map_err(|e| format!("OpenClipboard: {e}"))?;
        let result = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| format!("EmptyClipboard: {e}"))?;
            // Ownership of the memory transfers to the clipboard on success.
            SetClipboardData(CF_DIB, Some(HANDLE(handle.0 as *mut _)))
                .map_err(|e| format!("SetClipboardData: {e}"))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}

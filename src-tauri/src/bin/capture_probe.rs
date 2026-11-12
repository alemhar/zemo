// Zemo — Phase 0 probe: Windows.Graphics.Capture
//
// The GDI screenshot and the ffmpeg `ddagrab` run both say the overlay is
// captured, but neither is the path Teams and Zoom actually take. Teams on
// Win10 2004+ and recent Zoom default to Windows.Graphics.Capture (WGC);
// DXGI Desktop Duplication (which ddagrab covers) is the fallback.
//
// This grabs one frame of the primary monitor through WGC and writes it to PNG,
// so the two questions can be answered without a live call:
//
//   Test A analog — is the spike's overlay present in the frame?
//   Test B analog — is the capture-excluded control panel absent from it?
//
// Caveat worth keeping in mind: a conferencing app can filter windows on top of
// these APIs. A pass here raises confidence; it does not replace the live test.
//
// Usage:  cargo run --bin capture-probe -- out.png

use std::{thread, time::Duration};

use windows::{
    core::{factory, Interface, Result as WinResult},
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::{E_FAIL, HMODULE, POINT},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_HARDWARE,
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
                D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING,
            },
            Dxgi::IDXGIDevice,
            Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY},
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RoInitialize, RO_INIT_MULTITHREADED,
        },
    },
};

fn main() -> WinResult<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "wgc.png".into());

    unsafe {
        // WGC is WinRT; the apartment has to exist before any of it is touched.
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }

    let (device, context) = create_d3d_device()?;
    let winrt_device = to_winrt_device(&device)?;
    let item = primary_monitor_item()?;
    let size = item.Size()?;
    println!("[probe] capture item: {}x{}", size.Width, size.Height);

    // Free-threaded pool: frames arrive without needing a message pump, which
    // this console binary doesn't have.
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &winrt_device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        2,
        size,
    )?;
    let session = pool.CreateCaptureSession(&item)?;

    // Cosmetic only, and unsupported on older builds — ignore failures.
    let _ = session.SetIsCursorCaptureEnabled(false);

    session.StartCapture()?;

    // The first frame or two can be empty while the pool spins up.
    let mut frame = None;
    for _ in 0..120 {
        if let Ok(f) = pool.TryGetNextFrame() {
            frame = Some(f);
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    let frame = frame.ok_or_else(|| {
        windows::core::Error::new(E_FAIL, "WGC produced no frame within 3s")
    })?;

    let surface = frame.Surface()?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
    let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };

    let pixels = read_back(&device, &context, &texture)?;

    session.Close().ok();
    pool.Close().ok();

    let (w, h) = (size.Width as u32, size.Height as u32);
    image::save_buffer(&out, &pixels, w, h, image::ColorType::Rgba8)
        .unwrap_or_else(|e| panic!("failed to write {out}: {e}"));
    println!("[probe] wrote {out} ({w}x{h})");
    Ok(())
}

fn create_d3d_device() -> WinResult<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            // BGRA support is required for D3D11 <-> WinRT interop.
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    Ok((device.unwrap(), context.unwrap()))
}

fn to_winrt_device(device: &ID3D11Device) -> WinResult<IDirect3DDevice> {
    let dxgi: IDXGIDevice = device.cast()?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? };
    inspectable.cast()
}

fn primary_monitor_item() -> WinResult<GraphicsCaptureItem> {
    let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    // GraphicsCaptureItem has no public constructor; the interop factory is the
    // only way to build one for an HMONITOR.
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    unsafe { interop.CreateForMonitor(monitor) }
}

/// Copy the GPU texture into a CPU-readable staging texture and return RGBA8.
fn read_back(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: &ID3D11Texture2D,
) -> WinResult<Vec<u8>> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { source.GetDesc(&mut desc) };

    let mut staging_desc = desc;
    staging_desc.Usage = D3D11_USAGE_STAGING;
    staging_desc.BindFlags = 0;
    staging_desc.MiscFlags = 0;
    staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;

    let mut staging: Option<ID3D11Texture2D> = None;
    unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging))? };
    let staging = staging.unwrap();

    unsafe { context.CopyResource(&staging, source) };

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };

    let (w, h) = (desc.Width as usize, desc.Height as usize);
    let mut rgba = vec![0u8; w * h * 4];
    unsafe {
        let src = mapped.pData as *const u8;
        for y in 0..h {
            // RowPitch is not necessarily width*4 — walk row by row.
            let row = src.add(y * mapped.RowPitch as usize);
            for x in 0..w {
                let p = row.add(x * 4);
                let i = (y * w + x) * 4;
                // WGC hands back BGRA; swizzle to RGBA and force alpha opaque so
                // the overlay's transparent regions don't save as see-through.
                rgba[i] = *p.add(2);
                rgba[i + 1] = *p.add(1);
                rgba[i + 2] = *p;
                rgba[i + 3] = 255;
            }
        }
        context.Unmap(&staging, 0);
    }
    Ok(rgba)
}

use image::{RgbImage, RgbaImage};

pub struct Capture {
    pub img: RgbaImage,
    pub origin: (i32, i32),
}

pub fn capture_full() -> Result<Capture, String> {
    imp::capture_full()
}

/// Windows grabs just the region; Linux tools only give whole-desktop shots (see capture_full + crop).
#[cfg(windows)]
pub fn capture_region(r: [i32; 4]) -> Result<RgbImage, String> {
    // keep it on the desktop: BitBlt outside the monitors gives black
    let (vx, vy, vw, vh) = imp::virtual_screen();
    let (x0, y0) = (r[0].max(vx), r[1].max(vy));
    let (x1, y1) = ((r[0] + r[2]).min(vx + vw), (r[1] + r[3]).min(vy + vh));
    imp::gdi_capture(x0, y0, x1 - x0, y1 - y0).map(|img| image::DynamicImage::ImageRgba8(img).to_rgb8())
}

#[cfg_attr(windows, allow(dead_code))]
pub fn crop(cap: &Capture, r: [i32; 4]) -> Result<RgbImage, String> {
    let (iw, ih) = (cap.img.width() as i32, cap.img.height() as i32);
    let x0 = (r[0] - cap.origin.0).clamp(0, iw);
    let y0 = (r[1] - cap.origin.1).clamp(0, ih);
    let x1 = (r[0] - cap.origin.0 + r[2]).clamp(0, iw);
    let y1 = (r[1] - cap.origin.1 + r[3]).clamp(0, ih);
    if x1 - x0 < 1 || y1 - y0 < 1 {
        return Err(format!("zone {r:?} is outside the {iw}x{ih} screenshot"));
    }
    let sub = image::imageops::crop_imm(&cap.img, x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32).to_image();
    Ok(image::DynamicImage::ImageRgba8(sub).to_rgb8())
}

#[cfg(target_os = "linux")]
mod imp {
    use super::Capture;
    use image::RgbaImage;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};

    type Method = (&'static str, fn(&Path) -> Result<RgbaImage, String>);

    const WAYLAND_METHODS: &[Method] = &[
        ("spectacle", |p| run_tool("spectacle", &["-b", "-n", "-f", "-o"], p)),
        ("grim", |p| run_tool("grim", &[], p)),
        ("portal", portal),
        ("gnome-screenshot", |p| run_tool("gnome-screenshot", &["-f"], p)),
        ("x11", |_| x11_full()),
    ];

    static LAST_OK: AtomicUsize = AtomicUsize::new(0);
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    pub fn capture_full() -> Result<Capture, String> {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        if !wayland {
            return x11_full().map(|img| Capture { img, origin: (0, 0) });
        }
        let tmp = tmp_path();
        let first = LAST_OK.load(Ordering::Relaxed);
        let order = std::iter::once(first).chain((0..WAYLAND_METHODS.len()).filter(|&i| i != first));
        let mut errors = Vec::new();
        for i in order {
            let (name, f) = WAYLAND_METHODS[i];
            match f(&tmp) {
                Ok(img) => {
                    let _ = std::fs::remove_file(&tmp);
                    if LAST_OK.swap(i, Ordering::Relaxed) != i {
                        log::info!("Скриншоты: используется {name}");
                    }
                    return Ok(Capture { img, origin: (0, 0) });
                }
                Err(e) => errors.push(format!("{name}: {e}")),
            }
        }
        let _ = std::fs::remove_file(&tmp);
        Err(format!("screen capture failed ({})", errors.join("; ")))
    }

    fn tmp_path() -> PathBuf {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        dir.join(format!(
            "wt-tracker-{}-{}.png",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn run_tool(cmd: &str, args: &[&str], out: &Path) -> Result<RgbaImage, String> {
        let _ = std::fs::remove_file(out);
        let status = Command::new(cmd)
            .args(args)
            .arg(out)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { "not installed".into() } else { e.to_string() })?;
        if !status.success() {
            return Err(format!("exit status {status}"));
        }
        image::open(out).map(|i| i.to_rgba8()).map_err(|e| e.to_string())
    }

    #[cfg(feature = "portal")]
    fn portal(_: &Path) -> Result<RgbaImage, String> {
        use ashpd::desktop::screenshot::Screenshot;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        let uri = rt.block_on(async {
            let resp = Screenshot::request().interactive(false).modal(false).send().await?.response()?;
            Ok::<_, ashpd::Error>(resp.uri().clone())
        });
        let uri = uri.map_err(|e| e.to_string())?;
        let path = uri.to_file_path().map_err(|_| format!("not a file: {uri}"))?;
        let img = image::open(&path).map(|i| i.to_rgba8()).map_err(|e| e.to_string());
        let _ = std::fs::remove_file(&path);
        img
    }

    #[cfg(not(feature = "portal"))]
    fn portal(_: &Path) -> Result<RgbaImage, String> {
        Err("built without the portal feature".into())
    }

    pub fn x11_full() -> Result<RgbaImage, String> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{ConnectionExt, ImageFormat};
        if std::env::var_os("DISPLAY").is_none() {
            return Err("no DISPLAY".into());
        }
        let (conn, screen_num) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let screen = &conn.setup().roots[screen_num];
        let (w, h) = (screen.width_in_pixels, screen.height_in_pixels);
        let reply = conn
            .get_image(ImageFormat::Z_PIXMAP, screen.root, 0, 0, w, h, !0)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        if reply.depth != 24 && reply.depth != 32 {
            return Err(format!("unsupported color depth {}", reply.depth));
        }
        let px = w as usize * h as usize;
        if reply.data.len() < px * 4 {
            return Err("short GetImage reply".into());
        }
        let mut rgba = Vec::with_capacity(px * 4);
        for p in reply.data.as_chunks::<4>().0.iter().take(px) {
            rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
        }
        RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or_else(|| "bad buffer".into())
    }
}

#[cfg(windows)]
mod imp {
    use super::Capture;
    use image::RgbaImage;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    pub fn virtual_screen() -> (i32, i32, i32, i32) {
        unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        }
    }

    pub fn capture_full() -> Result<Capture, String> {
        let (x, y, w, h) = virtual_screen();
        gdi_capture(x, y, w, h).map(|img| Capture { img, origin: (x, y) })
    }

    pub fn gdi_capture(x: i32, y: i32, w: i32, h: i32) -> Result<RgbaImage, String> {
        if w <= 0 || h <= 0 {
            return Err("empty region".into());
        }
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            if screen.is_null() {
                return Err("GetDC failed".into());
            }
            let mem = CreateCompatibleDC(screen);
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bmp as HGDIOBJ);
            let ok = BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY | CAPTUREBLT);
            let mut buf = vec![0u8; (w * h * 4) as usize];
            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w;
            bmi.bmiHeader.biHeight = -h; // negative = top-down
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let lines = if ok != 0 {
                GetDIBits(mem, bmp, 0, h as u32, buf.as_mut_ptr().cast(), &mut bmi, DIB_RGB_COLORS)
            } else {
                0
            };
            SelectObject(mem, old);
            DeleteObject(bmp as HGDIOBJ);
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);
            if lines == 0 {
                return Err("BitBlt/GetDIBits failed".into());
            }
            for p in buf.as_chunks_mut::<4>().0 {
                p.swap(0, 2);
                p[3] = 255;
            }
            RgbaImage::from_raw(w as u32, h as u32, buf).ok_or_else(|| "bad buffer".into())
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    pub fn capture_full() -> Result<super::Capture, String> {
        Err("screen capture is not supported on this OS".into())
    }
}

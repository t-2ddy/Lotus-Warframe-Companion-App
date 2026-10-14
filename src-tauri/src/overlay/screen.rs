//! Finding the Warframe window and copying part of the screen over it.
//!
//! The copy is taken from the desktop (what's actually on screen), so it works for Borderless
//! and Windowed modes. Exclusive fullscreen can't be overlaid anyway. Nothing is sent to the game.

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, ClientToScreen, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetClientRect, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, SetForegroundWindow,
};

/// The game window's client area in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameWindow {
    pub hwnd: isize,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The largest visible top-level window of process `pid`.
pub fn find_game_window(pid: u32) -> Option<GameWindow> {
    struct Search {
        pid: u32,
        best: Option<(HWND, i64)>,
    }
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut owner = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut owner)) };
        if owner == search.pid && unsafe { IsWindowVisible(hwnd) }.as_bool() && !unsafe { IsIconic(hwnd) }.as_bool() {
            let mut r = RECT::default();
            if unsafe { GetClientRect(hwnd, &mut r) }.is_ok() {
                let area = i64::from(r.right - r.left) * i64::from(r.bottom - r.top);
                if search.best.is_none_or(|(_, a)| area > a) {
                    search.best = Some((hwnd, area));
                }
            }
        }
        BOOL(1)
    }
    let mut search = Search { pid, best: None };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut search as *mut Search as isize));
    }
    let (hwnd, _) = search.best?;
    let mut r = RECT::default();
    let mut origin = POINT::default();
    unsafe {
        GetClientRect(hwnd, &mut r).ok()?;
        ClientToScreen(hwnd, &mut origin).ok().ok()?;
    }
    let (width, height) = (r.right - r.left, r.bottom - r.top);
    (width > 200 && height > 200).then_some(GameWindow { hwnd: hwnd.0 as isize, x: origin.x, y: origin.y, width, height })
}

/// Gives the game the keyboard back (after the overlay was clickable).
pub fn focus(window: &GameWindow) {
    if window.hwnd != 0 {
        let _ = unsafe { SetForegroundWindow(HWND(window.hwnd as *mut _)) };
    }
}

/// Brings the game to the front when another window has focus. Windows refuses focus changes
/// from background apps, so this briefly shares input with the window in front (the usual way
/// round it). True if the game is in front afterwards.
pub fn bring_to_front(window: &GameWindow) -> bool {
    let hwnd = HWND(window.hwnd as *mut _);
    unsafe {
        let front = GetForegroundWindow();
        if front == hwnd {
            return true;
        }
        let front_thread = GetWindowThreadProcessId(front, None);
        let ours = GetCurrentThreadId();
        let attached = front_thread != 0 && front_thread != ours && AttachThreadInput(ours, front_thread, true).as_bool();
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        if attached {
            let _ = AttachThreadInput(ours, front_thread, false);
        }
        GetForegroundWindow() == hwnd
    }
}

/// The foreground window's program and title, e.g. `Discord.exe "#general - Discord"` (for
/// logging what took focus from the game).
pub fn foreground_owner() -> String {
    unsafe {
        let front = GetForegroundWindow();
        if front.0.is_null() {
            return "(none)".into();
        }
        let mut title = [0u16; 256];
        let n = GetWindowTextW(front, &mut title).max(0) as usize;
        let title = String::from_utf16_lossy(&title[..n]);
        let mut pid = 0u32;
        GetWindowThreadProcessId(front, Some(&mut pid));
        let mut exe = String::from("?");
        if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
                let path = String::from_utf16_lossy(&buf[..len as usize]);
                exe = path.rsplit('\\').next().unwrap_or(&path).to_string();
            }
            let _ = windows::Win32::Foundation::CloseHandle(process);
        }
        format!("{exe} {title:?}")
    }
}

/// The foreground window's handle (to tell whether our own overlay has focus).
pub fn foreground() -> isize {
    unsafe { GetForegroundWindow() }.0 as isize
}

/// A crop of the screen as tightly packed BGRA pixels, top row first.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// A part of the game window, as fractions of a 16:9 area centred in it (the game keeps its UI
/// in a centred 16:9 area on wider screens).
#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Region {
    /// Screen rectangle (x, y, w, h) of this region over `window`.
    pub fn on(&self, window: &GameWindow) -> (i32, i32, i32, i32) {
        let (ww, wh) = (f64::from(window.width), f64::from(window.height));
        let area_w = ww.min(wh * 16.0 / 9.0);
        let area_h = wh.min(ww * 9.0 / 16.0);
        let (ox, oy) = ((ww - area_w) / 2.0, (wh - area_h) / 2.0);
        (
            window.x + (ox + self.x * area_w).round() as i32,
            window.y + (oy + self.y * area_h).round() as i32,
            (self.w * area_w).round() as i32,
            (self.h * area_h).round() as i32,
        )
    }
}

/// Copies a screen rectangle.
pub fn grab(x: i32, y: i32, w: i32, h: i32) -> Option<Image> {
    if w <= 0 || h <= 0 {
        return None;
    }
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bitmap.into());
        let copied = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY).is_ok();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let rows = if copied {
            GetDIBits(mem, bitmap, 0, h as u32, Some(bgra.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS)
        } else {
            0
        };
        SelectObject(mem, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        // GDI leaves alpha at 0; OCR wants it opaque.
        for px in bgra.chunks_exact_mut(4) {
            px[3] = 255;
        }
        (rows == h).then_some(Image { width: w as u32, height: h as u32, bgra })
    }
}

/// A rectangle of an image (clamped to its edges). Tests crop saved screenshots with it.
#[cfg(test)]
pub fn crop(img: &Image, x: i32, y: i32, w: i32, h: i32) -> Image {
    let x0 = x.clamp(0, img.width as i32) as usize;
    let y0 = y.clamp(0, img.height as i32) as usize;
    let x1 = (x + w).clamp(0, img.width as i32) as usize;
    let y1 = (y + h).clamp(0, img.height as i32) as usize;
    let mut bgra = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
    for row in y0..y1 {
        let start = (row * img.width as usize + x0) * 4;
        bgra.extend_from_slice(&img.bgra[start..start + (x1 - x0) * 4]);
    }
    Image { width: (x1 - x0) as u32, height: (y1 - y0) as u32, bgra }
}

/// Riven card text is lilac. A grid of where the lilac pixels are, or `None` if under 3% of the
/// area is lilac (no riven card there). Costs well under a millisecond, so the card area can be
/// checked several times a second and OCR run only when this changes. Measured on the user's
/// screenshots: riven card areas ~9-10% lilac, game backgrounds and other menus 0%.
pub fn card_signature(img: &Image) -> Option<Vec<u8>> {
    const COLS: usize = 16;
    const ROWS: usize = 12;
    let (w, h) = (img.width as usize, img.height as usize);
    if w < COLS || h < ROWS {
        return None;
    }
    let mut cells = vec![0u32; COLS * ROWS];
    let (mut lilac, mut total) = (0u32, 0u32);
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let i = (y * w + x) * 4;
            let (b, g, r) = (img.bgra[i] as i32, img.bgra[i + 1] as i32, img.bgra[i + 2] as i32);
            total += 1;
            if r > 120 && b > 150 && g < r && b > g + 20 {
                lilac += 1;
                cells[(y * ROWS / h) * COLS + x * COLS / w] += 1;
            }
        }
    }
    if total == 0 || lilac * 100 < total * 3 {
        return None;
    }
    let per_cell = (total / (COLS * ROWS) as u32).max(1);
    Some(cells.iter().map(|&c| ((c * 255) / per_cell).min(255) as u8).collect())
}

/// Two signatures of the same card (allowing for small animation and noise).
pub fn signatures_match(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as u32).sum::<u32>() < 255 * 6
}

/// Saves an image as a 32-bit BMP (debug copies of what the overlay read).
pub fn save_bmp(img: &Image, path: &std::path::Path) -> std::io::Result<()> {
    let (w, h) = (img.width, img.height);
    let data_len = w * h * 4;
    let mut out = Vec::with_capacity(54 + data_len as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + data_len).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes()); // bottom-up rows
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for row in (0..h as usize).rev() {
        let start = row * w as usize * 4;
        out.extend_from_slice(&img.bgra[start..start + w as usize * 4]);
    }
    std::fs::write(path, out)
}

/// The card cleaned up a few different ways for OCR, each doubled in size, as dark text on
/// light: brightness stretched between the card's own background and text levels (two ways),
/// stretched over a fixed range, and simply inverted. OCR's slips differ between them ("+1 91
/// .1%" in one, "+191.10/0" in another), so reading all of them and taking the majority (see
/// `card::consensus`) beats any single read.
///
/// Measured on 70 game screenshots, also dimmed to 75/55/40% like a card fading in (the
/// regression set's screenshots, 2026-10-13): the fixed range reads 99% of numbers right on a
/// fully shown card but none at 40%; the card's own levels keep 92–97% at every brightness. These
/// four work out 271 of 280 reads, against 204 for the copies used before (as shown, inverted,
/// fixed range, brightest channel). The riven text is coloured like its icons (purple), so
/// keeping only the text's colour doesn't remove the icons; it lost most of the text instead.
pub fn ocr_variants(img: &Image) -> Vec<Image> {
    vec![
        upscale2(&own_levels(img, 950, 15, 100)),
        upscale2(&own_levels(img, 980, 20, 95)),
        upscale2(&grey(img, |r, g, b| stretch(lum(r, g, b), 40, 140))),
        upscale2(&grey(img, |r, g, b| (255 - lum(r, g, b)) as u8)),
    ]
}

fn lum(r: i32, g: i32, b: i32) -> i32 {
    (r * 299 + g * 587 + b * 114) / 1000
}

/// Brightness `v` from `low` to `low + span` as white to black (dark text on light).
fn stretch(v: i32, low: i32, span: i32) -> u8 {
    (255 - (v - low).clamp(0, span) * 255 / span) as u8
}

/// Brightness stretched between the card's background (the median) and its text (the `top`
/// per-mille brightest pixel), from `from` to `to` percent of the way. Follows a card that's
/// still fading in, where a fixed range sees nothing.
fn own_levels(img: &Image, top: usize, from: i32, to: i32) -> Image {
    let mut levels: Vec<i32> = img.bgra.chunks_exact(4).map(|p| lum(i32::from(p[2]), i32::from(p[1]), i32::from(p[0]))).collect();
    if levels.is_empty() {
        return grey(img, |r, g, b| (255 - lum(r, g, b)) as u8);
    }
    levels.sort_unstable();
    let background = levels[levels.len() / 2];
    let text = levels[(levels.len() * top / 1000).min(levels.len() - 1)];
    let range = (text - background).max(20);
    let low = background + range * from / 100;
    let span = (range * (to - from) / 100).max(10);
    grey(img, move |r, g, b| stretch(lum(r, g, b), low, span))
}

/// Each pixel mapped to a grey level from its red, green and blue.
fn grey(img: &Image, f: impl Fn(i32, i32, i32) -> u8) -> Image {
    let mut bgra = img.bgra.clone();
    for p in bgra.chunks_exact_mut(4) {
        let v = f(i32::from(p[2]), i32::from(p[1]), i32::from(p[0]));
        p.copy_from_slice(&[v, v, v, 255]);
    }
    Image { width: img.width, height: img.height, bgra }
}

/// Doubles the size with bilinear filtering (OCR reads the game's small card text much better).
pub fn upscale2(img: &Image) -> Image {
    let (w, h) = (img.width as usize, img.height as usize);
    let (nw, nh) = (w * 2, h * 2);
    let mut out = vec![0u8; nw * nh * 4];
    let px = |x: usize, y: usize, c: usize| f32::from(img.bgra[(y.min(h - 1) * w + x.min(w - 1)) * 4 + c]);
    for y in 0..nh {
        let fy = (y as f32 + 0.5) / 2.0 - 0.5;
        let y0 = fy.floor().max(0.0) as usize;
        let ty = (fy - y0 as f32).clamp(0.0, 1.0);
        for x in 0..nw {
            let fx = (x as f32 + 0.5) / 2.0 - 0.5;
            let x0 = fx.floor().max(0.0) as usize;
            let tx = (fx - x0 as f32).clamp(0.0, 1.0);
            for c in 0..4 {
                let top = px(x0, y0, c) * (1.0 - tx) + px(x0 + 1, y0, c) * tx;
                let bottom = px(x0, y0 + 1, c) * (1.0 - tx) + px(x0 + 1, y0 + 1, c) * tx;
                out[(y * nw + x) * 4 + c] = (top * (1.0 - ty) + bottom * ty).round() as u8;
            }
        }
    }
    Image { width: nw as u32, height: nh as u32, bgra: out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_use_a_centred_16_9_area() {
        let r = Region { x: 0.5, y: 0.5, w: 0.1, h: 0.1 };
        let normal = GameWindow { hwnd: 0, x: 0, y: 0, width: 2560, height: 1440 };
        assert_eq!(r.on(&normal), (1280, 720, 256, 144));
        // 21:9: the UI area is 2560 wide in the middle of 3440.
        let wide = GameWindow { hwnd: 0, x: 100, y: 0, width: 3440, height: 1440 };
        assert_eq!(r.on(&wide), (100 + 440 + 1280, 720, 256, 144));
    }

    #[test]
    fn upscale_doubles_and_keeps_flat_colour() {
        let img = Image { width: 2, height: 2, bgra: [10u8, 20, 30, 255].repeat(4) };
        let big = upscale2(&img);
        assert_eq!((big.width, big.height), (4, 4));
        assert!(big.bgra.chunks(4).all(|p| p == [10, 20, 30, 255]));
    }
}

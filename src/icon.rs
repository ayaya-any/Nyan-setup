//! Application icon handling - the single source of truth for every icon in the project.
//!
//! * `resources/app.ico` is embedded into the stub `.exe` through `resources/app.rc`
//!   (resource group `1`). It is the file/taskbar icon *and* - read back through the very same
//!   resource - the icon painted inside the custom Direct2D UI (title bar, wizard banners).
//! * When a custom installer icon is requested at pack time, [`apply_icon_to_stub_bytes`]
//!   swaps that resource group inside the stub before the payload is appended. Because the
//!   uninstaller (`unins000.exe`) is a byte copy of the same stub, installer, uninstaller and
//!   their UI all pick the custom icon up automatically.
//! * `resources/app.ico` is also compiled in as a fallback for binaries built without the
//!   Windows resource compiler.

use std::ffi::c_void;
use std::fs;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::System::LibraryLoader::{
    BeginUpdateResourceW, EndUpdateResourceW, FindResourceW, GetModuleHandleW, LoadLibraryExW,
    LoadResource, LockResource, SizeofResource, UpdateResourceW, LOAD_LIBRARY_AS_DATAFILE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, DestroyIcon, GetIconInfo, LoadImageW, HICON, ICONINFO, IMAGE_ICON,
    LR_DEFAULTCOLOR, RT_GROUP_ICON, RT_ICON,
};

/// Built-in default icon (identical to the one the `.rc` embeds).
pub static APP_ICON_ICO: &[u8] = include_bytes!("../resources/app.ico");

/// Resource language used by `resources/app.rc` (`LANGUAGE 9, 1` = en-US).
const RES_LANG: u16 = 0x0409;
/// Resource id of the icon group (`1 ICON ...` in `app.rc`).
const ICON_GROUP_ID: usize = 1;
/// Upper bound of `RT_ICON` ids that are purged before a custom icon is written.

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Frame sizes the UI may ask for.
const UI_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

pub struct IconImage {
    pub size: u32,
    /// Top-down, premultiplied BGRA.
    pub bgra: Vec<u8>,
    /// Factor by which the bitmap must be enlarged (around its centre) so that the visible
    /// artwork - ignoring transparent padding/shadow - exactly fills the destination rect.
    pub art_scale: f32,
}

/// One image of an `.ico` file (either a BMP/DIB or a PNG frame, stored as in the file).
pub struct IcoFrame {
    pub width: u32,
    pub height: u32,
    pub color_count: u8,
    pub planes: u16,
    pub bit_count: u16,
    pub data: Vec<u8>,
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

/// Parse and validate an `.ico` file.
pub fn parse_ico(bytes: &[u8]) -> Result<Vec<IcoFrame>, String> {
    if bytes.len() < 6 || u16_at(bytes, 0) != Some(0) || u16_at(bytes, 2) != Some(1) {
        return Err("不是有效的 .ico 图标文件".to_string());
    }
    let count = u16_at(bytes, 4).unwrap_or(0) as usize;
    if count == 0 || count > 128 {
        return Err("图标文件不包含任何图像".to_string());
    }

    let mut frames = Vec::with_capacity(count);
    for i in 0..count {
        let e = 6 + 16 * i;
        let bad = || format!("图标文件已损坏 (第 {} 个图像)", i + 1);
        let entry = bytes.get(e..e + 16).ok_or_else(bad)?;
        let size = u32_at(bytes, e + 8).ok_or_else(bad)? as usize;
        let off = u32_at(bytes, e + 12).ok_or_else(bad)? as usize;
        let end = off.checked_add(size).ok_or_else(bad)?;
        let data = bytes.get(off..end).filter(|d| !d.is_empty()).ok_or_else(bad)?;
        if !(data.starts_with(PNG_SIGNATURE) || u32_at(data, 0) == Some(40)) {
            return Err(format!("图标文件第 {} 个图像格式不受支持", i + 1));
        }
        let dim = |b: u8| if b == 0 { 256 } else { b as u32 };
        let is_png = data.starts_with(PNG_SIGNATURE);
        frames.push(IcoFrame {
            width: dim(entry[0]),
            height: dim(entry[1]),
            color_count: entry[2],
            planes: if entry[4] == 0 && entry[5] == 0 && is_png { 1 } else { u16_at(bytes, e + 4).unwrap_or(1) },
            bit_count: if is_png && u16_at(bytes, e + 6) == Some(0) { 32 } else { u16_at(bytes, e + 6).unwrap_or(32) },
            data: data.to_vec(),
        });
    }
    Ok(frames)
}

/// Smallest standard frame size that is at least `target_px` wide, or the largest one.
pub fn pick_size(target_px: f32) -> u32 {
    UI_SIZES
        .iter()
        .copied()
        .find(|s| *s as f32 >= target_px - 0.5)
        .unwrap_or(UI_SIZES[UI_SIZES.len() - 1])
}

// ---------------------------------------------------------------------------------------------
// Decoding (HICON -> premultiplied BGRA)
// ---------------------------------------------------------------------------------------------

unsafe fn read_bits(hdc: HDC, hbm: HBITMAP, w: i32, h: i32) -> Option<Vec<u8>> {
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let lines = GetDIBits(
        hdc,
        hbm,
        0,
        h as u32,
        Some(buf.as_mut_ptr() as *mut c_void),
        &mut bmi,
        DIB_RGB_COLORS,
    );
    if lines == 0 {
        None
    } else {
        Some(buf)
    }
}

fn hicon_to_image(hicon: HICON) -> Option<IconImage> {
    unsafe {
        let mut info = ICONINFO::default();
        GetIconInfo(hicon, &mut info).ok()?;

        let result = (|| {
            if info.hbmColor.is_invalid() {
                return None;
            }
            let mut bm = BITMAP::default();
            let got = GetObjectW(
                HGDIOBJ(info.hbmColor.0),
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut _ as *mut c_void),
            );
            let (w, h) = (bm.bmWidth, bm.bmHeight);
            if got == 0 || w <= 0 || w != h {
                return None;
            }

            let hdc = GetDC(None);
            let mut bgra = read_bits(hdc, info.hbmColor, w, h);
            if let Some(ref mut px) = bgra {
                // Icons without an alpha channel carry their transparency in the AND mask.
                if px.chunks_exact(4).all(|p| p[3] == 0) {
                    if let Some(mask) = read_bits(hdc, info.hbmMask, w, h) {
                        for (p, m) in px.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
                            p[3] = if m[0] == 0 { 255 } else { 0 };
                        }
                    }
                }
            }
            ReleaseDC(None, hdc);
            let mut bgra = bgra?;

            // Premultiply + measure the visible artwork.
            let (mut min_x, mut min_y, mut max_x, mut max_y) = (w, h, -1, -1);
            for y in 0..h {
                for x in 0..w {
                    let i = ((y * w + x) * 4) as usize;
                    let a = bgra[i + 3] as u32;
                    for c in 0..3 {
                        bgra[i + c] = ((bgra[i + c] as u32 * a + 127) / 255) as u8;
                    }
                    if a >= 128 {
                        min_x = min_x.min(x);
                        max_x = max_x.max(x);
                        min_y = min_y.min(y);
                        max_y = max_y.max(y);
                    }
                }
            }
            let art_scale = if max_x >= min_x && max_y >= min_y {
                let art = ((max_x - min_x + 1).max(max_y - min_y + 1)) as f32;
                (w as f32 / art).clamp(1.0, 1.35)
            } else {
                1.0
            };

            Some(IconImage {
                size: w as u32,
                bgra,
                art_scale,
            })
        })();

        let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        result
    }
}

/// Build an `HICON` of `size`x`size` from the best matching frame of an `.ico` file.
fn hicon_from_ico(ico: &[u8], size: u32) -> Option<HICON> {
    let frames = parse_ico(ico).ok()?;
    let best = frames
        .iter()
        .filter(|f| f.width >= size)
        .min_by_key(|f| f.width)
        .or_else(|| frames.iter().max_by_key(|f| f.width))?;
    unsafe {
        CreateIconFromResourceEx(
            &best.data,
            true,
            0x0003_0000,
            size as i32,
            size as i32,
            LR_DEFAULTCOLOR,
        )
        .ok()
    }
}

/// Icon (resource group `1`) of the running executable, scaled by Windows to `size`.
fn hicon_from_module(size: u32) -> Option<HICON> {
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let handle = LoadImageW(
            module,
            PCWSTR(ICON_GROUP_ID as *const u16),
            IMAGE_ICON,
            size as i32,
            size as i32,
            LR_DEFAULTCOLOR,
        )
        .ok()?;
        Some(HICON(handle.0))
    }
}

fn finish(hicon: HICON) -> Option<IconImage> {
    let img = hicon_to_image(hicon);
    unsafe {
        let _ = DestroyIcon(hicon);
    }
    img
}

/// Decode the running executable's own icon at `size` px (embedded resource, built-in fallback).
pub fn decode(size: u32) -> Option<IconImage> {
    hicon_from_module(size)
        .and_then(finish)
        .or_else(|| decode_ico(APP_ICON_ICO, size))
}

/// Decode an arbitrary `.ico` file image at `size` px (used for the Studio icon preview).
pub fn decode_ico(ico: &[u8], size: u32) -> Option<IconImage> {
    hicon_from_ico(ico, size).and_then(finish)
}

// ---------------------------------------------------------------------------------------------
// Applying a custom icon to an executable
// ---------------------------------------------------------------------------------------------

fn get_existing_icon_ids(exe_path: &Path) -> Vec<u16> {
    let path_w = crate::win32_ops::to_wide(&exe_path.to_string_lossy());
    unsafe {
        if let Ok(module) = LoadLibraryExW(PCWSTR(path_w.as_ptr()), None, LOAD_LIBRARY_AS_DATAFILE) {
            let res = FindResourceW(module, PCWSTR(ICON_GROUP_ID as *const u16), RT_GROUP_ICON);
            if !res.is_invalid() {
                if let Ok(mem) = LoadResource(module, res) {
                    let ptr = LockResource(mem) as *const u8;
                    let len = SizeofResource(module, res) as usize;
                    if len >= 6 && !ptr.is_null() {
                        let data = std::slice::from_raw_parts(ptr, len);
                        let count = u16::from_le_bytes([data[4], data[5]]) as usize;
                        let mut ids = Vec::with_capacity(count);
                        for i in 0..count {
                            let entry_off = 6 + 14 * i;
                            if entry_off + 14 <= len {
                                let id = u16::from_le_bytes([data[entry_off + 12], data[entry_off + 13]]);
                                ids.push(id);
                            }
                        }
                        let _ = FreeLibrary(module);
                        return ids;
                    }
                }
            }
            let _ = FreeLibrary(module);
        }
    }
    Vec::new()
}

/// Replace the icon group of the PE file at `exe_path` with the frames of `ico`.
pub fn apply_icon_to_exe(exe_path: &Path, ico: &[u8]) -> Result<(), String> {
    let frames = parse_ico(ico)?;
    let old_ids = get_existing_icon_ids(exe_path);

    // GRPICONDIR: header + one 14-byte entry per frame (frame data lives in RT_ICON ids 1..=n).
    let mut group = Vec::with_capacity(6 + 14 * frames.len());
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&(frames.len() as u16).to_le_bytes());
    for (i, f) in frames.iter().enumerate() {
        group.push(if f.width >= 256 { 0 } else { f.width as u8 });
        group.push(if f.height >= 256 { 0 } else { f.height as u8 });
        group.push(f.color_count);
        group.push(0);
        group.extend_from_slice(&f.planes.to_le_bytes());
        group.extend_from_slice(&f.bit_count.to_le_bytes());
        group.extend_from_slice(&(f.data.len() as u32).to_le_bytes());
        group.extend_from_slice(&((i + 1) as u16).to_le_bytes());
    }

    let path_w = crate::win32_ops::to_wide(&exe_path.to_string_lossy());
    let id = |n: usize| PCWSTR(n as *const u16);

    unsafe {
        let handle = BeginUpdateResourceW(PCWSTR(path_w.as_ptr()), false)
            .map_err(|e| format!("无法打开 {} 以更新图标资源: {}", exe_path.display(), e))?;

        let write = || -> Result<(), String> {
            for (i, f) in frames.iter().enumerate() {
                UpdateResourceW(
                    handle,
                    RT_ICON,
                    id(i + 1),
                    RES_LANG,
                    Some(f.data.as_ptr() as *const c_void),
                    f.data.len() as u32,
                )
                .map_err(|e| format!("写入图标图像资源失败: {}", e))?;
            }
            UpdateResourceW(
                handle,
                RT_GROUP_ICON,
                id(ICON_GROUP_ID),
                RES_LANG,
                Some(group.as_ptr() as *const c_void),
                group.len() as u32,
            )
            .map_err(|e| format!("写入图标组资源失败: {}", e))?;

            // Only purge old icon IDs that actually existed and are not overwritten by new frames
            for old_id in old_ids {
                if (old_id as usize) > frames.len() {
                    let _ = UpdateResourceW(handle, RT_ICON, id(old_id as usize), RES_LANG, None, 0);
                }
            }
            Ok(())
        };

        match write() {
            Ok(()) => EndUpdateResourceW(handle, false)
                .map_err(|e| format!("提交图标资源失败: {}", e)),
            Err(e) => {
                let _ = EndUpdateResourceW(handle, true);
                Err(e)
            }
        }
    }
}

/// Return a copy of the PE image `stub` whose application icon is replaced by `ico`.
pub fn apply_icon_to_stub_bytes(stub: &[u8], ico: &[u8]) -> Result<Vec<u8>, String> {
    parse_ico(ico)?; // fail early, before touching the disk

    let tmp = std::env::temp_dir().join(format!(
        "nyan_stub_{}_{}.exe",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::write(&tmp, stub).map_err(|e| format!("写入临时 Stub 失败: {}", e))?;
    let result = apply_icon_to_exe(&tmp, ico)
        .and_then(|_| fs::read(&tmp).map_err(|e| format!("读取已更新图标的 Stub 失败: {}", e)));
    let _ = fs::remove_file(&tmp);
    result
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{
        FindResourceW, LoadLibraryExW, LoadResource, LockResource, SizeofResource,
        LOAD_LIBRARY_AS_DATAFILE,
    };

    /// Minimal valid single-frame 16x16 32bpp BMP/DIB `.ico` filled with one colour.
    pub fn make_test_ico(b: u8, g: u8, r: u8) -> Vec<u8> {
        let s = 16usize;
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&(s as i32).to_le_bytes());
        dib.extend_from_slice(&((s * 2) as i32).to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&[0u8; 24]);
        for _ in 0..s * s {
            dib.extend_from_slice(&[b, g, r, 255]);
        }
        dib.extend_from_slice(&vec![0u8; 4 * s]);

        let mut ico = Vec::new();
        ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
        ico.extend_from_slice(&[s as u8, s as u8, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(dib.len() as u32).to_le_bytes());
        ico.extend_from_slice(&22u32.to_le_bytes());
        ico.extend_from_slice(&dib);
        ico
    }

    /// (group resource bytes, RT_ICON #1 bytes, RT_ICON #2 exists) of an exe on disk.
    pub fn read_icon_resources(path: &Path) -> (Vec<u8>, Vec<u8>, bool) {
        let path_w = crate::win32_ops::to_wide(&path.to_string_lossy());
        unsafe {
            let module =
                LoadLibraryExW(PCWSTR(path_w.as_ptr()), None, LOAD_LIBRARY_AS_DATAFILE).unwrap();
            let get = |ty: PCWSTR, n: usize| -> Option<Vec<u8>> {
                let res = FindResourceW(module, PCWSTR(n as *const u16), ty);
                if res.is_invalid() {
                    return None;
                }
                let len = SizeofResource(module, res) as usize;
                let mem = LoadResource(module, res).ok()?;
                let ptr = LockResource(mem) as *const u8;
                Some(std::slice::from_raw_parts(ptr, len).to_vec())
            };
            let group = get(RT_GROUP_ICON, 1).unwrap_or_default();
            let first = get(RT_ICON, 1).unwrap_or_default();
            let second = get(RT_ICON, 2).is_some();
            let _ = FreeLibrary(module);
            (group, first, second)
        }
    }

    #[test]
    fn default_icon_decodes() {
        let frames = parse_ico(APP_ICON_ICO).expect("default ico parses");
        assert_eq!(frames.len(), 7);
        for sz in UI_SIZES {
            let img = decode(sz).expect("icon decodes");
            assert_eq!(img.size, sz);
            assert_eq!(img.bgra.len(), (sz * sz * 4) as usize);
            let c = ((sz / 2 * sz + sz / 2) * 4 + 3) as usize;
            assert_eq!(img.bgra[c], 255, "centre pixel opaque at {}px", sz);
        }
        // The built-in squircle sits inside transparent padding, so it must be enlarged.
        let big = decode(128).unwrap();
        assert!(big.art_scale > 1.05 && big.art_scale < 1.3, "{}", big.art_scale);
        assert_eq!(pick_size(28.0), 32);
        assert_eq!(pick_size(33.0), 48);
        assert_eq!(pick_size(1000.0), 256);
    }

    #[test]
    fn custom_ico_is_validated_and_decoded() {
        assert!(parse_ico(b"nope").is_err());
        assert!(parse_ico(&[0, 0, 1, 0, 0, 0]).is_err());
        let ico = make_test_ico(0, 0, 255); // red
        assert_eq!(parse_ico(&ico).unwrap().len(), 1);
        let img = decode_ico(&ico, 16).expect("custom ico decodes");
        let px = &img.bgra[((8 * 16 + 8) * 4)..][..4];
        assert_eq!(px, &[0, 0, 255, 255]);
        assert!((img.art_scale - 1.0).abs() < 0.01);
    }

    #[test]
    fn icon_resources_are_replaced_in_stub() {
        let exe = std::env::current_exe().unwrap();
        let stub = fs::read(&exe).unwrap();
        let ico = make_test_ico(10, 20, 30);
        let patched = apply_icon_to_stub_bytes(&stub, &ico).expect("icon applied");
        assert_ne!(patched, stub);

        let tmp = std::env::temp_dir().join(format!("nyan_icon_test_{}.exe", std::process::id()));
        fs::write(&tmp, &patched).unwrap();
        let (group, first, second_exists) = read_icon_resources(&tmp);
        let _ = fs::remove_file(&tmp);

        let frames = parse_ico(&ico).unwrap();
        assert_eq!(u16::from_le_bytes([group[4], group[5]]), 1, "one frame in group");
        assert_eq!(first, frames[0].data, "RT_ICON #1 equals custom frame");
        assert!(!second_exists, "stale frames of the old icon were removed");
    }
}

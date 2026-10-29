use eframe::egui::{self, ColorImage, TextureHandle, TextureId};
use std::{
    collections::HashMap,
    path::{Component, Path, Prefix},
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};

const SIDE: usize = 32;
const CACHE_LIMIT: usize = 512;
const QUEUE_LIMIT: usize = 128;
const RETRY_AFTER: Duration = Duration::from_secs(60);

enum CachedIcon {
    Pending,
    Missing(Instant),
    Ready(TextureHandle),
}
struct Entry {
    icon: CachedIcon,
    last_used: u64,
}

/// Icon extraction is performed off the UI and collector threads. Only visible
/// rows request icons; both the work queue and GPU texture cache are bounded.
pub struct IconCache {
    requests: Option<SyncSender<(String, egui::Context)>>,
    results: Option<Receiver<(String, Option<Vec<u8>>)>>,
    entries: HashMap<String, Entry>,
    access: u64,
}
impl Default for IconCache {
    fn default() -> Self {
        Self::new()
    }
}
impl IconCache {
    pub fn new() -> Self {
        let (requests, jobs) = mpsc::sync_channel::<(String, egui::Context)>(QUEUE_LIMIT);
        // At most QUEUE_LIMIT jobs can wait plus one processing, so results have
        // room even if the UI becomes hidden before it can poll them.
        let (results_tx, results) = mpsc::sync_channel(QUEUE_LIMIT + 1);
        let _ = std::thread::Builder::new()
            .name("app-icons".into())
            .spawn(move || {
                while let Ok((path, ctx)) = jobs.recv() {
                    let icon = extract_icon(&path);
                    if results_tx.send((path, icon)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        Self {
            requests: Some(requests),
            results: Some(results),
            entries: HashMap::new(),
            access: 0,
        }
    }
    #[cfg(any(test, feature = "ui-preview"))]
    pub fn disabled() -> Self {
        Self {
            requests: None,
            results: None,
            entries: HashMap::new(),
            access: 0,
        }
    }
    /// Install generated sample art without reading executables in UI previews.
    #[cfg(any(test, feature = "ui-preview"))]
    pub fn insert_preview(&mut self, ctx: &egui::Context, path: &str, image: ColorImage) {
        let texture = ctx.load_texture(
            format!("app-icon-preview:{path}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.insert(path.to_owned(), CachedIcon::Ready(texture));
    }
    fn insert(&mut self, path: String, icon: CachedIcon) {
        if !self.entries.contains_key(&path)
            && self.entries.len() >= CACHE_LIMIT
            && let Some(old) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(p, _)| p.clone())
        {
            self.entries.remove(&old);
        }
        self.entries.insert(
            path,
            Entry {
                icon,
                last_used: self.access,
            },
        );
    }
    /// Returns a cached texture, or schedules a background read and returns None.
    pub fn texture(&mut self, ctx: &egui::Context, path: &str) -> Option<TextureId> {
        self.access = self.access.wrapping_add(1);
        while let Some((path, icon)) = self.results.as_ref().and_then(|rx| rx.try_recv().ok()) {
            if !self.entries.contains_key(&path) {
                continue;
            }
            let icon = match icon {
                Some(rgba) => CachedIcon::Ready(ctx.load_texture(
                    format!("app-icon:{path}"),
                    ColorImage::from_rgba_unmultiplied([SIDE, SIDE], &rgba),
                    egui::TextureOptions::LINEAR,
                )),
                None => CachedIcon::Missing(Instant::now()),
            };
            self.insert(path, icon);
            ctx.request_repaint();
        }
        if let Some(entry) = self.entries.get_mut(path) {
            entry.last_used = self.access;
            match &entry.icon {
                CachedIcon::Ready(texture) => return Some(texture.id()),
                CachedIcon::Pending => return None,
                CachedIcon::Missing(at) if at.elapsed() < RETRY_AFTER => return None,
                CachedIcon::Missing(_) => {}
            }
            self.entries.remove(path);
        }
        if !valid_executable_path(path) {
            return None;
        }
        if self
            .requests
            .as_ref()
            .is_some_and(|tx| tx.try_send((path.to_owned(), ctx.clone())).is_ok())
        {
            self.insert(path.to_owned(), CachedIcon::Pending);
            // Keep egui ticking until the background result is drained, including
            // while network snapshots are not otherwise repainting this view.
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        None
    }
}
fn valid_executable_path(path: &str) -> bool {
    let path = Path::new(path);
    path.is_absolute()
        && matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

fn extract_icon(path: &str) -> Option<Vec<u8>> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
            DeleteObject, GdiFlush, SelectObject,
        },
        UI::{
            Shell::ExtractIconExW,
            WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx},
        },
    };
    let wide: Vec<u16> = std::ffi::OsStr::new(path)
        .encode_wide()
        .chain(Some(0))
        .collect();
    if wide[..wide.len() - 1].contains(&0) {
        return None;
    }
    let mut icon = ptr::null_mut();
    // SAFETY: the path is NUL-terminated; the output receives at most one icon.
    if unsafe { ExtractIconExW(wide.as_ptr(), 0, &mut icon, ptr::null_mut(), 1) } == 0
        || icon.is_null()
    {
        return None;
    }
    // SAFETY: a null compatible-DC source creates an owned memory DC.
    let dc = unsafe { CreateCompatibleDC(ptr::null_mut()) };
    if dc.is_null() {
        // SAFETY: ExtractIconExW returned an owned HICON.
        unsafe {
            DestroyIcon(icon);
        }
        return None;
    }
    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = std::mem::size_of_val(&info.bmiHeader) as u32;
    info.bmiHeader.biWidth = SIDE as i32;
    info.bmiHeader.biHeight = -(SIDE as i32);
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    let mut bits = ptr::null_mut::<c_void>();
    // SAFETY: info and bits are valid; a top-down 32-bit DIB owns SIDE*SIDE pixels.
    let bitmap =
        unsafe { CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, ptr::null_mut(), 0) };
    if bitmap.is_null() || bits.is_null() {
        // SAFETY: the handles are owned and non-null checks guard optional bitmap deletion.
        unsafe {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            DeleteDC(dc);
            DestroyIcon(icon);
        }
        return None;
    }
    // SAFETY: both handles are valid and SelectObject returns the old bitmap for restoration.
    let old = unsafe { SelectObject(dc, bitmap) };
    if old.is_null() || old as isize == -1 {
        // SAFETY: selection failed, so deleting our bitmap and DC is safe.
        unsafe {
            DeleteObject(bitmap);
            DeleteDC(dc);
            DestroyIcon(icon);
        }
        return None;
    }
    let render = |background: u8| -> Option<Vec<u8>> {
        {
            // SAFETY: bits belongs to the selected 32-bit DIB, sized exactly SIDE*SIDE*4.
            let pixels =
                unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), SIDE * SIDE * 4) };
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.copy_from_slice(&[background, background, background, 255]);
            }
        }
        // SAFETY: dc owns a valid DIB, icon is valid, and dimensions are within the DIB.
        if unsafe {
            DrawIconEx(
                dc,
                0,
                0,
                icon,
                SIDE as i32,
                SIDE as i32,
                0,
                ptr::null_mut(),
                DI_NORMAL,
            )
        } == 0
        {
            return None;
        }
        // SAFETY: flush this thread's GDI drawing batch before reading DIB pixels.
        unsafe {
            GdiFlush();
        }
        // SAFETY: drawing has completed; the DIB remains allocated and selected.
        Some(unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), SIDE * SIDE * 4) }.to_vec())
    };
    let black = render(0);
    let white = render(255);
    // SAFETY: restore the original selected object before freeing all owned handles.
    unsafe {
        SelectObject(dc, old);
        DeleteObject(bitmap);
        DeleteDC(dc);
        DestroyIcon(icon);
    }
    Some(composite_to_rgba(&black?, &white?))
}

/// Recover alpha from black/white composites, including old mask-only Windows icons.
fn composite_to_rgba(black: &[u8], white: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(black.len());
    for (b, w) in black
        .as_chunks::<4>()
        .0
        .iter()
        .zip(white.as_chunks::<4>().0.iter())
    {
        let delta = (0..3)
            .map(|i| i32::from(w[i]) - i32::from(b[i]))
            .sum::<i32>()
            / 3;
        let alpha = (255 - delta).clamp(0, 255) as u8;
        let channel = |c: u8| {
            if alpha == 0 {
                0
            } else {
                ((u32::from(c) * 255 + u32::from(alpha) / 2) / u32::from(alpha)).min(255) as u8
            }
        };
        rgba.extend([channel(b[2]), channel(b[1]), channel(b[0]), alpha]);
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_or_relative_process_paths_do_not_trigger_executable_search() {
        for bad in [
            "",
            "chrome.exe",
            "Process 123",
            r"\\server\share\browser.exe",
            r"C:\Apps\data.txt",
        ] {
            assert!(!valid_executable_path(bad), "{bad}");
        }
        assert!(valid_executable_path(r"C:\Apps\Browser.EXE"));
    }
    #[test]
    fn composite_reconstruction_preserves_transparency_and_channels() {
        assert_eq!(
            composite_to_rgba(&[0, 0, 0, 255], &[255, 255, 255, 255]),
            [0, 0, 0, 0]
        );
        assert_eq!(
            composite_to_rgba(&[30, 20, 10, 255], &[30, 20, 10, 255]),
            [10, 20, 30, 255]
        );
        assert_eq!(
            composite_to_rgba(&[0, 0, 128, 255], &[127, 127, 255, 255]),
            [255, 0, 0, 128]
        );
    }
    #[test]
    #[ignore = "Reads the local Windows Notepad executable icon"]
    fn windows_executable_icon_contains_visible_pixels() {
        let path = r"C:\Windows\System32\notepad.exe";
        let rgba = extract_icon(path).expect("Could not extract Notepad icon");
        assert_eq!(rgba.len(), SIDE * SIDE * 4);
        assert!(rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
        assert!(rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 0));
        // Verify the UI-facing asynchronous path produces a persistent texture.
        let ctx = egui::Context::default();
        let mut cache = IconCache::new();
        assert!(cache.texture(&ctx, path).is_none());
        let start = Instant::now();
        let texture = loop {
            if let Some(texture) = cache.texture(&ctx, path) {
                break texture;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "Icon worker did not return a texture"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(cache.texture(&ctx, path), Some(texture));
    }
}

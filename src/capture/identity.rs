use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    path::Path,
    ptr,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};

#[derive(Clone)]
struct CachedLabel {
    modified: Option<SystemTime>,
    length: u64,
    label: String,
}

/// Keep the executable path as identity and use its Windows resource strings for display.
/// Metadata is read once per executable version, even when many browser processes share it.
pub fn process_labels(paths: &HashMap<u32, String>) -> HashMap<String, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedLabel>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache) = cache.lock() else {
        return paths
            .values()
            .map(|p| (p.clone(), fallback_label(p)))
            .collect();
    };
    let unique: HashSet<&String> = paths.values().collect();
    // Closed executables do not make the cache grow forever; the worker retains display labels.
    if cache.len() > 2048 {
        cache.retain(|path, _| unique.contains(path));
    }
    let mut result = HashMap::with_capacity(unique.len());
    for path in unique {
        let metadata = std::fs::metadata(path).ok();
        let modified = metadata.as_ref().and_then(|m| m.modified().ok());
        let length = metadata.as_ref().map_or(0, |m| m.len());
        if let Some(value) = cache.get(path)
            && value.modified == modified
            && value.length == length
        {
            result.insert(path.clone(), value.label.clone());
            continue;
        }
        let label = executable_label(path).unwrap_or_else(|| fallback_label(path));
        if cache.len() < 2048 {
            cache.insert(
                path.clone(),
                CachedLabel {
                    modified,
                    length,
                    label: label.clone(),
                },
            );
        }
        result.insert(path.clone(), label);
    }
    result
}

fn fallback_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|p| p.to_str())
        .unwrap_or(path)
        .to_owned()
}

fn choose_label(description: Option<String>, product: Option<String>) -> Option<String> {
    let description = description.filter(|s| !s.trim().is_empty());
    let product = product.filter(|s| !s.trim().is_empty());
    // Windows descriptions identify helpers/services better than an umbrella product name.
    // A branded browser product is more useful than a generic Chromium description.
    let generic = description.as_ref().is_none_or(|d| {
        matches!(
            d.trim().to_lowercase().as_str(),
            "chromium" | "chromium browser" | "chrome.exe" | "application"
        )
    });
    if generic {
        product.or(description)
    } else {
        description.or(product)
    }
}

fn executable_label(path: &str) -> Option<String> {
    // Relative names from protected processes must not be searched through the DLL search path.
    if !Path::new(path).is_absolute() || path.contains('\0') {
        return None;
    }
    let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    // SAFETY: path is terminated and the unused handle out parameter may be null.
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), ptr::null_mut()) };
    if size == 0 || size > 16 * 1024 * 1024 {
        return None;
    }
    let mut block = vec![0u32; (size as usize).div_ceil(4)];
    // SAFETY: the initialized DWORD storage is aligned and has at least size writable bytes.
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, block.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    let data = &block;
    let mut translations = Vec::<(u16, u16)>::new();
    if let Some((pointer, length)) = query(data, "\\VarFileInfo\\Translation") {
        let length = length as usize;
        if inside(data, pointer, length) {
            // SAFETY: the translation bytes lie inside the retained version resource block.
            let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), length) };
            for pair in bytes.as_chunks::<4>().0 {
                translations.push((
                    u16::from_le_bytes([pair[0], pair[1]]),
                    u16::from_le_bytes([pair[2], pair[3]]),
                ));
            }
        }
    }
    // Some executables omit Translation even though they contain an English string table.
    for fallback in [(0x0409, 0x04b0), (0x0409, 0x04e4), (0x0409, 0)] {
        if !translations.contains(&fallback) {
            translations.push(fallback);
        }
    }
    for (language, codepage) in translations {
        let prefix = format!("\\StringFileInfo\\{language:04x}{codepage:04x}\\");
        let description = string_value(data, &(prefix.clone() + "FileDescription"));
        let product = string_value(data, &(prefix + "ProductName"));
        if let Some(label) = choose_label(description, product) {
            return Some(label);
        }
    }
    None
}

fn query(block: &[u32], key: &str) -> Option<(*mut c_void, u32)> {
    let key: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
    let mut value = ptr::null_mut();
    let mut length = 0;
    // SAFETY: block came from GetFileVersionInfoW and remains alive; key is terminated and out pointers are valid.
    if unsafe { VerQueryValueW(block.as_ptr().cast(), key.as_ptr(), &mut value, &mut length) } == 0
        || value.is_null()
    {
        return None;
    }
    Some((value, length))
}
fn inside(block: &[u32], pointer: *mut c_void, bytes: usize) -> bool {
    let start = block.as_ptr() as usize;
    let address = pointer as usize;
    address >= start
        && address
            .checked_add(bytes)
            .is_some_and(|end| end <= start + std::mem::size_of_val(block))
}
fn string_value(block: &[u32], key: &str) -> Option<String> {
    let (pointer, length) = query(block, key)?;
    let bytes = (length as usize).checked_mul(2)?;
    if !inside(block, pointer, bytes) {
        return None;
    }
    // SAFETY: VerQueryValueW returns a UTF-16 string; unaligned reads avoid assuming resource alignment.
    let characters: Vec<u16> = (0..length as usize)
        .map(|n| unsafe { ptr::read_unaligned(pointer.cast::<u8>().add(n * 2).cast::<u16>()) })
        .take_while(|c| *c != 0)
        .collect();
    let value = String::from_utf16_lossy(&characters).trim().to_owned();
    if value.is_empty() { None } else { Some(value) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn branded_product_replaces_generic_chromium_description() {
        assert_eq!(
            choose_label(Some("Chromium".into()), Some("Helium".into())).as_deref(),
            Some("Helium")
        );
        assert_eq!(
            choose_label(Some("Helium".into()), Some("Helium".into())).as_deref(),
            Some("Helium")
        );
        assert_eq!(
            choose_label(
                Some("Browser Update Service".into()),
                Some("Browser".into())
            )
            .as_deref(),
            Some("Browser Update Service")
        );
    }
    #[test]
    fn missing_metadata_keeps_executable_identity_readable() {
        assert_eq!(
            fallback_label("C:\\Apps\\Example\\example.exe"),
            "example.exe"
        );
        assert!(choose_label(Some("  ".into()), None).is_none());
    }
    #[test]
    fn installed_helium_uses_its_own_metadata_when_available() {
        let path = "C:\\Program Files\\imput\\Helium\\Application\\chrome.exe";
        if Path::new(path).exists() {
            assert_eq!(executable_label(path).as_deref(), Some("Helium"));
        }
    }
}

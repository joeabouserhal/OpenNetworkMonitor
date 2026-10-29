//! Connected Wi-Fi identity from the SSID-only WinRT API. This module reads
//! connection identity; traffic counters and event capture remain independent.
use crate::model::WifiNetwork;
use std::collections::{HashMap, HashSet};
use windows::{
    Networking::Connectivity::{NetworkConnectivityLevel, NetworkInformation},
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    core::GUID,
};

#[derive(Debug, Default, Clone)]
pub struct WifiSnapshot {
    /// The currently connected SSID, keyed by the application's adapter GUID.
    pub networks: HashMap<String, WifiNetwork>,
    /// Adapters known to have WLAN profiles, including disconnected adapters.
    pub wifi_adapters: HashSet<String>,
    /// A non-sensitive explanation when Windows could not identify some networks.
    pub warning: Option<String>,
}

#[derive(Default)]
pub struct WifiMonitor;
impl WifiMonitor {
    pub fn new() -> Self {
        Self
    }

    /// Poll current profile identities. Disconnected profiles are not treated
    /// as active; local-only profiles are valid Wi-Fi connections.
    pub fn poll(&mut self) -> WifiSnapshot {
        let apartment = match Apartment::enter() {
            Ok(apartment) => apartment,
            Err(_) => {
                return WifiSnapshot {
                    warning: Some("Windows could not initialize Wi-Fi identification.".into()),
                    ..Default::default()
                };
            }
        };
        let mut result = WifiSnapshot::default();
        let profiles = match NetworkInformation::GetConnectionProfiles() {
            Ok(profiles) => profiles,
            Err(_) => {
                return WifiSnapshot {
                    warning: Some("Windows could not read Wi-Fi connections.".into()),
                    ..Default::default()
                };
            }
        };
        let count = match profiles.Size() {
            Ok(count) => count,
            Err(_) => {
                return WifiSnapshot {
                    warning: Some("Windows could not read Wi-Fi connections.".into()),
                    ..Default::default()
                };
            }
        };
        let mut ambiguous = HashSet::new();
        for index in 0..count {
            let profile = match profiles.GetAt(index) {
                Ok(profile) => profile,
                Err(_) => {
                    result.warning = Some("Windows could not identify some Wi-Fi networks.".into());
                    continue;
                }
            };
            match profile.IsWlanConnectionProfile() {
                Ok(true) => {}
                Ok(false) => continue,
                Err(_) => {
                    result.warning = Some("Windows could not identify some Wi-Fi networks.".into());
                    continue;
                }
            }
            let guid = match profile.NetworkAdapter().and_then(|a| a.NetworkAdapterId()) {
                Ok(guid) => guid,
                Err(_) => continue, // Remembered profiles can lack a current adapter.
            };
            let adapter = adapter_key(guid);
            result.wifi_adapters.insert(adapter.clone());
            match profile.GetNetworkConnectivityLevel() {
                Ok(NetworkConnectivityLevel::None) => continue,
                Ok(_) => {}
                Err(_) => {
                    result.warning = Some("Windows could not identify some Wi-Fi networks.".into());
                    continue;
                }
            }
            let ssid = match profile
                .WlanConnectionProfileDetails()
                .and_then(|d| d.GetConnectedSsid())
            {
                Ok(ssid) => ssid,
                Err(_) => {
                    result.warning = Some("Windows could not identify some Wi-Fi networks.".into());
                    continue;
                }
            };
            let Some(network) = network_from_utf16(&ssid) else {
                continue;
            };
            if ambiguous.contains(&adapter) {
                continue;
            }
            if result
                .networks
                .get(&adapter)
                .is_some_and(|previous| previous.id != network.id)
            {
                result.networks.remove(&adapter);
                ambiguous.insert(adapter);
                result.warning = Some(
                    "Windows reported conflicting Wi-Fi connections; affected usage is unassigned."
                        .into(),
                );
            } else {
                result.networks.insert(adapter, network);
            }
        }
        drop(profiles);
        drop(apartment);
        result
    }
}

/// WinRT apartment initialization is balanced on the same polling thread.
struct Apartment(bool);
impl Apartment {
    fn enter() -> windows::core::Result<Self> {
        // SAFETY: this call initializes WinRT for the current thread and the
        // guard balances every successful initialization on the same thread.
        match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
            Ok(()) => Ok(Self(true)),
            // A pre-existing STA apartment can use the API; do not uninitialize it.
            Err(err) if err.code().0 == 0x80010106_u32 as i32 => Ok(Self(false)),
            Err(err) => Err(err),
        }
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: this thread successfully initialized WinRT in enter().
            unsafe { RoUninitialize() };
        }
    }
}

fn adapter_key(guid: GUID) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}

fn network_from_utf16(ssid: &[u16]) -> Option<WifiNetwork> {
    if ssid.is_empty() {
        return None;
    }
    // Preserve WinRT's exact UTF-16, including case and invalid surrogates, in
    // the stable ID; the rendered label may be lossy and has controls removed.
    let mut id = String::with_capacity(5 + ssid.len() * 4);
    id.push_str("ssid:");
    for unit in ssid {
        use std::fmt::Write;
        let _ = write!(id, "{unit:04x}");
    }
    let name: String = String::from_utf16_lossy(ssid)
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect();
    let name = if name.trim().is_empty() {
        "Unnamed Wi-Fi".into()
    } else {
        name
    };
    Some(WifiNetwork { id, name })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wifi_identity_preserves_case_and_utf16_distinctions() {
        assert!(network_from_utf16(&[]).is_none());
        let upper = network_from_utf16(&[0x0041]).unwrap();
        let lower = network_from_utf16(&[0x0061]).unwrap();
        assert_ne!(upper.id, lower.id);
        assert_eq!(upper.id, "ssid:0041");
        let invalid_a = network_from_utf16(&[0xd800]).unwrap();
        let invalid_b = network_from_utf16(&[0xd801]).unwrap();
        assert_ne!(invalid_a.id, invalid_b.id);
        assert_eq!(invalid_a.name, invalid_b.name);
        assert_eq!(network_from_utf16(&[0x0020]).unwrap().name, "Unnamed Wi-Fi");
        assert!(
            !network_from_utf16(&[0x0041, 0x000a, 0x202e])
                .unwrap()
                .name
                .contains('\n')
        );
    }
    #[test]
    fn adapter_guid_matches_counter_adapter_format() {
        let guid = GUID {
            data1: 0x12345678,
            data2: 0x9abc,
            data3: 0xdef0,
            data4: [1, 2, 3, 4, 5, 6, 7, 8],
        };
        assert_eq!(adapter_key(guid), "12345678-9abc-def0-0102030405060708");
    }
    #[test]
    #[ignore = "Queries current Windows Wi-Fi profile state without exposing SSIDs"]
    fn native_wifi_query_returns_sanitized_status() {
        let snapshot = WifiMonitor::new().poll();
        assert!(
            snapshot
                .networks
                .keys()
                .all(|id| snapshot.wifi_adapters.contains(id))
        );
        assert!(
            snapshot
                .networks
                .values()
                .all(|network| !network.id.is_empty() && !network.name.is_empty())
        );
        println!(
            "Wi-Fi profile query: {} adapters, {} connected; warning present: {}",
            snapshot.wifi_adapters.len(),
            snapshot.networks.len(),
            snapshot.warning.is_some()
        );
    }
}

use crate::model::{Adapter, Connection, ipv4_from_windows};
use anyhow::{Context, Result, bail};
use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    ptr,
};
use windows_sys::Win32::{
    Devices::DeviceAndDriverInstallation::*,
    Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, ERROR_INSUFFICIENT_BUFFER, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE,
    },
    NetworkManagement::IpHelper::*,
    Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC},
    System::{Diagnostics::ToolHelp::*, Registry::*, Threading::*},
};

fn wide_text(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|v| *v == 0).unwrap_or(value.len())])
}
struct MibMemory(*mut c_void);
impl Drop for MibMemory {
    fn drop(&mut self) {
        // SAFETY: the pointer came from an IP Helper allocation, owned by this guard.
        unsafe {
            FreeMibTable(self.0);
        }
    }
}
pub fn all_adapters() -> Result<Vec<Adapter>> {
    let mut table = ptr::null_mut();
    // SAFETY: Windows writes a valid table pointer; the allocation is freed below.
    let code = unsafe { GetIfTable2(&mut table) };
    if code != 0 {
        bail!("GetIfTable2: Windows error {code}");
    }
    if table.is_null() {
        bail!("GetIfTable2 returned no table");
    }
    let _owner = MibMemory(table.cast());
    // SAFETY: the successful API call allocated NumEntries contiguous rows in Table.
    let rows = unsafe {
        std::slice::from_raw_parts(
            ptr::addr_of!((*table).Table).cast::<MIB_IF_ROW2>(),
            (*table).NumEntries as usize,
        )
    };
    let mut result = Vec::new();
    for row in rows {
        // Filter-module interfaces are extra observations of their parent adapter.
        if row.InterfaceAndOperStatusFlags._bitfield & 2 != 0 {
            continue;
        }
        let guid = row.InterfaceGuid;
        let id = format!(
            "{:08x}-{:04x}-{:04x}-{}",
            guid.data1,
            guid.data2,
            guid.data3,
            guid.data4
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        result.push(Adapter {
            id,
            name: wide_text(&row.Alias),
            description: wide_text(&row.Description),
            index: row.InterfaceIndex,
            up: row.OperStatus == 1,
            physical: row.InterfaceAndOperStatusFlags._bitfield & 1 != 0,
            loopback: row.Type == 24,
            is_wifi: row.Type == 71, // IF_TYPE_IEEE80211
            received: row.InOctets,
            sent: row.OutOctets,
            addresses: Vec::new(),
        });
    }
    let mut ip_table = ptr::null_mut();
    // SAFETY: valid out pointer; ownership is transferred to the guard on success.
    let code = unsafe { GetUnicastIpAddressTable(AF_UNSPEC, &mut ip_table) };
    if code != 0 {
        bail!("GetUnicastIpAddressTable: Windows error {code}");
    }
    if !ip_table.is_null() {
        let _owner = MibMemory(ip_table.cast());
        // SAFETY: Windows allocated NumEntries rows following the table header.
        let rows = unsafe {
            std::slice::from_raw_parts(
                ptr::addr_of!((*ip_table).Table).cast::<MIB_UNICASTIPADDRESS_ROW>(),
                (*ip_table).NumEntries as usize,
            )
        };
        for row in rows {
            // SAFETY: reading a union member is guarded by the address family tag.
            let ip = unsafe {
                match row.Address.si_family {
                    AF_INET => Some(IpAddr::V4(ipv4_from_windows(
                        row.Address.Ipv4.sin_addr.S_un.S_addr,
                    ))),
                    AF_INET6 => Some(IpAddr::V6(Ipv6Addr::from(
                        row.Address.Ipv6.sin6_addr.u.Byte,
                    ))),
                    _ => None,
                }
            };
            if let (Some(ip), Some(adapter)) = (
                ip,
                result.iter_mut().find(|a| a.index == row.InterfaceIndex),
            ) {
                adapter.addresses.push((ip, row.OnLinkPrefixLength));
            }
        }
    }
    result.sort_by_key(|a| (!a.up, !a.physical, a.name.clone()));
    Ok(result)
}
struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this guard owns a successfully created kernel handle.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub struct InstanceGuard {
    _handle: OwnedHandle,
}
impl InstanceGuard {
    pub fn acquire() -> Result<Self> {
        Self::try_acquire()?
            .context("OpenNetworkMonitor is already running. Open it from its system tray icon.")
    }
    pub fn try_acquire() -> Result<Option<Self>> {
        let name: Vec<u16> = "Local\\OpenNetworkMonitor.Collector.v1\0"
            .encode_utf16()
            .collect();
        // SAFETY: null security attributes and a terminated name are accepted by CreateMutexW.
        let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            bail!("Cannot create application lock");
        }
        let guard = OwnedHandle(handle);
        // SAFETY: GetLastError reads the last API result for this thread.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Ok(None);
        }
        Ok(Some(Self { _handle: guard }))
    }
}
pub fn process_names() -> Result<HashMap<u32, String>> {
    // SAFETY: documented flag with no PID restriction.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        bail!("Cannot enumerate running applications");
    }
    let _owner = OwnedHandle(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut names = HashMap::new();
    // SAFETY: entry has the required size and remains valid for each call.
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more {
        let mut name = wide_text(&entry.szExeFile);
        // SAFETY: query-only permissions; null handles are checked and valid ones are closed.
        let handle =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID) };
        if !handle.is_null() {
            let _owner = OwnedHandle(handle);
            let mut buffer = [0u16; 32768];
            let mut size = buffer.len() as u32;
            // SAFETY: the provided buffer has size UTF-16 elements and the handle grants query access.
            if unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size) } != 0
            {
                name = String::from_utf16_lossy(&buffer[..size as usize]);
            }
        }
        names.insert(entry.th32ProcessID, name);
        // SAFETY: same valid snapshot and sized entry as above.
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    Ok(names)
}
fn rows<T: Copy>(
    mut query: impl FnMut(*mut c_void, &mut u32) -> u32,
    offset: usize,
) -> Result<Vec<T>> {
    let mut size = 0;
    let first = query(ptr::null_mut(), &mut size);
    if first != ERROR_INSUFFICIENT_BUFFER && first != 0 {
        bail!("Connection table: Windows error {first}");
    }
    for _ in 0..5 {
        if size == 0 {
            return Ok(Vec::new());
        }
        if size > 32 * 1024 * 1024 {
            bail!("Connection table exceeds the 32 MiB limit");
        }
        // u64 storage guarantees sufficient alignment for every table used here.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let code = query(buffer.as_mut_ptr().cast(), &mut size);
        if code == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        if code != 0 {
            bail!("Connection table: Windows error {code}");
        }
        if (size as usize) < offset {
            return Ok(Vec::new());
        }
        // SAFETY: buffer contains at least a DWORD count; returned rows are copied with bounds checks.
        let count = unsafe { ptr::read_unaligned(buffer.as_ptr().cast::<u32>()) } as usize;
        let bytes = buffer.len() * 8;
        if count > (bytes - offset) / std::mem::size_of::<T>() {
            bail!("Invalid connection table length");
        }
        let mut result = Vec::with_capacity(count);
        for n in 0..count {
            // SAFETY: each complete row lies inside the initialized API output buffer.
            result.push(unsafe {
                ptr::read_unaligned(
                    buffer
                        .as_ptr()
                        .cast::<u8>()
                        .add(offset + n * std::mem::size_of::<T>())
                        .cast::<T>(),
                )
            });
        }
        return Ok(result);
    }
    bail!("Connection table kept changing; retrying next refresh")
}
fn state(value: u32) -> &'static str {
    match value {
        1 => "Closed",
        2 => "Listening",
        3 => "SYN sent",
        4 => "SYN received",
        5 => "Established",
        6 => "FIN wait 1",
        7 => "FIN wait 2",
        8 => "Close wait",
        9 => "Closing",
        10 => "Last ACK",
        11 => "Time wait",
        12 => "Deleting",
        _ => "Unknown",
    }
}
fn endpoint(ip: IpAddr, port: u32) -> String {
    SocketAddr::new(ip, u16::from_be(port as u16)).to_string()
}
pub fn connections(names: &HashMap<u32, String>) -> Result<Vec<Connection>> {
    let mut result = Vec::new();
    // SAFETY: rows supplies a sized, aligned buffer and the correct table class for IPv4.
    let tcp4 = rows::<MIB_TCPROW_OWNER_PID>(
        |b, s| unsafe { GetExtendedTcpTable(b, s, 0, AF_INET as u32, TCP_TABLE_OWNER_PID_ALL, 0) },
        std::mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table),
    )?;
    for r in tcp4 {
        result.push(Connection {
            pid: r.dwOwningPid,
            local: endpoint(ipv4_from_windows(r.dwLocalAddr).into(), r.dwLocalPort),
            remote: endpoint(ipv4_from_windows(r.dwRemoteAddr).into(), r.dwRemotePort),
            protocol: "TCP4".into(),
            state: state(r.dwState).into(),
            ..Default::default()
        });
    }
    // SAFETY: the query and row layout both use the documented IPv6 owner-PID table.
    let tcp6 = rows::<MIB_TCP6ROW_OWNER_PID>(
        |b, s| unsafe { GetExtendedTcpTable(b, s, 0, AF_INET6 as u32, TCP_TABLE_OWNER_PID_ALL, 0) },
        std::mem::offset_of!(MIB_TCP6TABLE_OWNER_PID, table),
    )?;
    for r in tcp6 {
        result.push(Connection {
            pid: r.dwOwningPid,
            local: SocketAddr::V6(std::net::SocketAddrV6::new(
                Ipv6Addr::from(r.ucLocalAddr),
                u16::from_be(r.dwLocalPort as u16),
                0,
                r.dwLocalScopeId,
            ))
            .to_string(),
            remote: SocketAddr::V6(std::net::SocketAddrV6::new(
                Ipv6Addr::from(r.ucRemoteAddr),
                u16::from_be(r.dwRemotePort as u16),
                0,
                r.dwRemoteScopeId,
            ))
            .to_string(),
            protocol: "TCP6".into(),
            state: state(r.dwState).into(),
            ..Default::default()
        });
    }
    // SAFETY: correct IPv4 UDP owner-PID class and matching row layout.
    let udp4 = rows::<MIB_UDPROW_OWNER_PID>(
        |b, s| unsafe { GetExtendedUdpTable(b, s, 0, AF_INET as u32, UDP_TABLE_OWNER_PID, 0) },
        std::mem::offset_of!(MIB_UDPTABLE_OWNER_PID, table),
    )?;
    for r in udp4 {
        result.push(Connection {
            pid: r.dwOwningPid,
            local: endpoint(ipv4_from_windows(r.dwLocalAddr).into(), r.dwLocalPort),
            remote: "Not available in socket table".into(),
            protocol: "UDP4".into(),
            state: "Bound".into(),
            ..Default::default()
        });
    }
    // SAFETY: correct IPv6 UDP owner-PID class and matching row layout.
    let udp6 = rows::<MIB_UDP6ROW_OWNER_PID>(
        |b, s| unsafe { GetExtendedUdpTable(b, s, 0, AF_INET6 as u32, UDP_TABLE_OWNER_PID, 0) },
        std::mem::offset_of!(MIB_UDP6TABLE_OWNER_PID, table),
    )?;
    for r in udp6 {
        result.push(Connection {
            pid: r.dwOwningPid,
            local: SocketAddr::V6(std::net::SocketAddrV6::new(
                Ipv6Addr::from(r.ucLocalAddr),
                u16::from_be(r.dwLocalPort as u16),
                0,
                r.dwLocalScopeId,
            ))
            .to_string(),
            remote: "Not available in socket table".into(),
            protocol: "UDP6".into(),
            state: "Bound".into(),
            ..Default::default()
        });
    }
    for r in &mut result {
        r.app = names
            .get(&r.pid)
            .cloned()
            .unwrap_or_else(|| format!("Process {}", r.pid));
    }
    result.sort_by(|a, b| a.app.cmp(&b.app).then(a.pid.cmp(&b.pid)));
    Ok(result)
}

/// Normal Windows network-adapter inventory, excluding hidden internal interfaces.
/// all_adapters() remains available for capture attribution, including loopback.
pub fn adapters() -> Result<Vec<Adapter>> {
    let visible = visible_adapter_ids()?;
    Ok(all_adapters()?
        .into_iter()
        .filter(|a| visible.contains(&a.id))
        .collect())
}

struct RegistryKey(HKEY);
impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: this guard exclusively owns a successfully opened registry handle.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

/// Present network devices with Windows' NCF_HIDDEN user-interface flag excluded.
/// Device presence is separate from connection state: unplugged Ethernet/Wi-Fi stays visible,
/// while removed USB devices and internal miniports do not inflate the adapter count.
pub fn visible_adapter_ids() -> Result<HashSet<String>> {
    type CatalogCache = Option<(std::time::Instant, HashSet<String>)>;
    static CACHE: std::sync::OnceLock<std::sync::Mutex<CatalogCache>> = std::sync::OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .map_err(|_| anyhow::anyhow!("Windows adapter catalog cache is unavailable"))?;
    if let Some((updated, ids)) = &*cache
        && updated.elapsed() < std::time::Duration::from_secs(3)
    {
        return Ok(ids.clone());
    }
    let ids = read_adapter_ids(DIGCF_PRESENT)?;
    *cache = Some((std::time::Instant::now(), ids.clone()));
    Ok(ids)
}
struct DeviceSet(HDEVINFO);
impl Drop for DeviceSet {
    fn drop(&mut self) {
        // SAFETY: the owned device-information set was returned by SetupDiGetClassDevsW.
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}
/// Installed non-hidden adapters include disconnected or removed devices for historical membership.
pub fn historical_adapter_ids() -> Result<HashSet<String>> {
    read_adapter_ids(0)
}
fn read_adapter_ids(flags: SETUP_DI_GET_CLASS_DEVS_FLAGS) -> Result<HashSet<String>> {
    // SAFETY: the network class GUID is valid; enumeration is read-only and has no parent UI.
    let set =
        unsafe { SetupDiGetClassDevsW(&GUID_DEVCLASS_NET, ptr::null(), ptr::null_mut(), flags) };
    if set == -1 {
        // SAFETY: reads this thread's last Windows error immediately after the failed call.
        let code = unsafe { GetLastError() };
        bail!("Cannot enumerate Windows network devices (error {code})");
    }
    let set = DeviceSet(set);
    let mut result = HashSet::new();
    for index in 0..4096 {
        let mut device = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        // SAFETY: set is valid and device has the documented size for this architecture.
        if unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut device) } == 0 {
            // SAFETY: read the failed enumeration's last error on this thread.
            let code = unsafe { GetLastError() };
            if code == windows_sys::Win32::Foundation::ERROR_NO_MORE_ITEMS {
                break;
            }
            bail!("Cannot enumerate Windows network devices (error {code})");
        }
        // SAFETY: querying only the enumerated device's driver key; no write access is requested.
        let key = unsafe {
            SetupDiOpenDevRegKey(
                set.0,
                &device,
                DICS_FLAG_GLOBAL,
                0,
                DIREG_DRV,
                KEY_QUERY_VALUE,
            )
        };
        if key == INVALID_HANDLE_VALUE {
            continue;
        }
        let owner = RegistryKey(key);
        let characteristics_name = to_wide("Characteristics");
        let mut characteristics = 0u32;
        let mut bytes = std::mem::size_of::<u32>() as u32;
        // SAFETY: querying a DWORD into a u32 with its byte size cannot overflow the output.
        let code = unsafe {
            RegGetValueW(
                owner.0,
                ptr::null(),
                characteristics_name.as_ptr(),
                RRF_RT_REG_DWORD,
                ptr::null_mut(),
                (&mut characteristics as *mut u32).cast(),
                &mut bytes,
            )
        };
        const NCF_HIDDEN: u32 = 0x8;
        if code != 0 || characteristics & NCF_HIDDEN != 0 {
            continue;
        }
        let instance_name = to_wide("NetCfgInstanceId");
        let mut instance = [0u16; 64];
        let mut bytes = std::mem::size_of_val(&instance) as u32;
        // SAFETY: a bounded UTF-16 output array is supplied for a REG_SZ value; no registry mutation occurs.
        let code = unsafe {
            RegGetValueW(
                owner.0,
                ptr::null(),
                instance_name.as_ptr(),
                RRF_RT_REG_SZ,
                ptr::null_mut(),
                instance.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if code == 0
            && let Some(id) = adapter_key_from_guid_text(&wide_text(&instance))
        {
            result.insert(id);
        }
    }
    if result.is_empty() {
        bail!("Windows returned an empty network adapter catalog");
    }
    Ok(result)
}
fn adapter_key_from_guid_text(text: &str) -> Option<String> {
    let digits: String = text
        .chars()
        .filter(|c| !matches!(c, '{' | '}' | '-'))
        .collect();
    if digits.len() != 32 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digits = digits.to_ascii_lowercase();
    // Preserve the existing database ID layout so upgrades keep their recorded history.
    Some(format!(
        "{}-{}-{}-{}",
        &digits[..8],
        &digits[8..12],
        &digits[12..16],
        &digits[16..]
    ))
}
#[cfg(test)]
mod visibility_tests {
    use super::*;
    #[test]
    fn catalog_guids_keep_existing_history_identity() {
        assert_eq!(
            adapter_key_from_guid_text("{DF509F36-AE47-4FC8-9B50-A11FC4170DC4}").as_deref(),
            Some("df509f36-ae47-4fc8-9b50a11fc4170dc4")
        );
        assert!(adapter_key_from_guid_text("not a guid").is_none());
    }
}

#[cfg(test)]
mod native_probe {
    use super::*;
    #[test]
    #[ignore = "Read-only live-machine adapter and application metadata probe"]
    fn native_inventory_identity_probe() -> Result<()> {
        let visible = adapters()?;
        let all = all_adapters()?;
        let historical = historical_adapter_ids()?;
        let expected = visible_adapter_ids()?;
        assert!(
            visible
                .iter()
                .all(|adapter| expected.contains(&adapter.id) && !adapter.loopback)
        );
        println!(
            "Native adapter inventory: {} visible, {} internal",
            visible.len(),
            all.len()
        );
        println!("Installed non-hidden adapter catalog: {}", historical.len());
        assert!(visible.iter().all(|a| historical.contains(&a.id)));
        for adapter in &visible {
            println!("Visible adapter: {}", adapter.name);
        }
        let names = process_names()?;
        let labels = super::super::process_labels(&names);
        let helium: Vec<_> = labels
            .iter()
            .filter(|(path, _)| path.to_lowercase().contains("\\helium\\"))
            .collect();
        for (path, label) in helium {
            assert_eq!(
                label, "Helium",
                "Wrong metadata label for the installed Helium executable: {path}"
            );
            println!("Helium executable metadata: {label}");
        }
        Ok(())
    }
}

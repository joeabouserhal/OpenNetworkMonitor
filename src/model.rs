use chrono::{Datelike, TimeZone};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Source {
    #[default]
    Adapter,
    Transport,
}
impl Source {
    pub fn key(self) -> &'static str {
        match self {
            Self::Adapter => "adapter",
            Self::Transport => "transport",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Adapter => "Overall Traffic",
            Self::Transport => "App traffic (TCP + UDP)",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Scope {
    #[default]
    All,
    Lan,
    Wan,
    Unknown,
}
impl Scope {
    pub fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Lan => "LAN",
            Self::Wan => "WAN",
            Self::Unknown => "Unknown",
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct Adapter {
    pub id: String,
    pub name: String,
    pub description: String,
    pub index: u32,
    pub up: bool,
    pub physical: bool,
    pub is_wifi: bool,
    pub loopback: bool,
    pub received: u64,
    pub sent: u64,
    pub addresses: Vec<(IpAddr, u8)>,
}
#[derive(Debug, Clone, Default)]
pub struct Connection {
    pub pid: u32,
    pub app: String,
    pub local: String,
    pub remote: String,
    pub country: String,
    pub protocol: String,
    pub state: String,
}
#[derive(Debug, Clone)]
pub struct TrafficEvent {
    pub local_port: u16,
    pub remote_port: u16,
    pub timestamp: i64,
    pub pid: u32,
    pub local: IpAddr,
    pub remote: IpAddr,
    pub protocol: &'static str,
    pub down: u64,
    pub up: u64,
}
/// Traffic observed on Wi-Fi while the connected network is unavailable or changing.
pub const WIFI_UNKNOWN: &str = "__wifi_unknown__";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WifiNetwork {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageRow {
    #[serde(default)]
    pub traffic_type: String,
    pub source: String,
    pub adapter_id: String,
    pub adapter: String,
    #[serde(default)]
    pub wifi_network: String,
    pub app: String,
    pub pid: u32,
    pub remote: String,
    pub country: String,
    pub protocol: String,
    pub scope: String,
    pub down: u64,
    pub up: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: i64,
    pub end: i64,
}

/// Dynamic calendar period; resolves again for each query, including after reopening.
pub const PERIOD_MONTH_TO_DATE: i64 = -1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    pub source: Source,
    pub scope: Scope,
    pub adapters: HashSet<String>,
    #[serde(default)]
    pub wifi_network: Option<String>,
    /// Zero selects all saved history, -1 is month-to-date; positive values are rolling durations.
    pub seconds: i64,
    #[serde(default)]
    pub time_range: Option<TimeRange>,
    pub search: String,
    #[serde(default)]
    pub apps: HashSet<String>,
}
impl Default for Filter {
    fn default() -> Self {
        Self {
            source: Source::Adapter,
            scope: Scope::All,
            adapters: HashSet::new(),
            wifi_network: None,
            seconds: 3600,
            time_range: None,
            search: String::new(),
            apps: HashSet::new(),
        }
    }
}
impl Filter {
    pub fn time_bounds(&self, now: i64) -> (i64, i64) {
        if let Some(range) = self.time_range {
            let start = range.start.max(0);
            (start, range.end.max(start))
        } else if self.month_to_date() {
            let start = chrono::Local
                .timestamp_opt(now, 0)
                .single()
                .map(month_start)
                .unwrap_or(now);
            (start, now)
        } else {
            (
                if self.seconds <= 0 {
                    0
                } else {
                    now.saturating_sub(self.seconds).max(0)
                },
                now,
            )
        }
    }
    pub fn month_to_date(&self) -> bool {
        self.time_range.is_none() && self.seconds == PERIOD_MONTH_TO_DATE
    }
    pub fn all_time(&self) -> bool {
        self.time_range.is_none() && self.seconds == 0
    }
    pub fn matches(&self, row: &UsageRow) -> bool {
        row.source == self.source.key()
            && (self.apps.is_empty() || self.apps.contains(&row.app))
            && (self.adapters.is_empty() || self.adapters.contains(&row.adapter_id))
            && self
                .wifi_network
                .as_ref()
                .is_none_or(|id| id == &row.wifi_network)
            && (self.scope == Scope::All || self.scope.key() == row.scope)
            && (self.search.is_empty()
                || format!("{} {} {} {}", row.app, row.remote, row.adapter, row.country)
                    .to_lowercase()
                    .contains(&self.search.to_lowercase()))
    }
}
/// Use local calendar boundaries, rather than assuming every month is 30 days
/// or using today's UTC offset at a boundary where DST may have differed.
fn month_start<T: TimeZone>(now: chrono::DateTime<T>) -> i64 {
    let midnight = now
        .date_naive()
        .with_day(1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    // Some time zones skip or repeat midnight. Choose the first valid instant
    // of the month, resolving repeated local times to their earlier occurrence.
    (0..=2880)
        .find_map(|minute| {
            let local = midnight.checked_add_signed(chrono::Duration::minutes(minute))?;
            now.timezone()
                .from_local_datetime(&local)
                .earliest()
                .map(|time| time.timestamp())
        })
        .unwrap_or_else(|| now.timestamp())
}
#[derive(Debug, Clone, Default)]
pub struct Point {
    pub observed: bool,
    pub timestamp: i64,
    pub down: u64,
    pub up: u64,
}
#[derive(Debug, Clone, Default)]
pub struct History {
    pub window_start: i64,
    pub window_end: i64,
    pub points: Vec<Point>,
    pub rows: Vec<UsageRow>,
    pub down: u64,
    pub up: u64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppUsage {
    pub app: String,
    pub down: u64,
    pub up: u64,
}
#[derive(Debug, Clone, Default)]
pub struct TransportSummary {
    pub classified: ScopeTotals,
    pub top_apps: Vec<AppUsage>,
}
#[derive(Debug, Clone, Default)]
pub struct ScopeTotals {
    pub lan: u64,
    pub wan: u64,
    pub unknown: u64,
}
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub captured_at: i64,
    pub app_labels: HashMap<String, String>,
    pub host_labels: HashMap<String, String>,
    pub adapters: Vec<Adapter>,
    pub wifi_networks: Vec<WifiNetwork>,
    pub connections: Vec<Connection>,
    pub live_rows: Vec<UsageRow>,
    pub interval: f64,
    pub etw_active: bool,
    pub capture_status: String,
    pub parsed_events: u64,
    pub parse_errors: u64,
    pub queue_drops: u64,
    pub kernel_losses: u64,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Default)]
pub struct LiveTick {
    pub timestamp: i64,
    pub interval: f64,
    pub adapters: HashMap<String, (u64, u64)>,
    pub wifi_networks: HashMap<String, String>,
}
impl LiveTick {
    pub fn point(&self, selected: &HashSet<String>) -> Point {
        let (down, up) = self
            .adapters
            .iter()
            .filter(|(id, _)| selected.is_empty() || selected.contains(*id))
            .fold((0u64, 0u64), |(d, u), (_, v)| {
                (d.saturating_add(v.0), u.saturating_add(v.1))
            });
        Point {
            observed: !self.adapters.is_empty(),
            timestamp: self.timestamp,
            down: (down as f64 / self.interval.max(0.001)) as u64,
            up: (up as f64 / self.interval.max(0.001)) as u64,
        }
    }
}
pub fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v) => v.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        _ => ip,
    }
}
pub fn same_subnet(a: IpAddr, b: IpAddr, prefix: u8) -> bool {
    match (normalize_ip(a), normalize_ip(b)) {
        (IpAddr::V4(a), IpAddr::V4(b)) if prefix <= 32 => {
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            (u32::from(a) & mask) == (u32::from(b) & mask)
        }
        (IpAddr::V6(a), IpAddr::V6(b)) if prefix <= 128 => {
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            (u128::from(a) & mask) == (u128::from(b) & mask)
        }
        _ => false,
    }
}
pub fn classify(remote: IpAddr, local_networks: &[(IpAddr, u8)]) -> Scope {
    let remote = normalize_ip(remote);
    // Ignore /0 routes: a VPN default route must not turn the internet into LAN.
    if local_networks
        .iter()
        .any(|(ip, p)| *p > 0 && same_subnet(*ip, remote, *p))
    {
        return Scope::Lan;
    }
    match remote {
        IpAddr::V4(v) => {
            let [a, b, c, _] = v.octets();
            if v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_multicast()
                || v.is_broadcast()
            {
                Scope::Lan
            } else if v.is_unspecified()
                || a == 0
                || a >= 240
                || (a == 100 && (64..=127).contains(&b))
                || v.is_documentation()
                || (a == 198 && (b == 18 || b == 19))
                || (a == 192 && b == 0 && c == 0)
            {
                Scope::Unknown
            } else {
                Scope::Wan
            }
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            if v.is_loopback()
                || v.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
            {
                Scope::Lan
            } else if (s[0] & 0xe000) == 0x2000 && !(s[0] == 0x2001 && s[1] == 0x0db8) {
                Scope::Wan
            } else {
                Scope::Unknown
            }
        }
    }
}
pub fn adapter_for(local: IpAddr, adapters: &[Adapter]) -> Option<&Adapter> {
    let mut matches = adapters.iter().filter(|a| {
        a.addresses
            .iter()
            .any(|(ip, _)| normalize_ip(*ip) == normalize_ip(local))
    });
    let result = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(result)
    }
}
pub fn counter_delta(previous: u64, current: u64) -> u64 {
    current.saturating_sub(previous)
}
pub fn format_bytes(bytes: f64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut value = bytes.max(0.0);
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.2} {}", units[unit])
    }
}
pub fn ipv4_from_windows(value: u32) -> Ipv4Addr {
    Ipv4Addr::from(value.to_ne_bytes())
}

pub fn format_compact_bytes(value: u64) -> String {
    let mut n = value as f64;
    let mut i = 0;
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    while n >= 1024.0 && i < units.len() - 1 {
        n /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{value} B")
    } else {
        format!("{n:.1} {}", units[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traffic_classes() {
        for ip in [
            "192.168.1.4",
            "10.0.0.1",
            "172.20.2.3",
            "127.0.0.1",
            "169.254.2.2",
            "224.0.0.1",
            "fd00::1",
            "fe80::1",
            "::1",
            "::ffff:192.168.1.2",
        ] {
            assert_eq!(classify(ip.parse().unwrap(), &[]), Scope::Lan, "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert_eq!(classify(ip.parse().unwrap(), &[]), Scope::Wan);
        }
        for ip in [
            "0.0.0.0",
            "100.64.2.1",
            "192.0.2.2",
            "198.18.0.1",
            "2001:db8::1",
            "::",
        ] {
            assert_eq!(classify(ip.parse().unwrap(), &[]), Scope::Unknown, "{ip}");
        }
    }
    #[test]
    fn vpn_default_is_not_lan() {
        assert_eq!(
            classify(
                "1.1.1.1".parse().unwrap(),
                &[("10.1.2.3".parse().unwrap(), 0)]
            ),
            Scope::Wan
        );
    }
    #[test]
    fn public_on_link_is_lan() {
        assert_eq!(
            classify(
                "8.8.8.4".parse().unwrap(),
                &[("8.8.8.2".parse().unwrap(), 24)]
            ),
            Scope::Lan
        );
    }
    #[test]
    fn counters_reset_without_spikes() {
        assert_eq!(counter_delta(100, 5), 0);
        assert_eq!(counter_delta(5, 100), 95);
    }
    #[test]
    fn sources_never_mix() {
        let row = UsageRow {
            source: "transport".into(),
            down: 100,
            ..Default::default()
        };
        assert!(!Filter::default().matches(&row));
    }
    #[test]
    fn ambiguous_local_address_stays_unassigned() {
        let a = Adapter {
            addresses: vec![("10.1.1.1".parse().unwrap(), 24)],
            ..Default::default()
        };
        assert!(adapter_for("10.1.1.1".parse().unwrap(), &[a.clone(), a]).is_none());
    }
    #[test]
    fn windows_address_order() {
        assert_eq!(
            ipv4_from_windows(u32::from_ne_bytes([192, 168, 1, 10])).to_string(),
            "192.168.1.10"
        );
    }
}

#[cfg(test)]
mod calendar_period_tests {
    use super::*;
    #[test]
    fn month_to_date_uses_local_boundaries_and_rolls_into_the_next_month() {
        let zone = chrono::FixedOffset::east_opt(3 * 3600).unwrap();
        let october = zone.with_ymd_and_hms(2026, 10, 29, 12, 30, 0).unwrap();
        let first = chrono::Utc
            .with_ymd_and_hms(2026, 9, 30, 21, 0, 0)
            .unwrap()
            .timestamp();
        assert_eq!(month_start(october), first);
        let november = zone.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap();
        assert_eq!(month_start(november), november.timestamp());
        let january = zone.with_ymd_and_hms(2027, 1, 15, 12, 0, 0).unwrap();
        assert_eq!(
            month_start(january),
            chrono::Utc
                .with_ymd_and_hms(2026, 12, 31, 21, 0, 0)
                .unwrap()
                .timestamp()
        );
        let filter = Filter {
            seconds: PERIOD_MONTH_TO_DATE,
            ..Default::default()
        };
        let saved = serde_json::to_string(&filter).unwrap();
        let restored: Filter = serde_json::from_str(&saved).unwrap();
        assert!(restored.month_to_date());
        assert!(!restored.all_time());
        let local = chrono::Local
            .timestamp_opt(october.timestamp(), 0)
            .single()
            .unwrap();
        assert_eq!(
            restored.time_bounds(october.timestamp()),
            (month_start(local), october.timestamp())
        );
        let local = chrono::Local
            .timestamp_opt(november.timestamp(), 0)
            .single()
            .unwrap();
        assert_eq!(
            restored.time_bounds(november.timestamp()),
            (month_start(local), november.timestamp())
        );
    }
}

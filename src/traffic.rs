use crate::model::UsageRow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

pub fn executable_filename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|p| p.to_str())
        .unwrap_or(path)
        .to_owned()
}
pub fn executable_directory(path: &str) -> String {
    Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

/// Connection-port classification, not decrypted payload inspection.
pub fn classify(protocol: &str, local: u16, remote: u16) -> &'static str {
    fn service(protocol: &str, port: u16) -> Option<&'static str> {
        match (protocol, port) {
            (_, 53) => Some("DNS"),
            ("UDP", 5353) => Some("mDNS"),
            (_, 5355) => Some("LLMNR"),
            ("TCP", 80 | 8080 | 8000) => Some("HTTP"),
            ("TCP", 443 | 8443) => Some("HTTPS / TLS"),
            ("UDP", 443) => Some("QUIC"),
            (_, 853) => Some("Encrypted DNS"),
            ("UDP", 123) => Some("NTP"),
            ("UDP", 67 | 68 | 546 | 547) => Some("DHCP"),
            ("TCP", 22) => Some("SSH"),
            (_, 3389) => Some("Remote Desktop"),
            ("TCP", 445) => Some("SMB"),
            ("TCP", 25 | 465 | 587) => Some("SMTP"),
            ("TCP", 110 | 995) => Some("POP3"),
            ("TCP", 143 | 993) => Some("IMAP"),
            ("TCP", 21) => Some("FTP"),
            _ => None,
        }
    }
    if local == 0 && remote == 0 {
        return "Unclassified";
    }
    service(protocol, remote)
        .or_else(|| service(protocol, local))
        .unwrap_or(match protocol {
            "TCP" => "Other TCP",
            "UDP" => "Other UDP",
            _ => "Other",
        })
}
pub fn type_description(label: &str) -> &'static str {
    match label {
        "HTTP" => "Common HTTP ports",
        "HTTPS / TLS" => "Encrypted TCP web ports",
        "DNS" => "DNS port 53",
        "QUIC" => "UDP port 443",
        "Encrypted DNS" => "DNS over TLS / QUIC port",
        "mDNS" => "Local multicast DNS",
        "LLMNR" => "Local name resolution",
        "Unclassified" => "No service-port information recorded",
        "Other TCP" => "Other TCP ports",
        "Other UDP" => "Other UDP ports",
        _ => "Based on the connection ports",
    }
}
#[derive(Debug, Default)]
pub struct RankedUsage {
    pub label: String,
    pub subtitle: String,
    pub down: u64,
    pub up: u64,
    pub addresses: Vec<String>,
}
#[derive(Debug, Default)]
pub struct Breakdown {
    pub hosts: Vec<RankedUsage>,
    pub types: Vec<RankedUsage>,
}
pub fn breakdown(
    rows: &[UsageRow],
    apps: &HashSet<String>,
    selected: Option<&str>,
    names: &HashMap<String, String>,
) -> Breakdown {
    let mut hosts = BTreeMap::<String, RankedUsage>::new();
    let mut types = BTreeMap::<String, RankedUsage>::new();
    for row in rows {
        if row.source != "transport"
            || !apps.contains(&row.app)
            || selected.is_some_and(|p| p != row.app)
        {
            continue;
        }
        if !row.remote.is_empty() {
            let host = names
                .get(&row.remote)
                .filter(|s| !s.is_empty())
                .cloned()
                .unwrap_or_else(|| row.remote.clone());
            let h = hosts.entry(host.clone()).or_insert_with(|| RankedUsage {
                label: host,
                ..Default::default()
            });
            h.down = h.down.saturating_add(row.down);
            h.up = h.up.saturating_add(row.up);
            if !h.addresses.contains(&row.remote) {
                h.addresses.push(row.remote.clone());
            }
        }
        let kind = if row.traffic_type.is_empty() {
            "Unclassified"
        } else {
            &row.traffic_type
        };
        let t = types.entry(kind.into()).or_insert_with(|| RankedUsage {
            label: kind.into(),
            subtitle: type_description(kind).into(),
            ..Default::default()
        });
        t.down = t.down.saturating_add(row.down);
        t.up = t.up.saturating_add(row.up);
    }
    let mut hosts: Vec<_> = hosts.into_values().collect();
    for h in &mut hosts {
        h.addresses.sort();
        h.subtitle = if h.addresses.len() > 1 {
            format!("{} addresses", h.addresses.len())
        } else if h.addresses.first().is_some_and(|ip| ip != &h.label) {
            h.addresses[0].clone()
        } else {
            "Name unavailable".into()
        };
    }
    let rank = |a: &RankedUsage, b: &RankedUsage| {
        (b.down.saturating_add(b.up))
            .cmp(&a.down.saturating_add(a.up))
            .then(a.label.cmp(&b.label))
    };
    hosts.sort_by(rank);
    let mut types: Vec<_> = types.into_values().collect();
    types.sort_by(rank);
    Breakdown { hosts, types }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keeps_filename_extension_and_directory() {
        assert_eq!(executable_filename("C:\\Apps\\plain.exe"), "plain.exe");
        assert_eq!(executable_directory("C:\\Apps\\plain.exe"), "C:\\Apps");
    }
    #[test]
    fn service_types_include_servers_and_encrypted_transports() {
        assert_eq!(classify("TCP", 50000, 80), "HTTP");
        assert_eq!(classify("TCP", 80, 50000), "HTTP");
        assert_eq!(classify("UDP", 50000, 53), "DNS");
        assert_eq!(classify("TCP", 50000, 443), "HTTPS / TLS");
        assert_eq!(classify("UDP", 50000, 443), "QUIC");
        assert_eq!(classify("TCP", 0, 0), "Unclassified");
    }
    #[test]
    fn all_columns_share_scope_and_group_same_host_without_duplicating_bytes() {
        let rows = vec![
            UsageRow {
                source: "transport".into(),
                app: "a.exe".into(),
                remote: "1.1.1.1".into(),
                traffic_type: "DNS".into(),
                down: 100,
                up: 20,
                ..Default::default()
            },
            UsageRow {
                source: "transport".into(),
                app: "a.exe".into(),
                remote: "1.0.0.1".into(),
                traffic_type: "HTTPS / TLS".into(),
                down: 40,
                ..Default::default()
            },
            UsageRow {
                source: "transport".into(),
                app: "b.exe".into(),
                remote: "8.8.8.8".into(),
                traffic_type: "DNS".into(),
                down: 999,
                ..Default::default()
            },
        ];
        let names = HashMap::from([
            ("1.1.1.1".into(), "host.test".into()),
            ("1.0.0.1".into(), "host.test".into()),
        ]);
        let data = breakdown(
            &rows,
            &HashSet::from(["a.exe".into(), "b.exe".into()]),
            Some("a.exe"),
            &names,
        );
        assert_eq!(data.hosts.len(), 1);
        assert_eq!((data.hosts[0].down, data.hosts[0].up), (140, 20));
        assert_eq!(data.hosts[0].addresses.len(), 2);
        assert_eq!(data.types[0].label, "DNS");
        assert_eq!(data.types.iter().map(|r| r.down + r.up).sum::<u64>(), 160);
    }
}

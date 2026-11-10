//! Sorting, filtering and actions for the Connections list.
use crate::model::Connection;
use std::{cmp::Ordering, net::IpAddr, net::SocketAddr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Column {
    Application,
    Protocol,
    Remote,
    Local,
    Country,
    Status,
}
impl Column {
    pub(super) const ALL: [Self; 6] = [
        Self::Application,
        Self::Protocol,
        Self::Remote,
        Self::Local,
        Self::Country,
        Self::Status,
    ];
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Application => "Application",
            Self::Protocol => "Protocol",
            Self::Remote => "Remote endpoint",
            Self::Local => "Local endpoint",
            Self::Country => "Country",
            Self::Status => "Status",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Sort {
    pub(super) column: Column,
    pub(super) ascending: bool,
}
impl Sort {
    /// Clicking a header: first ascending, then descending, then back to the natural
    /// order. Clicking another header starts that one ascending.
    pub(super) fn clicked(current: Option<Self>, column: Column) -> Option<Self> {
        match current {
            Some(sort) if sort.column == column && sort.ascending => Some(Self {
                column,
                ascending: false,
            }),
            Some(sort) if sort.column == column => None,
            _ => Some(Self {
                column,
                ascending: true,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ProtocolFilter {
    #[default]
    All,
    Tcp,
    Udp,
}
impl ProtocolFilter {
    pub(super) const ALL: [Self; 3] = [Self::All, Self::Tcp, Self::Udp];
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
        }
    }
    pub(super) fn matches(self, protocol: &str) -> bool {
        match self {
            Self::All => true,
            Self::Tcp => protocol.starts_with("TCP"),
            Self::Udp => protocol.starts_with("UDP"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum StatusFilter {
    #[default]
    All,
    Established,
    Listening,
    Other,
}
impl StatusFilter {
    pub(super) const ALL: [Self; 4] = [Self::All, Self::Established, Self::Listening, Self::Other];
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Established => "Established",
            Self::Listening => "Listening",
            Self::Other => "Other",
        }
    }
    pub(super) fn matches(self, state: &str) -> bool {
        let is = |name: &str| state.eq_ignore_ascii_case(name);
        match self {
            Self::All => true,
            Self::Established => is("Established"),
            Self::Listening => is("Listening"),
            Self::Other => !is("Established") && !is("Listening"),
        }
    }
}

/// The address part of an `ip:port` or `[ip]:port` endpoint.
pub(super) fn endpoint_ip(endpoint: &str) -> Option<IpAddr> {
    endpoint
        .parse::<SocketAddr>()
        .ok()
        .map(|socket| socket.ip())
}

/// Addresses order numerically, then by port; anything unparsable sorts after them.
fn endpoint_order(a: &str, b: &str) -> Ordering {
    match (a.parse::<SocketAddr>(), b.parse::<SocketAddr>()) {
        (Ok(a), Ok(b)) => (a.ip(), a.port()).cmp(&(b.ip(), b.port())),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => a.cmp(b),
    }
}

pub(super) fn compare(column: Column, a: (&Connection, &str), b: (&Connection, &str)) -> Ordering {
    match column {
        Column::Application => {
            a.1.to_lowercase()
                .cmp(&b.1.to_lowercase())
                .then(a.0.pid.cmp(&b.0.pid))
        }
        Column::Protocol => a.0.protocol.cmp(&b.0.protocol),
        Column::Remote => endpoint_order(&a.0.remote, &b.0.remote),
        Column::Local => endpoint_order(&a.0.local, &b.0.local),
        Column::Country => a.0.country.cmp(&b.0.country),
        Column::Status => a.0.state.cmp(&b.0.state),
    }
}

/// One line per field, for pasting into a ticket or chat.
pub(super) fn describe(name: &str, c: &Connection) -> String {
    format!(
        "{name} (PID {}) {} {} -> {} [{}] {}",
        c.pid, c.protocol, c.local, c.remote, c.state, c.country
    )
}

/// Opens Explorer with the executable selected. Only an existing absolute path is passed on.
pub(super) fn reveal_in_explorer(path: &str) -> bool {
    use std::os::windows::process::CommandExt;
    let path = std::path::Path::new(path);
    if !path.is_absolute() || !path.exists() {
        return false;
    }
    std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .is_ok()
}

/// Opens a folder in Explorer. Only an existing absolute folder is passed on.
pub(super) fn open_folder(path: &std::path::Path) -> bool {
    path.is_absolute()
        && path.is_dir()
        && std::process::Command::new("explorer.exe")
            .arg(path)
            .spawn()
            .is_ok()
}

/// Ends a process on request. The system's own processes and this app are refused.
pub(super) fn end_process(pid: u32) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess},
    };
    if pid <= 4 || pid == std::process::id() {
        return Err("This process cannot be ended from here.".into());
    }
    // SAFETY: terminate-only access to one process id; a null handle is reported and a
    // valid one is closed before returning.
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return Err(format!(
                "Windows would not let the app open process {pid}: {}",
                std::io::Error::last_os_error()
            ));
        }
        let ended = TerminateProcess(handle, 1) != 0;
        let error = std::io::Error::last_os_error();
        CloseHandle(handle);
        if ended {
            Ok(())
        } else {
            Err(format!("Process {pid} could not be ended: {error}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(remote: &str, state: &str, protocol: &str) -> Connection {
        Connection {
            pid: 7,
            app: "C:/Apps/a.exe".into(),
            local: "10.0.0.2:5000".into(),
            remote: remote.into(),
            country: "DE".into(),
            protocol: protocol.into(),
            state: state.into(),
        }
    }

    #[test]
    fn header_clicks_cycle_ascending_descending_and_natural_order() {
        let first = Sort::clicked(None, Column::Remote);
        assert_eq!(
            first,
            Some(Sort {
                column: Column::Remote,
                ascending: true
            })
        );
        let second = Sort::clicked(first, Column::Remote);
        assert_eq!(second.map(|s| s.ascending), Some(false));
        assert_eq!(Sort::clicked(second, Column::Remote), None);
        assert_eq!(
            Sort::clicked(second, Column::Local).map(|s| (s.column, s.ascending)),
            Some((Column::Local, true))
        );
    }

    #[test]
    fn endpoints_sort_numerically_not_alphabetically() {
        let low = connection("9.9.9.9:443", "Established", "TCP4");
        let high = connection("10.0.0.1:80", "Established", "TCP4");
        let odd = connection("not an endpoint", "Established", "TCP4");
        assert_eq!(
            compare(Column::Remote, (&low, ""), (&high, "")),
            Ordering::Less
        );
        assert_eq!(
            compare(Column::Remote, (&high, ""), (&odd, "")),
            Ordering::Less,
            "Unparsable endpoints go last"
        );
        let v6 = connection("[2001:db8::1]:443", "Established", "TCP6");
        assert_eq!(endpoint_ip(&v6.remote), "2001:db8::1".parse().ok());
        assert_eq!(endpoint_ip("garbage"), None);
    }

    #[test]
    fn application_sort_ignores_case_and_breaks_ties_by_pid() {
        let a = connection("1.1.1.1:1", "Established", "TCP4");
        let mut b = a.clone();
        b.pid = 9;
        assert_eq!(
            compare(Column::Application, (&a, "chrome"), (&b, "Chrome")),
            Ordering::Less
        );
        assert_eq!(
            compare(Column::Application, (&a, "Zed"), (&b, "apple")),
            Ordering::Greater
        );
    }

    #[test]
    fn filters_match_protocol_families_and_states() {
        assert!(ProtocolFilter::Tcp.matches("TCP6") && !ProtocolFilter::Tcp.matches("UDP4"));
        assert!(ProtocolFilter::Udp.matches("UDP4") && ProtocolFilter::All.matches(""));
        assert!(StatusFilter::Established.matches("established"));
        assert!(StatusFilter::Listening.matches("Listening"));
        assert!(StatusFilter::Other.matches("Bound") && !StatusFilter::Other.matches("Listening"));
        assert!(StatusFilter::All.matches(""));
    }

    #[test]
    fn system_processes_and_this_app_cannot_be_ended() {
        for pid in [0, 4, std::process::id()] {
            assert!(end_process(pid).is_err(), "{pid}");
        }
        assert!(!reveal_in_explorer("relative.exe"));
        assert!(!reveal_in_explorer("C:/definitely/not/here.exe"));
        assert!(!open_folder(std::path::Path::new("relative")));
        assert!(!open_folder(std::path::Path::new("C:/definitely/not/here")));
    }

    #[test]
    fn describe_lists_every_field() {
        let c = connection("1.1.1.1:443", "Established", "TCP4");
        let text = describe("A", &c);
        for part in [
            "A",
            "PID 7",
            "TCP4",
            "10.0.0.2:5000",
            "1.1.1.1:443",
            "Established",
            "DE",
        ] {
            assert!(text.contains(part), "{text}");
        }
    }
}

//! Alerts for a first-time network app and for a monthly data cap.
//!
//! The decisions live here, away from Windows, so they can be tested. The worker feeds
//! in what it sees and shows whatever notices come back.
use chrono::{Datelike, TimeZone};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// What the user asked to be told about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AlertConfig {
    pub new_apps: bool,
    /// Monthly allowance in bytes, if a cap is on.
    pub cap_bytes: Option<u64>,
    /// Day of the month (1 to 28) on which the allowance starts over.
    pub reset_day: u8,
}

/// How much of a cap has been used. Only ever moves up within one cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub enum CapLevel {
    #[default]
    Below,
    /// 80 percent or more.
    Warning,
    Reached,
}

pub fn cap_level(used: u64, cap: u64) -> CapLevel {
    if cap == 0 || used >= cap {
        CapLevel::Reached
    } else if u128::from(used) * 100 >= u128::from(cap) * 80 {
        CapLevel::Warning
    } else {
        CapLevel::Below
    }
}

/// Where the current allowance period began: midnight on `reset_day`, this month if it
/// has come round yet and last month otherwise. Days past the 28th are not offered, so
/// every month has one.
pub fn cycle_start<T: TimeZone>(now: chrono::DateTime<T>, reset_day: u8) -> i64 {
    let day = u32::from(reset_day.clamp(1, 28));
    let date = now.date_naive();
    let month_of_start = if date.day() >= day {
        date.with_day(1)
    } else {
        date.with_day(1)
            .and_then(|first| first.checked_sub_months(chrono::Months::new(1)))
    };
    let start = month_of_start
        .and_then(|first| first.with_day(day))
        .and_then(|day| day.and_hms_opt(0, 0, 0));
    start
        .and_then(|local| now.timezone().from_local_datetime(&local).earliest())
        .map_or_else(|| now.timestamp(), |time| time.timestamp())
}

/// A message to show the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
}

/// The newest allowance state the user was told about, kept across restarts so a
/// restart does not repeat a warning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
struct CapMemory {
    cycle_start: i64,
    level: CapLevel,
}

pub struct Tracker {
    file: Option<PathBuf>,
    memory: CapMemory,
    seen: HashSet<String>,
}
impl Tracker {
    /// `folder` is where the cap memory is kept; none keeps it in memory only.
    pub fn new(folder: Option<&Path>) -> Self {
        let file = folder.map(|folder| folder.join("alerts.json"));
        let memory = file
            .as_ref()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            file,
            memory,
            seen: HashSet::new(),
        }
    }
    /// Apps already in the saved history, which are never "new".
    pub fn know(&mut self, apps: impl IntoIterator<Item = String>) {
        self.seen.extend(apps);
    }
    /// Notes that `app` used the network, returning a notice the first time ever.
    pub fn app_seen(&mut self, app: &str, name: &str, config: &AlertConfig) -> Option<Notice> {
        if app.is_empty() || !self.seen.insert(app.to_owned()) || !config.new_apps {
            return None;
        }
        Some(Notice {
            title: "New app on the network".into(),
            body: format!("{name} is using the network for the first time."),
        })
    }
    /// Compares this cycle's usage with the cap; speaks up once per level per cycle.
    pub fn cap_checked(&mut self, cycle_start: i64, used: u64, cap: u64) -> Option<Notice> {
        if self.memory.cycle_start != cycle_start {
            self.memory = CapMemory {
                cycle_start,
                level: CapLevel::Below,
            };
        }
        let level = cap_level(used, cap);
        if level <= self.memory.level {
            return None;
        }
        self.memory.level = level;
        if let Some(file) = &self.file
            && let Ok(text) = serde_json::to_string(&self.memory)
        {
            let _ = std::fs::write(file, text);
        }
        let used_text = crate::model::format_bytes(used as f64);
        let cap_text = crate::model::format_bytes(cap as f64);
        Some(match level {
            CapLevel::Reached => Notice {
                title: "Data cap reached".into(),
                body: format!("You have used {used_text} of your {cap_text} monthly cap."),
            },
            _ => Notice {
                title: "Data cap nearly reached".into(),
                body: format!(
                    "You have used {used_text} of your {cap_text} monthly cap (80% or more)."
                ),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn day(y: i32, m: u32, d: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 15, 30, 0).unwrap()
    }
    fn midnight(y: i32, m: u32, d: u32) -> i64 {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap().timestamp()
    }

    #[test]
    fn caps_warn_at_eighty_percent_and_stop_at_the_limit() {
        assert_eq!(cap_level(79, 100), CapLevel::Below);
        assert_eq!(cap_level(80, 100), CapLevel::Warning);
        assert_eq!(cap_level(99, 100), CapLevel::Warning);
        assert_eq!(cap_level(100, 100), CapLevel::Reached);
        assert_eq!(cap_level(1, 0), CapLevel::Reached);
        assert_eq!(cap_level(u64::MAX, u64::MAX), CapLevel::Reached);
        assert!(CapLevel::Reached > CapLevel::Warning && CapLevel::Warning > CapLevel::Below);
    }

    #[test]
    fn the_cycle_starts_on_the_reset_day_of_this_or_the_previous_month() {
        assert_eq!(cycle_start(day(2026, 11, 20), 15), midnight(2026, 11, 15));
        assert_eq!(cycle_start(day(2026, 11, 15), 15), midnight(2026, 11, 15));
        assert_eq!(cycle_start(day(2026, 11, 14), 15), midnight(2026, 10, 15));
        assert_eq!(cycle_start(day(2026, 1, 3), 10), midnight(2025, 12, 10));
        assert_eq!(cycle_start(day(2026, 3, 1), 1), midnight(2026, 3, 1));
        // Days that some months lack are not offered.
        assert_eq!(cycle_start(day(2026, 3, 30), 31), midnight(2026, 3, 28));
        assert_eq!(cycle_start(day(2026, 3, 5), 0), midnight(2026, 3, 1));
    }

    #[test]
    fn each_cap_level_is_announced_once_per_cycle_and_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut tracker = Tracker::new(Some(dir.path()));
        let cycle = 1_000;
        assert!(tracker.cap_checked(cycle, 10, 100).is_none());
        let warning = tracker.cap_checked(cycle, 85, 100).expect("warns at 85%");
        assert!(warning.title.contains("nearly"));
        assert!(tracker.cap_checked(cycle, 90, 100).is_none(), "Not again");
        let reached = tracker.cap_checked(cycle, 100, 100).expect("reached");
        assert!(reached.title.contains("reached"));
        assert!(tracker.cap_checked(cycle, 500, 100).is_none());

        let mut restarted = Tracker::new(Some(dir.path()));
        assert!(
            restarted.cap_checked(cycle, 500, 100).is_none(),
            "A restart does not repeat what the user was told"
        );
        assert!(
            restarted.cap_checked(cycle + 1, 500, 100).is_some(),
            "A new cycle starts over"
        );
    }

    #[test]
    fn only_the_first_use_of_an_unknown_app_is_announced() {
        let mut tracker = Tracker::new(None);
        tracker.know(["C:/old.exe".to_owned()]);
        let on = AlertConfig {
            new_apps: true,
            ..Default::default()
        };
        assert!(tracker.app_seen("C:/old.exe", "old", &on).is_none());
        let first = tracker.app_seen("C:/new.exe", "new", &on).expect("new");
        assert!(first.body.contains("new"));
        assert!(tracker.app_seen("C:/new.exe", "new", &on).is_none());
        assert!(tracker.app_seen("", "", &on).is_none());

        // With the alert off the app is still remembered, so turning it on later
        // does not announce everything seen meanwhile.
        let off = AlertConfig::default();
        assert!(tracker.app_seen("C:/quiet.exe", "quiet", &off).is_none());
        assert!(tracker.app_seen("C:/quiet.exe", "quiet", &on).is_none());
    }
}

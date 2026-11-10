//! Questions the pages ask about the current data and filters.
use super::*;

/// How much disk space the app's data folder takes, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct StorageUsage {
    /// The usage database and its write-ahead log.
    pub history: u64,
    pub settings: u64,
    pub other: u64,
}
impl StorageUsage {
    pub(super) fn total(&self) -> u64 {
        self.history
            .saturating_add(self.settings)
            .saturating_add(self.other)
    }
    /// Adds up every file under `dir`, sorting them by what they hold. A missing folder is empty.
    pub(super) fn measure(dir: &std::path::Path) -> Self {
        let mut usage = Self::default();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(folder) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                if meta.is_dir() {
                    pending.push(entry.path());
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_lowercase();
                let bucket = if name.starts_with("usage.db") {
                    &mut usage.history
                } else if name.starts_with("preferences.") {
                    &mut usage.settings
                } else {
                    &mut usage.other
                };
                *bucket = bucket.saturating_add(meta.len());
            }
        }
        usage
    }
}

impl MonitorApp {
    /// The data folder's size, measured again when it is a few seconds old.
    pub(super) fn storage_usage(&mut self) -> StorageUsage {
        const FRESH: std::time::Duration = std::time::Duration::from_secs(5);
        match self.storage {
            Some((read_at, usage)) if read_at.elapsed() < FRESH => usage,
            _ => {
                let usage = StorageUsage::measure(&self.data_dir);
                self.storage = Some((std::time::Instant::now(), usage));
                usage
            }
        }
    }
    /// Highest download and upload speeds among the chart's observed samples, or None when
    /// the period has none. A frozen chart reports the frozen samples.
    pub(super) fn peak_rates(&self) -> (Option<f64>, Option<f64>) {
        let (start, end) = self.chart_window();
        let points = self
            .frozen
            .as_ref()
            .map_or(&self.data.history.points, |frozen| &frozen.points);
        let mut peaks = (None::<u64>, None::<u64>);
        for point in points
            .iter()
            .filter(|p| p.observed && (start..=end).contains(&p.timestamp))
        {
            peaks.0 = Some(peaks.0.map_or(point.down, |peak| peak.max(point.down)));
            peaks.1 = Some(peaks.1.map_or(point.up, |peak| peak.max(point.up)));
        }
        (peaks.0.map(|v| v as f64), peaks.1.map(|v| v as f64))
    }

    /// The adapter filter's label, its value, and the full explanation shown on hover.
    pub(super) fn adapter_filter_caption(&self) -> (String, String, String) {
        let adapters = if self.settings.filter.adapters.is_empty() {
            "All adapters".to_owned()
        } else if self.settings.filter.adapters.contains(NO_ADAPTERS) {
            "No adapters".to_owned()
        } else {
            format!("{} adapters selected", self.settings.filter.adapters.len())
        };
        if let Some(selected) = &self.settings.filter.wifi_network {
            let name = if selected == WIFI_UNKNOWN {
                "Unidentified Wi-Fi".to_owned()
            } else {
                self.data
                    .snapshot
                    .wifi_networks
                    .iter()
                    .find(|network| network.id == *selected)
                    .map(|network| wifi_display_name(&network.name))
                    .unwrap_or_else(|| "Unavailable network".to_owned())
            };
            let tooltip = format!(
                "Wi-Fi: {name} · {adapters}. Only traffic on this connection within the selected adapters is shown."
            );
            ("Wi-Fi".to_owned(), name, tooltip)
        } else {
            let value = if self.settings.filter.adapters.is_empty() {
                "All".to_owned()
            } else if self.settings.filter.adapters.contains(NO_ADAPTERS) {
                "None".to_owned()
            } else {
                self.settings.filter.adapters.len().to_string()
            };
            (
                "Adapters".to_owned(),
                value,
                format!("{adapters} · All Wi-Fi connections"),
            )
        }
    }
}

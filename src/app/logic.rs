//! Questions the pages ask about the current data and filters.
use super::*;

impl MonitorApp {
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

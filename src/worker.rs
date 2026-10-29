use crate::{capture, model::*, store::Store};
use anyhow::Result;
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::atomic::Ordering,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Default, Clone)]
pub struct Shared {
    pub snapshot: Snapshot,
    pub history: Arc<History>,
    pub classified: ScopeTotals,
    pub top_apps: Vec<AppUsage>,
    pub history_generation: u64,
    pub notice: String,
    pub revision: u64,
    pub history_revision: u64,
}
pub enum Command {
    Filter(Filter, u64),
    Export(PathBuf, Filter),
    Geo(Option<PathBuf>),
    RetryCapture,
    ResolveHosts(Vec<String>),
    Stop,
}
pub struct Worker {
    pub shared: Arc<Mutex<Shared>>,
    pub tx: Sender<Command>,
    join: Option<thread::JoinHandle<()>>,
}
impl Worker {
    pub fn start(
        store: Store,
        filter: Filter,
        geo: Option<PathBuf>,
        repaint: eframe::egui::Context,
    ) -> Result<Self> {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (tx, rx) = mpsc::channel();
        let state = shared.clone();
        let join = thread::Builder::new()
            .name("network-collector".into())
            .spawn(move || run(store, filter, geo, state, rx, repaint))?;
        Ok(Self {
            shared,
            tx,
            join: Some(join),
        })
    }
    pub fn shutdown(&mut self) {
        if self.join.is_some() {
            let _ = self.tx.send(Command::Stop);
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
    #[cfg(any(test, feature = "ui-preview"))]
    pub(crate) fn for_ui_test() -> Self {
        let (tx, _) = mpsc::channel();
        Self {
            shared: Arc::default(),
            tx,
            join: None,
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown();
    }
}
#[derive(Default)]
struct Geo {
    reader: Option<maxminddb::Reader<Vec<u8>>>,
    cache: HashMap<IpAddr, String>,
}
impl Geo {
    fn load(&mut self, path: Option<PathBuf>) -> Result<()> {
        let reader = match path {
            Some(p) => Some(maxminddb::Reader::open_readfile(p)?),
            None => None,
        };
        self.reader = reader;
        self.cache.clear();
        Ok(())
    }
    fn country(&mut self, ip: IpAddr) -> String {
        let ip = normalize_ip(ip);
        if classify(ip, &[]) == Scope::Lan {
            return "Local".into();
        }
        if let Some(value) = self.cache.get(&ip) {
            return value.clone();
        }
        let value = self
            .reader
            .as_ref()
            .and_then(|r| r.lookup::<maxminddb::geoip2::Country>(ip).ok().flatten())
            .and_then(|c| c.country.and_then(|c| c.iso_code).map(str::to_owned))
            .unwrap_or_else(|| "Unknown".into());
        if self.cache.len() >= 4096 {
            self.cache.clear();
        }
        self.cache.insert(ip, value.clone());
        value
    }
}
fn notice(shared: &Arc<Mutex<Shared>>, text: String) {
    if let Ok(mut s) = shared.lock() {
        s.notice = text;
    }
}

fn effective_filter(filter: &Filter, known: &HashSet<String>) -> Filter {
    let mut result = filter.clone();
    if result.source == Source::Adapter {
        result.adapters = if result.adapters.is_empty() {
            known.clone()
        } else {
            result.adapters.intersection(known).cloned().collect()
        };
        if result.adapters.is_empty() {
            result.adapters.insert("__none__".into());
        }
    }
    result
}
fn read_histories(
    store: &Store,
    filter: &Filter,
    known: &HashSet<String>,
    now: i64,
) -> Result<(History, TransportSummary)> {
    let effective = effective_filter(filter, known);
    let history = store.history(&effective, now)?;
    let summary = if filter.source == Source::Adapter {
        store.transport_summary(
            &Filter {
                source: Source::Transport,
                scope: Scope::All,
                search: String::new(),
                apps: HashSet::new(),
                // An empty selection means all captured app traffic, just as on
                // App traffic. Expanding it to visible counter adapters drops
                // loopback and unattributed transfers (for example Steam LAN).
                ..filter.clone()
            },
            now,
        )?
    } else {
        TransportSummary::default()
    };
    Ok((history, summary))
}
#[derive(Clone, Debug)]
struct WifiInterval {
    network: String,
    start_ms: i64,
    end_ms: i64,
}

/// Keep confirmed, stable observation intervals for delayed ETW events. A
/// transition leaves the interval between the two observations unidentified.
#[derive(Default)]
struct WifiTracker {
    adapters: HashSet<String>,
    active: HashMap<String, WifiInterval>,
    past: VecDeque<(String, WifiInterval)>,
    last_ms: Option<i64>,
}
impl WifiTracker {
    fn observe(
        &mut self,
        now_ms: i64,
        adapters: &HashSet<String>,
        networks: &HashMap<String, WifiNetwork>,
    ) {
        self.adapters.extend(adapters.iter().cloned());
        self.adapters.extend(networks.keys().cloned());
        if self.last_ms.is_some_and(|last| now_ms < last) {
            // A system clock adjustment invalidates the old timestamp mapping.
            self.active.clear();
            self.past.clear();
        }
        self.last_ms = Some(now_ms);
        for adapter in &self.adapters {
            let network = networks
                .get(adapter)
                .map_or(WIFI_UNKNOWN, |n| n.id.as_str());
            if let Some(interval) = self.active.get_mut(adapter) {
                if interval.network == network {
                    interval.end_ms = now_ms;
                    continue;
                }
                self.past.push_back((adapter.clone(), interval.clone()));
            }
            self.active.insert(
                adapter.clone(),
                WifiInterval {
                    network: network.to_owned(),
                    start_ms: now_ms,
                    end_ms: now_ms,
                },
            );
        }
        let cutoff = now_ms.saturating_sub(120_000);
        self.past.retain(|(_, interval)| interval.end_ms >= cutoff);
        while self.past.len() > 4096 {
            self.past.pop_front();
        }
    }
    fn current(&self, adapter: &str) -> Option<&str> {
        self.active.get(adapter).map(|v| v.network.as_str())
    }
    fn event_network(&self, adapter: &str, timestamp: i64) -> String {
        if !self.adapters.contains(adapter) {
            return String::new();
        }
        let start = timestamp.saturating_mul(1000);
        let end = start.saturating_add(1000);
        let contains = |span: &WifiInterval| span.start_ms <= start && span.end_ms >= end;
        if let Some(span) = self.active.get(adapter).filter(|span| contains(span)) {
            return span.network.clone();
        }
        self.past
            .iter()
            .rev()
            .find(|(id, span)| id == adapter && contains(span))
            .map_or_else(|| WIFI_UNKNOWN.to_owned(), |(_, span)| span.network.clone())
    }
    fn sample_network(&self, adapter: &str, previous: Option<&str>) -> String {
        match self.current(adapter) {
            None => String::new(),
            Some(current) if previous == Some(current) => current.to_owned(),
            Some(_) => WIFI_UNKNOWN.to_owned(),
        }
    }
}
// Events have second-resolution timestamps. Hold Wi-Fi events until another
// observation confirms the end of their timestamp second. Bound the queue;
// overflow and shutdown are processed immediately as unidentified when needed.
struct BufferedEvent {
    event: TrafficEvent,
    adapter_id: String,
    adapter: String,
    scope: Scope,
    queued_at: Instant,
}
#[derive(Default)]
struct EventBuffer {
    pending: VecDeque<BufferedEvent>,
}
impl EventBuffer {
    const LIMIT: usize = 65_536;
    fn ready(event: &BufferedEvent, tracker: &WifiTracker, flush: bool) -> bool {
        flush
            || !tracker.adapters.contains(&event.adapter_id)
            || tracker.last_ms.is_some_and(|now| {
                event.event.timestamp.saturating_mul(1000).saturating_add(1000) <= now
            })
            // A future timestamp after a clock adjustment must not wait forever.
            || event.queued_at.elapsed() >= Duration::from_secs(3)
    }
    fn take_ready(&mut self, tracker: &WifiTracker, flush: bool) -> Vec<BufferedEvent> {
        let mut ready = Vec::new();
        let mut waiting = VecDeque::new();
        for event in self.pending.drain(..) {
            if Self::ready(&event, tracker, flush) {
                ready.push(event);
            } else {
                waiting.push_back(event);
            }
        }
        self.pending = waiting;
        ready
    }
    fn push(
        &mut self,
        event: BufferedEvent,
        tracker: &WifiTracker,
        flush: bool,
        ready: &mut Vec<BufferedEvent>,
    ) {
        if Self::ready(&event, tracker, flush) {
            ready.push(event);
        } else {
            if self.pending.len() >= Self::LIMIT
                && let Some(oldest) = self.pending.pop_front()
            {
                ready.push(oldest);
            }
            self.pending.push_back(event);
        }
    }
}

fn run(
    mut store: Store,
    mut filter: Filter,
    geo_path: Option<PathBuf>,
    shared: Arc<Mutex<Shared>>,
    rx: Receiver<Command>,
    ctx: eframe::egui::Context,
) {
    let mut hosts = crate::hosts::Resolver::new();
    let mut geo = Geo::default();
    if let Err(e) = geo.load(geo_path) {
        notice(&shared, format!("Country database: {e}"));
    }
    let mut etw = capture::EtwCapture::start();
    let mut wifi_monitor = capture::wifi::WifiMonitor::new();
    let mut wifi_tracker = WifiTracker::default();
    let mut wifi_catalog: HashMap<String, WifiNetwork> = store
        .known_wifi_networks()
        .unwrap_or_default()
        .into_iter()
        .map(|network| (network.id.clone(), network))
        .collect();
    let mut wifi_catalog_dirty = false;
    let mut previous_wifi = HashMap::<String, String>::new();
    let mut previous = HashMap::<String, (u64, u64)>::new();
    let mut names = HashMap::new();
    let mut labels = HashMap::new();
    let mut connections = Vec::new();
    let mut visible = HashSet::new();
    let mut visibility_error = None;
    let mut known = store.known_adapter_ids().unwrap_or_default();
    if let Ok(ids) = capture::visible_adapter_ids() {
        visible = ids;
        known.extend(visible.iter().cloned());
    }
    let mut wifi_state = wifi_monitor.poll();
    if let Ok(adapters) = capture::all_adapters() {
        wifi_state
            .wifi_adapters
            .extend(adapters.iter().filter(|a| a.is_wifi).map(|a| a.id.clone()));
        previous = adapters
            .into_iter()
            .filter(|a| visible.contains(&a.id))
            .map(|a| (a.id, (a.received, a.sent)))
            .collect();
    }
    wifi_tracker.observe(
        chrono::Utc::now().timestamp_millis(),
        &wifi_state.wifi_adapters,
        &wifi_state.networks,
    );
    for (id, interval) in &wifi_tracker.active {
        previous_wifi.insert(id.clone(), interval.network.clone());
    }
    for network in wifi_state.networks.values() {
        if wifi_catalog.get(&network.id) != Some(network) {
            wifi_catalog.insert(network.id.clone(), network.clone());
            wifi_catalog_dirty = true;
        }
    }
    if let Ok(mut state) = shared.lock() {
        state.snapshot.wifi_networks = wifi_catalog.values().cloned().collect();
    }
    let mut tick = Instant::now();
    let mut last_refresh = Instant::now() - Duration::from_secs(10);
    let mut last_names = Instant::now() - Duration::from_secs(10);
    let mut last_visibility = Instant::now() - Duration::from_secs(60);
    let mut last_prune = Instant::now() - Duration::from_secs(3600);
    let mut filter_revision = 0;
    let mut refresh = true;
    let mut pending = BTreeMap::<i64, Vec<UsageRow>>::new();
    let mut storage_failed = false;
    let mut pending_samples = Vec::<LiveTick>::new();
    let mut event_buffer = EventBuffer::default();
    // Load saved data immediately; reopening must not wait for a new live sample.
    if let Ok((history, summary)) =
        read_histories(&store, &filter, &known, chrono::Utc::now().timestamp())
    {
        let paths: HashMap<u32, String> = summary
            .top_apps
            .iter()
            .enumerate()
            .map(|(i, a)| (i as u32, a.app.clone()))
            .collect();
        labels.extend(capture::process_labels(&paths));
        if let Ok(mut state) = shared.lock() {
            state.history = Arc::new(history);
            state.classified = summary.classified;
            state.top_apps = summary.top_apps;
            state.snapshot.app_labels = labels.clone();
            state.history_generation += 1;
        }
    }
    loop {
        let wait = Duration::from_secs(1).saturating_sub(tick.elapsed());
        let mut stop = false;
        let first = rx.recv_timeout(wait);
        for cmd in first.into_iter().chain(rx.try_iter()) {
            match cmd {
                Command::Stop => {
                    stop = true;
                    break;
                }
                Command::Filter(f, revision) => {
                    filter = f;
                    filter_revision = revision;
                    refresh = true;
                }
                Command::Export(path, f) => {
                    let msg = match store.export(
                        &path,
                        &effective_filter(&f, &known),
                        chrono::Utc::now().timestamp(),
                    ) {
                        Ok(n) => format!("Exported {n} rows to {}", path.display()),
                        Err(e) => format!("Export failed: {e:#}"),
                    };
                    notice(&shared, msg);
                }
                Command::Geo(path) => {
                    notice(
                        &shared,
                        match geo.load(path) {
                            Ok(()) => "Country lookup updated for new traffic.".into(),
                            Err(e) => format!("Country database could not load: {e}"),
                        },
                    );
                }
                Command::ResolveHosts(addresses) => hosts.request(&addresses),
                Command::RetryCapture => {
                    drop(etw);
                    etw = capture::EtwCapture::start();
                }
            }
        }
        if !stop
            && hosts.poll()
            && let Ok(mut state) = shared.lock()
        {
            state.snapshot.host_labels = hosts.labels();
            state.revision += 1;
        }
        let sample = stop || tick.elapsed() >= Duration::from_secs(1);
        if sample {
            if stop && let Ok(c) = &mut etw {
                c.stop();
            }
            let interval = tick.elapsed().as_secs_f64().max(0.001);
            tick = Instant::now();
            let now = chrono::Utc::now().timestamp();
            let mut snapshot = Snapshot {
                interval,
                captured_at: now,
                ..Default::default()
            };
            let all_adapters = match capture::all_adapters() {
                Ok(a) => a,
                Err(e) => {
                    snapshot.warnings.push(e.to_string());
                    previous.clear();
                    Vec::new()
                }
            };
            let mut wifi_state = wifi_monitor.poll();
            wifi_state.wifi_adapters.extend(
                all_adapters
                    .iter()
                    .filter(|a| a.is_wifi)
                    .map(|a| a.id.clone()),
            );
            wifi_tracker.observe(
                chrono::Utc::now().timestamp_millis(),
                &wifi_state.wifi_adapters,
                &wifi_state.networks,
            );
            for network in wifi_state.networks.values() {
                if wifi_catalog.get(&network.id) != Some(network) {
                    wifi_catalog.insert(network.id.clone(), network.clone());
                    wifi_catalog_dirty = true;
                }
            }
            if wifi_catalog_dirty {
                let networks: Vec<_> = wifi_catalog.values().cloned().collect();
                match store.remember_wifi_networks(&networks) {
                    Ok(()) => wifi_catalog_dirty = false,
                    Err(e) => snapshot
                        .warnings
                        .push(format!("Could not save Wi-Fi catalog: {e}")),
                }
            }
            if !wifi_state.wifi_adapters.is_empty()
                && let Some(warning) = wifi_state.warning
            {
                snapshot.warnings.push(warning);
            }
            snapshot.wifi_networks = wifi_catalog.values().cloned().collect();
            snapshot
                .wifi_networks
                .sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
            if last_visibility.elapsed() >= Duration::from_secs(30) {
                match capture::visible_adapter_ids() {
                    Ok(ids) => {
                        known.extend(
                            capture::historical_adapter_ids().unwrap_or_else(|_| ids.clone()),
                        );
                        known.extend(ids.iter().cloned());
                        if let Err(e) = store.remember_adapters(&known, &all_adapters, now) {
                            snapshot
                                .warnings
                                .push(format!("Could not save adapter catalog: {e}"));
                        }
                        visible = ids;
                        visibility_error = None;
                    }
                    Err(e) => {
                        visibility_error =
                            Some(format!("Could not read Windows adapter visibility: {e}"))
                    }
                }
                last_visibility = Instant::now();
            }
            if let Some(e) = &visibility_error {
                snapshot.warnings.push(e.clone());
            }
            if last_names.elapsed() >= Duration::from_secs(5) {
                match capture::process_names() {
                    Ok(n) => {
                        // Final event attribution still needs fresh PID names, but
                        // display labels and socket inventories are no longer needed.
                        if !stop {
                            labels.extend(capture::process_labels(&n));
                        }
                        names = n;
                    }
                    Err(e) => snapshot.warnings.push(e.to_string()),
                }
                if !stop {
                    match capture::connections(&names) {
                        Ok(mut c) => {
                            for r in &mut c {
                                if let Ok(addr) = r.remote.parse::<SocketAddr>() {
                                    r.country = geo.country(addr.ip());
                                }
                            }
                            connections = c;
                        }
                        Err(e) => snapshot.warnings.push(e.to_string()),
                    }
                }
                last_names = Instant::now();
            }
            let mut rows = Vec::new();
            let mut current = HashMap::new();
            for a in all_adapters.iter().filter(|a| visible.contains(&a.id)) {
                current.insert(a.id.clone(), (a.received, a.sent));
                if let Some((down, up)) = previous.get(&a.id) {
                    rows.push(UsageRow {
                        source: "adapter".into(),
                        adapter_id: a.id.clone(),
                        adapter: a.name.clone(),
                        wifi_network: wifi_tracker
                            .sample_network(&a.id, previous_wifi.get(&a.id).map(String::as_str)),
                        scope: "Unknown".into(),
                        down: counter_delta(*down, a.received),
                        up: counter_delta(*up, a.sent),
                        ..Default::default()
                    });
                }
            }
            previous = current;
            previous_wifi = wifi_tracker
                .active
                .iter()
                .map(|(id, interval)| (id.clone(), interval.network.clone()))
                .collect();
            let live_tick = LiveTick {
                timestamp: now,
                interval,
                adapters: rows
                    .iter()
                    .map(|r| (r.adapter_id.clone(), (r.down, r.up)))
                    .collect(),
                wifi_networks: rows
                    .iter()
                    .map(|r| (r.adapter_id.clone(), r.wifi_network.clone()))
                    .collect(),
            };
            pending_samples.push(live_tick);
            let mut event_rows = BTreeMap::<
                (i64, String, u32, String, String, String, String, String),
                UsageRow,
            >::new();
            let mut ready_events = event_buffer.take_ready(&wifi_tracker, stop);
            match &etw {
                Ok(c) => {
                    snapshot.etw_active = true;
                    snapshot.capture_status = "Detailed monitoring is active".into();
                    snapshot.parsed_events = c.stats.parsed.load(Ordering::Relaxed);
                    snapshot.parse_errors = c.stats.errors.load(Ordering::Relaxed);
                    snapshot.queue_drops = c.stats.drops.load(Ordering::Relaxed);
                    if !stop {
                        match c.health() {
                            Ok(lost) => snapshot.kernel_losses = lost,
                            Err(e) => {
                                snapshot.etw_active = false;
                                snapshot.warnings.push(e.to_string());
                            }
                        }
                    }
                    for event in c.events.try_iter().take(65_536) {
                        let adapter = adapter_for(event.local, &all_adapters);
                        let scope = classify(
                            event.remote,
                            adapter.map(|a| a.addresses.as_slice()).unwrap_or(&[]),
                        );
                        event_buffer.push(
                            BufferedEvent {
                                event,
                                adapter_id: adapter
                                    .map(|a| a.id.clone())
                                    .unwrap_or_else(|| "unassigned".into()),
                                adapter: adapter
                                    .map(|a| a.name.clone())
                                    .unwrap_or_else(|| "Unassigned".into()),
                                scope,
                                queued_at: Instant::now(),
                            },
                            &wifi_tracker,
                            stop,
                            &mut ready_events,
                        );
                    }
                }
                Err(e) => snapshot.capture_status = e.to_string(),
            }
            for buffered in ready_events {
                let BufferedEvent {
                    event,
                    adapter_id,
                    adapter,
                    scope,
                    ..
                } = buffered;
                let wifi_network = wifi_tracker.event_network(&adapter_id, event.timestamp);
                let remote = normalize_ip(event.remote).to_string();
                let traffic_type =
                    crate::traffic::classify(event.protocol, event.local_port, event.remote_port)
                        .to_owned();
                let key = (
                    event.timestamp / 60 * 60,
                    adapter_id.to_owned(),
                    event.pid,
                    remote.clone(),
                    event.protocol.into(),
                    scope.key().into(),
                    traffic_type.clone(),
                    wifi_network.clone(),
                );
                let row = event_rows.entry(key).or_insert_with(|| UsageRow {
                    traffic_type,
                    wifi_network,
                    source: "transport".into(),
                    adapter_id,
                    adapter,
                    app: names
                        .get(&event.pid)
                        .cloned()
                        .unwrap_or_else(|| format!("Process {}", event.pid)),
                    pid: event.pid,
                    remote,
                    country: geo.country(event.remote),
                    protocol: event.protocol.into(),
                    scope: scope.key().into(),
                    ..Default::default()
                });
                row.down += event.down;
                row.up += event.up;
            }
            if !storage_failed {
                pending
                    .entry(now / 60 * 60)
                    .or_default()
                    .extend(rows.iter().cloned());
                for ((minute, ..), row) in &event_rows {
                    pending.entry(*minute).or_default().push(row.clone());
                }
                match store.write_capture(&pending, &pending_samples) {
                    Ok(()) => {
                        pending.clear();
                        pending_samples.clear();
                    }
                    Err(e) => snapshot.warnings.push(format!(
                        "History write failed; retaining samples for retry: {e}"
                    )),
                }
                if pending.values().map(Vec::len).sum::<usize>() > 100_000 {
                    storage_failed = true;
                    pending.clear();
                    pending_samples.clear();
                }
            }
            // The final ETW buffers and adapter sample have now been collected
            // and a transactional write attempted. Don't rebuild UI snapshots,
            // reread history, or prune while exiting.
            if stop {
                if let Err(e) = store.checkpoint() {
                    notice(&shared, format!("Final history checkpoint: {e}"));
                }
                break;
            }
            if storage_failed {
                snapshot.warnings.push("History recording stopped after repeated storage failures. Restart after fixing disk access.".into());
            }
            rows.extend(event_rows.into_values());
            snapshot.live_rows = rows;
            snapshot.adapters = all_adapters
                .into_iter()
                .filter(|a| visible.contains(&a.id))
                .collect();
            snapshot.connections = connections.clone();
            snapshot.app_labels = labels.clone();
            snapshot.host_labels = hosts.labels();
            if let Ok(mut s) = shared.lock() {
                s.snapshot = snapshot;
                s.revision += 1;
            }
            if last_prune.elapsed() >= Duration::from_secs(3600) {
                if let Err(e) = store.prune_samples(now - 86400) {
                    notice(&shared, format!("History cleanup failed: {e}"));
                }
                last_prune = Instant::now();
            }
        }
        let (start, end) = filter.time_bounds(chrono::Utc::now().timestamp());
        let refresh_interval = if !filter.all_time() && end.saturating_sub(start) <= 86400 {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(5)
        };
        if (refresh && !visible.is_empty())
            || (sample && last_refresh.elapsed() >= refresh_interval)
        {
            match read_histories(&store, &filter, &known, chrono::Utc::now().timestamp()) {
                Ok((history, summary)) => {
                    let unique: HashSet<String> = history
                        .rows
                        .iter()
                        .filter(|r| !r.app.is_empty())
                        .map(|r| r.app.clone())
                        .chain(summary.top_apps.iter().map(|r| r.app.clone()))
                        .collect();
                    let paths: HashMap<u32, String> = unique
                        .into_iter()
                        .enumerate()
                        .map(|(i, p)| (i as u32, p))
                        .collect();
                    labels.extend(capture::process_labels(&paths));
                    if let Ok(mut s) = shared.lock() {
                        s.history = Arc::new(history);
                        s.history_generation += 1;
                        s.classified = summary.classified;
                        s.top_apps = summary.top_apps;
                        s.history_revision = filter_revision;
                        s.snapshot.app_labels = labels.clone();
                    }
                }
                Err(e) => notice(&shared, format!("History could not load: {e}")),
            }
            refresh = false;
            last_refresh = Instant::now();
        }
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_adapter_history_excludes_old_hidden_samples() -> Result<()> {
        let mut store = Store::open(std::path::Path::new(":memory:"))?;
        store.write(
            600,
            &[
                UsageRow {
                    source: "adapter".into(),
                    adapter_id: "visible".into(),
                    down: 42,
                    ..Default::default()
                },
                UsageRow {
                    source: "adapter".into(),
                    adapter_id: "hidden".into(),
                    down: 999,
                    ..Default::default()
                },
            ],
        )?;
        let (history, _) = read_histories(
            &store,
            &Filter::default(),
            &HashSet::from(["visible".into()]),
            660,
        )?;
        assert_eq!(history.down, 42);
        Ok(())
    }
    #[test]
    fn overall_top_apps_match_app_traffic_including_loopback_and_unattributed_bytes() -> Result<()>
    {
        let mut store = Store::open(std::path::Path::new(":memory:"))?;
        let base = UsageRow {
            source: "transport".into(),
            adapter_id: "visible".into(),
            scope: "WAN".into(),
            ..Default::default()
        };
        let mut rows: Vec<_> = (0..6)
            .map(|i| UsageRow {
                app: format!("browser{i}.exe"),
                down: (i + 1) * 1000,
                ..base.clone()
            })
            .collect();
        rows.extend([
            UsageRow {
                app: "steam.exe".into(),
                adapter_id: "loopback".into(),
                scope: "LAN".into(),
                down: 7 * 1024 * 1024 * 1024,
                ..base.clone()
            },
            UsageRow {
                app: "steam.exe".into(),
                pid: 42,
                down: 10,
                up: 5,
                ..base.clone()
            },
            UsageRow {
                app: "unattributed.exe".into(),
                adapter_id: String::new(),
                down: 2 * 1024 * 1024 * 1024,
                ..base.clone()
            },
            UsageRow {
                source: "adapter".into(),
                adapter_id: "visible".into(),
                down: 42,
                ..Default::default()
            },
            UsageRow {
                source: "adapter".into(),
                adapter_id: "loopback".into(),
                down: 999,
                ..Default::default()
            },
        ]);
        store.write(600, &rows)?;
        store.write(
            300,
            &[UsageRow {
                app: "old.exe".into(),
                down: u64::MAX / 2,
                ..base
            }],
        )?;
        let known = HashSet::from(["visible".into()]);
        for adapters in [
            HashSet::new(),
            HashSet::from(["visible".into()]),
            HashSet::from(["visible".into(), "loopback".into()]),
            HashSet::from(["missing".into()]),
        ] {
            let filter = Filter {
                seconds: 300,
                adapters,
                ..Default::default()
            };
            let (history, summary) = read_histories(&store, &filter, &known, 900)?;
            assert_eq!(
                history.down,
                if filter.adapters.contains("missing") {
                    0
                } else {
                    42
                }
            );
            let apps = store.history(
                &Filter {
                    source: Source::Transport,
                    ..filter.clone()
                },
                900,
            )?;
            let mut expected = std::collections::BTreeMap::<String, crate::model::AppUsage>::new();
            for row in apps.rows {
                let app =
                    expected
                        .entry(row.app.clone())
                        .or_insert_with(|| crate::model::AppUsage {
                            app: row.app,
                            ..Default::default()
                        });
                app.down += row.down;
                app.up += row.up;
            }
            let mut expected: Vec<_> = expected.into_values().collect();
            expected.sort_by(|a, b| {
                (b.down + b.up)
                    .cmp(&(a.down + a.up))
                    .then(a.app.cmp(&b.app))
            });
            expected.truncate(5);
            assert_eq!(
                summary.top_apps, expected,
                "selection: {:?}",
                filter.adapters
            );
            assert_eq!(
                summary.classified.lan + summary.classified.wan,
                apps.down + apps.up
            );
            if filter.adapters.is_empty() {
                assert_eq!(summary.top_apps[0].app, "steam.exe");
                assert_eq!(summary.top_apps[0].down, 7 * 1024 * 1024 * 1024 + 10);
                // No known counter adapters must not hide saved app transfers either.
                assert_eq!(
                    read_histories(&store, &filter, &HashSet::new(), 900)?
                        .1
                        .top_apps,
                    summary.top_apps
                );
            }
        }
        Ok(())
    }

    #[test]
    fn wifi_timeline_keeps_switch_intervals_and_unobserved_events_unknown() {
        let mut tracker = WifiTracker::default();
        let adapters = HashSet::from(["wifi".to_owned()]);
        let home = HashMap::from([(
            "wifi".to_owned(),
            WifiNetwork {
                id: "ssid:home".into(),
                name: "Home".into(),
            },
        )]);
        let office = HashMap::from([(
            "wifi".to_owned(),
            WifiNetwork {
                id: "ssid:office".into(),
                name: "Office".into(),
            },
        )]);
        tracker.observe(300, &adapters, &home);
        assert_eq!(tracker.sample_network("wifi", None), WIFI_UNKNOWN);
        tracker.observe(1300, &adapters, &home);
        tracker.observe(2300, &adapters, &home);
        assert_eq!(tracker.event_network("wifi", 1), "ssid:home");
        assert_eq!(tracker.event_network("wifi", 0), WIFI_UNKNOWN);
        assert_eq!(
            tracker.sample_network("wifi", Some("ssid:home")),
            "ssid:home"
        );
        tracker.observe(3300, &adapters, &office);
        assert_eq!(
            tracker.sample_network("wifi", Some("ssid:home")),
            WIFI_UNKNOWN
        );
        assert_eq!(tracker.event_network("wifi", 2), WIFI_UNKNOWN);
        assert_eq!(tracker.event_network("wifi", 1), "ssid:home");
        tracker.observe(4300, &adapters, &office);
        tracker.observe(5300, &adapters, &office);
        assert_eq!(tracker.event_network("wifi", 4), "ssid:office");
        assert_eq!(tracker.event_network("ethernet", 4), "");
        tracker.observe(6300, &adapters, &HashMap::new());
        assert_eq!(
            tracker.sample_network("wifi", Some("ssid:office")),
            WIFI_UNKNOWN
        );
        tracker.observe(100, &adapters, &home);
        assert_eq!(tracker.event_network("wifi", 1), WIFI_UNKNOWN);
    }
    #[test]
    fn wifi_events_wait_for_confirmed_timestamp_and_flush_on_exit() {
        let mut tracker = WifiTracker::default();
        let adapters = HashSet::from(["wifi".into()]);
        let networks = HashMap::from([(
            "wifi".into(),
            WifiNetwork {
                id: "home".into(),
                name: "Home".into(),
            },
        )]);
        tracker.observe(100, &adapters, &networks);
        tracker.observe(1100, &adapters, &networks);
        let make_event = || BufferedEvent {
            event: TrafficEvent {
                local_port: 10,
                remote_port: 443,
                timestamp: 1,
                pid: 1,
                local: "192.168.0.2".parse().unwrap(),
                remote: "1.1.1.1".parse().unwrap(),
                protocol: "TCP",
                down: 7,
                up: 0,
            },
            adapter_id: "wifi".into(),
            adapter: "Wi-Fi".into(),
            scope: Scope::Wan,
            queued_at: Instant::now(),
        };
        let mut buffer = EventBuffer::default();
        let mut ready = Vec::new();
        buffer.push(make_event(), &tracker, false, &mut ready);
        assert!(ready.is_empty());
        assert!(buffer.take_ready(&tracker, false).is_empty());
        tracker.observe(2100, &adapters, &networks);
        let ready = buffer.take_ready(&tracker, false);
        assert_eq!(ready.len(), 1);
        assert_eq!(
            tracker.event_network(&ready[0].adapter_id, ready[0].event.timestamp),
            "home"
        );

        let mut next = make_event();
        next.event.timestamp = 2;
        buffer.push(next, &tracker, false, &mut Vec::new());
        let flushed = buffer.take_ready(&tracker, true);
        assert_eq!(flushed.len(), 1);
        assert_eq!(
            tracker.event_network(&flushed[0].adapter_id, flushed[0].event.timestamp),
            WIFI_UNKNOWN
        );
    }
    #[test]
    fn live_points_follow_selected_adapter_and_actual_interval() {
        let sample = LiveTick {
            timestamp: 100,
            interval: 0.5,
            adapters: HashMap::from([("a".into(), (500, 100)), ("b".into(), (1000, 200))]),
            wifi_networks: HashMap::new(),
        };
        assert_eq!(sample.point(&HashSet::from(["a".into()])).down, 1000);
        assert_eq!(sample.point(&HashSet::new()).up, 600);
    }
}

#[cfg(test)]
mod native_restart_test {
    use super::*;
    #[test]
    #[ignore = "Requires elevated ETW access and generates local traffic in a temporary database"]
    fn graceful_exit_preserves_final_tcp_events_and_adapter_samples() -> Result<()> {
        use std::{
            io::{Read, Write},
            net::{TcpListener, TcpStream},
        };

        let dir = tempfile::tempdir()?;
        let path = dir.path().join("final-tcp-events.db");
        let mut worker = Worker::start(
            Store::open(&path)?,
            Filter::default(),
            None,
            eframe::egui::Context::default(),
        )?;
        let started = Instant::now();
        loop {
            if worker
                .shared
                .lock()
                .map(|s| s.snapshot.etw_active)
                .unwrap_or(false)
            {
                break;
            }
            anyhow::ensure!(
                started.elapsed() < Duration::from_secs(5),
                "ETW capture is unavailable; run the test as administrator"
            );
            thread::sleep(Duration::from_millis(20));
        }
        // Generate known TCP traffic just before stopping the producer. The
        // temporary store must contain the final, drained ETW events after exit.
        const PAYLOAD_BYTES: usize = 256 * 1024;
        let listener = TcpListener::bind("127.0.0.2:0")?;
        let address = listener.local_addr()?;
        let reader = thread::spawn(move || -> std::io::Result<usize> {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            let mut total = 0;
            let mut buffer = [0u8; 16384];
            loop {
                let n = stream.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                total += n;
            }
            Ok(total)
        });
        let mut stream = TcpStream::connect(address)?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        stream.write_all(&vec![42u8; PAYLOAD_BYTES])?;
        drop(stream);
        anyhow::ensure!(
            reader
                .join()
                .map_err(|_| anyhow::anyhow!("TCP test reader stopped"))??
                == PAYLOAD_BYTES,
            "TCP test payload was not delivered"
        );
        let last_sample_time = chrono::Utc::now().timestamp();
        let shutdown_started = Instant::now();
        worker.shutdown();
        let shutdown_elapsed = shutdown_started.elapsed();

        let saved = Store::open(&path)?;
        let now = chrono::Utc::now().timestamp();
        let app_history = saved.history(
            &Filter {
                source: Source::Transport,
                seconds: 300,
                ..Default::default()
            },
            now,
        )?;
        let (down, up) = app_history
            .rows
            .iter()
            .filter(|r| r.pid == std::process::id() && r.protocol == "TCP")
            .fold((0u64, 0u64), |(d, u), r| (d + r.down, u + r.up));
        anyhow::ensure!(
            (down, up) == (PAYLOAD_BYTES as u64, PAYLOAD_BYTES as u64),
            "Final TCP bytes did not survive shutdown: downloaded {down}, uploaded {up}"
        );
        let adapter_history = saved.history(
            &Filter {
                seconds: 300,
                ..Default::default()
            },
            now,
        )?;
        anyhow::ensure!(
            adapter_history
                .points
                .iter()
                .any(|p| p.observed && p.timestamp >= last_sample_time),
            "Final adapter sample did not survive shutdown"
        );
        println!(
            "Final TCP bytes and adapter sample restored; worker shutdown took {:.1} ms",
            shutdown_elapsed.as_secs_f64() * 1000.0
        );
        Ok(())
    }

    #[test]
    #[ignore = "Uses the local Windows adapter inventory and a temporary database"]
    fn graceful_exit_and_immediate_reopen_restore_the_recent_chart() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("live-restart.db");
        let filter = Filter {
            seconds: 300,
            ..Default::default()
        };
        let mut first = Worker::start(
            Store::open(&path)?,
            filter.clone(),
            None,
            eframe::egui::Context::default(),
        )?;
        thread::sleep(Duration::from_millis(2150));
        first.shutdown();
        let stored = Store::open(&path)?.history(&filter, chrono::Utc::now().timestamp())?;
        let last = stored
            .points
            .iter()
            .filter(|p| p.observed)
            .map(|p| p.timestamp)
            .max()
            .ok_or_else(|| anyhow::anyhow!("No native adapter samples were recorded"))?;
        let mut second = Worker::start(
            Store::open(&path)?,
            filter,
            None,
            eframe::egui::Context::default(),
        )?;
        let started = Instant::now();
        let mut restored = false;
        while started.elapsed() < Duration::from_millis(900) {
            if let Ok(state) = second.shared.lock() {
                restored = state.history_generation > 0
                    && state
                        .history
                        .points
                        .iter()
                        .any(|p| p.observed && p.timestamp == last);
            }
            if restored {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        second.shutdown();
        anyhow::ensure!(restored, "Recent chart did not restore before a new sample");
        println!(
            "Recent chart restored before the first new live tick; final recorded timestamp {last}"
        );
        Ok(())
    }
}

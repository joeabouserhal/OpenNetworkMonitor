use crate::{capture, model::*, store::Store};
use anyhow::Result;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
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
    if let Ok(adapters) = capture::all_adapters() {
        previous = adapters
            .into_iter()
            .filter(|a| visible.contains(&a.id))
            .map(|a| (a.id, (a.received, a.sent)))
            .collect();
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
        if hosts.poll()
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
                        labels.extend(capture::process_labels(&n));
                        names = n;
                    }
                    Err(e) => snapshot.warnings.push(e.to_string()),
                }
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
                        scope: "Unknown".into(),
                        down: counter_delta(*down, a.received),
                        up: counter_delta(*up, a.sent),
                        ..Default::default()
                    });
                }
            }
            previous = current;
            let live_tick = LiveTick {
                timestamp: now,
                interval,
                adapters: rows
                    .iter()
                    .map(|r| (r.adapter_id.clone(), (r.down, r.up)))
                    .collect(),
            };
            pending_samples.push(live_tick);
            let mut event_rows =
                BTreeMap::<(i64, String, u32, String, String, String, String), UsageRow>::new();
            match &etw {
                Ok(c) => {
                    snapshot.etw_active = true;
                    snapshot.capture_status = "Detailed monitoring is active".into();
                    snapshot.parsed_events = c.stats.parsed.load(Ordering::Relaxed);
                    snapshot.parse_errors = c.stats.errors.load(Ordering::Relaxed);
                    snapshot.queue_drops = c.stats.drops.load(Ordering::Relaxed);
                    match c.health() {
                        Ok(lost) => snapshot.kernel_losses = lost,
                        Err(e) if !stop => {
                            snapshot.etw_active = false;
                            snapshot.warnings.push(e.to_string());
                        }
                        _ => {}
                    }
                    for event in c.events.try_iter().take(65_536) {
                        let adapter = adapter_for(event.local, &all_adapters);
                        let scope = classify(
                            event.remote,
                            adapter.map(|a| a.addresses.as_slice()).unwrap_or(&[]),
                        );
                        let adapter_id = adapter.map(|a| a.id.as_str()).unwrap_or("unassigned");
                        let remote = normalize_ip(event.remote).to_string();
                        let traffic_type = crate::traffic::classify(
                            event.protocol,
                            event.local_port,
                            event.remote_port,
                        )
                        .to_owned();
                        let key = (
                            event.timestamp / 60 * 60,
                            adapter_id.to_owned(),
                            event.pid,
                            remote.clone(),
                            event.protocol.into(),
                            scope.key().into(),
                            traffic_type.clone(),
                        );
                        let row = event_rows.entry(key).or_insert_with(|| UsageRow {
                            traffic_type,
                            source: "transport".into(),
                            adapter_id: adapter_id.into(),
                            adapter: adapter
                                .map(|a| a.name.clone())
                                .unwrap_or_else(|| "Unassigned".into()),
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
                }
                Err(e) => snapshot.capture_status = e.to_string(),
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
                if let Err(e) = store.prune(now - 30 * 86400) {
                    notice(&shared, format!("History cleanup failed: {e}"));
                }
                last_prune = Instant::now();
            }
        }
        let refresh_interval = if filter.source == Source::Adapter || filter.seconds <= 86400 {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(5)
        };
        if (refresh && !visible.is_empty())
            || (sample && last_refresh.elapsed() >= refresh_interval)
            || stop
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
        if stop {
            if let Err(e) = store.checkpoint() {
                notice(&shared, format!("Final history checkpoint: {e}"));
            }
            break;
        }
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
    fn live_points_follow_selected_adapter_and_actual_interval() {
        let sample = LiveTick {
            timestamp: 100,
            interval: 0.5,
            adapters: HashMap::from([("a".into(), (500, 100)), ("b".into(), (1000, 200))]),
        };
        assert_eq!(sample.point(&HashSet::from(["a".into()])).down, 1000);
        assert_eq!(sample.point(&HashSet::new()).up, 600);
    }
}

#[cfg(test)]
mod native_restart_test {
    use super::*;
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

use crate::model::{
    Adapter, AppUsage, Filter, History, LiveTick, Point, ScopeTotals, TransportSummary, UsageRow,
    WIFI_UNKNOWN, WifiNetwork,
};
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::Path;

pub struct Store {
    db: Connection,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let mut db = Connection::open(path).context("Open usage database")?;
        db.busy_timeout(std::time::Duration::from_secs(3))?;
        let version: i64 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
        anyhow::ensure!(
            version <= 5,
            "History was created by a newer OpenNetworkMonitor version"
        );
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS traffic (
            minute INTEGER NOT NULL, source TEXT NOT NULL, adapter_id TEXT NOT NULL,
            adapter TEXT NOT NULL, app TEXT NOT NULL, pid INTEGER NOT NULL, remote TEXT NOT NULL,
            country TEXT NOT NULL, protocol TEXT NOT NULL, scope TEXT NOT NULL,
            down INTEGER NOT NULL, up INTEGER NOT NULL, traffic_type TEXT NOT NULL DEFAULT '',
            PRIMARY KEY(minute,source,adapter_id,app,pid,remote,protocol,scope,traffic_type));
            CREATE INDEX IF NOT EXISTS traffic_time_source ON traffic(source,minute);
            CREATE TABLE IF NOT EXISTS visible_adapters (id TEXT PRIMARY KEY, name TEXT NOT NULL DEFAULT '', last_seen INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS adapter_samples (
                timestamp INTEGER NOT NULL, adapter_id TEXT NOT NULL,
                down INTEGER NOT NULL, up INTEGER NOT NULL, elapsed REAL NOT NULL,
                PRIMARY KEY(timestamp,adapter_id));
            ",
        )?;
        let integrity: String = db.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
        anyhow::ensure!(
            integrity == "ok",
            "Saved history failed its integrity check. Preserve usage.db and its WAL before recovery: {integrity}"
        );
        let columns: Vec<String> = {
            let mut q = db.prepare("PRAGMA table_info(traffic)")?;
            q.query_map([], |r| r.get(1))?
                .collect::<rusqlite::Result<_>>()?
        };
        if !columns.iter().any(|name| name == "traffic_type") {
            let tx = db.transaction()?;
            tx.execute_batch("ALTER TABLE traffic RENAME TO traffic_v3;
                CREATE TABLE traffic(minute INTEGER NOT NULL,source TEXT NOT NULL,adapter_id TEXT NOT NULL,adapter TEXT NOT NULL,app TEXT NOT NULL,pid INTEGER NOT NULL,remote TEXT NOT NULL,country TEXT NOT NULL,protocol TEXT NOT NULL,scope TEXT NOT NULL,down INTEGER NOT NULL,up INTEGER NOT NULL,traffic_type TEXT NOT NULL DEFAULT '',PRIMARY KEY(minute,source,adapter_id,app,pid,remote,protocol,scope,traffic_type));
                INSERT INTO traffic SELECT minute,source,adapter_id,adapter,app,pid,remote,country,protocol,scope,down,up,'' FROM traffic_v3;
                DROP TABLE traffic_v3;
                CREATE INDEX traffic_time_source ON traffic(source,minute);
                PRAGMA user_version=4;")?;
            tx.commit()?;
        }
        // Rebuild both primary keys together. Legacy traffic has no recoverable
        // Wi-Fi identity, so keep it distinct from newly observed networks.
        let columns = |db: &Connection, table: &str| -> Result<Vec<String>> {
            let mut stmt = db.prepare(&format!("PRAGMA table_info({table})"))?;
            Ok(stmt
                .query_map([], |r| r.get(1))?
                .collect::<rusqlite::Result<_>>()?)
        };
        let has_traffic_wifi = columns(&db, "traffic")?
            .iter()
            .any(|name| name == "wifi_network");
        let has_sample_wifi = columns(&db, "adapter_samples")?
            .iter()
            .any(|name| name == "wifi_network");
        anyhow::ensure!(
            has_traffic_wifi == has_sample_wifi,
            "History Wi-Fi schema is incomplete"
        );
        let tx = db.transaction()?;
        if !has_traffic_wifi {
            tx.execute_batch("ALTER TABLE traffic RENAME TO traffic_v4;
                CREATE TABLE traffic(minute INTEGER NOT NULL,source TEXT NOT NULL,adapter_id TEXT NOT NULL,adapter TEXT NOT NULL,app TEXT NOT NULL,pid INTEGER NOT NULL,remote TEXT NOT NULL,country TEXT NOT NULL,protocol TEXT NOT NULL,scope TEXT NOT NULL,down INTEGER NOT NULL,up INTEGER NOT NULL,traffic_type TEXT NOT NULL DEFAULT '',wifi_network TEXT NOT NULL DEFAULT '',PRIMARY KEY(minute,source,adapter_id,app,pid,remote,protocol,scope,traffic_type,wifi_network));
                INSERT INTO traffic SELECT minute,source,adapter_id,adapter,app,pid,remote,country,protocol,scope,down,up,traffic_type,'' FROM traffic_v4;
                DROP TABLE traffic_v4;
                CREATE INDEX traffic_time_source ON traffic(source,minute);
                ALTER TABLE adapter_samples RENAME TO adapter_samples_v4;
                CREATE TABLE adapter_samples(timestamp INTEGER NOT NULL,adapter_id TEXT NOT NULL,down INTEGER NOT NULL,up INTEGER NOT NULL,elapsed REAL NOT NULL,wifi_network TEXT NOT NULL DEFAULT '',PRIMARY KEY(timestamp,adapter_id,wifi_network));
                INSERT INTO adapter_samples SELECT timestamp,adapter_id,down,up,elapsed,'' FROM adapter_samples_v4;
                DROP TABLE adapter_samples_v4;")?;
        }
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS wifi_networks(id TEXT PRIMARY KEY,name TEXT NOT NULL);
            PRAGMA user_version=5;",
        )?;
        tx.commit()?;
        Ok(Self { db })
    }
    pub fn write(&mut self, timestamp: i64, rows: &[UsageRow]) -> Result<()> {
        let tx = self.db.transaction()?;
        write_rows(&tx, timestamp, rows)?;
        tx.commit()?;
        Ok(())
    }
    pub fn write_capture(
        &mut self,
        batches: &std::collections::BTreeMap<i64, Vec<UsageRow>>,
        samples: &[LiveTick],
    ) -> Result<()> {
        let tx = self.db.transaction()?;
        for (timestamp, rows) in batches {
            write_rows(&tx, *timestamp, rows)?;
        }
        {
            let mut stmt=tx.prepare_cached("INSERT INTO adapter_samples(timestamp,adapter_id,down,up,elapsed,wifi_network) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(timestamp,adapter_id,wifi_network) DO UPDATE SET down=down+excluded.down,up=up+excluded.up,elapsed=elapsed+excluded.elapsed")?;
            for sample in samples {
                for (id, (down, up)) in &sample.adapters {
                    stmt.execute(params![
                        sample.timestamp,
                        id,
                        *down,
                        *up,
                        sample.interval.max(0.001),
                        sample
                            .wifi_networks
                            .get(id)
                            .map(String::as_str)
                            .unwrap_or("")
                    ])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }
    /// Checkpoint whatever is available without waiting for readers. Committed
    /// transactions remain durable in the WAL if an active reader pins frames;
    /// SQLite can checkpoint the remainder after that reader releases its snapshot.
    pub fn checkpoint(&self) -> Result<()> {
        self.db
            .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?;
        Ok(())
    }
    pub fn history(&self, filter: &Filter, now: i64) -> Result<History> {
        let (mut window_start, window_end) = filter.time_bounds(now);
        if filter.all_time() {
            window_start = self
                .db
                .query_row(
                    "SELECT MIN(minute) FROM traffic WHERE source=?1 AND minute<=?2",
                    params![filter.source.key(), window_end],
                    |row| row.get::<_, Option<i64>>(0),
                )?
                .unwrap_or(window_end.saturating_sub(3600));
        }
        // Totals use complete minute buckets, consistently across every view.
        let start = window_start / 60 * 60;
        let mut stmt = self.db.prepare_cached("SELECT minute,source,adapter_id,adapter,app,pid,remote,country,protocol,scope,SUM(down),SUM(up),traffic_type,wifi_network
            FROM traffic WHERE source=?1 AND minute>=?2 AND minute<=?3
            GROUP BY minute,adapter_id,app,pid,remote,protocol,scope,traffic_type,wifi_network ORDER BY minute")?;
        let mut result = History {
            window_start,
            window_end,
            ..Default::default()
        };
        let mut points = std::collections::BTreeMap::<i64, (u64, u64)>::new();
        let mut totals = std::collections::BTreeMap::<
            (String, String, u32, String, String, String, String, String),
            UsageRow,
        >::new();
        let iter = stmt.query_map(params![filter.source.key(), start, window_end], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                UsageRow {
                    source: r.get(1)?,
                    adapter_id: r.get(2)?,
                    adapter: r.get(3)?,
                    app: r.get(4)?,
                    pid: r.get(5)?,
                    remote: r.get(6)?,
                    country: r.get(7)?,
                    protocol: r.get(8)?,
                    scope: r.get(9)?,
                    down: r.get(10)?,
                    up: r.get(11)?,
                    traffic_type: r.get(12)?,
                    wifi_network: r.get(13)?,
                },
            ))
        })?;
        for item in iter {
            let (minute, row) = item?;
            if !filter.matches(&row) {
                continue;
            }
            let point = points.entry(minute).or_default();
            point.0 += row.down;
            point.1 += row.up;
            result.down += row.down;
            result.up += row.up;
            let key = (
                row.adapter_id.clone(),
                row.app.clone(),
                row.pid,
                row.remote.clone(),
                row.protocol.clone(),
                row.scope.clone(),
                row.traffic_type.clone(),
                row.wifi_network.clone(),
            );
            let item = totals.entry(key).or_insert_with(|| UsageRow {
                down: 0,
                up: 0,
                ..row.clone()
            });
            item.down += row.down;
            item.up += row.up;
        }
        let mut series: Vec<(i64, u64, u64, f64, i64)> = Vec::new();
        let mut raw_minutes = std::collections::HashSet::new();
        if filter.source == crate::model::Source::Adapter {
            let mut raw = self.db.prepare_cached("SELECT timestamp,adapter_id,down,up,elapsed,wifi_network FROM adapter_samples WHERE timestamp>=?1 AND timestamp<=?2 ORDER BY timestamp")?;
            // A single adapter may change networks within one second. Its
            // elapsed intervals must be added, but elapsed time across separate
            // adapters in the same second must not be counted twice.
            let mut by_adapter =
                std::collections::BTreeMap::<(i64, String), (u64, u64, f64)>::new();
            for row in raw.query_map(params![window_start, window_end], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, u64>(2)?,
                    r.get::<_, u64>(3)?,
                    r.get::<_, f64>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })? {
                let (t, id, down, up, elapsed, wifi) = row?;
                if (!filter.adapters.is_empty() && !filter.adapters.contains(&id))
                    || filter
                        .wifi_network
                        .as_ref()
                        .is_some_and(|selected| selected != &wifi)
                {
                    continue;
                }
                let entry = by_adapter.entry((t, id)).or_default();
                entry.0 = entry.0.saturating_add(down);
                entry.1 = entry.1.saturating_add(up);
                entry.2 += elapsed;
                raw_minutes.insert(t / 60 * 60);
            }
            let mut samples = std::collections::BTreeMap::<i64, (u64, u64, f64)>::new();
            for ((t, _), (down, up, elapsed)) in by_adapter {
                let entry = samples.entry(t).or_default();
                entry.0 = entry.0.saturating_add(down);
                entry.1 = entry.1.saturating_add(up);
                entry.2 = entry.2.max(elapsed);
            }
            series.extend(
                samples
                    .into_iter()
                    .map(|(t, (d, u, elapsed))| (t, d, u, elapsed, 1)),
            );
        }
        for (minute, (down, up)) in points {
            if !raw_minutes.contains(&minute) {
                series.push((minute, down, up, 60.0, 60));
            }
        }
        result.points = plot_series(series, window_start, window_end);
        result.rows = totals.into_values().collect();
        result
            .rows
            .sort_by_key(|r| std::cmp::Reverse(r.down + r.up));
        Ok(result)
    }
    pub fn export(&self, path: &Path, filter: &Filter, now: i64) -> Result<usize> {
        let (start, end) = filter.time_bounds(now);
        let start = start / 60 * 60;
        let mut stmt = self.db.prepare("SELECT minute,source,adapter_id,adapter,app,pid,remote,country,protocol,scope,down,up,traffic_type,wifi_network FROM traffic WHERE source=?1 AND minute>=?2 AND minute<=?3 ORDER BY minute")?;
        // create_new prevents accidental overwrite or a partial replacement of an existing export.
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .context("Create CSV (choose a new filename if it already exists)")?;
        let mut writer = csv::Writer::from_writer(file);
        writer.write_record([
            "minute_utc",
            "source",
            "adapter_id",
            "adapter",
            "app",
            "pid",
            "remote_ip",
            "country",
            "protocol",
            "scope",
            "download_bytes",
            "upload_bytes",
            "traffic_type",
            "wifi_network",
        ])?;
        let wifi_names: std::collections::HashMap<String, String> = self
            .known_wifi_networks()?
            .into_iter()
            .map(|n| (n.id, n.name))
            .collect();
        let mut count = 0;
        let rows = stmt.query_map(params![filter.source.key(), start, end], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                UsageRow {
                    source: r.get(1)?,
                    adapter_id: r.get(2)?,
                    adapter: r.get(3)?,
                    app: r.get(4)?,
                    pid: r.get(5)?,
                    remote: r.get(6)?,
                    country: r.get(7)?,
                    protocol: r.get(8)?,
                    scope: r.get(9)?,
                    down: r.get(10)?,
                    up: r.get(11)?,
                    traffic_type: r.get(12)?,
                    wifi_network: r.get(13)?,
                },
            ))
        })?;
        for row in rows {
            let (minute, r) = row?;
            if !filter.matches(&r) {
                continue;
            }
            let date = chrono::DateTime::from_timestamp(minute, 0)
                .map(|d| d.to_rfc3339())
                .unwrap_or_else(|| minute.to_string());
            writer.write_record([
                date,
                r.source,
                r.adapter_id,
                csv_safe(&r.adapter),
                csv_safe(&r.app),
                r.pid.to_string(),
                r.remote,
                csv_safe(&r.country),
                r.protocol,
                r.scope,
                r.down.to_string(),
                r.up.to_string(),
                csv_safe(&r.traffic_type),
                csv_safe(if r.wifi_network == WIFI_UNKNOWN {
                    "Unknown Wi-Fi"
                } else {
                    wifi_names
                        .get(&r.wifi_network)
                        .map(String::as_str)
                        .unwrap_or(r.wifi_network.as_str())
                }),
            ])?;
            count += 1;
        }
        writer.flush()?;
        Ok(count)
    }
    pub fn remember_adapters(
        &mut self,
        ids: &std::collections::HashSet<String>,
        adapters: &[Adapter],
        now: i64,
    ) -> Result<()> {
        let tx = self.db.transaction()?;
        for id in ids {
            let name = adapters
                .iter()
                .find(|a| a.id == *id)
                .map(|a| a.name.as_str())
                .unwrap_or("");
            tx.execute("INSERT INTO visible_adapters(id,name,last_seen) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET name=CASE WHEN excluded.name='' THEN name ELSE excluded.name END,last_seen=excluded.last_seen",params![id,name,now])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn known_adapter_ids(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.db.prepare("SELECT id FROM visible_adapters")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(ids)
    }
    pub fn remember_wifi_networks(&mut self, networks: &[WifiNetwork]) -> Result<()> {
        let tx = self.db.transaction()?;
        for network in networks {
            if !network.id.is_empty() && network.id != WIFI_UNKNOWN {
                tx.execute("INSERT INTO wifi_networks(id,name) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET name=excluded.name", params![network.id, network.name])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn known_wifi_networks(&self) -> Result<Vec<WifiNetwork>> {
        let mut stmt = self
            .db
            .prepare("SELECT id,name FROM wifi_networks ORDER BY name COLLATE NOCASE,id")?;
        let networks = stmt
            .query_map([], |r| {
                Ok(WifiNetwork {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(networks)
    }
    pub fn scope_totals(&self, filter: &Filter, now: i64) -> Result<ScopeTotals> {
        Ok(self.transport_summary(filter, now)?.classified)
    }
    /// Shared period/adapter rollup for the Overall Traffic footer, independent of app-page search.
    pub fn transport_summary(&self, filter: &Filter, now: i64) -> Result<TransportSummary> {
        let mut stmt=self.db.prepare_cached("SELECT adapter_id,app,scope,SUM(down),SUM(up),wifi_network FROM traffic WHERE source='transport' AND minute>=?1 AND minute<=?2 GROUP BY adapter_id,app,scope,wifi_network")?;
        let (start, end) = filter.time_bounds(now);
        let rows = stmt.query_map(params![start / 60 * 60, end], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, u64>(3)?,
                r.get::<_, u64>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        let mut classified = ScopeTotals::default();
        let mut apps = std::collections::BTreeMap::<String, AppUsage>::new();
        for row in rows {
            let (adapter, app, scope, down, up, wifi) = row?;
            if (!filter.adapters.is_empty() && !filter.adapters.contains(&adapter))
                || filter
                    .wifi_network
                    .as_ref()
                    .is_some_and(|selected| selected != &wifi)
            {
                continue;
            }
            let bytes = down.saturating_add(up);
            match scope.as_str() {
                "LAN" => classified.lan = classified.lan.saturating_add(bytes),
                "WAN" => classified.wan = classified.wan.saturating_add(bytes),
                _ => classified.unknown = classified.unknown.saturating_add(bytes),
            }
            if app.is_empty() || bytes == 0 {
                continue;
            }
            let item = apps.entry(app.clone()).or_insert_with(|| AppUsage {
                app,
                ..Default::default()
            });
            item.down = item.down.saturating_add(down);
            item.up = item.up.saturating_add(up);
        }
        let mut top_apps: Vec<_> = apps.into_values().collect();
        top_apps.sort_by(|a, b| {
            (b.down.saturating_add(b.up))
                .cmp(&a.down.saturating_add(a.up))
                .then(a.app.cmp(&b.app))
        });
        top_apps.truncate(5);
        Ok(TransportSummary {
            classified,
            top_apps,
        })
    }
    /// Compact recent chart samples only. Minute usage history has no expiry.
    pub fn prune_samples(&self, before: i64) -> Result<()> {
        self.db
            .execute("DELETE FROM adapter_samples WHERE timestamp<?1", [before])?;
        Ok(())
    }
}
fn write_rows(tx: &rusqlite::Transaction<'_>, timestamp: i64, rows: &[UsageRow]) -> Result<()> {
    let mut stmt=tx.prepare_cached("INSERT INTO traffic(minute,source,adapter_id,adapter,app,pid,remote,country,protocol,scope,down,up,traffic_type,wifi_network) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14) ON CONFLICT(minute,source,adapter_id,app,pid,remote,protocol,scope,traffic_type,wifi_network) DO UPDATE SET down=down+excluded.down,up=up+excluded.up,country=excluded.country,adapter=excluded.adapter")?;
    for r in rows {
        stmt.execute(params![
            timestamp / 60 * 60,
            r.source,
            r.adapter_id,
            r.adapter,
            r.app,
            r.pid,
            r.remote,
            r.country,
            r.protocol,
            r.scope,
            r.down,
            r.up,
            r.traffic_type,
            r.wifi_network
        ])?;
    }
    Ok(())
}
fn plot_series(mut rows: Vec<(i64, u64, u64, f64, i64)>, start: i64, end: i64) -> Vec<Point> {
    rows.retain(|r| r.0 >= start && r.0 <= end);
    rows.sort_by_key(|r| r.0);
    let step = ((end - start) / 1200).max(1);
    let mut bins = std::collections::BTreeMap::<i64, (i64, u64, u64, f64, i64)>::new();
    for (t, d, u, elapsed, span) in rows {
        let bin = bins.entry((t - start) / step).or_default();
        bin.0 = t;
        bin.1 = bin.1.saturating_add(d);
        bin.2 = bin.2.saturating_add(u);
        bin.3 += elapsed;
        bin.4 = bin.4.max(span).max(step);
    }
    let mut result = Vec::new();
    let mut previous: Option<(i64, i64)> = None;
    for (_, (t, d, u, elapsed, span)) in bins {
        if let Some((last, last_span)) = previous
            && t - last > last_span.max(span) + 2
        {
            result.push(Point {
                timestamp: last + 1,
                observed: false,
                ..Default::default()
            });
        }
        result.push(Point {
            timestamp: t,
            observed: true,
            down: (d as f64 / elapsed.max(0.001)) as u64,
            up: (u as f64 / elapsed.max(0.001)) as u64,
        });
        previous = Some((t, span));
    }
    result
}
fn csv_safe(value: &str) -> String {
    if value.starts_with(['=', '+', '-', '@', '\t', '\r', '\n']) {
        format!("'{value}")
    } else {
        value.into()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Source;
    #[test]
    fn persistence_filters_rollup_and_restart() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let a = UsageRow {
            source: "transport".into(),
            adapter_id: "a".into(),
            app: "browser.exe".into(),
            scope: "WAN".into(),
            down: 120,
            up: 60,
            ..Default::default()
        };
        store.write(
            120,
            &[
                a.clone(),
                UsageRow {
                    source: "adapter".into(),
                    down: 999,
                    ..a.clone()
                },
            ],
        )?;
        store.write(121, &[a])?;
        let mut filter = Filter {
            source: Source::Transport,
            seconds: 300,
            ..Default::default()
        };
        let result = store.history(&filter, 300)?;
        assert_eq!((result.down, result.up), (240, 120));
        assert_eq!(result.rows.len(), 1);
        assert!(
            result
                .points
                .iter()
                .any(|p| p.timestamp == 120 && p.down == 4)
        );
        assert!(result.points.iter().all(|p| p.timestamp != 240));
        filter.adapters.insert("other".into());
        assert_eq!(store.history(&filter, 300)?.down, 0);
        store.prune_samples(180)?;
        assert_eq!(
            store
                .history(
                    &Filter {
                        source: Source::Transport,
                        ..Default::default()
                    },
                    300
                )?
                .down,
            240
        );
        Ok(())
    }
    #[test]
    fn spreadsheet_formulas_escaped() {
        assert_eq!(csv_safe("=bad"), "'=bad");
        assert_eq!(csv_safe("app.exe"), "app.exe");
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;
    use crate::model::{Scope, Source};
    #[test]
    fn disk_restart_csv_filters_and_overwrite_protection() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let db = dir.path().join("usage.db");
        let csv = dir.path().join("usage.csv");
        let row = UsageRow {
            source: "transport".into(),
            adapter_id: "eth".into(),
            adapter: "Ethernet, office".into(),
            wifi_network: String::new(),
            app: "=unsafe.exe".into(),
            traffic_type: "DNS".into(),
            pid: 42,
            remote: "1.1.1.1".into(),
            country: "AU".into(),
            protocol: "UDP".into(),
            scope: "WAN".into(),
            down: 4096,
            up: 1024,
        };
        {
            let mut store = Store::open(&db)?;
            store.write(
                600,
                &[
                    row.clone(),
                    UsageRow {
                        adapter_id: "vpn".into(),
                        down: 9000,
                        ..row.clone()
                    },
                ],
            )?;
        }
        let store = Store::open(&db)?;
        let mut filter = Filter {
            source: Source::Transport,
            scope: Scope::Wan,
            seconds: 300,
            ..Default::default()
        };
        filter.adapters.insert("eth".into());
        assert_eq!(store.history(&filter, 660)?.down, 4096);
        assert_eq!(store.export(&csv, &filter, 660)?, 1);
        let mut reader = csv::Reader::from_path(&csv)?;
        let records: Vec<_> = reader.records().collect::<std::result::Result<_, _>>()?;
        assert_eq!(records.len(), 1);
        assert_eq!(&records[0][3], "Ethernet, office");
        assert_eq!(&records[0][4], "'=unsafe.exe");
        assert_eq!(&records[0][10], "4096");
        assert_eq!(&records[0][0], "1970-01-01T00:10:00+00:00");
        let before = std::fs::read(&csv)?;
        assert!(store.export(&csv, &filter, 660).is_err());
        assert_eq!(std::fs::read(&csv)?, before);
        filter.search = "no match".into();
        assert_eq!(store.history(&filter, 660)?.rows.len(), 0);
        Ok(())
    }
}

#[cfg(test)]
mod summary_tests {
    use super::*;
    use crate::model::Source;
    #[test]
    fn registered_adapter_history_survives_unplugging_and_scope_totals_exclude_internal_interfaces()
    -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        store.remember_adapters(
            &std::collections::HashSet::from(["ethernet".into()]),
            &[],
            600,
        )?;
        store.remember_adapters(&std::collections::HashSet::from(["usb".into()]), &[], 601)?;
        assert_eq!(store.known_adapter_ids()?.len(), 2);
        store.write(
            600,
            &[
                UsageRow {
                    source: "transport".into(),
                    adapter_id: "ethernet".into(),
                    scope: "WAN".into(),
                    down: 100,
                    up: 50,
                    ..Default::default()
                },
                UsageRow {
                    source: "transport".into(),
                    adapter_id: "internal".into(),
                    scope: "LAN".into(),
                    down: 999,
                    ..Default::default()
                },
            ],
        )?;
        let filter = Filter {
            source: Source::Transport,
            adapters: std::collections::HashSet::from(["ethernet".into()]),
            ..Default::default()
        };
        let totals = store.scope_totals(&filter, 660)?;
        assert_eq!((totals.lan, totals.wan, totals.unknown), (0, 150, 0));
        Ok(())
    }
    #[test]
    fn csv_and_history_can_match_a_friendly_name_search_by_stable_executable_path() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(Path::new(":memory:"))?;
        let heliumpath = "C:\\Helium\\chrome.exe";
        store.write(
            600,
            &[
                UsageRow {
                    source: "transport".into(),
                    app: heliumpath.into(),
                    down: 42,
                    ..Default::default()
                },
                UsageRow {
                    source: "transport".into(),
                    app: "other.exe".into(),
                    down: 999,
                    ..Default::default()
                },
            ],
        )?;
        let filter = Filter {
            source: Source::Transport,
            apps: std::collections::HashSet::from([heliumpath.into()]),
            ..Default::default()
        };
        assert_eq!(store.history(&filter, 660)?.down, 42);
        assert_eq!(
            store.export(&dir.path().join("helium.csv"), &filter, 660)?,
            1
        );
        Ok(())
    }
}

#[cfg(test)]
mod durable_chart_tests {
    use super::*;
    use crate::model::Source;
    use std::collections::{BTreeMap, HashMap};
    #[test]
    fn all_five_minutes_and_partial_exit_sample_survive_reopen() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("restart.db");
        let filter = Filter {
            source: Source::Adapter,
            seconds: 300,
            ..Default::default()
        };
        {
            let mut store = Store::open(&path)?;
            for t in 6000..6300 {
                let row = UsageRow {
                    source: "adapter".into(),
                    adapter_id: "eth".into(),
                    down: 100,
                    up: 40,
                    ..Default::default()
                };
                store.write_capture(
                    &BTreeMap::from([(t, vec![row])]),
                    &[LiveTick {
                        timestamp: t,
                        interval: 1.0,
                        adapters: HashMap::from([("eth".into(), (100, 40))]),
                        wifi_networks: HashMap::new(),
                    }],
                )?;
            }
            store.write_capture(
                &BTreeMap::from([(
                    6299,
                    vec![UsageRow {
                        source: "adapter".into(),
                        adapter_id: "eth".into(),
                        down: 25,
                        up: 10,
                        ..Default::default()
                    }],
                )]),
                &[LiveTick {
                    timestamp: 6299,
                    interval: 0.25,
                    adapters: HashMap::from([("eth".into(), (25, 10))]),
                    wifi_networks: HashMap::new(),
                }],
            )?;
            store.checkpoint()?;
        }
        let reopened = Store::open(&path)?;
        let h = reopened.history(&filter, 6299)?;
        assert_eq!((h.down, h.up), (30025, 12010));
        assert_eq!((h.window_start, h.window_end), (5999, 6299));
        assert_eq!(h.points.iter().filter(|p| p.observed).count(), 300);
        assert!(
            h.points
                .iter()
                .any(|p| p.timestamp == 6299 && p.down == 100 && p.up == 40)
        );
        let hour = reopened.history(
            &Filter {
                seconds: 3600,
                ..filter
            },
            6299,
        )?;
        assert_eq!((hour.window_start, hour.window_end), (2699, 6299));
        assert!(hour.points.iter().all(|p| p.timestamp >= 6000));
        Ok(())
    }
    #[test]
    fn checkpoint_does_not_wait_for_a_reader_and_commits_survive_reopen() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("history.db");
        let mut store = Store::open(&path)?;
        let row = UsageRow {
            source: "adapter".into(),
            adapter_id: "eth".into(),
            down: 40,
            up: 10,
            ..Default::default()
        };
        store.write(600, std::slice::from_ref(&row))?;
        store.db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;

        let reader = Connection::open(&path)?;
        reader.execute_batch("BEGIN")?;
        assert_eq!(
            reader.query_row("SELECT SUM(down) FROM traffic", [], |r| r.get::<_, u64>(0))?,
            40
        );
        store.write_capture(
            &BTreeMap::from([(
                660,
                vec![UsageRow {
                    down: 100,
                    up: 20,
                    ..row
                }],
            )]),
            &[LiveTick {
                timestamp: 660,
                interval: 0.5,
                adapters: HashMap::from([("eth".into(), (100, 20))]),
                wifi_networks: HashMap::new(),
            }],
        )?;
        let started = std::time::Instant::now();
        store.checkpoint()?;
        drop(store);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "Checkpoint or writer close blocked on an active reader"
        );

        // The old reader keeps its snapshot while a fresh connection sees the
        // committed rows and adapter sample even though WAL frames are pinned.
        assert_eq!(
            reader.query_row("SELECT SUM(down) FROM traffic", [], |r| r.get::<_, u64>(0))?,
            40
        );
        let reopened = Store::open(&path)?;
        assert_eq!(
            reopened
                .db
                .query_row("SELECT SUM(down) FROM traffic", [], |r| r.get::<_, u64>(0))?,
            140
        );
        assert_eq!(
            reopened
                .db
                .query_row("SELECT SUM(down) FROM adapter_samples", [], |r| r
                    .get::<_, u64>(0))?,
            100
        );
        drop(reopened);
        reader.execute_batch("ROLLBACK")?;
        drop(reader);
        let reopened = Store::open(&path)?;
        assert_eq!(reopened.history(&Filter::default(), 700)?.down, 140);
        Ok(())
    }
    #[test]
    fn unknown_intervals_are_gaps_but_measured_zero_is_retained() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        for t in [6000, 6001, 6010, 6011] {
            store.write_capture(
                &BTreeMap::new(),
                &[LiveTick {
                    timestamp: t,
                    interval: 1.0,
                    adapters: HashMap::from([("a".into(), (0, 0))]),
                    wifi_networks: HashMap::new(),
                }],
            )?;
        }
        let h = store.history(
            &Filter {
                seconds: 300,
                ..Default::default()
            },
            6020,
        )?;
        assert_eq!(h.points.iter().filter(|p| p.observed).count(), 4);
        assert!(h.points.iter().any(|p| !p.observed));
        assert!(!h.points.iter().any(|p| p.timestamp == 6005));
        Ok(())
    }
    #[test]
    fn live_samples_and_usage_totals_roll_back_together() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        store.db.execute_batch("CREATE TRIGGER fail_sample BEFORE INSERT ON adapter_samples BEGIN SELECT RAISE(ABORT,'test write failure'); END;")?;
        let rows = BTreeMap::from([(
            6000,
            vec![UsageRow {
                source: "adapter".into(),
                adapter_id: "a".into(),
                down: 100,
                ..Default::default()
            }],
        )]);
        let samples = [LiveTick {
            timestamp: 6000,
            interval: 1.0,
            adapters: HashMap::from([("a".into(), (100, 0))]),
            wifi_networks: HashMap::new(),
        }];
        assert!(store.write_capture(&rows, &samples).is_err());
        assert_eq!(store.history(&Filter::default(), 6010)?.down, 0);
        store.db.execute_batch("DROP TRIGGER fail_sample;")?;
        store.write_capture(&rows, &samples)?;
        assert_eq!(store.history(&Filter::default(), 6010)?.down, 100);
        Ok(())
    }
    #[test]
    fn bundled_sqlite_has_the_wal_reset_fix() {
        assert!(rusqlite::version_number() >= 3_051_003);
    }
}

#[cfg(test)]
mod service_history_tests {
    use super::*;
    use crate::model::Source;
    #[test]
    fn v3_upgrade_retains_old_bytes_and_keeps_new_service_buckets_distinct() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("usage.db");
        {
            let old = Connection::open(&path)?;
            old.execute_batch("CREATE TABLE traffic(minute INTEGER NOT NULL,source TEXT NOT NULL,adapter_id TEXT NOT NULL,adapter TEXT NOT NULL,app TEXT NOT NULL,pid INTEGER NOT NULL,remote TEXT NOT NULL,country TEXT NOT NULL,protocol TEXT NOT NULL,scope TEXT NOT NULL,down INTEGER NOT NULL,up INTEGER NOT NULL,PRIMARY KEY(minute,source,adapter_id,app,pid,remote,protocol,scope));
                CREATE INDEX traffic_time_source ON traffic(source,minute);
                INSERT INTO traffic VALUES(600,'transport','eth','Ethernet','browser.exe',42,'1.1.1.1','','TCP','WAN',100,20);
                PRAGMA user_version=3;")?;
        }
        let filter = Filter {
            source: Source::Transport,
            seconds: 300,
            ..Default::default()
        };
        {
            let mut store = Store::open(&path)?;
            let before = store.history(&filter, 660)?;
            assert_eq!((before.down, before.up), (100, 20));
            assert_eq!(before.rows[0].traffic_type, "");
            let row = before.rows[0].clone();
            store.write(
                600,
                &[
                    UsageRow {
                        traffic_type: "HTTP".into(),
                        down: 200,
                        up: 40,
                        ..row.clone()
                    },
                    UsageRow {
                        traffic_type: "HTTPS / TLS".into(),
                        down: 300,
                        up: 60,
                        ..row.clone()
                    },
                ],
            )?;
            store.write(
                601,
                &[UsageRow {
                    traffic_type: "HTTP".into(),
                    down: 50,
                    up: 10,
                    ..row
                }],
            )?;
            store.checkpoint()?;
        }
        let store = Store::open(&path)?;
        assert_eq!(
            store
                .db
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
            5
        );
        assert_eq!(
            store
                .db
                .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))?,
            "ok"
        );
        let after = store.history(&filter, 660)?;
        assert_eq!((after.down, after.up), (650, 130));
        assert_eq!(after.rows.len(), 3);
        let http = after
            .rows
            .iter()
            .find(|r| r.traffic_type == "HTTP")
            .unwrap();
        assert_eq!((http.down, http.up), (250, 50));
        let csv = dir.path().join("services.csv");
        assert_eq!(store.export(&csv, &filter, 660)?, 3);
        let mut reader = csv::Reader::from_path(csv)?;
        assert_eq!(reader.headers()?.get(12), Some("traffic_type"));
        let rows = reader
            .records()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        assert!(rows.iter().any(|r| r.get(12) == Some("HTTPS / TLS")));
        Ok(())
    }
}

#[cfg(test)]
mod wifi_history_tests {
    use super::*;
    use crate::model::Source;
    use std::collections::{BTreeMap, HashMap, HashSet};

    #[test]
    fn v4_upgrade_preserves_legacy_rows_and_separates_networks_through_reopen() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("wifi-migration.db");
        {
            let old = Connection::open(&path)?;
            old.execute_batch("CREATE TABLE traffic(minute INTEGER NOT NULL,source TEXT NOT NULL,adapter_id TEXT NOT NULL,adapter TEXT NOT NULL,app TEXT NOT NULL,pid INTEGER NOT NULL,remote TEXT NOT NULL,country TEXT NOT NULL,protocol TEXT NOT NULL,scope TEXT NOT NULL,down INTEGER NOT NULL,up INTEGER NOT NULL,traffic_type TEXT NOT NULL DEFAULT '',PRIMARY KEY(minute,source,adapter_id,app,pid,remote,protocol,scope,traffic_type));
                CREATE INDEX traffic_time_source ON traffic(source,minute);
                CREATE TABLE adapter_samples(timestamp INTEGER NOT NULL,adapter_id TEXT NOT NULL,down INTEGER NOT NULL,up INTEGER NOT NULL,elapsed REAL NOT NULL,PRIMARY KEY(timestamp,adapter_id));
                INSERT INTO traffic VALUES(600,'adapter','wifi','Wi-Fi','',0,'','','','Unknown',42,2,'');
                INSERT INTO adapter_samples VALUES(600,'wifi',42,2,1.0);
                PRAGMA user_version=4;")?;
        }
        let base = UsageRow {
            source: "adapter".into(),
            adapter_id: "wifi".into(),
            adapter: "Wi-Fi".into(),
            scope: "Unknown".into(),
            ..Default::default()
        };
        {
            let mut store = Store::open(&path)?;
            assert_eq!(store.history(&Filter::default(), 660)?.down, 42);
            assert!(
                store
                    .history(&Filter::default(), 660)?
                    .rows
                    .iter()
                    .all(|r| r.wifi_network.is_empty())
            );
            store.remember_wifi_networks(&[
                WifiNetwork {
                    id: "ssid:home".into(),
                    name: "=Home".into(),
                },
                WifiNetwork {
                    id: "ssid:office".into(),
                    name: "Office".into(),
                },
            ])?;
            for (wifi, down, up, elapsed) in [
                ("ssid:home", 100, 10, 0.25),
                ("ssid:office", 200, 20, 0.5),
                (WIFI_UNKNOWN, 30, 3, 0.25),
            ] {
                let row = UsageRow {
                    wifi_network: wifi.into(),
                    down,
                    up,
                    ..base.clone()
                };
                let app = UsageRow {
                    source: "transport".into(),
                    app: "browser.exe".into(),
                    pid: 7,
                    remote: "1.1.1.1".into(),
                    protocol: "TCP".into(),
                    scope: "WAN".into(),
                    ..row.clone()
                };
                store.write_capture(
                    &BTreeMap::from([(601, vec![row, app])]),
                    &[LiveTick {
                        timestamp: 601,
                        interval: elapsed,
                        adapters: HashMap::from([("wifi".into(), (down, up))]),
                        wifi_networks: HashMap::from([("wifi".into(), wifi.into())]),
                    }],
                )?;
            }
            store.checkpoint()?;
        }
        let store = Store::open(&path)?;
        assert_eq!(
            store
                .db
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
            5
        );
        assert_eq!(
            store
                .db
                .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))?,
            "ok"
        );
        let all = Filter {
            seconds: 300,
            adapters: HashSet::from(["wifi".into()]),
            ..Default::default()
        };
        let history = store.history(&all, 660)?;
        assert_eq!((history.down, history.up), (372, 35));
        assert_eq!(history.rows.len(), 4);
        assert_eq!(
            history
                .points
                .iter()
                .find(|p| p.timestamp == 601)
                .map(|p| (p.down, p.up)),
            Some((330, 33))
        );
        for (id, down, up, rate) in [
            ("ssid:home", 100, 10, 400),
            ("ssid:office", 200, 20, 400),
            (WIFI_UNKNOWN, 30, 3, 120),
        ] {
            let filter = Filter {
                wifi_network: Some(id.into()),
                ..all.clone()
            };
            let history = store.history(&filter, 660)?;
            assert_eq!((history.down, history.up), (down, up));
            assert_eq!(history.rows.len(), 1);
            assert_eq!(
                history
                    .points
                    .iter()
                    .find(|p| p.timestamp == 601)
                    .map(|p| p.down),
                Some(rate)
            );
            let app_filter = Filter {
                source: Source::Transport,
                ..filter
            };
            let apps = store.history(&app_filter, 660)?;
            assert_eq!((apps.down, apps.up), (down, up));
            let summary = store.transport_summary(&app_filter, 660)?;
            assert_eq!(summary.classified.wan, down + up);
            assert_eq!(
                (summary.top_apps[0].down, summary.top_apps[0].up),
                (down, up)
            );
        }
        assert_eq!(store.known_wifi_networks()?.len(), 2);
        let csv = dir.path().join("home.csv");
        let filter = Filter {
            source: Source::Transport,
            wifi_network: Some("ssid:home".into()),
            ..all
        };
        assert_eq!(store.export(&csv, &filter, 660)?, 1);
        let mut reader = csv::Reader::from_path(csv)?;
        assert_eq!(reader.headers()?.get(13), Some("wifi_network"));
        let row = reader.records().next().unwrap()?;
        assert_eq!(row.get(13), Some("'=Home"));
        assert_eq!(row.get(10), Some("100"));
        Ok(())
    }

    #[test]
    fn old_preferences_default_to_all_wifi_networks() -> Result<()> {
        let filter: Filter = serde_json::from_str(
            r#"{"source":"Adapter","scope":"All","adapters":[],"seconds":300,"search":"","apps":[]}"#,
        )?;
        assert!(filter.wifi_network.is_none());
        Ok(())
    }
}

#[cfg(test)]
mod top_app_tests {
    use super::*;
    #[test]
    fn footer_ranks_apps_after_combining_processes_and_filtering_period_and_adapters() -> Result<()>
    {
        let mut store = Store::open(Path::new(":memory:"))?;
        let base = UsageRow {
            source: "transport".into(),
            adapter_id: "visible".into(),
            scope: "WAN".into(),
            ..Default::default()
        };
        for i in 0..7 {
            store.write(
                600,
                &[UsageRow {
                    app: format!("app{i}.exe"),
                    pid: i,
                    scope: match i {
                        1 => "LAN",
                        2 => "Unknown",
                        _ => "WAN",
                    }
                    .into(),
                    down: (u64::from(i) + 1) * 100,
                    up: u64::from(i) * 10,
                    ..base.clone()
                }],
            )?;
        }
        store.write(
            660,
            &[
                UsageRow {
                    app: "app0.exe".into(),
                    pid: 99,
                    remote: "1.1.1.1".into(),
                    traffic_type: "DNS".into(),
                    down: 1000,
                    up: 20,
                    ..base.clone()
                },
                UsageRow {
                    app: "excluded-adapter.exe".into(),
                    adapter_id: "other".into(),
                    down: 99999,
                    ..base.clone()
                },
                UsageRow {
                    app: "interface-only.exe".into(),
                    source: "adapter".into(),
                    down: 99999,
                    ..base.clone()
                },
            ],
        )?;
        store.write(
            500,
            &[UsageRow {
                app: "too-old.exe".into(),
                down: 99999,
                ..base.clone()
            }],
        )?;
        store.write(
            960,
            &[UsageRow {
                app: "future.exe".into(),
                down: 99999,
                ..base
            }],
        )?;
        let filter = Filter {
            seconds: 300,
            adapters: std::collections::HashSet::from(["visible".into()]),
            ..Default::default()
        };
        let summary = store.transport_summary(&filter, 900)?;
        assert_eq!(
            summary
                .top_apps
                .iter()
                .map(|a| a.app.as_str())
                .collect::<Vec<_>>(),
            vec!["app0.exe", "app6.exe", "app5.exe", "app4.exe", "app3.exe"]
        );
        assert_eq!(
            (summary.top_apps[0].down, summary.top_apps[0].up),
            (1100, 20)
        );
        assert_eq!(
            (
                summary.classified.lan,
                summary.classified.wan,
                summary.classified.unknown
            ),
            (210, 3500, 320)
        );
        assert!(
            store
                .transport_summary(
                    &Filter {
                        adapters: std::collections::HashSet::from(["__none__".into()]),
                        ..filter
                    },
                    900
                )?
                .top_apps
                .is_empty()
        );
        Ok(())
    }
}

#[cfg(test)]
mod unlimited_history_tests {
    use super::*;
    use crate::model::{Source, TimeRange};
    #[test]
    fn old_history_survives_cleanup_and_date_ranges_match_chart_summary_and_export() -> Result<()> {
        use std::collections::{BTreeMap, HashMap};
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("history.db");
        let now = 60 * 86400;
        let old = 600;
        let recent = now - 600;
        {
            let mut store = Store::open(&path)?;
            for (time, down) in [(old, 100), (recent, 200)] {
                let adapter = UsageRow {
                    source: "adapter".into(),
                    adapter_id: "wifi".into(),
                    wifi_network: "home".into(),
                    down,
                    up: down / 10,
                    ..Default::default()
                };
                let app = UsageRow {
                    source: "transport".into(),
                    app: "browser.exe".into(),
                    scope: "WAN".into(),
                    ..adapter.clone()
                };
                store.write_capture(
                    &BTreeMap::from([(time, vec![adapter, app])]),
                    &[LiveTick {
                        timestamp: time,
                        interval: 1.0,
                        adapters: HashMap::from([("wifi".into(), (down, down / 10))]),
                        wifi_networks: HashMap::from([("wifi".into(), "home".into())]),
                    }],
                )?;
            }
            store.prune_samples(now - 86400)?;
        }
        let store = Store::open(&path)?;
        let all = Filter {
            seconds: 0,
            wifi_network: Some("home".into()),
            ..Default::default()
        };
        let history = store.history(&all, now)?;
        assert_eq!((history.down, history.up), (300, 30));
        assert_eq!((history.window_start, history.window_end), (old, now));
        assert!(
            history
                .points
                .iter()
                .any(|p| p.observed && p.timestamp == old)
        );
        assert_eq!(
            store
                .db
                .query_row("SELECT COUNT(*) FROM adapter_samples", [], |r| r
                    .get::<_, i64>(0))?,
            1
        );
        let custom = Filter {
            seconds: 60,
            time_range: Some(TimeRange {
                start: old,
                end: old + 59,
            }),
            ..all.clone()
        };
        let history = store.history(&custom, now)?;
        assert_eq!((history.down, history.up), (100, 10));
        assert_eq!((history.window_start, history.window_end), (old, old + 59));
        assert!(history.points.iter().all(|p| p.timestamp <= old + 59));
        assert_eq!(store.transport_summary(&custom, now)?.top_apps[0].down, 100);
        let csv = dir.path().join("custom.csv");
        assert_eq!(store.export(&csv, &custom, now)?, 1);
        assert_eq!(
            csv::Reader::from_path(csv)?
                .records()
                .next()
                .unwrap()?
                .get(10),
            Some("100")
        );
        let long = Filter {
            source: Source::Transport,
            seconds: 90 * 86400,
            ..all
        };
        assert_eq!(store.history(&long, now)?.down, 300);
        let json = serde_json::to_string(&custom)?;
        let restored: Filter = serde_json::from_str(&json)?;
        assert_eq!(restored.time_range, custom.time_range);
        assert_eq!(store.history(&restored, now)?.down, 100);
        Ok(())
    }
}

#[cfg(test)]
mod month_to_date_tests {
    use super::*;
    use crate::model::{PERIOD_MONTH_TO_DATE, Source};
    use chrono::TimeZone;
    #[test]
    fn month_to_date_filters_history_summary_and_csv_consistently() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let now = chrono::Local
            .with_ymd_and_hms(2026, 10, 29, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp();
        let filter = Filter {
            seconds: PERIOD_MONTH_TO_DATE,
            ..Default::default()
        };
        let (start, _) = filter.time_bounds(now);
        for (timestamp, down) in [(start - 60, 100), (start, 200), (now, 300), (now + 60, 999)] {
            let row = UsageRow {
                source: "adapter".into(),
                adapter_id: "wifi".into(),
                down,
                up: down / 10,
                ..Default::default()
            };
            let app = UsageRow {
                source: "transport".into(),
                app: "browser.exe".into(),
                scope: "WAN".into(),
                ..row.clone()
            };
            store.write(timestamp, &[row, app])?;
        }
        let history = store.history(&filter, now)?;
        assert_eq!((history.down, history.up), (500, 50));
        assert_eq!((history.window_start, history.window_end), (start, now));
        assert_eq!(store.transport_summary(&filter, now)?.top_apps[0].down, 500);
        let app_filter = Filter {
            source: Source::Transport,
            ..filter
        };
        assert_eq!(store.history(&app_filter, now)?.down, 500);
        let directory = tempfile::tempdir()?;
        assert_eq!(
            store.export(&directory.path().join("month.csv"), &app_filter, now)?,
            2
        );
        Ok(())
    }
}

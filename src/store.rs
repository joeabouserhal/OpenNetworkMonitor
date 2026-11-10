use crate::model::{
    Adapter, AppSeries, AppUsage, CompactionPlan, Filter, History, HostTotal, LiveTick, Point,
    RecentApp, ScopeTotals, SeriesView, TransportSummary, UsageRow, WIFI_UNKNOWN, WifiNetwork,
    local_day_start,
};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension as _, params};
use std::path::Path;

/// The database was written by a later release, so this one must not touch it.
#[derive(Debug)]
pub struct NewerVersion;
impl std::fmt::Display for NewerVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("History was created by a newer OpenNetworkMonitor version")
    }
}
impl std::error::Error for NewerVersion {}

const SCHEMA_VERSION: i64 = 7;

/// Sets a database that cannot be opened aside, together with its WAL and shared
/// memory files, so a fresh one can take its place without destroying anything.
pub fn move_aside(path: &Path, label: &str) -> Result<std::path::PathBuf> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("The database path has no file name")?;
    let moved = path.with_file_name(format!("{name}.damaged-{label}"));
    anyhow::ensure!(!moved.exists(), "{} already exists", moved.display());
    std::fs::rename(path, &moved).context("Move the damaged database aside")?;
    for suffix in ["-wal", "-shm"] {
        let side = path.with_file_name(format!("{name}{suffix}"));
        if side.exists() {
            let _ = std::fs::rename(
                &side,
                path.with_file_name(format!("{name}{suffix}.damaged-{label}")),
            );
        }
    }
    Ok(moved)
}

pub struct Store {
    db: Connection,
    /// What each host used since the totals table was last written. Hosts are scattered across
    /// that table, so updating them every second would rewrite dozens of pages a second; they
    /// are saved together about once a minute instead, and when the store is closed.
    hosts: std::collections::HashMap<String, HostTotal>,
    hosts_saved: std::time::Instant,
}
/// Sorting and grouping a long period needs scratch space. By default SQLite puts what does not
/// fit its small cache in a temporary file, which wrote megabytes to the disk on every refresh;
/// keep it in memory, and give each connection a cache big enough for a period's rows.
const READ_PRAGMAS: &str = "PRAGMA temp_store=MEMORY; PRAGMA cache_size=-8192;";
/// How long host totals are held back before they are written.
const HOSTS_SAVE_EVERY: std::time::Duration = std::time::Duration::from_secs(60);
impl Drop for Store {
    fn drop(&mut self) {
        let _ = self.flush_hosts();
    }
}
/// Old history is merged into these, and `traffic_all` reads all three as one table with the
/// length of each row's bucket in `span`. Merged rows have no process, address or country.
const TIERS_SQL: &str = "
    CREATE TABLE IF NOT EXISTS traffic_hourly(bucket INTEGER NOT NULL,source TEXT NOT NULL,adapter_id TEXT NOT NULL,adapter TEXT NOT NULL,app TEXT NOT NULL,protocol TEXT NOT NULL,scope TEXT NOT NULL,traffic_type TEXT NOT NULL DEFAULT '',wifi_network TEXT NOT NULL DEFAULT '',down INTEGER NOT NULL,up INTEGER NOT NULL,PRIMARY KEY(source,bucket,adapter_id,app,protocol,scope,traffic_type,wifi_network)) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS traffic_daily(bucket INTEGER NOT NULL,source TEXT NOT NULL,adapter_id TEXT NOT NULL,adapter TEXT NOT NULL,app TEXT NOT NULL,protocol TEXT NOT NULL,scope TEXT NOT NULL,traffic_type TEXT NOT NULL DEFAULT '',wifi_network TEXT NOT NULL DEFAULT '',down INTEGER NOT NULL,up INTEGER NOT NULL,PRIMARY KEY(source,bucket,adapter_id,app,protocol,scope,traffic_type,wifi_network)) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS host_totals(remote TEXT PRIMARY KEY,country TEXT NOT NULL DEFAULT '',down INTEGER NOT NULL,up INTEGER NOT NULL,first_seen INTEGER NOT NULL,last_seen INTEGER NOT NULL) WITHOUT ROWID;
    CREATE VIEW IF NOT EXISTS traffic_all AS
        SELECT minute,source,adapter_id,adapter,app,pid,remote,country,protocol,scope,down,up,traffic_type,wifi_network,60 AS span FROM traffic
        UNION ALL SELECT bucket,source,adapter_id,adapter,app,0,'','',protocol,scope,down,up,traffic_type,wifi_network,3600 FROM traffic_hourly
        UNION ALL SELECT bucket,source,adapter_id,adapter,app,0,'','',protocol,scope,down,up,traffic_type,wifi_network,86400 FROM traffic_daily;";

fn db_integrity(db: &Connection) -> Result<()> {
    let integrity: String = db.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
    anyhow::ensure!(
        integrity == "ok",
        "Saved history failed its integrity check. Preserve usage.db and its WAL before recovery: {integrity}"
    );
    Ok(())
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let mut db = Connection::open(path).context("Open usage database")?;
        db.busy_timeout(std::time::Duration::from_secs(3))?;
        let version: i64 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(NewerVersion.into());
        }
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            PRAGMA journal_size_limit=67108864;
            PRAGMA temp_store=MEMORY; PRAGMA cache_size=-8192;
            CREATE TABLE IF NOT EXISTS traffic (
            minute INTEGER NOT NULL, source TEXT NOT NULL, adapter_id TEXT NOT NULL,
            adapter TEXT NOT NULL, app TEXT NOT NULL, pid INTEGER NOT NULL, remote TEXT NOT NULL,
            country TEXT NOT NULL, protocol TEXT NOT NULL, scope TEXT NOT NULL,
            down INTEGER NOT NULL, up INTEGER NOT NULL, traffic_type TEXT NOT NULL DEFAULT '',
            PRIMARY KEY(minute,source,adapter_id,app,pid,remote,protocol,scope,traffic_type));
            CREATE TABLE IF NOT EXISTS visible_adapters (id TEXT PRIMARY KEY, name TEXT NOT NULL DEFAULT '', last_seen INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS adapter_samples (
                timestamp INTEGER NOT NULL, adapter_id TEXT NOT NULL,
                down INTEGER NOT NULL, up INTEGER NOT NULL, elapsed REAL NOT NULL,
                PRIMARY KEY(timestamp,adapter_id));
            ",
        )?;
        // Older layouts get the time index their queries relied on until the last step
        // below replaces the whole table; the current layout needs none.
        if version < 6 {
            db.execute_batch(
                "CREATE INDEX IF NOT EXISTS traffic_time_source ON traffic(source,minute);",
            )?;
        }
        // A current database is checked in the background after launch, since the scan
        // grows with its size. One that is about to be migrated is checked first so
        // that damage is never rewritten into the new layout.
        if version < SCHEMA_VERSION {
            db_integrity(&db)?;
        }
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
            "CREATE TABLE IF NOT EXISTS wifi_networks(id TEXT PRIMARY KEY,name TEXT NOT NULL);",
        )?;
        if version < 5 {
            tx.execute_batch("PRAGMA user_version=5;")?;
        }
        tx.commit()?;
        if version < 6 {
            // Lead the key with source and minute and drop the separate rowid table and
            // time index: every period query is a range over exactly those two columns,
            // and the key was otherwise stored a second and third time.
            let tx = db.transaction()?;
            tx.execute_batch(
                "ALTER TABLE traffic RENAME TO traffic_v5;
                CREATE TABLE traffic(minute INTEGER NOT NULL,source TEXT NOT NULL,adapter_id TEXT NOT NULL,adapter TEXT NOT NULL,app TEXT NOT NULL,pid INTEGER NOT NULL,remote TEXT NOT NULL,country TEXT NOT NULL,protocol TEXT NOT NULL,scope TEXT NOT NULL,down INTEGER NOT NULL,up INTEGER NOT NULL,traffic_type TEXT NOT NULL DEFAULT '',wifi_network TEXT NOT NULL DEFAULT '',PRIMARY KEY(source,minute,adapter_id,app,pid,remote,protocol,scope,traffic_type,wifi_network)) WITHOUT ROWID;
                INSERT INTO traffic SELECT minute,source,adapter_id,adapter,app,pid,remote,country,protocol,scope,down,up,traffic_type,wifi_network FROM traffic_v5 ORDER BY source,minute;
                DROP TABLE traffic_v5;
                PRAGMA user_version=6;",
            )?;
            tx.commit()?;
            // The old table's pages are now free but the file would keep its size. Give
            // them back once, here; failing to is harmless, they are reused by new data.
            let _ = db.execute_batch("VACUUM;");
        }
        // The coarser tiers old history is merged into, the per-host lifetime totals and the
        // view that reads every tier as one table. Creating them is harmless on any layout.
        db.execute_batch(TIERS_SQL)?;
        if version < 7 {
            // Lifetime totals start from the detail that is already saved.
            let tx = db.transaction()?;
            tx.execute_batch(
                "INSERT OR IGNORE INTO host_totals(remote,country,down,up,first_seen,last_seen)
                    SELECT remote,MAX(country),SUM(down),SUM(up),MIN(minute),MAX(minute)
                    FROM traffic WHERE source='transport' AND remote<>'' GROUP BY remote;
                PRAGMA user_version=7;",
            )?;
            tx.commit()?;
        }
        Ok(Self::with(db))
    }
    fn with(db: Connection) -> Self {
        Self {
            db,
            hosts: std::collections::HashMap::new(),
            hosts_saved: std::time::Instant::now(),
        }
    }
    /// The time before which history may be merged: the end of the newest merged bucket.
    fn compacted_end(&self) -> Result<i64> {
        let mut end = 0;
        for (table, span) in [("traffic_hourly", 3600), ("traffic_daily", 86_400)] {
            for source in ["adapter", "transport"] {
                let newest: Option<i64> = self
                    .db
                    .query_row(
                        &format!(
                            "SELECT bucket FROM {table} WHERE source=?1 ORDER BY bucket DESC LIMIT 1"
                        ),
                        [source],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(bucket) = newest {
                    end = end.max(bucket + span);
                }
            }
        }
        Ok(end)
    }
    /// What to read a period from: the plain minute rows when it starts after everything merged,
    /// otherwise the view over every tier, which costs a little more.
    fn table_for(&self, start: i64) -> Result<&'static str> {
        Ok(if start < self.compacted_end()? {
            "traffic_all"
        } else {
            "traffic"
        })
    }
    /// The earliest time with saved traffic for `source` up to `end`, in any tier.
    fn oldest_minute(&self, source: &str, end: i64) -> Result<Option<i64>> {
        let mut oldest: Option<i64> = None;
        for (table, column) in [
            ("traffic", "minute"),
            ("traffic_hourly", "bucket"),
            ("traffic_daily", "bucket"),
        ] {
            let found: Option<i64> = self.db.query_row(
                &format!("SELECT MIN({column}) FROM {table} WHERE source=?1 AND {column}<=?2"),
                params![source, end],
                |r| r.get(0),
            )?;
            oldest = match (oldest, found) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        Ok(oldest)
    }
    /// The same for one app's traffic.
    fn oldest_app_minute(&self, app: &str, end: i64) -> Result<Option<i64>> {
        let mut oldest: Option<i64> = None;
        for (table, column) in [
            ("traffic", "minute"),
            ("traffic_hourly", "bucket"),
            ("traffic_daily", "bucket"),
        ] {
            let found: Option<i64> = self.db.query_row(
                &format!(
                    "SELECT MIN({column}) FROM {table} WHERE source='transport' AND app=?1 AND {column}<=?2"
                ),
                params![app, end],
                |r| r.get(0),
            )?;
            oldest = match (oldest, found) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        Ok(oldest)
    }
    /// Merges one more slice of old history into coarser totals, as `plan` says, and returns how
    /// many rows it replaced; zero means nothing is left to merge. A slice is at most a day of
    /// minutes or a month of hours, so each call is short and the collector keeps up.
    pub fn compact_step(&mut self, plan: CompactionPlan, now: i64) -> Result<usize> {
        let (hours, days) = plan.cutoffs(now);
        if let Some(cut) = hours {
            let mut oldest: Option<i64> = None;
            for source in ["adapter", "transport"] {
                let found: Option<i64> = self.db.query_row(
                    "SELECT MIN(minute) FROM traffic WHERE source=?1",
                    [source],
                    |r| r.get(0),
                )?;
                oldest = match (oldest, found) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
            }
            if let Some(oldest) = oldest.filter(|oldest| *oldest < cut) {
                let start = oldest / 3600 * 3600;
                let end = (start + 86_400).min(cut);
                return self.merge_minutes(start, end);
            }
        }
        if let Some(cut) = days {
            let oldest: Option<i64> = self.db.query_row(
                "SELECT MIN(bucket) FROM (
                    SELECT MIN(bucket) AS bucket FROM traffic_hourly WHERE source='adapter'
                    UNION ALL SELECT MIN(bucket) FROM traffic_hourly WHERE source='transport')",
                [],
                |r| r.get(0),
            )?;
            if let Some(oldest) = oldest.filter(|oldest| *oldest < cut) {
                let start = local_day_start(oldest);
                let end = local_day_start(start + 30 * 86_400 + 43_200).min(cut);
                return self.merge_hours(start, end);
            }
        }
        Ok(0)
    }
    /// Replaces the minute rows in `[start, end)` with hourly totals per app and adapter. The
    /// per-host totals were already kept as the rows were written.
    fn merge_minutes(&mut self, start: i64, end: i64) -> Result<usize> {
        let tx = self.db.transaction()?;
        let mut moved = 0;
        for source in ["adapter", "transport"] {
            tx.execute(
                "INSERT INTO traffic_hourly SELECT minute/3600*3600,source,adapter_id,MAX(adapter),app,protocol,scope,traffic_type,wifi_network,SUM(down),SUM(up)
                FROM traffic WHERE source=?1 AND minute>=?2 AND minute<?3
                GROUP BY minute/3600*3600,adapter_id,app,protocol,scope,traffic_type,wifi_network
                ON CONFLICT DO UPDATE SET down=down+excluded.down,up=up+excluded.up",
                params![source, start, end],
            )?;
            moved += tx.execute(
                "DELETE FROM traffic WHERE source=?1 AND minute>=?2 AND minute<?3",
                params![source, start, end],
            )?;
        }
        tx.commit()?;
        Ok(moved)
    }
    /// Replaces the hourly rows in `[start, end)` with daily totals, days being local calendar
    /// days so they line up with the daily charts.
    fn merge_hours(&mut self, start: i64, end: i64) -> Result<usize> {
        let tx = self.db.transaction()?;
        let mut moved = 0;
        for source in ["adapter", "transport"] {
            tx.execute(
                "INSERT INTO traffic_daily SELECT CAST(strftime('%s',bucket,'unixepoch','localtime','start of day','utc') AS INTEGER),source,adapter_id,MAX(adapter),app,protocol,scope,traffic_type,wifi_network,SUM(down),SUM(up)
                FROM traffic_hourly WHERE source=?1 AND bucket>=?2 AND bucket<?3
                GROUP BY 1,adapter_id,app,protocol,scope,traffic_type,wifi_network
                ON CONFLICT DO UPDATE SET down=down+excluded.down,up=up+excluded.up",
                params![source, start, end],
            )?;
            moved += tx.execute(
                "DELETE FROM traffic_hourly WHERE source=?1 AND bucket>=?2 AND bucket<?3",
                params![source, start, end],
            )?;
        }
        tx.commit()?;
        Ok(moved)
    }
    /// The biggest hosts by lifetime use, however much detailed history has been merged since.
    pub fn host_totals(&self, limit: usize) -> Result<Vec<HostTotal>> {
        let mut stmt = self.db.prepare_cached(
            "SELECT remote,country,down,up,first_seen,last_seen FROM host_totals
            ORDER BY down+up DESC,remote LIMIT ?1",
        )?;
        Ok(stmt
            .query_map([limit as i64], |r| {
                Ok(HostTotal {
                    remote: r.get(0)?,
                    country: r.get(1)?,
                    down: r.get(2)?,
                    up: r.get(3)?,
                    first_seen: r.get(4)?,
                    last_seen: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Space inside the file that holds nothing now, for example after history was merged.
    pub fn free_bytes(&self) -> Result<u64> {
        let free: u64 = self
            .db
            .pragma_query_value(None, "freelist_count", |r| r.get(0))?;
        let page: u64 = self
            .db
            .pragma_query_value(None, "page_size", |r| r.get(0))?;
        Ok(free * page)
    }
    /// The size of the database file in bytes.
    pub fn file_bytes(&self) -> Result<u64> {
        let pages: u64 = self
            .db
            .pragma_query_value(None, "page_count", |r| r.get(0))?;
        let page: u64 = self
            .db
            .pragma_query_value(None, "page_size", |r| r.get(0))?;
        Ok(pages * page)
    }
    /// Gives the unused space back to the system by rewriting the file. Takes a few seconds
    /// on a large history, during which nothing else can write to it.
    pub fn reclaim(&mut self) -> Result<()> {
        self.db
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;")?;
        Ok(())
    }
    /// Scans the file for damage. Slow on a large history, so not part of `open`.
    pub fn integrity_check(&self) -> Result<()> {
        db_integrity(&self.db)
    }
    /// The apps most recently recorded using the network, newest first, whatever the
    /// period or adapter filters say.
    pub fn recent_apps(&self, limit: usize) -> Result<Vec<RecentApp>> {
        let mut stmt = self.db.prepare_cached(
            "SELECT app,MAX(minute) AS last FROM traffic_all WHERE source='transport' AND app<>''
            GROUP BY app ORDER BY last DESC,app LIMIT ?1",
        )?;
        Ok(stmt
            .query_map([limit as i64], |r| {
                Ok(RecentApp {
                    app: r.get(0)?,
                    last_seen: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Every app that has ever been recorded as using the network.
    pub fn known_apps(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.db.prepare(
            "SELECT app FROM traffic WHERE source='transport'
                UNION SELECT app FROM traffic_hourly WHERE source='transport'
                UNION SELECT app FROM traffic_daily WHERE source='transport'",
        )?;
        Ok(stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    /// Bytes carried by `adapters` between two times: what the Overall total counts.
    pub fn adapter_usage(
        &self,
        start: i64,
        end: i64,
        adapters: &std::collections::HashSet<String>,
    ) -> Result<u64> {
        let table = self.table_for(start / 60 * 60)?;
        let mut stmt = self.db.prepare_cached(&format!(
            "SELECT adapter_id,SUM(down),SUM(up) FROM {table}
            WHERE source='adapter' AND minute>=?1 AND minute<=?2 GROUP BY adapter_id"
        ))?;
        let mut total = 0u64;
        for row in stmt.query_map(params![start / 60 * 60, end], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, u64>(1)?,
                r.get::<_, u64>(2)?,
            ))
        })? {
            let (adapter, down, up) = row?;
            if adapters.contains(&adapter) {
                total = total.saturating_add(down).saturating_add(up);
            }
        }
        Ok(total)
    }
    /// The folder holding the database file, if it has one.
    pub fn folder(&self) -> Option<std::path::PathBuf> {
        self.db
            .path()
            .filter(|path| !path.is_empty())
            .and_then(|path| Path::new(path).parent().map(Path::to_path_buf))
    }
    /// A second, read-only connection to the same database file for a background
    /// reader. WAL lets it read while the collector keeps writing. `None` for
    /// databases that have no file (in-memory), which cannot be shared.
    pub fn open_reader(&self) -> Result<Option<Self>> {
        let Some(path) = self.db.path().filter(|path| !path.is_empty()) else {
            return Ok(None);
        };
        let db = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .context("Open usage database for reading")?;
        db.busy_timeout(std::time::Duration::from_secs(3))?;
        db.execute_batch(READ_PRAGMAS)?;
        Ok(Some(Self::with(db)))
    }
    /// Lets another thread abort a query running on this connection.
    pub fn interrupt_handle(&self) -> rusqlite::InterruptHandle {
        self.db.get_interrupt_handle()
    }
    pub fn write(&mut self, timestamp: i64, rows: &[UsageRow]) -> Result<()> {
        let tx = self.db.transaction()?;
        write_rows(&tx, timestamp, rows)?;
        tx.commit()?;
        self.note_hosts(timestamp, rows);
        self.flush_hosts()
    }
    /// Adds what `rows` carried to each host's running total, in memory for now.
    fn note_hosts(&mut self, timestamp: i64, rows: &[UsageRow]) {
        let minute = timestamp / 60 * 60;
        for row in rows
            .iter()
            .filter(|row| row.source == "transport" && !row.remote.is_empty())
        {
            let host = self
                .hosts
                .entry(row.remote.clone())
                .or_insert_with(|| HostTotal {
                    remote: row.remote.clone(),
                    first_seen: minute,
                    last_seen: minute,
                    ..Default::default()
                });
            host.down = host.down.saturating_add(row.down);
            host.up = host.up.saturating_add(row.up);
            if !row.country.is_empty() {
                host.country.clone_from(&row.country);
            }
            host.first_seen = host.first_seen.min(minute);
            host.last_seen = host.last_seen.max(minute);
        }
    }
    /// Writes the held-back host totals to the table. Done at least once a minute and on exit.
    pub fn flush_hosts(&mut self) -> Result<()> {
        if self.hosts.is_empty() {
            return Ok(());
        }
        let tx = self.db.transaction()?;
        {
            let mut stmt = tx.prepare_cached("INSERT INTO host_totals(remote,country,down,up,first_seen,last_seen) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(remote) DO UPDATE SET down=down+excluded.down,up=up+excluded.up,country=CASE WHEN excluded.country='' THEN country ELSE excluded.country END,first_seen=MIN(first_seen,excluded.first_seen),last_seen=MAX(last_seen,excluded.last_seen)")?;
            for host in self.hosts.values() {
                stmt.execute(params![
                    host.remote,
                    host.country,
                    host.down,
                    host.up,
                    host.first_seen,
                    host.last_seen
                ])?;
            }
        }
        tx.commit()?;
        self.hosts.clear();
        self.hosts_saved = std::time::Instant::now();
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
        for (timestamp, rows) in batches {
            self.note_hosts(*timestamp, rows);
        }
        if self.hosts_saved.elapsed() >= HOSTS_SAVE_EVERY {
            // A failure keeps the totals held back; the next minute tries again.
            let _ = self.flush_hosts();
        }
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
    /// One app's transport traffic over the period of `filter`, honouring its adapter,
    /// Wi-Fi network and destination-scope choices but not its text search.
    pub fn app_series(
        &self,
        filter: &Filter,
        app: &str,
        now: i64,
        view: SeriesView,
    ) -> Result<AppSeries> {
        let (mut start, end) = view.window(now).unwrap_or_else(|| filter.time_bounds(now));
        if view == SeriesView::Period && filter.all_time() {
            start = self
                .oldest_app_minute(app, end)?
                .unwrap_or(end.saturating_sub(3600));
        }
        let start = start / 60 * 60;
        // Long periods are read an hour at a time, which keeps the answer small.
        let step: i64 = if end.saturating_sub(start) > 2 * 86_400 {
            3600
        } else {
            60
        };
        let table = self.table_for(start)?;
        let mut stmt = self.db.prepare_cached(&format!(
            "SELECT minute/?1*?1,adapter_id,scope,wifi_network,SUM(down),SUM(up)
            FROM {table} WHERE source='transport' AND minute>=?2 AND minute<=?3 AND app=?4
            GROUP BY minute/?1*?1,adapter_id,scope,wifi_network ORDER BY 1"
        ))?;
        let mut points = std::collections::BTreeMap::<i64, (u64, u64)>::new();
        let rows = stmt.query_map(params![step, start, end, app], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, u64>(4)?,
                r.get::<_, u64>(5)?,
            ))
        })?;
        for row in rows {
            let (at, adapter, scope, wifi, down, up) = row?;
            let wanted = (filter.adapters.is_empty() || filter.adapters.contains(&adapter))
                && filter.wifi_network.as_ref().is_none_or(|id| id == &wifi)
                && (filter.scope == crate::model::Scope::All || filter.scope.key() == scope);
            if wanted {
                let point = points.entry(at).or_default();
                point.0 = point.0.saturating_add(down);
                point.1 = point.1.saturating_add(up);
            }
        }
        Ok(AppSeries {
            app: app.to_owned(),
            view,
            read_at: now,
            revision: 0,
            step,
            window: (start, end),
            points: points.into_iter().map(|(at, (d, u))| (at, d, u)).collect(),
        })
    }
    pub fn history(&self, filter: &Filter, now: i64) -> Result<History> {
        let (mut window_start, window_end) = filter.time_bounds(now);
        if filter.all_time() {
            window_start = self
                .oldest_minute(filter.source.key(), window_end)?
                .unwrap_or(window_end.saturating_sub(3600));
        }
        // Totals use complete minute buckets, consistently across every view.
        let start = window_start / 60 * 60;
        // Merged tiers are only read when the period reaches back into them.
        let table = self.table_for(start)?;
        let (span, by_span) = if table == "traffic_all" {
            ("span", ",span")
        } else {
            ("60", "")
        };
        let mut stmt = self.db.prepare_cached(&format!("SELECT minute,source,adapter_id,adapter,app,pid,remote,country,protocol,scope,SUM(down),SUM(up),traffic_type,wifi_network,{span}
            FROM {table} WHERE source=?1 AND minute>=?2 AND minute<=?3
            GROUP BY minute,adapter_id,app,pid,remote,protocol,scope,traffic_type,wifi_network{by_span} ORDER BY minute"))?;
        let mut result = History {
            window_start,
            window_end,
            ..Default::default()
        };
        let mut points = std::collections::BTreeMap::<i64, (u64, u64, i64)>::new();
        let mut totals = std::collections::BTreeMap::<
            (String, String, u32, String, String, String, String, String),
            UsageRow,
        >::new();
        let iter = stmt.query_map(params![filter.source.key(), start, window_end], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(14)?,
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
            let (minute, bucket, row) = item?;
            if !filter.matches(&row) {
                continue;
            }
            let point = points.entry(minute).or_default();
            point.0 = point.0.saturating_add(row.down);
            point.1 = point.1.saturating_add(row.up);
            point.2 = bucket;
            result.down = result.down.saturating_add(row.down);
            result.up = result.up.saturating_add(row.up);
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
            item.down = item.down.saturating_add(row.down);
            item.up = item.up.saturating_add(row.up);
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
        for (minute, (down, up, bucket)) in points {
            if !raw_minutes.contains(&minute) {
                series.push((minute, down, up, bucket as f64, bucket));
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
        let table = self.table_for(start)?;
        let span = if table == "traffic_all" { "span" } else { "60" };
        let mut stmt = self.db.prepare(&format!("SELECT minute,source,adapter_id,adapter,app,pid,remote,country,protocol,scope,down,up,traffic_type,wifi_network,{span} FROM {table} WHERE source=?1 AND minute>=?2 AND minute<=?3 ORDER BY minute"))?;
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
            "bucket_seconds",
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
                r.get::<_, i64>(14)?,
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
            let (minute, bucket, r) = row?;
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
                bucket.to_string(),
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
        let (start, end) = filter.time_bounds(now);
        let table = self.table_for(start / 60 * 60)?;
        let mut stmt=self.db.prepare_cached(&format!("SELECT adapter_id,app,scope,SUM(down),SUM(up),wifi_network FROM {table} WHERE source='transport' AND minute>=?1 AND minute<=?2 GROUP BY adapter_id,app,scope,wifi_network"))?;
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
    let mut stmt=tx.prepare_cached("INSERT INTO traffic(minute,source,adapter_id,adapter,app,pid,remote,country,protocol,scope,down,up,traffic_type,wifi_network) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14) ON CONFLICT(source,minute,adapter_id,app,pid,remote,protocol,scope,traffic_type,wifi_network) DO UPDATE SET down=down+excluded.down,up=up+excluded.up,country=excluded.country,adapter=excluded.adapter")?;
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
/// Keeps spreadsheets from running a cell as a formula. Excel ignores leading
/// whitespace and also treats the full-width forms and `|` as formula starts.
fn csv_safe(value: &str) -> String {
    const FORMULA_STARTS: [char; 9] = ['=', '+', '-', '@', '|', '＝', '＋', '－', '＠'];
    let risky = value.starts_with(['\t', '\r', '\n'])
        || value
            .trim_start()
            .starts_with(|first| FORMULA_STARTS.contains(&first));
    if risky {
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
    fn traffic(source: &str, app: &str, remote: &str, down: u64, up: u64) -> UsageRow {
        UsageRow {
            source: source.into(),
            adapter_id: "eth".into(),
            adapter: "Ethernet".into(),
            app: app.into(),
            pid: 77,
            remote: remote.into(),
            country: if remote.is_empty() { "" } else { "XX" }.into(),
            protocol: "TCP".into(),
            scope: "WAN".into(),
            traffic_type: "HTTPS / TLS".into(),
            down,
            up,
            ..Default::default()
        }
    }
    /// Ten days of traffic starting on a day boundary: every minute of the first two hours of
    /// each day, from two hosts, plus the adapter's own counters.
    fn ten_days(store: &mut Store) -> i64 {
        let first = 1_700_000_000 / 86_400 * 86_400;
        for day in 0..10 {
            for minute in 0..120 {
                let at = first + day * 86_400 + minute * 60;
                store
                    .write(
                        at,
                        &[
                            traffic("transport", "a.exe", "1.1.1.1", 100, 10),
                            traffic("transport", "a.exe", "8.8.8.8", 50, 5),
                            traffic("adapter", "", "", 150, 15),
                        ],
                    )
                    .unwrap();
            }
        }
        first
    }

    #[test]
    fn merging_minutes_into_hours_keeps_totals_charts_and_exports() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let first = ten_days(&mut store);
        let now = first + 11 * 86_400;
        let filter = Filter {
            source: Source::Transport,
            time_range: Some(crate::model::TimeRange {
                start: first,
                end: now,
            }),
            ..Default::default()
        };
        let before = store.history(&filter, now)?;
        assert_eq!((before.down, before.up), (10 * 120 * 150, 10 * 120 * 15));
        let plan = CompactionPlan {
            detail_days: Some(3),
            daily_days: None,
        };
        let mut moved = 0;
        while let rows @ 1.. = store.compact_step(plan, now)? {
            moved += rows;
        }
        assert!(moved > 0, "old minutes were merged");
        let (hours, _) = plan.cutoffs(now);
        let left: i64 = store.db.query_row(
            "SELECT COUNT(*) FROM traffic WHERE minute<?1",
            [hours.unwrap()],
            |r| r.get(0),
        )?;
        assert_eq!(left, 0, "nothing old stays as minutes");
        let after = store.history(&filter, now)?;
        assert_eq!((after.down, after.up), (before.down, before.up));
        assert!(
            after.rows.iter().any(|r| r.remote.is_empty()),
            "merged rows have no host"
        );
        // An hour holding 60 minutes of 150 bytes reads as bytes per second over that hour.
        let early = after
            .points
            .iter()
            .find(|p| p.observed && p.timestamp == first)
            .expect("the first hour is charted");
        assert_eq!(early.down, (60 * 150) / 3600);
        assert_eq!(store.compact_step(plan, now)?, 0, "nothing is left to do");
        // A recent period reads the minute rows alone.
        let recent = store.history(
            &Filter {
                source: Source::Transport,
                time_range: Some(crate::model::TimeRange {
                    start: first + 9 * 86_400,
                    end: now,
                }),
                ..Default::default()
            },
            now,
        )?;
        assert_eq!(recent.down, 120 * 150);
        // The export carries merged rows and says how wide each bucket is.
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("out.csv");
        let rows = store.export(&path, &filter, now)?;
        let text = std::fs::read_to_string(&path)?;
        assert!(text.lines().next().unwrap().ends_with("bucket_seconds"));
        assert!(text.lines().any(|line| line.ends_with(",3600")));
        assert!(text.lines().any(|line| line.ends_with(",60")));
        assert_eq!(text.lines().count(), rows + 1);
        // The data cap's use of the adapter counts merged hours too.
        let used = store.adapter_usage(
            first,
            now,
            &std::collections::HashSet::from(["eth".to_string()]),
        )?;
        assert_eq!(used, 10 * 120 * (150 + 15));
        Ok(())
    }

    #[test]
    fn merging_hours_into_days_lines_up_with_local_days() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let first = ten_days(&mut store);
        let now = first + 11 * 86_400;
        let plan = CompactionPlan {
            detail_days: Some(2),
            daily_days: Some(5),
        };
        while store.compact_step(plan, now)? > 0 {}
        let (_, days) = plan.cutoffs(now);
        let days = days.unwrap();
        let stray: i64 = store.db.query_row(
            "SELECT COUNT(*) FROM traffic_hourly WHERE bucket<?1",
            [days],
            |r| r.get(0),
        )?;
        assert_eq!(stray, 0, "old hours became days");
        let buckets: Vec<i64> = {
            let mut q = store
                .db
                .prepare("SELECT DISTINCT bucket FROM traffic_daily ORDER BY bucket")?;
            q.query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        assert!(!buckets.is_empty());
        for bucket in &buckets {
            assert_eq!(
                *bucket,
                local_day_start(*bucket),
                "a day starts at local midnight"
            );
        }
        // A period starts on a merged bucket's first second to count it whole.
        let filter = Filter {
            source: Source::Transport,
            time_range: Some(crate::model::TimeRange {
                start: local_day_start(first),
                end: now,
            }),
            ..Default::default()
        };
        let all = store.history(&filter, now)?;
        assert_eq!(
            all.down,
            10 * 120 * 150,
            "no byte is lost across three tiers"
        );
        Ok(())
    }

    #[test]
    fn lifetime_host_totals_outlive_the_detail_they_came_from() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let first = ten_days(&mut store);
        let now = first + 11 * 86_400;
        let plan = CompactionPlan {
            detail_days: Some(1),
            daily_days: None,
        };
        while store.compact_step(plan, now)? > 0 {}
        let hosts = store.host_totals(10)?;
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].remote, "1.1.1.1");
        assert_eq!(
            (hosts[0].down, hosts[0].up),
            (10 * 120 * 100, 10 * 120 * 10)
        );
        assert_eq!(hosts[0].country, "XX");
        assert_eq!(hosts[0].first_seen, first);
        assert_eq!(hosts[0].last_seen, first + 9 * 86_400 + 119 * 60);
        assert_eq!(hosts[1].remote, "8.8.8.8");
        // The old per-host rows really are gone from the history tables.
        let detail: i64 = store.db.query_row(
            "SELECT COUNT(*) FROM traffic WHERE minute<?1 AND remote<>''",
            [plan.cutoffs(now).0.unwrap()],
            |r| r.get(0),
        )?;
        assert_eq!(detail, 0);
        Ok(())
    }

    #[test]
    fn host_totals_are_held_back_and_saved_together_without_losing_any() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("usage.db");
        let mut store = Store::open(&path)?;
        let mut batches = std::collections::BTreeMap::new();
        for second in 0..5 {
            batches.insert(
                600 + second,
                vec![
                    traffic("transport", "a.exe", "1.1.1.1", 100, 10),
                    traffic("transport", "a.exe", "8.8.8.8", 50, 5),
                ],
            );
            store.write_capture(&batches, &[])?;
            batches.clear();
        }
        // The history rows are saved at once; the per-host totals wait for the next save.
        assert!(store.host_totals(10)?.is_empty());
        let rows: i64 = store
            .db
            .query_row("SELECT COUNT(*) FROM traffic", [], |r| r.get(0))?;
        assert_eq!(rows, 2);
        store.flush_hosts()?;
        let hosts = store.host_totals(10)?;
        assert_eq!(hosts.len(), 2);
        assert_eq!((hosts[0].down, hosts[0].up), (500, 50));
        // Closing the store saves what is still held back.
        store.write_capture(
            &std::collections::BTreeMap::from([(
                660,
                vec![traffic("transport", "a.exe", "1.1.1.1", 7, 1)],
            )]),
            &[],
        )?;
        drop(store);
        let store = Store::open(&path)?;
        let hosts = store.host_totals(10)?;
        assert_eq!((hosts[0].down, hosts[0].up), (507, 51));
        assert_eq!(hosts[0].last_seen, 660);
        Ok(())
    }

    #[test]
    fn an_older_database_gets_lifetime_totals_from_its_detail() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("usage.db");
        {
            let mut store = Store::open(&path)?;
            store.write(600, &[traffic("transport", "a.exe", "9.9.9.9", 400, 40)])?;
            store.write(660, &[traffic("transport", "a.exe", "9.9.9.9", 100, 10)])?;
            // Back to the layout before lifetime totals.
            store
                .db
                .execute_batch("DELETE FROM host_totals; PRAGMA user_version=6;")?;
        }
        let store = Store::open(&path)?;
        let hosts = store.host_totals(5)?;
        assert_eq!(hosts.len(), 1);
        assert_eq!((hosts[0].down, hosts[0].up), (500, 50));
        assert_eq!((hosts[0].first_seen, hosts[0].last_seen), (600, 660));
        Ok(())
    }

    #[test]
    fn the_daily_layer_needs_the_hourly_layer_and_starts_after_it() {
        let now = 1_800_000_000;
        let plan = CompactionPlan {
            detail_days: Some(30),
            daily_days: Some(10),
        };
        let (hours, days) = plan.cutoffs(now);
        assert_eq!(hours, Some((now - 30 * 86_400) / 3600 * 3600));
        assert!(
            days.unwrap() < hours.unwrap(),
            "days reach further back than hours"
        );
        let alone = CompactionPlan {
            detail_days: None,
            daily_days: Some(60),
        };
        assert_eq!(alone.cutoffs(now), (None, None));
        assert_eq!(CompactionPlan::default().cutoffs(now), (None, None));
    }

    /// Runs the whole migration and compaction on a copy of a real history, to see that nothing
    /// is lost and how long it takes. `ONM_REAL_DB` names the folder holding usage.db.
    #[test]
    #[ignore = "Needs a copy of a real history folder in ONM_REAL_DB"]
    fn a_real_history_migrates_and_compacts_without_losing_a_byte() -> Result<()> {
        let source = std::path::PathBuf::from(std::env::var("ONM_REAL_DB")?);
        let dir = tempfile::tempdir()?;
        for entry in std::fs::read_dir(&source)? {
            let entry = entry?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("usage.db") {
                std::fs::copy(entry.path(), dir.path().join(&name))?;
            }
        }
        let started = std::time::Instant::now();
        let mut store = Store::open(&dir.path().join("usage.db"))?;
        println!("opened and migrated in {:?}", started.elapsed());
        let sums = |store: &Store| -> Result<(u64, u64, u64, u64)> {
            let by = |source: &str| -> Result<(u64, u64)> {
                Ok(store.db.query_row(
                    "SELECT COALESCE(SUM(down),0),COALESCE(SUM(up),0) FROM traffic_all WHERE source=?1",
                    [source],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            };
            let (td, tu) = by("transport")?;
            let (ad, au) = by("adapter")?;
            Ok((td, tu, ad, au))
        };
        let before = sums(&store)?;
        let hosts_before: (u64, u64) = store.db.query_row(
            "SELECT COALESCE(SUM(down),0),COALESCE(SUM(up),0) FROM host_totals",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let file_before = store.file_bytes()?;
        let now = chrono::Utc::now().timestamp();
        let plan = CompactionPlan {
            detail_days: Some(7),
            daily_days: Some(60),
        };
        let started = std::time::Instant::now();
        let mut steps = 0;
        let mut rows = 0;
        loop {
            let moved = store.compact_step(plan, now)?;
            if moved == 0 {
                break;
            }
            steps += 1;
            rows += moved;
        }
        println!(
            "compacted {rows} rows in {steps} steps, {:?}",
            started.elapsed()
        );
        assert_eq!(sums(&store)?, before, "no byte lost in any tier");
        let started = std::time::Instant::now();
        store.reclaim()?;
        println!(
            "reclaimed in {:?}: {:.1} MB -> {:.1} MB",
            started.elapsed(),
            file_before as f64 / 1e6,
            store.file_bytes()? as f64 / 1e6
        );
        let hosts_after: (u64, u64) = store.db.query_row(
            "SELECT COALESCE(SUM(down),0),COALESCE(SUM(up),0) FROM host_totals",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(hosts_after, hosts_before, "lifetime host totals untouched");
        println!(
            "hosts {}, transport {} bytes down, host totals {} bytes down",
            store.host_totals(1_000_000)?.len(),
            before.0,
            hosts_after.0
        );
        Ok(())
    }

    /// Replays the collector's write pattern against a copy of a real history and prints how many
    /// bytes this process writes each second, so the cost of saving can be measured.
    /// `ONM_REAL_DB` names the folder holding usage.db; `ONM_PROBE_BATCH` is how many seconds of
    /// traffic go into each save (default 1, the current behaviour).
    #[test]
    #[ignore = "Needs a copy of a real history folder in ONM_REAL_DB"]
    fn write_pattern_probe() -> Result<()> {
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, GetProcessIoCounters, IO_COUNTERS,
        };
        let written = || -> u64 {
            // SAFETY: fills a plain struct for the current process.
            unsafe {
                let mut io: IO_COUNTERS = std::mem::zeroed();
                GetProcessIoCounters(GetCurrentProcess(), &mut io);
                io.WriteTransferCount
            }
        };
        let source = std::path::PathBuf::from(std::env::var("ONM_REAL_DB")?);
        let batch: i64 = std::env::var("ONM_PROBE_BATCH")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let seconds: i64 = std::env::var("ONM_PROBE_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120);
        let dir = tempfile::tempdir()?;
        for entry in std::fs::read_dir(&source)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with("usage.db") {
                std::fs::copy(entry.path(), dir.path().join(entry.file_name()))?;
            }
        }
        let mut store = Store::open(&dir.path().join("usage.db"))?;
        let adapters: Vec<String> = (0..7).map(|i| format!("adapter-{i}")).collect();
        let hosts: Vec<String> = store
            .host_totals(400)?
            .into_iter()
            .map(|h| h.remote)
            .collect();
        let apps = [
            "chrome.exe",
            "steam.exe",
            "svchost.exe",
            "qbittorrent.exe",
            "stremio.exe",
        ];
        let start = chrono::Utc::now().timestamp() / 60 * 60;
        let mut pending = std::collections::BTreeMap::<i64, Vec<UsageRow>>::new();
        let mut samples = Vec::<LiveTick>::new();
        let mut per_second = Vec::new();
        let mut last = written();
        for second in 0..seconds {
            let now = start + second;
            let rows = pending.entry(now / 60 * 60).or_default();
            for i in 0..30 {
                rows.push(UsageRow {
                    source: "transport".into(),
                    adapter_id: adapters[i % 3].clone(),
                    adapter: "x".into(),
                    app: apps[i % apps.len()].into(),
                    pid: 100 + (i % apps.len()) as u32,
                    remote: hosts[(second as usize * 7 + i * 13) % hosts.len()].clone(),
                    protocol: "TCP".into(),
                    scope: "WAN".into(),
                    traffic_type: "HTTPS / TLS".into(),
                    down: 1500 * (i as u64 + 1),
                    up: 90,
                    ..Default::default()
                });
            }
            samples.push(LiveTick {
                timestamp: now,
                interval: 1.0,
                adapters: adapters.iter().map(|a| (a.clone(), (1000, 100))).collect(),
                wifi_networks: Default::default(),
            });
            if (second + 1) % batch == 0 {
                store.write_capture(&pending, &samples)?;
                pending.clear();
                samples.clear();
            }
            // One second of work, spent waiting so the log's own pattern shows.
            std::thread::sleep(std::time::Duration::from_millis(1000));
            let total = written();
            per_second.push((total - last) / 1024);
            last = total;
        }
        let sum: u64 = per_second.iter().sum();
        println!("batch {batch}s: KB written per second: {per_second:?}");
        println!(
            "batch {batch}s: {} KB in {seconds} s = {:.0} KB/s average, {:.1} GB per day",
            sum,
            sum as f64 / seconds as f64,
            sum as f64 / seconds as f64 * 86_400.0 / 1_048_576.0 / 1.0
        );
        Ok(())
    }

    #[test]
    fn reading_history_keeps_its_scratch_space_out_of_temporary_files() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = Store::open(&dir.path().join("usage.db"))?;
        let reader = store.open_reader()?.expect("a file database can be shared");
        for db in [&store.db, &reader.db] {
            let mode: i64 = db.pragma_query_value(None, "temp_store", |r| r.get(0))?;
            assert_eq!(mode, 2, "scratch space is kept in memory");
        }
        Ok(())
    }

    #[test]
    fn reclaiming_space_shrinks_a_database_after_history_is_merged() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let mut store = Store::open(&dir.path().join("usage.db"))?;
        let first = ten_days(&mut store);
        let big = store.file_bytes()?;
        let now = first + 11 * 86_400;
        let plan = CompactionPlan {
            detail_days: Some(1),
            daily_days: None,
        };
        while store.compact_step(plan, now)? > 0 {}
        assert!(store.free_bytes()? > 0, "merged rows leave unused pages");
        store.reclaim()?;
        assert_eq!(store.free_bytes()?, 0);
        assert!(store.file_bytes()? < big, "the file got smaller");
        Ok(())
    }

    #[test]
    fn spreadsheet_formulas_escaped() {
        assert_eq!(csv_safe("=bad"), "'=bad");
        assert_eq!(csv_safe("app.exe"), "app.exe");
        for risky in [" =SUM(A1)", "|calc", "＝1+1", "\u{a0}@cmd", "\t=x", "－5"] {
            assert_eq!(csv_safe(risky), format!("'{risky}"), "{risky:?}");
        }
        assert_eq!(csv_safe("a=b"), "a=b");
        assert_eq!(csv_safe("  plain"), "  plain");
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
            SCHEMA_VERSION
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
            SCHEMA_VERSION
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

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn a_newer_history_is_refused_with_a_recognisable_error() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("usage.db");
        drop(Store::open(&path)?);
        Connection::open(&path)?.execute_batch("PRAGMA user_version=99;")?;
        let error = Store::open(&path).err().expect("newer history is refused");
        assert!(error.downcast_ref::<NewerVersion>().is_some(), "{error:#}");
        Ok(())
    }

    #[test]
    fn a_damaged_file_is_moved_aside_with_its_wal_and_a_fresh_history_opens() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("usage.db");
        std::fs::write(&path, b"this is not a database, just some damaged bytes")?;
        std::fs::write(dir.path().join("usage.db-wal"), b"wal")?;

        let moved = move_aside(&path, "test")?;
        assert_eq!(moved, dir.path().join("usage.db.damaged-test"));
        assert!(moved.exists());
        assert!(dir.path().join("usage.db-wal.damaged-test").exists());
        assert!(!path.exists() && !dir.path().join("usage.db-wal").exists());
        assert_eq!(
            std::fs::read(&moved)?,
            b"this is not a database, just some damaged bytes",
            "The old bytes are kept untouched"
        );
        Store::open(&path)?.integrity_check()?;
        let garbage = dir.path().join("other.db");
        std::fs::write(&garbage, b"this is not a database, just some damaged bytes")?;
        assert!(Store::open(&garbage).is_err());
        assert!(
            move_aside(&path, "test").is_err(),
            "An earlier set-aside copy is never overwritten"
        );
        Ok(())
    }

    #[test]
    fn opening_a_current_history_defers_the_scan_to_the_explicit_check() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("usage.db");
        let mut store = Store::open(&path)?;
        store.write(
            60,
            &[UsageRow {
                source: "adapter".into(),
                adapter_id: "a".into(),
                down: 1,
                ..Default::default()
            }],
        )?;
        store.integrity_check()?;
        let reader = store.open_reader()?.expect("file database");
        reader.integrity_check()?;
        Ok(())
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    fn table_sql(db: &Connection) -> Result<String> {
        Ok(db.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='traffic'",
            [],
            |r| r.get(0),
        )?)
    }
    fn indexes(db: &Connection) -> Result<Vec<String>> {
        let mut stmt = db.prepare(
            "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='traffic' AND sql IS NOT NULL",
        )?;
        Ok(stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    #[test]
    fn a_v5_history_is_rewritten_compactly_without_losing_or_duplicating_rows() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("usage.db");
        {
            let old = Connection::open(&path)?;
            old.execute_batch(
                "PRAGMA journal_mode=WAL;
                CREATE TABLE traffic(minute INTEGER NOT NULL,source TEXT NOT NULL,adapter_id TEXT NOT NULL,adapter TEXT NOT NULL,app TEXT NOT NULL,pid INTEGER NOT NULL,remote TEXT NOT NULL,country TEXT NOT NULL,protocol TEXT NOT NULL,scope TEXT NOT NULL,down INTEGER NOT NULL,up INTEGER NOT NULL,traffic_type TEXT NOT NULL DEFAULT '',wifi_network TEXT NOT NULL DEFAULT '',PRIMARY KEY(minute,source,adapter_id,app,pid,remote,protocol,scope,traffic_type,wifi_network));
                CREATE INDEX traffic_time_source ON traffic(source,minute);
                CREATE TABLE adapter_samples(timestamp INTEGER NOT NULL,adapter_id TEXT NOT NULL,down INTEGER NOT NULL,up INTEGER NOT NULL,elapsed REAL NOT NULL,wifi_network TEXT NOT NULL DEFAULT '',PRIMARY KEY(timestamp,adapter_id,wifi_network));
                CREATE TABLE visible_adapters(id TEXT PRIMARY KEY, name TEXT NOT NULL DEFAULT '', last_seen INTEGER NOT NULL DEFAULT 0);
                CREATE TABLE wifi_networks(id TEXT PRIMARY KEY,name TEXT NOT NULL);
                INSERT INTO traffic VALUES(600,'transport','eth','Ethernet','browser.exe',42,'1.1.1.1','DE','TCP','WAN',100,20,'Web','');
                INSERT INTO traffic VALUES(660,'adapter','eth','Ethernet','',0,'','','','Unknown',70,7,'','ssid:61');
                PRAGMA user_version=5;",
            )?;
        }
        let store = Store::open(&path)?;
        let sql = table_sql(&store.db)?;
        assert!(sql.contains("WITHOUT ROWID"), "{sql}");
        assert!(
            indexes(&store.db)?.is_empty(),
            "The separate time index is gone"
        );
        let (rows, down): (i64, i64) =
            store
                .db
                .query_row("SELECT COUNT(*), SUM(down) FROM traffic", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?;
        assert_eq!((rows, down), (2, 170));
        let leftover: i64 = store.db.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'traffic_v%'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(leftover, 0, "No old copy of the table is kept");
        let free: i64 = store
            .db
            .query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
        assert_eq!(free, 0, "The space of the old table is returned");

        // Writing the same minute again still adds to the existing row.
        let mut store = store;
        store.write(
            600,
            &[UsageRow {
                source: "transport".into(),
                adapter_id: "eth".into(),
                adapter: "Ethernet".into(),
                app: "browser.exe".into(),
                pid: 42,
                remote: "1.1.1.1".into(),
                protocol: "TCP".into(),
                scope: "WAN".into(),
                traffic_type: "Web".into(),
                down: 5,
                ..Default::default()
            }],
        )?;
        let down: i64 =
            store
                .db
                .query_row("SELECT down FROM traffic WHERE minute=600", [], |r| {
                    r.get(0)
                })?;
        assert_eq!(down, 105);
        drop(store);

        // Reopening a current history changes nothing and does not bring the index back.
        let store = Store::open(&path)?;
        assert!(indexes(&store.db)?.is_empty());
        assert_eq!(
            store
                .db
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
            SCHEMA_VERSION
        );
        Ok(())
    }

    #[test]
    fn a_fresh_history_starts_in_the_compact_layout_and_limits_its_journal() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = Store::open(&dir.path().join("usage.db"))?;
        assert!(table_sql(&store.db)?.contains("WITHOUT ROWID"));
        let limit: i64 = store
            .db
            .query_row("PRAGMA journal_size_limit", [], |r| r.get(0))?;
        assert_eq!(limit, 67_108_864);
        Ok(())
    }
}

#[cfg(test)]
mod app_series_tests {
    use super::*;
    use crate::model::Scope;

    fn row(app: &str, adapter: &str, scope: &str, down: u64) -> UsageRow {
        UsageRow {
            source: "transport".into(),
            adapter_id: adapter.into(),
            adapter: adapter.into(),
            app: app.into(),
            pid: 1,
            remote: "1.1.1.1".into(),
            protocol: "TCP".into(),
            scope: scope.into(),
            down,
            up: down / 10,
            ..Default::default()
        }
    }

    #[test]
    fn one_apps_series_follows_the_adapter_and_scope_choices_but_not_other_apps() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        store.write(
            600,
            &[
                row("a.exe", "eth", "WAN", 100),
                row("a.exe", "wifi", "WAN", 40),
                row("b.exe", "eth", "WAN", 999),
            ],
        )?;
        store.write(660, &[row("a.exe", "eth", "LAN", 20)])?;
        let now = 700;
        let all = Filter {
            source: crate::model::Source::Transport,
            seconds: 3600,
            ..Default::default()
        };
        let series = store.app_series(&all, "a.exe", now, SeriesView::Period)?;
        assert_eq!(series.step, 60);
        assert_eq!(series.points, vec![(600, 140, 14), (660, 20, 2)]);

        let only_eth = Filter {
            adapters: ["eth".to_owned()].into(),
            scope: Scope::Wan,
            ..all.clone()
        };
        assert_eq!(
            store
                .app_series(&only_eth, "a.exe", now, SeriesView::Period)?
                .points,
            vec![(600, 100, 10)]
        );
        assert!(
            store
                .app_series(&all, "missing.exe", now, SeriesView::Period)?
                .points
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn long_periods_are_read_an_hour_at_a_time() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        store.write(3_600_000, &[row("a.exe", "eth", "WAN", 10)])?;
        store.write(3_600_060, &[row("a.exe", "eth", "WAN", 5)])?;
        let week = Filter {
            source: crate::model::Source::Transport,
            seconds: 7 * 86_400,
            ..Default::default()
        };
        let series = store.app_series(&week, "a.exe", 3_610_000, SeriesView::Period)?;
        assert_eq!(series.step, 3600);
        assert_eq!(series.points, vec![(3_600_000, 15, 1)]);
        Ok(())
    }
}

#[cfg(test)]
mod usage_tests {
    use super::*;

    fn adapter_row(adapter: &str, down: u64, up: u64) -> UsageRow {
        UsageRow {
            source: "adapter".into(),
            adapter_id: adapter.into(),
            down,
            up,
            ..Default::default()
        }
    }

    #[test]
    fn adapter_usage_counts_known_adapters_inside_the_period_only() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        store.write(
            600,
            &[adapter_row("eth", 100, 10), adapter_row("old-vpn", 999, 9)],
        )?;
        store.write(7200, &[adapter_row("eth", 50, 5)])?;
        let known = ["eth".to_owned()].into();
        assert_eq!(store.adapter_usage(0, 1000, &known)?, 110);
        assert_eq!(store.adapter_usage(0, 8000, &known)?, 165);
        assert_eq!(store.adapter_usage(660, 8000, &known)?, 55);
        assert_eq!(store.adapter_usage(0, 8000, &Default::default())?, 0);
        Ok(())
    }

    #[test]
    fn known_apps_lists_each_recorded_app_once() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let app = |name: &str, minute: i64| {
            (
                minute,
                UsageRow {
                    source: "transport".into(),
                    app: name.into(),
                    adapter_id: "eth".into(),
                    remote: "1.1.1.1".into(),
                    ..Default::default()
                },
            )
        };
        for (minute, row) in [app("a.exe", 600), app("a.exe", 660), app("b.exe", 660)] {
            store.write(minute, &[row])?;
        }
        store.write(600, &[adapter_row("eth", 1, 1)])?;
        let apps = store.known_apps()?;
        assert_eq!(
            apps,
            ["a.exe".to_owned(), "b.exe".to_owned()]
                .into_iter()
                .collect()
        );
        assert!(Store::open(Path::new(":memory:"))?.folder().is_none());
        Ok(())
    }
}

#[cfg(test)]
mod series_view_store_tests {
    use super::*;

    #[test]
    fn daily_and_monthly_views_read_beyond_a_five_minute_period() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let now = chrono::Utc::now().timestamp() / 60 * 60;
        let row = |down| UsageRow {
            source: "transport".into(),
            adapter_id: "eth".into(),
            adapter: "eth".into(),
            app: "a.exe".into(),
            pid: 1,
            remote: "1.1.1.1".into(),
            protocol: "TCP".into(),
            scope: "WAN".into(),
            down,
            ..Default::default()
        };
        store.write(now - 3 * 86_400, &[row(30)])?;
        store.write(now - 40 * 86_400, &[row(400)])?;
        store.write(now, &[row(5)])?;
        let short = Filter {
            source: crate::model::Source::Transport,
            seconds: 300,
            ..Default::default()
        };
        let total = |view| -> Result<u64> {
            Ok(store
                .app_series(&short, "a.exe", now, view)?
                .points
                .iter()
                .map(|point| point.1)
                .sum())
        };
        assert_eq!(total(SeriesView::Period)?, 5, "Only the five minutes");
        assert_eq!(total(SeriesView::Daily)?, 35, "The last 30 days");
        assert_eq!(total(SeriesView::Monthly)?, 435, "The last 12 months");
        assert_eq!(
            store
                .app_series(&short, "a.exe", now, SeriesView::Daily)?
                .view,
            SeriesView::Daily
        );
        Ok(())
    }
}

#[cfg(test)]
mod recent_apps_tests {
    use super::*;

    #[test]
    fn apps_come_back_newest_first_with_their_last_active_minute() -> Result<()> {
        let mut store = Store::open(Path::new(":memory:"))?;
        let row = |app: &str| UsageRow {
            source: "transport".into(),
            adapter_id: "eth".into(),
            app: app.into(),
            remote: "1.1.1.1".into(),
            down: 1,
            ..Default::default()
        };
        store.write(600, &[row("old.exe"), row("both.exe")])?;
        store.write(1200, &[row("both.exe")])?;
        store.write(1800, &[row("new.exe")])?;
        store.write(
            1900,
            &[UsageRow {
                source: "adapter".into(),
                adapter_id: "eth".into(),
                down: 5,
                ..Default::default()
            }],
        )?;
        let recent = store.recent_apps(10)?;
        let names: Vec<_> = recent
            .iter()
            .map(|r| (r.app.as_str(), r.last_seen))
            .collect();
        assert_eq!(
            names,
            [("new.exe", 1800), ("both.exe", 1200), ("old.exe", 600)]
        );
        assert_eq!(store.recent_apps(2)?.len(), 2);
        Ok(())
    }
}

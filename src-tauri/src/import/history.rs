//! Browsing-history reader (P11a core).
//!
//! History is plaintext (low risk) and NOT covered by `rookie`, so this is
//! net-new: open the history DB read-only with `rusqlite` and SELECT the engine's
//! table — Chromium `urls`, Firefox `moz_places`. Timestamps differ per engine
//! and are normalized to Unix epoch milliseconds.
//!
//! Read-only: the DB is opened with `SQLITE_OPEN_READ_ONLY` so a running browser's
//! store is never mutated. A locked DB surfaces as an error the caller maps to a
//! warning (we never force-unlock — that is an infostealer behavior).

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use super::types::{BrowserFamily, HistoryEntry};

/// Microseconds between the Windows/Chromium epoch (1601-01-01) and the Unix
/// epoch (1970-01-01). Chromium stores `last_visit_time` as microseconds since
/// 1601; subtract this then convert µs→ms.
const CHROMIUM_EPOCH_DELTA_US: i64 = 11_644_473_600_000_000;

/// Read history from `db_path` for the given engine `family`, normalizing visit
/// times to Unix epoch ms. Webkit (Safari) history is not read here (binary
/// format) — returns an empty vec rather than erroring.
pub fn read_history(db_path: &Path, family: BrowserFamily) -> rusqlite::Result<Vec<HistoryEntry>> {
    // Webkit (Safari) history is a binary format not read in P11a — return empty
    // without touching the DB (the path may not exist for this family).
    if family == BrowserFamily::Webkit {
        return Ok(Vec::new());
    }
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;
    match family {
        BrowserFamily::Chromium => read_chromium(&conn),
        BrowserFamily::Firefox => read_firefox(&conn),
        BrowserFamily::Webkit => Ok(Vec::new()),
    }
}

/// Chromium `urls` table. `last_visit_time` = µs since 1601-01-01.
fn read_chromium(conn: &Connection) -> rusqlite::Result<Vec<HistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT url, title, visit_count, last_visit_time FROM urls ORDER BY last_visit_time DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let raw_time: i64 = row.get(3)?;
        Ok(HistoryEntry {
            url: row.get(0)?,
            title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            visit_count: row.get(2)?,
            last_visit_ms: chromium_time_to_unix_ms(raw_time),
        })
    })?;
    rows.collect()
}

/// Firefox `moz_places`. `last_visit_date` = µs since Unix epoch (nullable).
fn read_firefox(conn: &Connection) -> rusqlite::Result<Vec<HistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT url, title, visit_count, last_visit_date FROM moz_places \
         WHERE last_visit_date IS NOT NULL ORDER BY last_visit_date DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let raw_time: i64 = row.get::<_, Option<i64>>(3)?.unwrap_or(0);
        Ok(HistoryEntry {
            url: row.get(0)?,
            title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            visit_count: row.get(2)?,
            last_visit_ms: raw_time / 1_000, // µs → ms
        })
    })?;
    rows.collect()
}

/// Chromium µs-since-1601 → Unix epoch ms. A `0` timestamp (never visited) maps
/// to `0` rather than a large negative number.
fn chromium_time_to_unix_ms(raw_us: i64) -> i64 {
    if raw_us == 0 {
        return 0;
    }
    (raw_us - CHROMIUM_EPOCH_DELTA_US) / 1_000
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an in-process Chromium-shaped `urls` DB fixture.
    fn chromium_fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, title TEXT, \
             visit_count INTEGER, typed_count INTEGER, last_visit_time INTEGER, hidden INTEGER);",
        )
        .unwrap();
        // last_visit_time for 1970-01-01T00:00:00Z = CHROMIUM_EPOCH_DELTA_US.
        conn.execute(
            "INSERT INTO urls (url, title, visit_count, last_visit_time) VALUES (?,?,?,?)",
            rusqlite::params!["https://a.test/", "A", 3i64, CHROMIUM_EPOCH_DELTA_US],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO urls (url, title, visit_count, last_visit_time) VALUES (?,?,?,?)",
            // +2000ms after epoch.
            rusqlite::params![
                "https://b.test/",
                "B",
                1i64,
                CHROMIUM_EPOCH_DELTA_US + 2_000_000
            ],
        )
        .unwrap();
        conn
    }

    fn firefox_fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT, title TEXT, \
             visit_count INTEGER, last_visit_date INTEGER);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO moz_places (url, title, visit_count, last_visit_date) VALUES (?,?,?,?)",
            rusqlite::params!["https://f.test/", "F", 5i64, 2_000_000i64], // 2000ms
        )
        .unwrap();
        // NULL last_visit_date row must be skipped (never visited).
        conn.execute(
            "INSERT INTO moz_places (url, title, visit_count, last_visit_date) VALUES (?,?,?,NULL)",
            rusqlite::params!["https://never.test/", "N", 0i64],
        )
        .unwrap();
        conn
    }

    #[test]
    fn chromium_epoch_converts_to_unix_ms() {
        assert_eq!(chromium_time_to_unix_ms(CHROMIUM_EPOCH_DELTA_US), 0);
        assert_eq!(
            chromium_time_to_unix_ms(CHROMIUM_EPOCH_DELTA_US + 2_000_000),
            2_000
        );
        assert_eq!(chromium_time_to_unix_ms(0), 0);
    }

    #[test]
    fn reads_chromium_urls_ordered_newest_first() {
        let conn = chromium_fixture();
        let entries = read_chromium(&conn).unwrap();
        assert_eq!(entries.len(), 2);
        // ORDER BY last_visit_time DESC → b.test first.
        assert_eq!(entries[0].url, "https://b.test/");
        assert_eq!(entries[0].last_visit_ms, 2_000);
        assert_eq!(entries[1].url, "https://a.test/");
        assert_eq!(entries[1].visit_count, 3);
    }

    #[test]
    fn reads_firefox_moz_places_skipping_null_visits() {
        let conn = firefox_fixture();
        let entries = read_firefox(&conn).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url, "https://f.test/");
        assert_eq!(entries[0].last_visit_ms, 2_000);
        assert_eq!(entries[0].visit_count, 5);
    }

    #[test]
    fn webkit_family_returns_empty_without_error() {
        // No DB touched for webkit — uses a path that need not exist.
        let entries = read_history(Path::new("/nonexistent"), BrowserFamily::Webkit).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn read_history_opens_file_read_only_and_parses() {
        // Persist a chromium fixture to a temp file, then read through the public
        // read-only entry point (proves the OpenFlags path works on a real file).
        let dir = std::env::temp_dir().join(format!("weftgrid-hist-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("History");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, title TEXT, \
                 visit_count INTEGER, typed_count INTEGER, last_visit_time INTEGER, hidden INTEGER);\
                 INSERT INTO urls (url,title,visit_count,last_visit_time) \
                 VALUES ('https://x.test/','X',7,0);",
            )
            .unwrap();
        }
        let entries = read_history(&db, BrowserFamily::Chromium).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url, "https://x.test/");
        assert_eq!(entries[0].last_visit_ms, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

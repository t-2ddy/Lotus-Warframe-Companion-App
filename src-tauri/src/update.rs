//! Update: compare each capture with the last one, save what changed, keep the current state.
//!
//! Everything lives in one local SQLite file in the user's app data folder. Nothing is uploaded,
//! and each Warframe account (see `translate::inventory::account_key`) has its own history.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::translate::{diff, Change, Flat, Inventory, OwnedRiven, FLATTEN_VERSION};

/// Raw blobs kept per account; older snapshots keep their change history but drop the blob.
const KEEP_RAW: i64 = 50;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS snapshots (
    id          INTEGER PRIMARY KEY,
    account     TEXT    NOT NULL,
    captured_at INTEGER NOT NULL,   -- unix ms
    size        INTEGER NOT NULL,   -- raw JSON bytes
    changes     INTEGER NOT NULL,   -- number of rows in `changes`
    raw         BLOB                -- zstd-compressed JSON; NULL once pruned
);
CREATE TABLE IF NOT EXISTS items_current (
    account TEXT    NOT NULL,
    section TEXT    NOT NULL,
    item    TEXT    NOT NULL,
    value   INTEGER NOT NULL,
    PRIMARY KEY (account, section, item)
);
CREATE TABLE IF NOT EXISTS changes (
    id          INTEGER PRIMARY KEY,
    snapshot_id INTEGER NOT NULL REFERENCES snapshots(id),
    account     TEXT    NOT NULL,
    section     TEXT    NOT NULL,
    item        TEXT    NOT NULL,
    before      INTEGER NOT NULL,
    after       INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS changes_by_account ON changes(account, snapshot_id);
CREATE TABLE IF NOT EXISTS favorites (
    account TEXT NOT NULL,
    item    TEXT NOT NULL,
    PRIMARY KEY (account, item)
);
-- In-game name per account, from the 'Logged in <name>' line in EE.log (the inventory doesn't have it).
CREATE TABLE IF NOT EXISTS account_names (
    account TEXT PRIMARY KEY,
    name    TEXT NOT NULL
);
-- App settings (Tools toggles etc.), shared by all accounts on this PC.
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Rivens as of the latest capture (replaced every capture, even when nothing else changed).
CREATE TABLE IF NOT EXISTS rivens (
    account     TEXT NOT NULL,
    id          TEXT NOT NULL,
    item_type   TEXT NOT NULL,
    fingerprint TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS rivens_by_account ON rivens(account);
-- warframe.market prices per item and rank (shared by all accounts on this PC).
CREATE TABLE IF NOT EXISTS market_prices (
    slug       TEXT    NOT NULL,
    rank       INTEGER NOT NULL,
    wts        INTEGER,            -- lowest sell order (online / in game), NULL if none
    wtb        INTEGER,            -- highest buy order, NULL if none
    fetched_at INTEGER NOT NULL,   -- unix ms
    PRIMARY KEY (slug, rank)
);
";

/// A cached warframe.market price.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredPrice {
    pub wts: Option<i64>,
    pub wtb: Option<i64>,
    pub fetched_at: i64,
}

/// What happened to one capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// First capture for this account: stored as the starting point, no changes recorded.
    Baseline { snapshot_id: i64 },
    /// Stored with these changes.
    Changed { snapshot_id: i64, changes: Vec<Change> },
    /// Identical to the current state; nothing stored.
    Unchanged,
}

/// A recorded change with when it was seen.
#[derive(Debug, Clone)]
pub struct ChangeRow {
    pub captured_at: i64,
    pub section: String,
    pub item: String,
    pub before: i64,
    pub after: i64,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        // Added after the first release of the schema: which `flatten` version a snapshot used.
        let has_version: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('snapshots') WHERE name = 'version'")?
            .exists([])?;
        if !has_version {
            conn.execute_batch("ALTER TABLE snapshots ADD COLUMN version INTEGER NOT NULL DEFAULT 1")?;
        }
        Ok(Self { conn })
    }

    /// The `flatten` version of the account's newest snapshot.
    fn stored_version(&self, account: &str) -> rusqlite::Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT version FROM snapshots WHERE account = ?1 ORDER BY id DESC LIMIT 1",
                [account],
                |r| r.get(0),
            )
            .optional()
    }

    /// The stored current state of an account.
    pub fn current(&self, account: &str) -> rusqlite::Result<Flat> {
        let mut stmt = self.conn.prepare("SELECT section, item, value FROM items_current WHERE account = ?1")?;
        let rows = stmt.query_map([account], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))?;
        rows.collect()
    }

    /// Compares a capture with the stored state and saves it, all in one transaction.
    pub fn apply(&mut self, inv: &Inventory, raw: &[u8], captured_at: i64) -> Result<Outcome, String> {
        // Rerolling or ranking a riven changes no flat value, so rivens are saved on every capture.
        self.save_rivens(&inv.account, &inv.rivens).map_err(|e| e.to_string())?;

        let before = self.current(&inv.account).map_err(|e| e.to_string())?;
        // A new flatten version tracks things the stored state doesn't have yet; comparing would
        // report every newly tracked value as a change, so start again from this capture.
        let outdated = self.stored_version(&inv.account).map_err(|e| e.to_string())?.is_some_and(|v| v < FLATTEN_VERSION);
        let baseline = before.is_empty() || outdated;
        let changes = if baseline { Vec::new() } else { diff(&before, &inv.values) };
        if !baseline && changes.is_empty() {
            return Ok(Outcome::Unchanged);
        }

        let compressed = zstd::encode_all(raw, 3).map_err(|e| e.to_string())?;
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        let snapshot_id = {
            tx.execute(
                "INSERT INTO snapshots (account, captured_at, size, changes, raw, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![inv.account, captured_at, raw.len() as i64, changes.len() as i64, compressed, FLATTEN_VERSION],
            )
            .map_err(|e| e.to_string())?;
            let id = tx.last_insert_rowid();

            let mut add = tx
                .prepare("INSERT INTO changes (snapshot_id, account, section, item, before, after) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
                .map_err(|e| e.to_string())?;
            for c in &changes {
                add.execute(params![id, inv.account, c.section, c.item, c.before, c.after])
                    .map_err(|e| e.to_string())?;
            }

            tx.execute("DELETE FROM items_current WHERE account = ?1", [&inv.account])
                .map_err(|e| e.to_string())?;
            let mut put = tx
                .prepare("INSERT INTO items_current (account, section, item, value) VALUES (?1, ?2, ?3, ?4)")
                .map_err(|e| e.to_string())?;
            for ((section, item), value) in &inv.values {
                put.execute(params![inv.account, section, item, value]).map_err(|e| e.to_string())?;
            }

            tx.execute(
                "UPDATE snapshots SET raw = NULL WHERE account = ?1 AND raw IS NOT NULL AND id NOT IN
                    (SELECT id FROM snapshots WHERE account = ?1 ORDER BY id DESC LIMIT ?2)",
                params![inv.account, KEEP_RAW],
            )
            .map_err(|e| e.to_string())?;
            id
        };
        tx.commit().map_err(|e| e.to_string())?;

        Ok(if baseline { Outcome::Baseline { snapshot_id } } else { Outcome::Changed { snapshot_id, changes } })
    }

    fn save_rivens(&mut self, account: &str, rivens: &[OwnedRiven]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM rivens WHERE account = ?1", [account])?;
        {
            let mut put = tx.prepare("INSERT INTO rivens (account, id, item_type, fingerprint) VALUES (?1, ?2, ?3, ?4)")?;
            for r in rivens {
                put.execute(params![account, r.id, r.item_type, r.fingerprint])?;
            }
        }
        tx.commit()
    }

    /// The newest stored inventory JSON for an account (decompressed), if any is kept.
    pub fn latest_raw(&self, account: &str) -> Result<Option<Vec<u8>>, String> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT raw FROM snapshots WHERE account = ?1 AND raw IS NOT NULL ORDER BY id DESC LIMIT 1",
                [account],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        blob.map(|b| zstd::decode_all(&b[..]).map_err(|e| e.to_string())).transpose()
    }

    /// Rivens from the account's latest capture.
    pub fn rivens(&self, account: &str) -> rusqlite::Result<Vec<OwnedRiven>> {
        let mut stmt = self.conn.prepare("SELECT id, item_type, fingerprint FROM rivens WHERE account = ?1 ORDER BY rowid")?;
        let rows = stmt.query_map([account], |r| Ok(OwnedRiven { id: r.get(0)?, item_type: r.get(1)?, fingerprint: r.get(2)? }))?;
        rows.collect()
    }

    pub fn setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// Items the user starred in the app.
    pub fn favorites(&self, account: &str) -> rusqlite::Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT item FROM favorites WHERE account = ?1")?;
        let rows = stmt.query_map([account], |r| r.get(0))?;
        rows.collect()
    }

    pub fn set_favorite(&self, account: &str, item: &str, on: bool) -> rusqlite::Result<()> {
        if on {
            self.conn.execute("INSERT OR IGNORE INTO favorites (account, item) VALUES (?1, ?2)", [account, item])?;
        } else {
            self.conn.execute("DELETE FROM favorites WHERE account = ?1 AND item = ?2", [account, item])?;
        }
        Ok(())
    }

    pub fn account_name(&self, account: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row("SELECT name FROM account_names WHERE account = ?1", [account], |r| r.get(0))
            .optional()
    }

    pub fn set_account_name(&self, account: &str, name: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO account_names (account, name) VALUES (?1, ?2)
             ON CONFLICT (account) DO UPDATE SET name = ?2",
            [account, name],
        )?;
        Ok(())
    }

    pub fn price(&self, slug: &str, rank: u32) -> rusqlite::Result<Option<StoredPrice>> {
        self.conn
            .query_row(
                "SELECT wts, wtb, fetched_at FROM market_prices WHERE slug = ?1 AND rank = ?2",
                params![slug, rank],
                |r| Ok(StoredPrice { wts: r.get(0)?, wtb: r.get(1)?, fetched_at: r.get(2)? }),
            )
            .optional()
    }

    pub fn put_price(&self, slug: &str, rank: u32, price: StoredPrice) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO market_prices (slug, rank, wts, wtb, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (slug, rank) DO UPDATE SET wts = ?3, wtb = ?4, fetched_at = ?5",
            params![slug, rank, price.wts, price.wtb, price.fetched_at],
        )?;
        Ok(())
    }

    /// The most recently seen account, if any.
    pub fn latest_account(&self) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row("SELECT account FROM snapshots ORDER BY id DESC LIMIT 1", [], |r| r.get(0))
            .optional()
    }

    /// The newest changes for an account, newest first.
    pub fn recent_changes(&self, account: &str, limit: usize) -> rusqlite::Result<Vec<ChangeRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.captured_at, c.section, c.item, c.before, c.after
             FROM changes c JOIN snapshots s ON s.id = c.snapshot_id
             WHERE c.account = ?1
             ORDER BY c.snapshot_id DESC, ABS(c.after - c.before) DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![account, limit as i64], |r| {
            Ok(ChangeRow {
                captured_at: r.get(0)?,
                section: r.get(1)?,
                item: r.get(2)?,
                before: r.get(3)?,
                after: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    /// Every recorded change to one item, oldest first: (captured at, before, after).
    pub fn item_history(&self, account: &str, section: &str, item: &str) -> rusqlite::Result<Vec<(i64, i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.captured_at, c.before, c.after
             FROM changes c JOIN snapshots s ON s.id = c.snapshot_id
             WHERE c.account = ?1 AND c.section = ?2 AND c.item = ?3
             ORDER BY c.snapshot_id",
        )?;
        let rows = stmt.query_map(params![account, section, item], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    /// Every change to a section's items whose path contains `part`, oldest first:
    /// (snapshot id, captured at, item, before, after).
    pub fn section_history(
        &self,
        account: &str,
        section: &str,
        part: &str,
    ) -> rusqlite::Result<Vec<(i64, i64, String, i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.captured_at, c.item, c.before, c.after
             FROM changes c JOIN snapshots s ON s.id = c.snapshot_id
             WHERE c.account = ?1 AND c.section = ?2 AND instr(c.item, ?3) > 0
             ORDER BY c.snapshot_id",
        )?;
        let rows = stmt.query_map(params![account, section, part], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        rows.collect()
    }

    /// When the account was first stored.
    pub fn first_capture(&self, account: &str) -> rusqlite::Result<Option<i64>> {
        self.conn.query_row("SELECT MIN(captured_at) FROM snapshots WHERE account = ?1", [account], |r| r.get(0))
    }

    /// When the account was last stored, and how many snapshots it has.
    pub fn snapshot_info(&self, account: &str) -> rusqlite::Result<(Option<i64>, i64)> {
        self.conn.query_row(
            "SELECT MAX(captured_at), COUNT(*) FROM snapshots WHERE account = ?1",
            [account],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::parse;

    fn inv(credits: i64, barrels: Option<i64>) -> (Inventory, Vec<u8>) {
        let barrel = barrels.map_or(String::new(), |n| {
            format!(r#",{{"ItemCount":{n},"ItemType":"/Lotus/Types/Recipes/Weapons/WeaponParts/BratonPrimeBarrel"}}"#)
        });
        let raw = format!(
            r#"{{"Created":{{"$date":{{"$numberLong":"1500000000000"}}}},"RegularCredits":{credits},
                "MiscItems":[{{"ItemCount":5,"ItemType":"/Lotus/Types/Items/MiscItems/Ferrite"}}{barrel}]}}"#
        );
        (parse(raw.as_bytes()).unwrap(), raw.into_bytes())
    }

    #[test]
    fn baseline_then_changes_then_unchanged() {
        let mut store = Store::open_in_memory().unwrap();
        let (a, raw) = inv(100, None);
        assert!(matches!(store.apply(&a, &raw, 1).unwrap(), Outcome::Baseline { .. }));
        assert!(store.recent_changes(&a.account, 10).unwrap().is_empty(), "baseline records no changes");

        let (b, raw) = inv(150, Some(1));
        let Outcome::Changed { changes, .. } = store.apply(&b, &raw, 2).unwrap() else { panic!("expected changes") };
        assert_eq!(changes.len(), 2);

        assert_eq!(store.apply(&b, &raw, 3).unwrap(), Outcome::Unchanged);

        let rows = store.recent_changes(&b.account, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].item, "RegularCredits", "biggest change first");
        assert_eq!((rows[0].before, rows[0].after), (100, 150));
        assert_eq!(store.snapshot_info(&b.account).unwrap(), (Some(2), 2));
        assert_eq!(store.latest_account().unwrap().as_deref(), Some("created-1500000000000"));
    }

    #[test]
    fn item_history_lists_one_items_changes_in_order() {
        let mut store = Store::open_in_memory().unwrap();
        for (at, credits) in [(1, 100), (2, 150), (3, 120)] {
            let (i, raw) = inv(credits, None);
            store.apply(&i, &raw, at).unwrap();
        }
        let (i, _) = inv(0, None);
        let h = store.item_history(&i.account, crate::translate::CURRENCY, "RegularCredits").unwrap();
        assert_eq!(h, [(2, 100, 150), (3, 150, 120)]);
        assert_eq!(store.first_capture(&i.account).unwrap(), Some(1));
    }

    #[test]
    fn older_flatten_version_starts_a_new_baseline() {
        let mut store = Store::open_in_memory().unwrap();
        let (a, raw) = inv(100, None);
        store.apply(&a, &raw, 1).unwrap();
        store.conn.execute("UPDATE snapshots SET version = ?1", [FLATTEN_VERSION - 1]).unwrap();

        let (b, raw) = inv(150, Some(1));
        assert!(matches!(store.apply(&b, &raw, 2).unwrap(), Outcome::Baseline { .. }));
        assert!(store.recent_changes(&b.account, 10).unwrap().is_empty());
        assert_eq!(store.current(&b.account).unwrap().len(), 3, "state still updated");
    }

    #[test]
    fn favorites_toggle_per_account() {
        let store = Store::open_in_memory().unwrap();
        store.set_favorite("a", "/Ash", true).unwrap();
        store.set_favorite("a", "/Ash", true).unwrap();
        store.set_favorite("a", "/Braton", true).unwrap();
        store.set_favorite("a", "/Braton", false).unwrap();
        store.set_favorite("b", "/Volt", true).unwrap();
        assert_eq!(store.favorites("a").unwrap(), ["/Ash".to_string()].into());
        assert_eq!(store.favorites("b").unwrap().len(), 1);
    }

    #[test]
    fn account_names_per_account() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.account_name("a").unwrap(), None);
        store.set_account_name("a", "Tenno1").unwrap();
        store.set_account_name("b", "Tenno2").unwrap();
        store.set_account_name("a", "Renamed").unwrap();
        assert_eq!(store.account_name("a").unwrap().as_deref(), Some("Renamed"));
        assert_eq!(store.account_name("b").unwrap().as_deref(), Some("Tenno2"));
    }

    #[test]
    fn market_prices_round_trip() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.price("serration", 0).unwrap(), None);
        let p = StoredPrice { wts: Some(3), wtb: None, fetched_at: 100 };
        store.put_price("serration", 0, p).unwrap();
        assert_eq!(store.price("serration", 0).unwrap(), Some(p));
        let newer = StoredPrice { wts: Some(4), wtb: Some(2), fetched_at: 200 };
        store.put_price("serration", 0, newer).unwrap();
        assert_eq!(store.price("serration", 0).unwrap(), Some(newer), "updated in place");
        assert_eq!(store.price("serration", 10).unwrap(), None, "ranks are separate");
    }

    #[test]
    fn adds_version_column_to_old_databases() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE snapshots (id INTEGER PRIMARY KEY, account TEXT NOT NULL, captured_at INTEGER NOT NULL,
             size INTEGER NOT NULL, changes INTEGER NOT NULL, raw BLOB);",
        )
        .unwrap();
        let store = Store::init(conn).unwrap();
        let has: bool = store
            .conn
            .prepare("SELECT 1 FROM pragma_table_info('snapshots') WHERE name = 'version'")
            .unwrap()
            .exists([])
            .unwrap();
        assert!(has);
    }

    #[test]
    fn accounts_are_separate() {
        let mut store = Store::open_in_memory().unwrap();
        let (a, raw) = inv(100, None);
        store.apply(&a, &raw, 1).unwrap();
        let other_raw = br#"{"Created":{"$date":{"$numberLong":"1600000000000"}},"RegularCredits":7}"#;
        let other = parse(other_raw).unwrap();
        assert!(matches!(store.apply(&other, other_raw, 2).unwrap(), Outcome::Baseline { .. }));
        assert_eq!(store.current(&a.account).unwrap().len(), 2);
        assert_eq!(store.current(&other.account).unwrap().len(), 1);
    }
}

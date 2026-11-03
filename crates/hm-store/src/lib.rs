//! The Learn facts store: one SQLite file per worktree (Learn spec §5.5).

use std::fmt;
use std::path::Path;

use rusqlite::{Connection, Transaction};

/// The schema version this build writes and understands.
pub const FACTS_SCHEMA: u32 = 2;

/// Migrations in order; each runs once, inside a transaction, when `user_version` is
/// below its version.
const MIGRATIONS: &[(u32, &str)] = &[
    (1, include_str!("schema/facts_v1.sql")),
    (2, include_str!("schema/facts_v2.sql")),
];

#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    /// The file was written by a newer Harbormap; it is never opened or changed.
    NewerSchema {
        found: u32,
        supported: u32,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Sqlite(err) => write!(f, "store database error: {err}"),
            StoreError::Io(err) => write!(f, "store file error: {err}"),
            StoreError::NewerSchema { found, supported } => write!(
                f,
                "the store has schema {found}, but this build supports up to {supported}; update Harbormap"
            ),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Sqlite(err) => Some(err),
            StoreError::Io(err) => Some(err),
            StoreError::NewerSchema { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(err: rusqlite::Error) -> Self {
        StoreError::Sqlite(err)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(err: std::io::Error) -> Self {
        StoreError::Io(err)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// An open facts store with its schema current.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (or creates) the store at `path`, creating missing parent folders.
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Store::from_connection(Connection::open(path)?)
    }

    /// An in-memory store, for tests.
    pub fn open_in_memory() -> Result<Store> {
        Store::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Store> {
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        // `journal_mode` returns a row, so it goes through a query. In memory it stays
        // `memory`, which is fine.
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<()> {
        let found = self.schema_version()?;
        if found > FACTS_SCHEMA {
            return Err(StoreError::NewerSchema {
                found,
                supported: FACTS_SCHEMA,
            });
        }
        for (version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > found) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
        }
        Ok(())
    }

    /// The schema version recorded in the file (`PRAGMA user_version`).
    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Runs `f` in one transaction: committed on `Ok`, rolled back on `Err`.
    pub fn write<T>(
        &mut self,
        f: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<T>,
    ) -> Result<T> {
        let tx = self.conn.transaction()?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pragma<T: rusqlite::types::FromSql>(store: &Store, name: &str) -> T {
        store
            .conn()
            .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn fresh_store_has_schema_2_and_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("learn").join("w_x").join("facts.sqlite");
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 2);
        assert_eq!(pragma::<String>(&store, "journal_mode"), "wal");
        assert_eq!(pragma::<i64>(&store, "foreign_keys"), 1);
        drop(store);

        let again = Store::open(&path).unwrap();
        assert_eq!(again.schema_version().unwrap(), FACTS_SCHEMA);
    }

    #[test]
    fn a_newer_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("facts.sqlite");
        rusqlite::Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", 99)
            .unwrap();
        match Store::open(&path) {
            Err(StoreError::NewerSchema { found, supported }) => {
                assert_eq!((found, supported), (99, 2));
            }
            other => panic!("expected NewerSchema, got {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn a_v1_store_migrates_and_keeps_its_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("facts.sqlite");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(include_str!("schema/facts_v1.sql"))
                .unwrap();
            conn.execute(
                "INSERT INTO file(id, path, lang, class, size, mtime_ns, loc, content_hash, parse_status, facts_json)
                 VALUES ('f_a', 'a.ts', 'ts', 'source', 1, 1, 1, 'h', 'parsed', '{}')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO edge(kind, src_id, dst_id, file_id, line, rule, prov, conf)
                 VALUES ('calls', 'y_a', 'y_b', 'f_a', 1, 'r', 'parsed', 'sure')",
                [],
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), 2);
        let edges: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM edge WHERE src_id = 'y_a' AND prov = 'parsed'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(edges, 1);
        let facts_null: bool = store
            .conn()
            .query_row(
                "SELECT facts_json IS NULL FROM file WHERE id = 'f_a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(facts_null);
        store
            .conn()
            .execute(
                "INSERT INTO edge(kind, src_id, dst_id, file_id, line, rule, prov, conf)
                 VALUES ('calls', 'y_a', 'y_b', 'f_a', 1, 'r', 'inferred', 'probably')",
                [],
            )
            .unwrap();
    }

    #[test]
    fn write_rolls_back_on_error() {
        let mut store = Store::open_in_memory().unwrap();
        let result: Result<()> = store.write(|tx| {
            tx.execute("INSERT INTO meta(key, value) VALUES ('k', 'v')", [])?;
            Err(rusqlite::Error::QueryReturnedNoRows)
        });
        assert!(matches!(result, Err(StoreError::Sqlite(_))));
        let rows: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM meta", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0);

        store
            .write(|tx| tx.execute("INSERT INTO meta(key, value) VALUES ('k', 'v')", []))
            .unwrap();
        let rows: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM meta", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn deleting_a_file_cascades() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .write(|tx| {
                tx.execute(
                    "INSERT INTO file(id, path, lang, class, size, mtime_ns, loc, content_hash, parse_status)
                     VALUES ('f_a', 'a.ts', 'ts', 'source', 1, 1, 1, 'h', 'parsed')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO symbol(id, file_id, kind, name, owner, sig, sig_hash, body_hash, l0, l1, exported, lines, branches)
                     VALUES ('y_a', 'f_a', 'function', 'a', '', 'a()', 's', 'b', 1, 2, 1, 2, 0)",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO edge(kind, src_id, dst_id, file_id, line, rule, prov, conf)
                     VALUES ('calls', 'y_a', 'y_b', 'f_a', 1, 'r', 'parsed', 'sure')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO ref(file_id, kind, text, line, status)
                     VALUES ('f_a', 'import', './x', 1, 'unresolved')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO reverse_import(dst_file_id, src_file_id) VALUES ('f_b', 'f_a')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO route(id, root, entry_kind, method, pattern, handler_id, file_id, line, rule, prov, conf, detail)
                     VALUES ('r_a', '', 'page', 'GET', '/', NULL, 'f_a', 1, 'ts.next.app.page', 'matched', 'sure', NULL)",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO client_call(file_id, src_id, line, method, url, rule)
                     VALUES ('f_a', 'y_a', 1, 'GET', '/x', 'ts.client.fetch')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO manifest(file_id, root, kind, name, lockfile, detail)
                     VALUES ('f_a', '', 'npm', 'shop', NULL, '{}')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO dependency(file_id, section, name, declared, resolved, dev, line)
                     VALUES ('f_a', 'dependencies', 'next', '^14', NULL, 0, 3)",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO pattern(file_id, subject_id, kind, line, rule, conf, term, detail)
                     VALUES ('f_a', 'y_a', 'env_read', 1, 'ts.pattern.env', 'sure', NULL, 'HOME')",
                    [],
                )?;
                tx.execute(
                    "INSERT INTO gap(file_id, subject_id, kind, line) VALUES ('f_a', 'y_a', 'k', 1)",
                    [],
                )
            })
            .unwrap();
        store
            .write(|tx| tx.execute("DELETE FROM file WHERE id = 'f_a'", []))
            .unwrap();
        for table in [
            "file",
            "symbol",
            "edge",
            "ref",
            "reverse_import",
            "route",
            "client_call",
            "manifest",
            "dependency",
            "pattern",
            "gap",
        ] {
            let rows: i64 = store
                .conn()
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(rows, 0, "{table} should be empty");
        }
    }
}

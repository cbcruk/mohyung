use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, Connection, OpenFlags, Transaction};

use crate::types::{BlobStats, FileRecord, FileRecordWithPath, LinkEntry, PackageInfo};

const SCHEMA_VERSION: &str = "3";

const CREATE_TABLES_SQL: &str = "
CREATE TABLE IF NOT EXISTS metadata (
  key TEXT PRIMARY KEY,
  value TEXT
);

CREATE TABLE IF NOT EXISTS packages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  version TEXT NOT NULL,
  path TEXT NOT NULL,
  UNIQUE(name, version, path)
);

CREATE TABLE IF NOT EXISTS blobs (
  hash BLOB PRIMARY KEY,
  content BLOB NOT NULL,
  original_size INTEGER,
  compressed_size INTEGER
);

CREATE TABLE IF NOT EXISTS files (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  package_id INTEGER REFERENCES packages(id),
  relative_path TEXT NOT NULL,
  blob_hash BLOB REFERENCES blobs(hash),
  mode INTEGER,
  mtime INTEGER,
  UNIQUE(package_id, relative_path)
);

CREATE TABLE IF NOT EXISTS links (
  path TEXT PRIMARY KEY,
  target TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS dirs (
  path TEXT PRIMARY KEY
);

CREATE INDEX IF NOT EXISTS idx_files_package ON files(package_id);
CREATE INDEX IF NOT EXISTS idx_files_blob ON files(blob_hash);
";

pub struct Store {
    conn: Connection,
}

pub fn insert_package(tx: &Transaction, pkg: &PackageInfo) -> Result<i64> {
    let mut stmt = tx.prepare_cached(
        "INSERT INTO packages (name, version, path) VALUES (?1, ?2, ?3)
         ON CONFLICT(name, version, path) DO UPDATE SET name = name
         RETURNING id",
    )?;
    let id: i64 = stmt.query_row(params![pkg.name, pkg.version, pkg.path], |row| row.get(0))?;
    Ok(id)
}

pub fn insert_blob(
    tx: &Transaction,
    hash: &[u8],
    content: &[u8],
    original_size: u64,
) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        "INSERT OR IGNORE INTO blobs (hash, content, original_size, compressed_size)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    stmt.execute(params![hash, content, original_size, content.len() as u64])?;
    Ok(())
}

pub fn insert_file(
    tx: &Transaction,
    package_id: i64,
    relative_path: &str,
    blob_hash: &[u8],
    mode: u32,
    mtime: i64,
) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        "INSERT INTO files (package_id, relative_path, blob_hash, mode, mtime)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(package_id, relative_path) DO UPDATE SET
           blob_hash = excluded.blob_hash,
           mode = excluded.mode,
           mtime = excluded.mtime",
    )?;
    stmt.execute(params![package_id, relative_path, blob_hash, mode, mtime])?;
    Ok(())
}

pub fn insert_link(tx: &Transaction, link: &LinkEntry) -> Result<()> {
    let mut stmt =
        tx.prepare_cached("INSERT OR REPLACE INTO links (path, target) VALUES (?1, ?2)")?;
    stmt.execute(params![link.path, link.target])?;
    Ok(())
}

pub fn insert_dir(tx: &Transaction, path: &str) -> Result<()> {
    let mut stmt = tx.prepare_cached("INSERT OR REPLACE INTO dirs (path) VALUES (?1)")?;
    stmt.execute(params![path])?;
    Ok(())
}

impl Store {
    pub fn create(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;

        conn.execute_batch(CREATE_TABLES_SQL)?;

        let store = Store { conn };
        store.set_metadata("schema_version", SCHEMA_VERSION)?;

        Ok(store)
    }

    pub fn finalize(self) -> Result<()> {
        self.conn.pragma_update(None, "journal_mode", "DELETE")?;
        Ok(())
    }

    pub fn open_readonly(db_path: &str) -> Result<Self> {
        let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let store = Store { conn };

        let version = store
            .get_metadata("schema_version")
            .with_context(|| format!("not a mohyung database: {}", db_path))?
            .ok_or_else(|| anyhow!("not a mohyung database: {}", db_path))?;

        if version != SCHEMA_VERSION {
            bail!(
                "unsupported schema version {} (this build supports {}): {}",
                version,
                SCHEMA_VERSION,
                db_path
            );
        }

        Ok(store)
    }

    pub fn set_metadata(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_metadata(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM metadata WHERE key = ?1")?;
        let result = stmt
            .query_row(params![key], |row| row.get::<_, String>(0))
            .ok();
        Ok(result)
    }

    pub fn get_blob(&self, hash: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT content FROM blobs WHERE hash = ?1")?;
        let result = stmt
            .query_row(params![hash], |row| row.get::<_, Vec<u8>>(0))
            .ok();
        Ok(result)
    }

    pub fn get_blob_stats(&self) -> Result<BlobStats> {
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*) as count,
                    COALESCE(SUM(original_size), 0) as original,
                    COALESCE(SUM(compressed_size), 0) as compressed
             FROM blobs",
        )?;
        let stats = stmt.query_row([], |row| {
            Ok(BlobStats {
                total_blobs: row.get::<_, i64>(0)? as usize,
                total_original_size: row.get::<_, i64>(1)? as u64,
                total_compressed_size: row.get::<_, i64>(2)? as u64,
            })
        })?;
        Ok(stats)
    }

    pub fn get_all_files(&self) -> Result<Vec<FileRecordWithPath>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.package_id, f.relative_path, f.blob_hash, f.mode, f.mtime, p.path as package_path
             FROM files f
             JOIN packages p ON f.package_id = p.id
             ORDER BY f.blob_hash",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(FileRecordWithPath {
                record: FileRecord {
                    id: Some(row.get::<_, i64>(0)?),
                    package_id: row.get(1)?,
                    relative_path: row.get(2)?,
                    blob_hash: row.get(3)?,
                    mode: row.get::<_, u32>(4)?,
                    mtime: row.get(5)?,
                },
                package_path: row.get(6)?,
            })
        })?;

        let mut files = Vec::new();
        for row in rows {
            files.push(row?);
        }
        Ok(files)
    }

    pub fn get_all_links(&self) -> Result<Vec<LinkEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, target FROM links ORDER BY path")?;
        let rows = stmt.query_map([], |row| {
            Ok(LinkEntry {
                path: row.get(0)?,
                target: row.get(1)?,
            })
        })?;

        let mut links = Vec::new();
        for row in rows {
            links.push(row?);
        }
        Ok(links)
    }

    pub fn get_all_dirs(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT path FROM dirs ORDER BY path")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

        let mut dirs = Vec::new();
        for row in rows {
            dirs.push(row?);
        }
        Ok(dirs)
    }

    pub fn get_total_file_count(&self) -> Result<usize> {
        let mut stmt = self.conn.prepare("SELECT COUNT(*) FROM files")?;
        let count: i64 = stmt.query_row([], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub fn transaction<F, T>(&mut self, f: F) -> Result<T>
    where
        F: FnOnce(&Transaction) -> Result<T>,
    {
        let tx = self.conn.transaction()?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }
}

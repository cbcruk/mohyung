import Database from 'better-sqlite3'
import type {
  PackageInfo,
  BlobInfo,
  FileRecord,
  MetadataKey,
} from '../types.js'

/** Current schema version */
const SCHEMA_VERSION = '1'

/** Table creation SQL */
const CREATE_TABLES_SQL = `
-- Metadata
CREATE TABLE IF NOT EXISTS metadata (
  key TEXT PRIMARY KEY,
  value TEXT
);

-- Package information
CREATE TABLE IF NOT EXISTS packages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  version TEXT NOT NULL,
  path TEXT NOT NULL,
  UNIQUE(name, version, path)
);

-- Content-addressable blob storage
CREATE TABLE IF NOT EXISTS blobs (
  hash TEXT PRIMARY KEY,
  content BLOB NOT NULL,
  original_size INTEGER,
  compressed_size INTEGER
);

-- Files per package
CREATE TABLE IF NOT EXISTS files (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  package_id INTEGER REFERENCES packages(id),
  relative_path TEXT NOT NULL,
  blob_hash TEXT REFERENCES blobs(hash),
  mode INTEGER,
  mtime INTEGER,
  UNIQUE(package_id, relative_path)
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_files_package ON files(package_id);
CREATE INDEX IF NOT EXISTS idx_files_blob ON files(blob_hash);
`

/**
 * SQLite database store
 */
export class Store {
  /** better-sqlite3 database instance */
  private db: Database.Database

  /**
   * Create Store instance
   * @param dbPath - SQLite database file path
   */
  constructor(dbPath: string) {
    this.db = new Database(dbPath)
    this.db.pragma('journal_mode = WAL')
    this.db.pragma('synchronous = NORMAL')

    this.initSchema()
  }

  /** Initialize schema (create tables and set version) */
  private initSchema(): void {
    this.db.exec(CREATE_TABLES_SQL)

    this.setMetadata('schema_version', SCHEMA_VERSION)
  }

  /**
   * Save metadata
   * @param key - Metadata key
   * @param value - Value to save
   */
  setMetadata(key: MetadataKey, value: string): void {
    const stmt = this.db.prepare(`
      INSERT OR REPLACE INTO metadata (key, value) VALUES (?, ?)
    `)

    stmt.run(key, value)
  }

  /**
   * Get metadata
   * @param key - Metadata key
   * @returns Stored value or null
   */
  getMetadata(key: MetadataKey): string | null {
    const stmt = this.db.prepare(`SELECT value FROM metadata WHERE key = ?`)
    const row = stmt.get(key) as { value: string } | undefined

    return row?.value ?? null
  }

  /**
   * Insert package (returns ID only if already exists)
   * @param pkg - Package information
   * @returns Inserted package ID
   */
  insertPackage(pkg: PackageInfo): number {
    const stmt = this.db.prepare(`
      INSERT INTO packages (name, version, path) VALUES (?, ?, ?)
      ON CONFLICT(name, version, path) DO UPDATE SET name = name
      RETURNING id
    `)
    const result = stmt.get(pkg.name, pkg.version, pkg.path) as { id: number }

    return result.id
  }

  /**
   * Get package by ID
   * @param id - Package ID
   * @returns Package info or null
   */
  getPackageById(id: number): PackageInfo | null {
    const stmt = this.db.prepare(`SELECT * FROM packages WHERE id = ?`)
    const row = stmt.get(id) as
      | { id: number; name: string; version: string; path: string }
      | undefined

    if (!row) return null

    return {
      id: row.id,
      name: row.name,
      version: row.version,
      path: row.path,
    }
  }

  /**
   * Get all packages
   * @returns All package list
   */
  getAllPackages(): PackageInfo[] {
    const stmt = this.db.prepare(`SELECT * FROM packages`)
    const rows = stmt.all() as Array<{
      id: number
      name: string
      version: string
      path: string
    }>

    return rows.map((row) => ({
      id: row.id,
      name: row.name,
      version: row.version,
      path: row.path,
    }))
  }

  /**
   * Check if blob exists
   * @param hash - Blob hash
   * @returns Whether blob exists
   */
  hasBlob(hash: string): boolean {
    const stmt = this.db.prepare(`SELECT 1 FROM blobs WHERE hash = ?`)

    return stmt.get(hash) !== undefined
  }

  /**
   * Insert blob (ignore duplicates)
   * @param blob - Blob information
   */
  insertBlob(blob: BlobInfo): void {
    const stmt = this.db.prepare(`
      INSERT OR IGNORE INTO blobs (hash, content, original_size, compressed_size)
      VALUES (?, ?, ?, ?)
    `)

    stmt.run(blob.hash, blob.content, blob.originalSize, blob.compressedSize)
  }

  /**
   * Get blob content by hash
   * @param hash - Blob hash
   * @returns Compressed blob content or null
   */
  getBlob(hash: string): Buffer | null {
    const stmt = this.db.prepare(`SELECT content FROM blobs WHERE hash = ?`)
    const row = stmt.get(hash) as { content: Buffer } | undefined

    return row?.content ?? null
  }

  /**
   * Get blob storage statistics
   * @returns Blob count and size statistics
   */
  getBlobStats(): {
    totalBlobs: number
    totalOriginalSize: number
    totalCompressedSize: number
  } {
    const stmt = this.db.prepare(`
      SELECT
        COUNT(*) as count,
        COALESCE(SUM(original_size), 0) as original,
        COALESCE(SUM(compressed_size), 0) as compressed
      FROM blobs
    `)
    const row = stmt.get() as {
      count: number
      original: number
      compressed: number
    }

    return {
      totalBlobs: row.count,
      totalOriginalSize: row.original,
      totalCompressedSize: row.compressed,
    }
  }

  /**
   * Insert file record (update on conflict)
   * @param file - File record information
   */
  insertFile(file: FileRecord): void {
    const stmt = this.db.prepare(`
      INSERT INTO files (package_id, relative_path, blob_hash, mode, mtime)
      VALUES (?, ?, ?, ?, ?)
      ON CONFLICT(package_id, relative_path) DO UPDATE SET
        blob_hash = excluded.blob_hash,
        mode = excluded.mode,
        mtime = excluded.mtime
    `)

    stmt.run(
      file.packageId,
      file.relativePath,
      file.blobHash,
      file.mode,
      file.mtime
    )
  }

  /**
   * Get files by package ID
   * @param packageId - Package ID
   * @returns File records for the package
   */
  getFilesByPackage(packageId: number): FileRecord[] {
    const stmt = this.db.prepare(`
      SELECT id, package_id, relative_path, blob_hash, mode, mtime
      FROM files WHERE package_id = ?
    `)
    const rows = stmt.all(packageId) as Array<{
      id: number
      package_id: number
      relative_path: string
      blob_hash: string
      mode: number
      mtime: number
    }>

    return rows.map((row) => ({
      id: row.id,
      packageId: row.package_id,
      relativePath: row.relative_path,
      blobHash: row.blob_hash,
      mode: row.mode,
      mtime: row.mtime,
    }))
  }

  /**
   * Get all files (with package path)
   * @returns All file records
   */
  getAllFiles(): Array<FileRecord & { packagePath: string }> {
    const stmt = this.db.prepare(`
      SELECT f.id, f.package_id, f.relative_path, f.blob_hash, f.mode, f.mtime, p.path as package_path
      FROM files f
      JOIN packages p ON f.package_id = p.id
    `)
    const rows = stmt.all() as Array<{
      id: number
      package_id: number
      relative_path: string
      blob_hash: string
      mode: number
      mtime: number
      package_path: string
    }>

    return rows.map((row) => ({
      id: row.id,
      packageId: row.package_id,
      relativePath: row.relative_path,
      blobHash: row.blob_hash,
      mode: row.mode,
      mtime: row.mtime,
      packagePath: row.package_path,
    }))
  }

  /**
   * Get total file count
   * @returns Total number of files
   */
  getTotalFileCount(): number {
    const stmt = this.db.prepare(`SELECT COUNT(*) as count FROM files`)
    const row = stmt.get() as { count: number }

    return row.count
  }

  /**
   * Execute work within transaction
   * @param fn - Function to execute within transaction
   * @returns Function execution result
   */
  transaction<T>(fn: () => T): T {
    return this.db.transaction(fn)()
  }

  /**
   * Get prepared statement for bulk file insertion
   * @returns Prepared statement for file insertion
   */
  prepareInsertFile(): Database.Statement {
    return this.db.prepare(`
      INSERT INTO files (package_id, relative_path, blob_hash, mode, mtime)
      VALUES (?, ?, ?, ?, ?)
      ON CONFLICT(package_id, relative_path) DO UPDATE SET
        blob_hash = excluded.blob_hash,
        mode = excluded.mode,
        mtime = excluded.mtime
    `)
  }

  /**
   * Get prepared statement for bulk blob insertion
   * @returns Prepared statement for blob insertion
   */
  prepareInsertBlob(): Database.Statement {
    return this.db.prepare(`
      INSERT OR IGNORE INTO blobs (hash, content, original_size, compressed_size)
      VALUES (?, ?, ?, ?)
    `)
  }

  /** Close database connection */
  close(): void {
    this.db.close()
  }
}

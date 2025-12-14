/**
 * File system entry information
 */
export interface FileEntry {
  /** Relative path from node_modules */
  relativePath: string
  /** Absolute path */
  absolutePath: string
  /** File permission (e.g., 0o755) */
  mode: number
  /** File size (bytes) */
  size: number
  /** Modification time (Unix timestamp ms) */
  mtime: number
}

/**
 * Package information (packages table record)
 */
export interface PackageInfo {
  id?: number
  /** Package name (e.g., lodash) */
  name: string
  /** Version (e.g., 4.17.21) */
  version: string
  /** Path within node_modules (e.g., node_modules/lodash) */
  path: string
}

/**
 * Blob information (blobs table record)
 */
export interface BlobInfo {
  /** SHA256 hash */
  hash: string
  /** gzip compressed content */
  content: Buffer
  /** Original size */
  originalSize: number
  /** Compressed size */
  compressedSize: number
}

/**
 * File record (files table record)
 */
export interface FileRecord {
  id?: number
  packageId: number
  /** Relative path within package */
  relativePath: string
  /** Blob hash */
  blobHash: string
  /** File permission */
  mode: number
  /** Modification time */
  mtime: number
}

/**
 * Pack options
 */
export interface PackOptions {
  /** Output file path */
  output: string
  /** node_modules path */
  source: string
  /** Compression level (1-9) */
  compressionLevel: number
  /** Include lockfile */
  includeLockfile: boolean
}

/**
 * Unpack options
 */
export interface UnpackOptions {
  /** Input DB file path */
  input: string
  /** Output directory */
  output: string
  /** Cache directory */
  cache?: string
  /** Overwrite existing node_modules */
  force: boolean
}

/**
 * Status result
 */
export interface StatusResult {
  /** Files only in DB */
  onlyInDb: string[]
  /** Files only in filesystem */
  onlyInFs: string[]
  /** Files with different content */
  modified: string[]
  /** Number of identical files */
  unchanged: number
}

/**
 * Metadata keys
 */
export type MetadataKey =
  | 'schema_version'
  | 'created_at'
  | 'node_version'
  | 'lockfile_hash'
  | 'source_path'

/**
 * Progress callback
 */
export type ProgressCallback = (
  current: number,
  total: number,
  message?: string
) => void

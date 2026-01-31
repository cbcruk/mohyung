#![allow(dead_code)]

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub mode: u32,
    pub size: u64,
    pub mtime: i64,
}

#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub id: Option<i64>,
    pub name: String,
    pub version: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct BlobInfo {
    pub hash: String,
    pub content: Vec<u8>,
    pub original_size: u64,
    pub compressed_size: u64,
}

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub id: Option<i64>,
    pub package_id: i64,
    pub relative_path: String,
    pub blob_hash: String,
    pub mode: u32,
    pub mtime: i64,
}

#[derive(Debug, Clone)]
pub struct FileRecordWithPath {
    pub record: FileRecord,
    pub package_path: String,
}

#[derive(Debug, Clone)]
pub struct PackOptions {
    pub output: String,
    pub source: String,
    pub compression_level: u32,
    pub include_lockfile: bool,
}

#[derive(Debug, Clone)]
pub struct UnpackOptions {
    pub input: String,
    pub output: String,
    pub force: bool,
}

#[derive(Debug, Clone, Default)]
pub struct StatusResult {
    pub only_in_db: Vec<String>,
    pub only_in_fs: Vec<String>,
    pub modified: Vec<String>,
    pub unchanged: usize,
}

#[derive(Debug, Clone)]
pub struct BlobStats {
    pub total_blobs: usize,
    pub total_original_size: u64,
    pub total_compressed_size: u64,
}

pub type ProgressCallback = Box<dyn Fn(usize, usize, Option<&str>) + Send + Sync>;

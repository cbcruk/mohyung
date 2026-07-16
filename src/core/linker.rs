//! Prototype `--link` restore path.
//!
//! Instead of writing every file byte-for-byte (see `extractor::extract_files_parallel`),
//! this decompresses each distinct blob **once** into a machine-local
//! content-addressable store and then links it into the target `node_modules`.
//! Repeated restores of the same snapshot ("warm") skip decompression entirely
//! and become pure link operations — the same idea global-virtual-store package
//! managers (pnpm/Nub/aube) use to make warm installs fast.
//!
//! Link strategy:
//! - hardlink (default) shares the store inode — one `link()` syscall per file,
//!   the fastest option and strongest on-disk dedup. Used for 0644 files; it
//!   treats `node_modules` as immutable (editing a restored file mutates the
//!   store), which is the convention for installed dependencies.
//! - reflink (opt-in, Linux FICLONE / macOS clonefile) gives an independent
//!   copy-on-write inode, so it is safe for any mode and survives edits — but
//!   creates a new inode per file, so it is heavier than hardlink. Used only
//!   when requested (`--reflink`) and the store and output share a CoW
//!   filesystem (Btrfs/XFS/APFS).
//! - copy is the universal fallback (and carries executable bits).

use anyhow::{anyhow, bail, Context, Result};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::core::hasher::to_hex;
use crate::core::store::Store;
use crate::types::ProgressFn;
use crate::utils::compression::decompress;
use crate::utils::fs::is_safe_relative_path;

const BATCH_SIZE: usize = 512;

/// Blobs are materialized into the store with this permission; files needing a
/// different mode (executables) fall back to copy so they never share an inode
/// with a differently-permissioned file.
const STORE_BLOB_MODE: u32 = 0o644;

#[derive(Debug, Default)]
pub struct LinkStats {
    pub reflinked: usize,
    pub hardlinked: usize,
    pub copied: usize,
    /// Blobs decompressed and written to the store during this run (cold).
    pub blobs_materialized: usize,
    /// Blobs already present in the store and reused (warm).
    pub blobs_reused: usize,
}

/// Resolve the machine-local store directory.
///
/// `MOHYUNG_STORE` overrides everything, else `$XDG_CACHE_HOME/mohyung/store`
/// (or `$HOME/.cache/...`) on unix and `%LOCALAPPDATA%\mohyung\store` on Windows.
pub fn resolve_store_dir() -> PathBuf {
    if let Ok(p) = std::env::var("MOHYUNG_STORE") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }

    #[cfg(not(windows))]
    {
        if let Ok(x) = std::env::var("XDG_CACHE_HOME") {
            if !x.is_empty() {
                return PathBuf::from(x).join("mohyung").join("store");
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() {
                return PathBuf::from(home)
                    .join(".cache")
                    .join("mohyung")
                    .join("store");
            }
        }
    }
    #[cfg(windows)]
    {
        if let Ok(la) = std::env::var("LOCALAPPDATA") {
            if !la.is_empty() {
                return PathBuf::from(la).join("mohyung").join("store");
            }
        }
    }

    PathBuf::from(".mohyung-store")
}

fn blob_path(store_dir: &Path, hex: &str) -> PathBuf {
    if hex.len() >= 2 {
        store_dir.join(&hex[0..2]).join(&hex[2..])
    } else {
        store_dir.join(hex)
    }
}

fn desired_perm(mode: u32) -> u32 {
    let p = mode & 0o777;
    if p == 0 {
        STORE_BLOB_MODE
    } else {
        p
    }
}

/// A per-process counter that makes temp filenames unique without a RNG (which
/// is unavailable in this environment) so concurrent materializations don't clash.
static TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Ensure the decompressed blob exists in the store, writing it atomically if
/// missing. Returns whether it was freshly materialized.
fn materialize_blob(store_dir: &Path, hex: &str, bytes: &[u8]) -> Result<bool> {
    let path = blob_path(store_dir, hex);
    if path.exists() {
        return Ok(false);
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("bad store path for {}", hex))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create store dir {}", parent.display()))?;

    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".tmp.{}.{}", std::process::id(), n));
    std::fs::write(&tmp, bytes)
        .with_context(|| format!("failed to write store temp {}", tmp.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(STORE_BLOB_MODE)).ok();
    }

    match std::fs::rename(&tmp, &path) {
        Ok(()) => Ok(true),
        // Lost a race with another writer for the same content — fine, keep theirs.
        Err(_) if path.exists() => {
            std::fs::remove_file(&tmp).ok();
            Ok(false)
        }
        Err(e) => {
            std::fs::remove_file(&tmp).ok();
            Err(e).with_context(|| format!("failed to publish store blob {}", path.display()))
        }
    }
}

enum Placed {
    Reflinked,
    Hardlinked,
    Copied,
}

/// Clone `src` to a new file at `dst` via a copy-on-write reflink, giving `dst`
/// permission `mode`. `dst` must not already exist. Content is shared until
/// either side is written; the two inodes are otherwise independent, so editing
/// a restored file never touches the store blob and any `mode` is safe.
#[cfg(target_os = "linux")]
fn reflink(src: &Path, dst: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;

    // FICLONE = _IOW(0x94, 9, int)
    const FICLONE: libc::c_ulong = 0x4004_9409;

    let src_f = std::fs::File::open(src)?;
    // Create with the final mode so no extra chmod syscall is needed per file.
    let dst_f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(dst)?;
    let ret = unsafe { libc::ioctl(dst_f.as_raw_fd(), FICLONE, src_f.as_raw_fd()) };
    if ret == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    drop(dst_f);
    let _ = std::fs::remove_file(dst);
    Err(err)
}

#[cfg(target_os = "macos")]
fn reflink(src: &Path, dst: &Path, mode: u32) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let src_c = CString::new(src.as_os_str().as_bytes())?;
    let dst_c = CString::new(dst.as_os_str().as_bytes())?;
    // clonefile clones metadata too, so dst inherits the store blob's mode.
    let ret = unsafe { libc::clonefile(src_c.as_ptr(), dst_c.as_ptr(), 0) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if mode != STORE_BLOB_MODE {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dst, std::fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn reflink(_src: &Path, _dst: &Path, _mode: u32) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "reflink not supported on this platform",
    ))
}

/// Probe once whether blobs can be reflinked from the store into the output
/// (same filesystem + copy-on-write support). Avoids a failed reflink syscall
/// per file on filesystems like ext4 that don't support it.
fn reflink_supported(store_dir: &Path, output_path: &Path) -> bool {
    if std::fs::create_dir_all(store_dir).is_err() || std::fs::create_dir_all(output_path).is_err()
    {
        return false;
    }
    let src = store_dir.join(".mohyung-reflink-probe.src");
    let dst = output_path.join(".mohyung-reflink-probe.dst");
    let _ = std::fs::remove_file(&dst);
    if std::fs::write(&src, b"probe").is_err() {
        return false;
    }
    let ok = reflink(&src, &dst, STORE_BLOB_MODE).is_ok();
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
    ok
}

/// Place a store blob at `dest`. On a CoW filesystem reflink (safe for any mode);
/// otherwise hardlink when the permission matches the store inode (0644), else
/// copy so we never mutate the shared inode's mode.
fn place(store_path: &Path, dest: &Path, perm: u32, use_reflink: bool) -> Result<Placed> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    if use_reflink && reflink(store_path, dest, perm).is_ok() {
        return Ok(Placed::Reflinked);
    }

    // Cross-device link or similar falls through to copy.
    if perm == STORE_BLOB_MODE && std::fs::hard_link(store_path, dest).is_ok() {
        return Ok(Placed::Hardlinked);
    }

    std::fs::copy(store_path, dest)
        .with_context(|| format!("failed to copy into {}", dest.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dest, std::fs::Permissions::from_mode(perm))
            .with_context(|| format!("failed to set permissions on {}", dest.display()))?;
    }
    Ok(Placed::Copied)
}

/// Restore all files by linking from the local store. Returns
/// `(total_files, total_logical_size, stats)`.
pub fn extract_files_linked(
    store: &Store,
    output_path: &Path,
    store_dir: &Path,
    reflink_requested: bool,
    on_progress: Option<ProgressFn>,
) -> Result<(usize, u64, LinkStats)> {
    let files = store.get_all_files()?;
    let total_files = files.len();

    // Hardlink is the default (fastest for immutable node_modules); reflink is
    // opt-in and only used when both requested and supported by the filesystem.
    let use_reflink = reflink_requested && reflink_supported(store_dir, output_path);

    let mut total_size: u64 = 0;
    let mut written: usize = 0;
    let materialized = AtomicUsize::new(0);
    let reused = AtomicUsize::new(0);
    let reflinked = AtomicUsize::new(0);
    let hardlinked = AtomicUsize::new(0);
    let copied = AtomicUsize::new(0);

    for chunk in files.chunks(BATCH_SIZE) {
        // Reject unsafe paths before touching any data.
        for file in chunk {
            if !is_safe_relative_path(&file.package_path)
                || !is_safe_relative_path(&file.record.relative_path)
            {
                bail!(
                    "unsafe path in database: {}/{}",
                    file.package_path,
                    file.record.relative_path
                );
            }
        }

        // Distinct blobs in this chunk, split into those already in the store
        // (warm — skip fetch+decompress) and those we must materialize (cold).
        let mut seen: HashSet<&[u8]> = HashSet::new();
        let mut missing: Vec<(&[u8], String)> = Vec::new();
        for file in chunk {
            let hash = file.record.blob_hash.as_slice();
            if seen.insert(hash) {
                let hex = to_hex(hash);
                if !blob_path(store_dir, &hex).exists() {
                    missing.push((hash, hex));
                } else {
                    reused.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        // Fetch missing compressed blobs (serial DB), then decompress + write to
        // the store in parallel.
        let fetched: Vec<(String, Vec<u8>)> = missing
            .iter()
            .map(|(hash, hex)| {
                let data = store
                    .get_blob(hash)?
                    .ok_or_else(|| anyhow!("blob {} missing in database", hex))?;
                Ok((hex.clone(), data))
            })
            .collect::<Result<_>>()?;

        fetched
            .par_iter()
            .map(|(hex, compressed)| -> Result<()> {
                let bytes = decompress(compressed)?;
                if materialize_blob(store_dir, hex, &bytes)? {
                    materialized.fetch_add(1, Ordering::Relaxed);
                } else {
                    reused.fetch_add(1, Ordering::Relaxed);
                }
                Ok(())
            })
            .collect::<Result<Vec<_>>>()?;

        // Size accounting: one stat per distinct blob, reused across its files.
        let mut size_by_hash: std::collections::HashMap<&[u8], u64> =
            std::collections::HashMap::new();
        for file in chunk {
            let hash = file.record.blob_hash.as_slice();
            if let std::collections::hash_map::Entry::Vacant(e) = size_by_hash.entry(hash) {
                let hex = to_hex(hash);
                let len = std::fs::metadata(blob_path(store_dir, &hex))
                    .map(|m| m.len())
                    .unwrap_or(0);
                e.insert(len);
            }
        }
        total_size += chunk
            .iter()
            .map(|f| size_by_hash[f.record.blob_hash.as_slice()])
            .sum::<u64>();

        // Link every file from the store (I/O bound → parallel).
        chunk
            .par_iter()
            .map(|file| -> Result<()> {
                let hex = to_hex(&file.record.blob_hash);
                let src = blob_path(store_dir, &hex);
                let dest = output_path
                    .join(&file.package_path)
                    .join(&file.record.relative_path);
                match place(&src, &dest, desired_perm(file.record.mode), use_reflink)? {
                    Placed::Reflinked => reflinked.fetch_add(1, Ordering::Relaxed),
                    Placed::Hardlinked => hardlinked.fetch_add(1, Ordering::Relaxed),
                    Placed::Copied => copied.fetch_add(1, Ordering::Relaxed),
                };
                Ok(())
            })
            .collect::<Result<Vec<_>>>()?;

        written += chunk.len();
        if let Some(progress) = on_progress {
            progress(written, total_files, "Linking files...");
        }
    }

    Ok((
        total_files,
        total_size,
        LinkStats {
            reflinked: reflinked.into_inner(),
            hardlinked: hardlinked.into_inner(),
            copied: copied.into_inner(),
            blobs_materialized: materialized.into_inner(),
            blobs_reused: reused.into_inner(),
        },
    ))
}

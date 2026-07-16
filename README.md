# mohyung

Snapshot and restore node_modules as a single SQLite file.

## Why?

- **Single file backup**: Manage as one `.db` file instead of tens of thousands of files
- **Fast restoration**: Quick node_modules restoration with compression + deduplication
- **Version control friendly**: SQLite format enables binary diff
- **Content-addressable**: Identical files are stored only once (deduplication)
- **Parallel processing**: Multi-core scanning, hashing, and compression via rayon

> SQLite can be [35% faster than filesystem](https://www.sqlite.org/fasterthanfs.html) for handling many small files due to reduced system call overhead. ([HN discussion](https://news.ycombinator.com/item?id=41085376))

## Installation

```bash
# From GitHub Releases
# Download the binary for your platform from:
# https://github.com/cbcruk/mohyung/releases

# Via cargo
cargo install mohyung

# Via npm (downloads pre-built binary)
npm install -g mohyung
```

## Usage

### pack - Snapshot node_modules into DB

```bash
mohyung pack [options]

Options:
  -s, --source <path>       node_modules path (default: "./node_modules")
  -o, --output <path>       output DB file path (default: "./node_modules.db")
  -c, --compression <level> compression level 1-9 (default: "6")
  --include-lockfile        record lockfile hash (package-lock.json, pnpm-lock.yaml, or yarn.lock)
```

**Examples:**

```bash
# Basic usage
mohyung pack

# Custom paths
mohyung pack -s ./my-project/node_modules -o ./backup.db

# Maximum compression
mohyung pack -c 9
```

### unpack - Restore node_modules from DB

```bash
mohyung unpack [options]

Options:
  -i, --input <path>   input DB file path (default: "./node_modules.db")
  -o, --output <path>  output directory (default: "./node_modules")
  -f, --force          overwrite existing node_modules
  -l, --link           link from a local store instead of writing each file
  --reflink            prefer copy-on-write reflinks (implies --link)
```

**Examples:**

```bash
# Basic restoration
mohyung unpack

# Force overwrite
mohyung unpack -f

# Restore to different location
mohyung unpack -o ./restored_modules

# Fast restore by linking from a local content-addressable store
mohyung unpack --link
```

#### Link restore (`--link`)

With `--link`, each distinct blob is decompressed **once** into a machine-local
store (`$MOHYUNG_STORE`, else `$XDG_CACHE_HOME/mohyung/store`) and hardlinked into
`node_modules` instead of being written byte-for-byte. Repeated ("warm") restores
of the same snapshot skip decompression entirely and become pure link operations
— roughly 3–4× faster in benchmarks — and identical files share a single inode on
disk.

- **`--link`** (default): hardlink. Fastest, strongest dedup. Treats
  `node_modules` as immutable — editing a restored file also changes the shared
  store blob. Executable files fall back to copy.
- **`--reflink`**: prefer copy-on-write reflinks (Btrfs/XFS/APFS) so restored
  files are independent and safe to edit. Falls back to hardlink/copy where the
  filesystem has no CoW support.

### status - Compare DB with current state

```bash
mohyung status [options]

Options:
  --db <path>               DB file path (default: "./node_modules.db")
  -n, --node-modules <path> node_modules path (default: "./node_modules")
```

**Examples:**

```bash
mohyung status
```

**Output:**

```
┌─ Status ─────────────────────────────┐
│ Unchanged: 12,345                    │
│ Modified: 3                          │
│ Only in DB: 1                        │
│                                      │
│ Modified files:                      │
│   M lodash/index.js                  │
│   M express/lib/router.js            │
└──────────────────────────────────────┘
```

## DB Schema

```
┌─────────────────────────────────────────────────────────────┐
│                        SQLite DB                            │
├─────────────┬───────────────────────────────────────────────┤
│  metadata   │ created_at, source_path, schema_version, ...  │
├─────────────┼───────────────────────────────────────────────┤
│  packages   │ id, name, version, path                       │
├─────────────┼───────────────────────────────────────────────┤
│  blobs      │ hash (PK), content (compressed), sizes        │
├─────────────┼───────────────────────────────────────────────┤
│  files      │ package_id, relative_path, blob_hash, mode    │
├─────────────┼───────────────────────────────────────────────┤
│  links      │ path, target (symlinks)                       │
└─────────────┴───────────────────────────────────────────────┘
```

**Content-addressable Storage:**

- Uses SHA-256 hash of file content as key
- Identical files are stored only once
- zstd compression for storage efficiency

## Requirements

- Supports npm, yarn, and pnpm directory structures
- Cross-platform: macOS (arm64/x64), Linux (x64/arm64), Windows (x64)

## Development

```bash
# Build
cargo build

# Build (release)
cargo build --release

# Test
cargo test

# Run
cargo run -- pack
cargo run -- unpack -f
cargo run -- status
```

## License

MIT

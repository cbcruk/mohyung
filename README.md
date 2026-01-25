# mohyung

Snapshot and restore node_modules as a single SQLite file.

## Why?

- **Single file backup**: Manage as one `.db` file instead of tens of thousands of files
- **Fast restoration**: Quick node_modules restoration with compression + deduplication
- **Version control friendly**: SQLite format enables binary diff
- **Content-addressable**: Identical files are stored only once (deduplication)

> 📖 SQLite can be [35% faster than filesystem](https://www.sqlite.org/fasterthanfs.html) for handling many small files due to reduced system call overhead. ([HN discussion](https://news.ycombinator.com/item?id=41085376))

## Installation

```bash
npm install -g mohyung
# or
pnpm add -g mohyung
```

## Usage

### pack - Snapshot node_modules into DB

```bash
mohyung pack [options]

Options:
  -s, --source <path>       node_modules path (default: "./node_modules")
  -o, --output <path>       output DB file path (default: "./node_modules.db")
  -c, --compression <level> compression level 1-9 (default: "6")
  --include-lockfile        include package-lock.json hash
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
```

**Examples:**

```bash
# Basic restoration
mohyung unpack

# Force overwrite
mohyung unpack -f

# Restore to different location
mohyung unpack -o ./restored_modules
```

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
│  metadata   │ created_at, node_version, schema_version, ... │
├─────────────┼───────────────────────────────────────────────┤
│  packages   │ id, name, version, path                       │
├─────────────┼───────────────────────────────────────────────┤
│  blobs      │ hash (PK), content (compressed), sizes        │
├─────────────┼───────────────────────────────────────────────┤
│  files      │ package_id, relative_path, blob_hash, mode    │
└─────────────┴───────────────────────────────────────────────┘
```

**Content-addressable Storage:**

- Uses SHA-256 hash of file content as key
- Identical files are stored only once
- zlib compression for storage efficiency

## Requirements

- Node.js >= 18.0.0
- Supports npm, yarn, and pnpm

## Library Usage

```typescript
import { pack, unpack, status, Store } from 'mohyung'

// Pack
await pack({
  source: './node_modules',
  output: './node_modules.db',
  compressionLevel: 6,
})

// Unpack
await unpack({
  input: './node_modules.db',
  output: './node_modules',
  force: true,
})

// Status
const result = await status({
  db: './node_modules.db',
  nodeModules: './node_modules',
})

console.log(result.unchanged, result.modified, result.onlyInDb)
```

## Development

```bash
# Install dependencies
pnpm install

# Development mode (watch)
pnpm dev

# Build
pnpm build

# Test
pnpm test

# Type check
pnpm typecheck
```

## License

MIT

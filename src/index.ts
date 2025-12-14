export { Store } from './core/store.js'
export { scanNodeModules, countFiles } from './core/scanner.js'
export { extractFiles } from './core/extractor.js'
export { hashBuffer, hashString } from './core/hasher.js'

export { compress, decompress } from './utils/compression.js'
export { createProgressBar, formatBytes } from './utils/progress.js'
export { logger, createLogger, consola } from './utils/logger.js'

export { pack } from './commands/pack.js'
export { unpack } from './commands/unpack.js'
export { status } from './commands/status.js'

export type {
  FileEntry,
  PackageInfo,
  BlobInfo,
  FileRecord,
  PackOptions,
  UnpackOptions,
  StatusResult,
  MetadataKey,
  ProgressCallback,
} from './types.js'

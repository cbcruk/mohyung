import { existsSync } from 'node:fs'
import { rm } from 'node:fs/promises'
import { resolve } from 'node:path'
import { Store } from '../core/store.js'
import { extractFiles } from '../core/extractor.js'
import { createProgressBar, formatBytes } from '../utils/progress.js'
import { logger } from '../utils/logger.js'
import type { UnpackOptions } from '../types.js'

const log = logger.unpack

/**
 * Restore node_modules from SQLite DB
 * @param options - Unpack command options
 */
export async function unpack(options: UnpackOptions): Promise<void> {
  const { input, output, force } = options

  const dbPath = resolve(input)
  const outputPath = resolve(output)

  if (!existsSync(dbPath)) {
    throw new Error(`Database not found: ${dbPath}`)
  }

  if (existsSync(outputPath)) {
    if (!force) {
      throw new Error(
        `Output directory already exists: ${outputPath}. Use --force to overwrite.`
      )
    }

    log.warn(`Removing existing ${outputPath}...`)

    await rm(outputPath, { recursive: true, force: true })
  }

  log.info(`Opening ${dbPath}`)
  const store = new Store(dbPath)

  const createdAt = store.getMetadata('created_at')
  const nodeVersion = store.getMetadata('node_version')
  const totalFileCount = store.getTotalFileCount()
  const blobStats = store.getBlobStats()

  log.box({
    title: 'Database Info',
    message: [
      `Created: ${createdAt ?? 'unknown'}`,
      `Node version: ${nodeVersion ?? 'unknown'}`,
      `Files: ${totalFileCount}`,
      `Original size: ${formatBytes(blobStats.totalOriginalSize)}`,
      `Compressed size: ${formatBytes(blobStats.totalCompressedSize)}`,
    ].join('\n'),
  })

  log.start(`Extracting to ${outputPath}`)
  const progress = createProgressBar(totalFileCount)

  const startTime = Date.now()
  const result = await extractFiles(store, outputPath, progress)
  const elapsed = ((Date.now() - startTime) / 1000).toFixed(1)

  store.close()

  log.box({
    title: 'Unpack Complete',
    message: [
      `Extracted: ${result.totalFiles} files (${formatBytes(
        result.totalSize
      )})`,
      `Time: ${elapsed}s`,
    ].join('\n'),
    style: {
      borderColor: 'green',
    },
  })
}

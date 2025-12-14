import { readFile, stat, rm } from 'node:fs/promises'
import { existsSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { Store } from '../core/store.js'
import { scanNodeModules } from '../core/scanner.js'
import { hashBuffer, hashString } from '../core/hasher.js'
import { compress } from '../utils/compression.js'
import { createProgressBar, formatBytes } from '../utils/progress.js'
import { logger } from '../utils/logger.js'
import type { PackOptions } from '../types.js'

const log = logger.pack

/**
 * Pack node_modules into SQLite DB
 * @param options - Pack command options
 */
export async function pack(options: PackOptions): Promise<void> {
  const { source, output, compressionLevel, includeLockfile } = options

  const nodeModulesPath = resolve(source)
  const dbPath = resolve(output)

  if (!existsSync(nodeModulesPath)) {
    throw new Error(`node_modules not found: ${nodeModulesPath}`)
  }

  log.start(`Scanning ${nodeModulesPath}`)

  const scanProgress = createProgressBar(100)
  const scanResult = await scanNodeModules(
    nodeModulesPath,
    (current, total, msg) => {
      scanProgress(current, total, msg)
    }
  )

  log.success(
    `Found ${scanResult.packages.length} packages, ${
      scanResult.totalFiles
    } files (${formatBytes(scanResult.totalSize)})`
  )

  // @fn cleanupDbFiles - cleanup existing DB and WAL/SHM files
  if (existsSync(dbPath)) {
    await rm(dbPath)
    if (existsSync(dbPath + '-wal')) await rm(dbPath + '-wal')
    if (existsSync(dbPath + '-shm')) await rm(dbPath + '-shm')
  }

  const store = new Store(dbPath)

  store.setMetadata('created_at', new Date().toISOString())
  store.setMetadata('node_version', process.version)
  store.setMetadata('source_path', nodeModulesPath)

  if (includeLockfile) {
    const lockfilePath = join(nodeModulesPath, '..', 'package-lock.json')

    if (existsSync(lockfilePath)) {
      const lockfileContent = await readFile(lockfilePath, 'utf8')
      store.setMetadata('lockfile_hash', hashString(lockfileContent))
    }
  }

  log.start('Packing files...')
  const packProgress = createProgressBar(scanResult.totalFiles)

  const insertBlob = store.prepareInsertBlob()
  const insertFile = store.prepareInsertFile()

  // @fn packFiles - iterate packages and pack files
  const packFiles = () => {
    let processedFiles = 0
    let deduplicatedCount = 0

    for (const pkg of scanResult.packages) {
      const packageId = store.insertPackage({
        name: pkg.name,
        version: pkg.version,
        path: pkg.path,
      })

      for (const file of pkg.files) {
        processedFiles++
        packProgress(
          processedFiles,
          scanResult.totalFiles,
          file.relativePath.slice(0, 40)
        )

        const content = readFileSync(file.absolutePath)
        const hash = hashBuffer(content)

        if (!store.hasBlob(hash)) {
          const compressed = compress(content, compressionLevel)
          insertBlob.run(hash, compressed, content.length, compressed.length)
        } else {
          deduplicatedCount++
        }

        insertFile.run(
          packageId,
          file.relativePath,
          hash,
          file.mode,
          file.mtime
        )
      }
    }

    return { deduplicatedCount }
  }

  const { deduplicatedCount } = store.transaction(packFiles)

  store.close()

  // @fn printPackSummary - print pack result summary
  const dbStats = await stat(dbPath)
  const compressionRatio = (
    (1 - dbStats.size / scanResult.totalSize) *
    100
  ).toFixed(1)

  log.box({
    title: 'Pack Complete',
    message: [
      `Output: ${dbPath}`,
      `Original: ${formatBytes(scanResult.totalSize)}`,
      `DB size: ${formatBytes(dbStats.size)}`,
      `Compression: ${compressionRatio}%`,
      `Deduplicated: ${deduplicatedCount}`,
    ].join('\n'),
    style: {
      borderColor: 'green',
    },
  })
}

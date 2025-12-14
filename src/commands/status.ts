import { existsSync } from 'node:fs'
import { readFile } from 'node:fs/promises'
import { resolve, join } from 'node:path'
import { Store } from '../core/store.js'
import { hashBuffer } from '../core/hasher.js'
import { createProgressBar } from '../utils/progress.js'
import { logger } from '../utils/logger.js'
import type { StatusResult } from '../types.js'

const log = logger.status

/**
 * Status command options
 */
interface StatusOptions {
  /** Database file path */
  db: string
  /** node_modules directory path */
  nodeModules: string
}

/**
 * Compare DB with current node_modules
 * @param options - Status command options
 * @returns Comparison result with modified/missing files
 */
export async function status(options: StatusOptions): Promise<StatusResult> {
  const { db, nodeModules } = options

  const dbPath = resolve(db)
  const nodeModulesPath = resolve(nodeModules)

  if (!existsSync(dbPath)) {
    throw new Error(`Database not found: ${dbPath}`)
  }

  if (!existsSync(nodeModulesPath)) {
    log.warn(`node_modules not found: ${nodeModulesPath}`)
    log.info('Run "nmsnap unpack" to restore from database.')

    return { onlyInDb: [], onlyInFs: [], modified: [], unchanged: 0 }
  }

  log.start('Comparing...')
  log.info(`DB: ${dbPath}`)
  log.info(`node_modules: ${nodeModulesPath}`)

  const store = new Store(dbPath)
  const files = store.getAllFiles()

  const result: StatusResult = {
    onlyInDb: [],
    onlyInFs: [],
    modified: [],
    unchanged: 0,
  }

  const progress = createProgressBar(files.length)
  const dbPaths = new Set<string>()

  for (const [index, file] of files.entries()) {
    const relativePath = join(file.packagePath, file.relativePath)
    const fullPath = join(nodeModulesPath, relativePath)

    dbPaths.add(relativePath)

    progress(index + 1, files.length, relativePath.slice(0, 40))

    if (!existsSync(fullPath)) {
      result.onlyInDb.push(relativePath)
      continue
    }

    // @fn compareFileHash - compare filesystem and DB hashes
    try {
      const fsContent = await readFile(fullPath)
      const fsHash = hashBuffer(fsContent)

      if (fsHash !== file.blobHash) {
        result.modified.push(relativePath)
      } else {
        result.unchanged++
      }
    } catch {
      result.modified.push(relativePath)
    }
  }

  store.close()

  const summaryLines = [
    `Unchanged: ${result.unchanged}`,
    `Modified: ${result.modified.length}`,
    `Only in DB: ${result.onlyInDb.length}`,
  ]

  if (result.modified.length > 0 && result.modified.length <= 10) {
    summaryLines.push('', 'Modified files:')
    result.modified.forEach((f) => summaryLines.push(`  M ${f}`))
  }

  if (result.onlyInDb.length > 0 && result.onlyInDb.length <= 10) {
    summaryLines.push('', 'Only in DB (deleted locally):')
    result.onlyInDb.forEach((f) => summaryLines.push(`  D ${f}`))
  }

  if (result.modified.length > 10 || result.onlyInDb.length > 10) {
    summaryLines.push('', '(Use verbose mode for full list)')
  }

  const isClean = result.modified.length === 0 && result.onlyInDb.length === 0

  log.box({
    title: 'Status',
    message: summaryLines.join('\n'),
    style: {
      borderColor: isClean ? 'green' : 'yellow',
    },
  })

  if (isClean) {
    log.success('All files match!')
  }

  return result
}

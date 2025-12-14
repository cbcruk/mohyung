import { mkdir, writeFile, chmod } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { Store } from './store.js'
import { decompress } from '../utils/compression.js'
import { logger } from '../utils/logger.js'
import type { ProgressCallback } from '../types.js'

const log = logger.unpack

/**
 * Extract files from SQLite DB
 * @param store - Store instance to read from
 * @param outputPath - Output directory path
 * @param onProgress - Optional progress callback
 * @returns Total files and size extracted
 */
export async function extractFiles(
  store: Store,
  outputPath: string,
  onProgress?: ProgressCallback
): Promise<{ totalFiles: number; totalSize: number }> {
  const files = store.getAllFiles()
  const totalFiles = files.length

  // Cache to prevent duplicate blob decompression
  const blobCache = new Map<string, Buffer>()

  // @fn getContent - get blob from cache or decompress
  const getContent = (blobHash: string): Buffer | null => {
    if (blobCache.has(blobHash)) {
      return blobCache.get(blobHash)!
    }

    const compressed = store.getBlob(blobHash)

    if (!compressed) return null

    const content = decompress(compressed)

    // Only cache files under 100KB (memory optimization)
    if (content.length < 100 * 1024) {
      blobCache.set(blobHash, content)
    }

    return content
  }

  // @fn iterateFiles - iterate and extract files
  const iterateFiles = async () => {
    let processedFiles = 0
    let totalSize = 0

    for (const file of files) {
      processedFiles++

      onProgress?.(processedFiles, totalFiles, file.relativePath.slice(0, 40))

      const fullPath = join(outputPath, file.packagePath, file.relativePath)

      await mkdir(dirname(fullPath), { recursive: true })

      const content = getContent(file.blobHash)

      if (!content) {
        log.warn(`Blob not found: ${file.relativePath}`)
        continue
      }

      await writeFile(fullPath, content)

      totalSize += content.length

      if (file.mode) {
        try {
          await chmod(fullPath, file.mode & 0o777)
        } catch {
          // Ignore permission errors (e.g., Windows)
        }
      }
    }

    return { totalSize }
  }

  const { totalSize } = await iterateFiles()

  return {
    totalFiles,
    totalSize,
  }
}

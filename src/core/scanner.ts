import { readdir, stat, readFile } from 'node:fs/promises'
import { join, relative } from 'node:path'
import { existsSync } from 'node:fs'
import type { FileEntry, PackageInfo, ProgressCallback } from '../types.js'

/**
 * Extract package info from package.json
 * @param pkgJsonPath - Path to package.json file
 * @returns Package name and version, or null if parse failed
 */
async function parsePackageJson(
  pkgJsonPath: string
): Promise<{ name: string; version: string } | null> {
  try {
    const content = await readFile(pkgJsonPath, 'utf8')
    const pkg = JSON.parse(content)

    return {
      name: pkg.name || 'unknown',
      version: pkg.version || '0.0.0',
    }
  } catch {
    return null
  }
}

/**
 * Check if pnpm structure
 * @param nodeModulesPath - Path to node_modules directory
 * @returns True if .pnpm directory exists
 */
function isPnpmStructure(nodeModulesPath: string): boolean {
  return existsSync(join(nodeModulesPath, '.pnpm'))
}

/**
 * Recursively scan directory to collect all files
 * @param dir - Directory to scan
 * @param baseDir - Base directory for relative path calculation
 * @yields FileEntry for each file found
 */
async function* walkDir(
  dir: string,
  baseDir: string
): AsyncGenerator<FileEntry> {
  const entries = await readdir(dir, { withFileTypes: true })

  for (const entry of entries) {
    const fullPath = join(dir, entry.name)

    if (entry.isDirectory()) {
      yield* walkDir(fullPath, baseDir)
    } else if (entry.isFile()) {
      const stats = await stat(fullPath)

      yield {
        relativePath: relative(baseDir, fullPath),
        absolutePath: fullPath,
        mode: stats.mode,
        size: stats.size,
        mtime: stats.mtimeMs,
      }
    }
  }
}

/**
 * Find package directories (handles scoped packages)
 * @param nodeModulesPath - Path to node_modules directory
 * @yields Package path and relative path
 */
async function* findPackageDirs(nodeModulesPath: string): AsyncGenerator<{
  path: string
  relativePath: string
}> {
  const entries = await readdir(nodeModulesPath, { withFileTypes: true })

  for (const entry of entries) {
    if (!entry.isDirectory()) continue
    if (
      entry.name === '.bin' ||
      entry.name === '.cache' ||
      entry.name === '.pnpm'
    )
      continue

    const fullPath = join(nodeModulesPath, entry.name)

    if (entry.name.startsWith('@')) {
      // Scoped package (e.g., @types/node)
      const scopedEntries = await readdir(fullPath, { withFileTypes: true })

      for (const scopedEntry of scopedEntries) {
        if (!scopedEntry.isDirectory()) continue

        yield {
          path: join(fullPath, scopedEntry.name),
          relativePath: join(entry.name, scopedEntry.name),
        }
      }
    } else {
      yield {
        path: fullPath,
        relativePath: entry.name,
      }
    }
  }
}

/**
 * Find packages in pnpm's .pnpm directory
 * @param nodeModulesPath - Path to node_modules directory
 * @yields Package path and relative path
 */
async function* findPnpmPackageDirs(nodeModulesPath: string): AsyncGenerator<{
  path: string
  relativePath: string
}> {
  const pnpmPath = join(nodeModulesPath, '.pnpm')
  const entries = await readdir(pnpmPath, { withFileTypes: true })

  for (const entry of entries) {
    if (!entry.isDirectory()) continue
    if (entry.name === 'node_modules' || entry.name.startsWith('.')) continue

    const fullPath = join(pnpmPath, entry.name)

    // pnpm structure: .pnpm/package-name@version/node_modules/package-name
    const innerNodeModules = join(fullPath, 'node_modules')

    if (!existsSync(innerNodeModules)) continue

    const innerEntries = await readdir(innerNodeModules, {
      withFileTypes: true,
    })

    for (const innerEntry of innerEntries) {
      if (!innerEntry.isDirectory()) continue
      if (innerEntry.name === '.bin') continue

      const pkgPath = join(innerNodeModules, innerEntry.name)

      if (innerEntry.name.startsWith('@')) {
        // Scoped package
        const scopedEntries = await readdir(pkgPath, {
          withFileTypes: true,
        })

        for (const scopedEntry of scopedEntries) {
          if (!scopedEntry.isDirectory()) continue

          yield {
            path: join(pkgPath, scopedEntry.name),
            relativePath: `.pnpm/${entry.name}/node_modules/${innerEntry.name}/${scopedEntry.name}`,
          }
        }
      } else {
        yield {
          path: pkgPath,
          relativePath: `.pnpm/${entry.name}/node_modules/${innerEntry.name}`,
        }
      }
    }
  }
}

/**
 * node_modules scan result
 */
export interface ScanResult {
  /** Scanned package list (with file info) */
  packages: Array<PackageInfo & { files: FileEntry[] }>
  /** Total file count */
  totalFiles: number
  /** Total file size (bytes) */
  totalSize: number
}

/**
 * Scan node_modules directory
 * @param nodeModulesPath - Path to node_modules directory
 * @param onProgress - Optional progress callback
 * @returns Scan result with packages and file statistics
 */
export async function scanNodeModules(
  nodeModulesPath: string,
  onProgress?: ProgressCallback
): Promise<ScanResult> {
  // Check if pnpm structure
  const usePnpm = isPnpmStructure(nodeModulesPath)

  // Collect package directories first
  const packageDirs: Array<{ path: string; relativePath: string }> = []

  if (usePnpm) {
    for await (const dir of findPnpmPackageDirs(nodeModulesPath)) {
      packageDirs.push(dir)
    }
  } else {
    for await (const dir of findPackageDirs(nodeModulesPath)) {
      packageDirs.push(dir)
    }
  }

  // @fn scanPackages - iterate packages and scan files
  const scanPackages = async () => {
    const packages: Array<PackageInfo & { files: FileEntry[] }> = []

    let totalFiles = 0
    let totalSize = 0
    let packageCount = 0

    for (const { path: pkgPath, relativePath } of packageDirs) {
      packageCount++

      onProgress?.(packageCount, packageDirs.length, relativePath)

      const pkgJsonPath = join(pkgPath, 'package.json')
      const pkgInfo = await parsePackageJson(pkgJsonPath)

      if (!pkgInfo) continue

      const files: FileEntry[] = []

      for await (const file of walkDir(pkgPath, pkgPath)) {
        files.push(file)
        totalFiles++
        totalSize += file.size
      }

      packages.push({
        name: pkgInfo.name,
        version: pkgInfo.version,
        path: relativePath,
        files,
      })
    }

    return { packages, totalFiles, totalSize }
  }

  return scanPackages()
}

/**
 * Quick file count only
 * @param nodeModulesPath - Path to node_modules directory
 * @returns Total number of files
 */
export async function countFiles(nodeModulesPath: string): Promise<number> {
  const countDir = async (dir: string): Promise<number> => {
    const entries = await readdir(dir, { withFileTypes: true })

    let count = 0

    for (const entry of entries) {
      const fullPath = join(dir, entry.name)

      if (entry.isDirectory()) {
        count += await countDir(fullPath)
      } else if (entry.isFile()) {
        count++
      }
    }

    return count
  }

  return countDir(nodeModulesPath)
}

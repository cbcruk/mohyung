import type { ProgressCallback } from '../types.js'

/**
 * Create a progress bar callback
 * @param total - Expected total count for progress calculation
 * @returns Progress callback function
 */
export function createProgressBar(total: number): ProgressCallback {
  const startTime = Date.now()

  return (current: number, actualTotal: number, message?: string) => {
    // @fn calculateProgress - calculate progress and render bar
    const t = actualTotal || total
    const ratio = t > 0 ? Math.min(current / t, 1) : 0
    const percent = Math.round(ratio * 100)
    const elapsed = ((Date.now() - startTime) / 1000).toFixed(1)
    const barWidth = 30
    const filled = Math.max(0, Math.round(ratio * barWidth))
    const empty = Math.max(0, barWidth - filled)
    const bar = '█'.repeat(filled) + '░'.repeat(empty)

    const line = `\r[${bar}] ${percent}% (${current}/${t}) ${elapsed}s${
      message ? ` - ${message}` : ''
    }`

    process.stdout.write(line)

    if (current >= t) {
      process.stdout.write('\n')
    }
  }
}

/**
 * Format bytes to human-readable string
 * @param bytes - Number of bytes
 * @returns Formatted string (e.g., "1.5 MB")
 */
export function formatBytes(bytes: number): string {
  const units = ['B', 'KB', 'MB', 'GB']

  let size = bytes
  let unitIndex = 0

  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024
    unitIndex++
  }

  return `${size.toFixed(1)} ${units[unitIndex]}`
}

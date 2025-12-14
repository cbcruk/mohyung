import { createHash } from 'node:crypto'

/**
 * Calculate SHA256 hash of a Buffer
 * @param data - Buffer to hash
 * @returns Hexadecimal hash string
 */
export function hashBuffer(data: Buffer): string {
  return createHash('sha256').update(data).digest('hex')
}

/**
 * Calculate SHA256 hash of a string
 * @param data - String to hash
 * @returns Hexadecimal hash string
 */
export function hashString(data: string): string {
  return createHash('sha256').update(data, 'utf8').digest('hex')
}

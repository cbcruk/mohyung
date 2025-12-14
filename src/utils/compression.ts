import { gzipSync, gunzipSync } from 'node:zlib'

/**
 * Compress data using gzip
 * @param data - Data to compress
 * @param level - Compression level (1-9, default: 6)
 * @returns Compressed buffer
 */
export function compress(data: Buffer, level: number = 6): Buffer {
  return gzipSync(data, { level })
}

/**
 * Decompress gzip data
 * @param data - Compressed data
 * @returns Decompressed buffer
 */
export function decompress(data: Buffer): Buffer {
  return gunzipSync(data)
}

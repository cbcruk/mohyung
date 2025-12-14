import { consola, type ConsolaInstance } from 'consola'

/** Default logger instances */
export const logger = {
  pack: consola.withTag('pack'),
  unpack: consola.withTag('unpack'),
  status: consola.withTag('status'),
  scan: consola.withTag('scan'),
}

/**
 * Create a new logger with tag
 * @param tag - Tag to identify log source
 * @returns Consola instance with tag
 */
export function createLogger(tag: string): ConsolaInstance {
  return consola.withTag(tag)
}

/** Re-export consola instance */
export { consola }

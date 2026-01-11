import { Command } from 'commander'
import { pack } from './commands/pack.js'
import { unpack } from './commands/unpack.js'
import { status } from './commands/status.js'
import { consola } from './utils/logger.js'

const program = new Command()

program
  .name('mohyung')
  .description('Snapshot and restore node_modules as a single SQLite file')
  .version('0.1.0')

program
  .command('pack')
  .description('Pack node_modules into SQLite DB')
  .option('-o, --output <path>', 'output file path', './node_modules.db')
  .option('-s, --source <path>', 'node_modules path', './node_modules')
  .option('-c, --compression <level>', 'compression level (1-9)', '6')
  .option('--include-lockfile', 'include package-lock.json', false)
  .action(async (options) => {
    try {
      await pack({
        output: options.output,
        source: options.source,
        compressionLevel: parseInt(options.compression, 10),
        includeLockfile: options.includeLockfile,
      })
    } catch (error) {
      consola.error(error instanceof Error ? error.message : error)
      process.exit(1)
    }
  })

program
  .command('unpack')
  .description('Restore node_modules from SQLite DB')
  .option('-i, --input <path>', 'input DB file path', './node_modules.db')
  .option('-o, --output <path>', 'output directory', './node_modules')
  .option('--cache <path>', 'cache directory')
  .option('-f, --force', 'overwrite existing node_modules', false)
  .action(async (options) => {
    try {
      await unpack({
        input: options.input,
        output: options.output,
        cache: options.cache,
        force: options.force,
      })
    } catch (error) {
      consola.error(error instanceof Error ? error.message : error)
      process.exit(1)
    }
  })

program
  .command('status')
  .description('Compare DB with current node_modules')
  .option('--db <path>', 'DB file path', './node_modules.db')
  .option('-n, --node-modules <path>', 'node_modules path', './node_modules')
  .action(async (options) => {
    try {
      await status({
        db: options.db,
        nodeModules: options.nodeModules,
      })
    } catch (error) {
      consola.error(error instanceof Error ? error.message : error)
      process.exit(1)
    }
  })

program.parse()

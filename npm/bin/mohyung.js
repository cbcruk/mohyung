#!/usr/bin/env node
const { spawnSync } = require("child_process");
const { join } = require("path");

const binName = process.platform === "win32" ? "mohyung.exe" : "mohyung";
const binPath = join(__dirname, binName);

const result = spawnSync(binPath, process.argv.slice(2), { stdio: "inherit" });

if (result.error) {
  console.error(`Failed to run mohyung: ${result.error.message}`);
  console.error("Try reinstalling: npm install -g mohyung");
  process.exit(1);
}

if (result.signal) {
  process.kill(process.pid, result.signal);
}

process.exit(result.status === null ? 1 : result.status);

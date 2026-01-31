const { execSync } = require("child_process");
const { existsSync, mkdirSync, chmodSync, createWriteStream } = require("fs");
const { join } = require("path");
const https = require("https");

const REPO = "cbcruk/mohyung";
const BIN_DIR = join(__dirname, "..", "bin");
const BIN_NAME = process.platform === "win32" ? "mohyung.exe" : "mohyung";
const BIN_PATH = join(BIN_DIR, BIN_NAME);

function getPlatformTarget() {
  const platform = process.platform;
  const arch = process.arch;

  const targets = {
    "darwin-arm64": "aarch64-apple-darwin",
    "darwin-x64": "x86_64-apple-darwin",
    "linux-x64": "x86_64-unknown-linux-gnu",
    "linux-arm64": "aarch64-unknown-linux-gnu",
    "win32-x64": "x86_64-pc-windows-msvc",
  };

  const key = `${platform}-${arch}`;
  const target = targets[key];

  if (!target) {
    console.error(`Unsupported platform: ${key}`);
    console.error(`Supported: ${Object.keys(targets).join(", ")}`);
    process.exit(1);
  }

  return target;
}

function getDownloadUrl(version, target) {
  const ext = process.platform === "win32" ? ".exe" : "";
  return `https://github.com/${REPO}/releases/download/v${version}/mohyung-${target}${ext}`;
}

async function download(url, dest) {
  return new Promise((resolve, reject) => {
    const follow = (url) => {
      https
        .get(url, (res) => {
          if (res.statusCode === 302 || res.statusCode === 301) {
            follow(res.headers.location);
            return;
          }
          if (res.statusCode !== 200) {
            reject(new Error(`Download failed: HTTP ${res.statusCode}`));
            return;
          }
          const file = createWriteStream(dest);
          res.pipe(file);
          file.on("finish", () => {
            file.close();
            resolve();
          });
        })
        .on("error", reject);
    };
    follow(url);
  });
}

async function main() {
  if (existsSync(BIN_PATH)) {
    return;
  }

  const pkg = require("../package.json");
  const version = pkg.version;
  const target = getPlatformTarget();
  const url = getDownloadUrl(version, target);

  console.log(`Downloading mohyung v${version} for ${target}...`);

  if (!existsSync(BIN_DIR)) {
    mkdirSync(BIN_DIR, { recursive: true });
  }

  try {
    await download(url, BIN_PATH);
    if (process.platform !== "win32") {
      chmodSync(BIN_PATH, 0o755);
    }
    console.log("mohyung installed successfully!");
  } catch (err) {
    console.error(`Failed to download mohyung: ${err.message}`);
    console.error(`URL: ${url}`);
    console.error(
      "You can manually download the binary from https://github.com/cbcruk/mohyung/releases"
    );
    process.exit(1);
  }
}

main();

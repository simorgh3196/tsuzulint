#!/usr/bin/env node
// postinstall: download the prebuilt `tzlint` binary matching this package's version and the
// host platform from the GitHub Release, verify it against the release SHA256SUMS, and place it
// next to the launcher. There is no JavaScript reimplementation — this only fetches and verifies.
"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const https = require("https");
const crypto = require("crypto");
const { execFileSync } = require("child_process");

const REPO = "simorgh3196/tsuzulint";
const pkg = require("./package.json");
const version = pkg.version;

// `process.platform`-`process.arch` -> Rust target triple (must match the release asset names).
const TARGETS = {
  "linux-x64": "x86_64-unknown-linux-gnu",
  "linux-arm64": "aarch64-unknown-linux-gnu",
  "darwin-x64": "x86_64-apple-darwin",
  "darwin-arm64": "aarch64-apple-darwin",
  "win32-x64": "x86_64-pc-windows-msvc",
};

function fail(msg) {
  console.error(`[tzlint] ${msg}`);
  process.exit(1);
}

// A dev checkout (version 0.0.0) or an explicit opt-out has no release to download from; the
// launcher then points at `cargo build`. Skipping keeps `npm install` in this repo from failing.
if (version === "0.0.0" || process.env.TZLINT_SKIP_DOWNLOAD === "1") {
  console.log("[tzlint] skipping binary download (dev checkout or TZLINT_SKIP_DOWNLOAD set).");
  process.exit(0);
}

const key = `${process.platform}-${process.arch}`;
const target = TARGETS[key];
if (!target) {
  // Exit 0 so installs in mixed-platform workspaces don't break; the launcher reports the gap.
  console.warn(`[tzlint] no prebuilt binary for ${key}; build from source: https://github.com/${REPO}`);
  process.exit(0);
}

const isWindows = process.platform === "win32";
const ext = isWindows ? "zip" : "tar.gz";
const archiveName = `tzlint-${version}-${target}.${ext}`;
const releaseBase = `https://github.com/${REPO}/releases/download/v${version}`;
const binDir = path.join(__dirname, "bin");
const finalBin = path.join(binDir, isWindows ? "tzlint-bin.exe" : "tzlint-bin");

function get(url) {
  // Follow GitHub's redirect to the asset CDN; resolve with the response body Buffer.
  return new Promise((resolve, reject) => {
    https
      .get(url, { headers: { "User-Agent": "tzlint-npm-installer" } }, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          res.resume();
          resolve(get(res.headers.location));
          return;
        }
        if (res.statusCode !== 200) {
          res.resume();
          reject(new Error(`HTTP ${res.statusCode} for ${url}`));
          return;
        }
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => resolve(Buffer.concat(chunks)));
      })
      .on("error", reject);
  });
}

// Parse a `sha256  filename` manifest into the expected hash for `archiveName`.
function expectedHash(sumsText) {
  for (const line of sumsText.split(/\r?\n/)) {
    const m = line.trim().match(/^([0-9a-f]{64})\s+\*?(.+)$/i);
    if (m && path.basename(m[2]) === archiveName) return m[1].toLowerCase();
  }
  return null;
}

(async () => {
  console.log(`[tzlint] downloading ${archiveName} ...`);
  let archive, sumsText;
  try {
    archive = await get(`${releaseBase}/${archiveName}`);
    sumsText = (await get(`${releaseBase}/SHA256SUMS`)).toString("utf8");
  } catch (e) {
    fail(`download failed: ${e.message}`);
  }

  const expected = expectedHash(sumsText);
  const actual = crypto.createHash("sha256").update(archive).digest("hex");
  if (!expected) {
    fail(`${archiveName} is not listed in SHA256SUMS; refusing to install an unverifiable binary.`);
  }
  if (actual !== expected) {
    fail(`checksum mismatch for ${archiveName}\n  expected ${expected}\n  actual   ${actual}`);
  }

  // Extract with the system `tar` (bsdtar handles .zip on Windows 10+ and .tar.gz everywhere),
  // then rename the extracted `tzlint`/`tzlint.exe` to the name the launcher execs.
  const tmp = path.join(os.tmpdir(), `tzlint-${process.pid}-${archiveName}`);
  fs.writeFileSync(tmp, archive);
  fs.mkdirSync(binDir, { recursive: true });
  try {
    execFileSync("tar", ["-xf", tmp, "-C", binDir], { stdio: "inherit" });
  } catch (e) {
    fail(`extraction failed (is \`tar\` on PATH?): ${e.message}`);
  } finally {
    fs.rmSync(tmp, { force: true });
  }

  const extracted = path.join(binDir, isWindows ? "tzlint.exe" : "tzlint");
  if (!fs.existsSync(extracted)) {
    fail(`archive did not contain the expected \`${path.basename(extracted)}\` binary.`);
  }
  fs.rmSync(finalBin, { force: true });
  fs.renameSync(extracted, finalBin);
  if (!isWindows) fs.chmodSync(finalBin, 0o755);

  console.log(`[tzlint] installed ${target} binary (verified).`);
})();

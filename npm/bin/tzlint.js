#!/usr/bin/env node
// Thin launcher: exec the native `tzlint` binary fetched by install.js, forwarding argv, stdio,
// and the exit code unchanged. This is the only JavaScript that runs at lint time.
"use strict";

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

const bin = path.join(__dirname, process.platform === "win32" ? "tzlint-bin.exe" : "tzlint-bin");

if (!fs.existsSync(bin)) {
  console.error(
    "[tzlint] native binary not found. Reinstall the package, or build from source:\n" +
      "  https://github.com/simorgh3196/tsuzulint (see docs/install.md)",
  );
  process.exit(1);
}

const result = spawnSync(bin, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`[tzlint] failed to run binary: ${result.error.message}`);
  process.exit(1);
}
// Mirror signal-kills as the conventional 128+signal code; otherwise pass the exit status through.
if (result.signal) process.exit(1);
process.exit(result.status === null ? 1 : result.status);

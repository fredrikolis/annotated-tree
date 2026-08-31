#!/usr/bin/env node
// Concern: resolves the prebuilt annotated-tree binary for the host and forwards argv, stdio and its exit code | Non-concern: the tool's behaviour | IO: (argv, platform) -> the binary's stdio + code
"use strict";

const path = require("path");
const { execFileSync } = require("child_process");

// Both Linux targets ship a static musl binary, so one entry per arch covers glibc and musl hosts.
const PLATFORM_PACKAGES = {
  "linux-x64": "annotated-tree-linux-x64-musl",
  "linux-arm64": "annotated-tree-linux-arm64-musl",
  "darwin-x64": "annotated-tree-darwin-x64",
  "darwin-arm64": "annotated-tree-darwin-arm64",
};

// pkg is null for an unsupported platform; bin is null when that package isn't installed.
function resolveBinary() {
  const key = `${process.platform}-${process.arch}`;
  const pkg = PLATFORM_PACKAGES[key];
  if (!pkg) {
    return { key, pkg: null, bin: null };
  }
  try {
    const pkgDir = path.dirname(require.resolve(`${pkg}/package.json`));
    return { key, pkg, bin: path.join(pkgDir, "annotated-tree") };
  } catch (_err) {
    return { key, pkg, bin: null };
  }
}

function fail(message) {
  process.stderr.write(`annotated-tree: ${message}\n`);
  process.exit(1);
}

function main() {
  const { key, pkg, bin } = resolveBinary();

  if (!pkg) {
    fail(
      `unsupported platform ${key}. No prebuilt binary is published for it. ` +
        `Install from source instead: cargo install annotated-tree`
    );
  }
  if (!bin) {
    fail(
      `the prebuilt binary for ${key} is missing (expected package "${pkg}"). ` +
        `Reinstall without skipping optional dependencies: ` +
        `npm install annotated-tree  (do not pass --no-optional), or install "${pkg}" directly.`
    );
  }

  try {
    execFileSync(bin, process.argv.slice(2), { stdio: "inherit" });
  } catch (err) {
    if (typeof err.status === "number") {
      process.exit(err.status);
    }
    fail(err.signal ? `binary terminated by signal ${err.signal}` : err.message);
  }
}

main();

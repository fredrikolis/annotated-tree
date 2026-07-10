// Launcher shim: resolves the platform-specific prebuilt annotated-tree binary and forwards argv, stdio, and its exit code. NOT the tool itself — adds no behaviour. | I/O: (process.argv, host platform/arch/libc) -> spawned binary's stdout/stderr + exit code
"use strict";

const path = require("path");
const { execFileSync } = require("child_process");

// Single source of truth mapping a host `${platform}-${arch}` to the npm
// platform package that carries its prebuilt binary. Both Linux targets ship a
// static musl binary, so one entry per Linux arch covers glibc and musl hosts.
const PLATFORM_PACKAGES = {
  "linux-x64": "annotated-tree-linux-x64-musl",
  "linux-arm64": "annotated-tree-linux-arm64-musl",
  "darwin-x64": "annotated-tree-darwin-x64",
  "darwin-arm64": "annotated-tree-darwin-arm64",
  "win32-x64": "annotated-tree-win32-x64",
};

function binaryName() {
  return process.platform === "win32" ? "annotated-tree.exe" : "annotated-tree";
}

// Resolve the absolute path to the binary shipped by the matching platform
// package. Returns { pkg, bin } where `pkg` is null for an unsupported
// platform and `bin` is null when the (supported) package is not installed.
function resolveBinary() {
  const key = `${process.platform}-${process.arch}`;
  const pkg = PLATFORM_PACKAGES[key];
  if (!pkg) {
    return { key, pkg: null, bin: null };
  }
  try {
    const pkgDir = path.dirname(require.resolve(`${pkg}/package.json`));
    return { key, pkg, bin: path.join(pkgDir, binaryName()) };
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
    // Fail-Fast: forward the binary's own exit code verbatim.
    if (typeof err.status === "number") {
      process.exit(err.status);
    }
    // Killed by a signal, or the binary could not be spawned at all.
    fail(err.signal ? `binary terminated by signal ${err.signal}` : err.message);
  }
}

main();

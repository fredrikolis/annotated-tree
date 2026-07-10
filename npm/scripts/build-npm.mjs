// Publish preparer: stamps the release version across the main + platform packages and drops each release binary into its platform dir, ready for `npm publish`. NOT a network/publish step. | I/O: (version arg, extracted-binaries dir) -> in-place npm/ tree + printed publish order
//
// Usage:  node npm/scripts/build-npm.mjs <version> <binaries-dir>
//
// <binaries-dir> holds one extracted binary per release target, at
//   <binaries-dir>/<rust-target>/annotated-tree        (annotated-tree.exe on win32)
// exactly as the GitHub-release tarballs (`annotated-tree-<rust-target>.tar.gz`)
// unpack. CI extracts those tarballs, then runs this once, then publishes each
// printed directory. Binaries are NEVER committed — they are injected here.

import { readFileSync, writeFileSync, copyFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// Single source of truth: npm platform <-> release target <-> binary filename.
const MATRIX = [
  { plat: "linux-x64-musl", target: "x86_64-unknown-linux-musl", bin: "annotated-tree" },
  { plat: "linux-arm64-musl", target: "aarch64-unknown-linux-musl", bin: "annotated-tree" },
  { plat: "darwin-x64", target: "x86_64-apple-darwin", bin: "annotated-tree" },
  { plat: "darwin-arm64", target: "aarch64-apple-darwin", bin: "annotated-tree" },
  { plat: "win32-x64", target: "x86_64-pc-windows-msvc", bin: "annotated-tree.exe" },
];

const [version, binariesDir] = process.argv.slice(2);
if (!version || !binariesDir) {
  console.error("usage: node build-npm.mjs <version> <binaries-dir>");
  process.exit(2);
}

const npmDir = dirname(dirname(fileURLToPath(import.meta.url)));

function stampVersion(pkgPath, mutate = (p) => p) {
  const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
  pkg.version = version;
  mutate(pkg);
  writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
}

const publishDirs = [];

// Platform packages: stamp version + inject the matching binary.
for (const { plat, target, bin } of MATRIX) {
  const platDir = join(npmDir, "platforms", plat);
  const src = join(binariesDir, target, bin);
  if (!existsSync(src)) {
    console.error(`missing binary for ${plat}: ${src}`);
    process.exit(1);
  }
  stampVersion(join(platDir, "package.json"));
  copyFileSync(src, join(platDir, bin));
  publishDirs.push(platDir);
}

// Main package: stamp version + pin every optionalDependency to it.
stampVersion(join(npmDir, "package.json"), (pkg) => {
  for (const dep of Object.keys(pkg.optionalDependencies ?? {})) {
    pkg.optionalDependencies[dep] = version;
  }
});

console.log("prepared npm packages for version " + version);
console.log("publish platform packages FIRST, then the main package:");
for (const dir of publishDirs) console.log("  npm publish " + dir);
console.log("  npm publish " + npmDir);

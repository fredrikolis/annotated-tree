// Publish preparer: stamps the release version across the main + platform packages, drops each release binary into its platform dir, and refreshes the package README from the canonical repo-root README.md, ready for `npm publish`. NOT a network/publish step. | I/O: (version arg, extracted-binaries dir) -> in-place distribution/npm/ tree + printed publish order
//
// Usage:  node distribution/npm/scripts/build-npm.mjs <version> <binaries-dir>
//
// <binaries-dir>/<rust-target>/annotated-tree. Never committed.

import { readFileSync, writeFileSync, copyFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const MATRIX = [
  { plat: "linux-x64-musl", target: "x86_64-unknown-linux-musl", bin: "annotated-tree" },
  { plat: "linux-arm64-musl", target: "aarch64-unknown-linux-musl", bin: "annotated-tree" },
  { plat: "darwin-x64", target: "x86_64-apple-darwin", bin: "annotated-tree" },
  { plat: "darwin-arm64", target: "aarch64-apple-darwin", bin: "annotated-tree" },
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

stampVersion(join(npmDir, "package.json"), (pkg) => {
  for (const dep of Object.keys(pkg.optionalDependencies ?? {})) {
    pkg.optionalDependencies[dep] = version;
  }
});

// From repo-root README.md; npm bundles README.md regardless of `files`.
const repoRoot = dirname(dirname(npmDir));
const rootReadme = join(repoRoot, "README.md");
if (!existsSync(rootReadme)) {
  console.error(`missing canonical README: ${rootReadme}`);
  process.exit(1);
}
copyFileSync(rootReadme, join(npmDir, "README.md"));

console.log("prepared npm packages for version " + version);
console.log("publish platform packages FIRST, then the main package:");
for (const dir of publishDirs) console.log("  npm publish " + dir);
console.log("  npm publish " + npmDir);

// Build the SWC plugin to wasm and copy the blob next to package.json, where
// `main` points so Next/Turbopack can `require.resolve` it. Run on the
// maintainer/CI machine (needs the Rust toolchain + the wasm32-wasip1 target);
// the committed/published .wasm is what end-user Next projects actually load.
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const pkgDir = dirname(dirname(fileURLToPath(import.meta.url)));
const TARGET = 'wasm32-wasip1';
const WASM = 'portbay_swc_plugin_loc.wasm';
const built = join(pkgDir, 'target', TARGET, 'release', WASM);
const shipped = join(pkgDir, WASM);

execFileSync(
  'cargo',
  ['build', '--release', '--target', TARGET, '--manifest-path', join(pkgDir, 'Cargo.toml')],
  { stdio: 'inherit' },
);

if (!existsSync(built)) {
  console.error(`build: expected wasm at ${built} but it is missing`);
  process.exit(1);
}
copyFileSync(built, shipped);
console.log(`build: ${shipped}`);

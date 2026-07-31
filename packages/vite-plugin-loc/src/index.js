// @portbay/vite-plugin-loc
//
// Stamps rendered elements with `data-pb-loc="<relpath>:<line>:<col>"` so
// PortBay's visual editor can resolve a clicked node to its authored source.
//
//   JSX / TSX  -> @portbay/babel-plugin-loc (AST, robust against TS generics)
//   Vue / Svelte / Astro -> tolerant markup stamper (template region only)
//
// Runs `enforce: 'pre'`, so it sees the *raw* source before the framework's
// own plugin splits/compiles it. Dev-only: nothing is emitted in a production
// build unless PORTBAY_LOC=1.

import path from 'node:path';
import * as babel from '@babel/core';
import babelPluginLoc from '@portbay/babel-plugin-loc';
import { stampMarkup } from './stamp-markup.js';

const MARKUP_MODE = { '.vue': 'vue', '.svelte': 'svelte', '.astro': 'astro' };

/** Project-root-relative POSIX path, or null when the id cannot be expressed as
 *  one (we never stamp a `..` path the resolver would reject).
 *
 *  Vite ids are usually absolute, but not always — and `path.relative` resolves
 *  a relative second argument against `process.cwd()`, NOT against `root`.
 *  That is right only by accident when Vite's `root` happens to equal the
 *  process cwd (the default), and silently produces a `..` path — no stamp, no
 *  diagnostic — for any project that sets `root` to a subdirectory. So a
 *  relative id is taken as root-relative and used as-is. Same defect the SWC
 *  port hit under Turbopack, where every filename arrives relative. */
function relPosix(root, filename) {
  if (!filename || !root) return null;
  const rel = path.isAbsolute(filename)
    ? path.relative(root, filename)
    : filename;
  const parts = [];
  for (const seg of rel.split(/[\\/]+/)) {
    if (seg === '' || seg === '.') continue;
    if (seg === '..') {
      if (parts.length === 0) return null;
      parts.pop();
      continue;
    }
    parts.push(seg);
  }
  return parts.length ? parts.join('/') : null;
}

/** Extensions this plugin can actually stamp. Used to decide whether a refused
 *  id is worth a diagnostic: a refused `.css` is routine, a refused `.tsx` is a
 *  broken visual editor. */
const STAMPABLE = new Set(['.jsx', '.tsx', '.vue', '.svelte', '.astro']);

/** The bail used to be silent, which is how a dead stamper ships unnoticed.
 *  Warn once per process, and only for a file we would otherwise have stamped. */
let warnedNoRel = false;
function warnNoRel(root, id) {
  if (warnedNoRel) return;
  warnedNoRel = true;
  console.warn(
    `[@portbay/vite-plugin-loc] not stamping ${id}: it does not resolve to a ` +
      `path under root=${JSON.stringify(root)}. No data-pb-loc will be ` +
      `emitted for it, so PortBay's visual editor cannot resolve it to source.`,
  );
}

function gateEnabled(option, viteCommand) {
  if (typeof option === 'boolean') return option;
  if (process.env.PORTBAY_LOC === '1') return true;
  if (process.env.PORTBAY_LOC === '0') return false;
  return viteCommand !== 'build'; // dev server by default
}

export default function portbayVitePluginLoc(userOpts = {}) {
  let root = process.cwd();
  let enabled = true;

  return {
    name: '@portbay/vite-plugin-loc',
    enforce: 'pre',

    configResolved(config) {
      root = (config && config.root) || process.cwd();
      enabled = gateEnabled(userOpts.enabled, config && config.command);
    },

    async transform(code, id) {
      if (!enabled || !id) return null;
      // Rollup/Vite virtual modules carry a NUL prefix and have no file on
      // disk. They must never be treated as a root-relative path — there is no
      // source for the editor to write back to.
      if (id.includes('\0')) return null;
      const filepath = id.split('?')[0];
      if (filepath.includes('/node_modules/')) return null;
      const ext = path.extname(filepath).toLowerCase();
      const rel = relPosix(root, filepath);
      if (!rel) {
        if (STAMPABLE.has(ext)) warnNoRel(root, filepath);
        return null;
      }

      if (ext === '.jsx' || ext === '.tsx') {
        let res;
        try {
          res = await babel.transformAsync(code, {
            filename: filepath,
            root,
            cwd: root,
            configFile: false,
            babelrc: false,
            sourceMaps: true,
            parserOpts: {
              plugins: ext === '.tsx' ? ['jsx', 'typescript'] : ['jsx'],
            },
            plugins: [[babelPluginLoc, { enabled: true, root }]],
          });
        } catch {
          return null; // never break the user's build on a parse hiccup
        }
        if (!res || res.code == null) return null;
        return { code: res.code, map: res.map };
      }

      const mode = MARKUP_MODE[ext];
      if (mode) {
        const out = stampMarkup(code, rel, { mode });
        return out == null ? null : { code: out, map: null };
      }
      return null;
    },
  };
}

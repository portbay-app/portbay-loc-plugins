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

function relPosix(root, filename) {
  let rel = path.relative(root, filename);
  if (!rel || rel.startsWith('..') || path.isAbsolute(rel)) return null;
  return rel.split(path.sep).join('/');
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
      const filepath = id.split('?')[0];
      if (filepath.includes('/node_modules/')) return null;
      const rel = relPosix(root, filepath);
      if (!rel) return null;
      const ext = path.extname(filepath).toLowerCase();

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

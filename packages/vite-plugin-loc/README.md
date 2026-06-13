# @portbay/vite-plugin-loc

Dev-time Vite plugin that stamps rendered elements with their authored source
location — `data-pb-loc="<relpath>:<line>:<col>"` — for
[PortBay](https://portbay.app)'s visual editor.

- **JSX / TSX** (React, Preact, Solid) — via `@portbay/babel-plugin-loc` (AST;
  TS generics like `useState<string>()` are never mistaken for elements).
- **Vue SFC** — host elements inside `<template>` (script/style skipped).
- **Svelte** — markup (script/style skipped).
- **Astro** — markup (frontmatter, script, style skipped).

## Install

```bash
pnpm add -D @portbay/vite-plugin-loc
```

```js
// vite.config.js
import { defineConfig } from 'vite'
import portbayLoc from '@portbay/vite-plugin-loc'
// import your framework plugin, e.g. @vitejs/plugin-react, @vitejs/plugin-vue …

export default defineConfig({
  // portbayLoc runs `enforce: 'pre'`, so list order does not matter.
  plugins: [portbayLoc(), /* react()/vue()/svelte()/… */],
})
```

That's it. In `vite dev`, every host element is stamped; `vite build` emits
nothing.

## Options

| Option | Default | Meaning |
|---|---|---|
| `enabled` | `true` on `vite dev`, `false` on `vite build` | Force on/off. |

`PORTBAY_LOC=1` forces stamping on (including builds); `PORTBAY_LOC=0` forces
it off.

## How it resolves edits

PortBay reads `data-pb-loc` off the clicked element (or its nearest stamped
ancestor), opens the authored file at that line, verifies the element's tag and
the original text/class still match, then patches that one element. If the
attribute is absent, PortBay falls back to its text-search resolver unchanged.

MIT © Tribal House LLC. Independent implementation; no third-party plugin code.

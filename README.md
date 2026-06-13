# PortBay source-location plugins

Dev-time build plugins that stamp each rendered element with its **authored
source location** — `data-pb-loc="<file>:<line>:<col>"` — so PortBay's visual
editor can resolve a clicked element back to the exact span in your source
files instead of guessing with a text search.

| Package | Pipeline | Frameworks |
|---|---|---|
| [`@portbay/vite-plugin-loc`](packages/vite-plugin-loc) | Vite | React/Preact/Solid (JSX/TSX), Vue SFC, Svelte, Astro |
| [`@portbay/babel-plugin-loc`](packages/babel-plugin-loc) | Babel (CRA / Next babel / webpack) | JSX/TSX |

## What it does

Each element carries its origin as a DOM attribute:

```html
<button data-pb-loc="src/components/Hero.jsx:42:7">Get started</button>
```

- `file` — project-root-relative, POSIX.
- `line` — **1-based** line of the element's opening `<` in the authored source.
- `col` — **1-based** column of that `<` (a tie-breaker; the resolver anchors on
  the line + tag name).

PortBay's resolver opens the authored file at that coordinate, verifies identity
(the captured original text/class is still present), and patches the one element
— so React `className`, `.map()` / `v-for` repeated copies, and scoped-component
styles all resolve deterministically. When the attribute is absent (no plugin),
PortBay falls back to its text-search resolver with **zero behavior change**.

## Dev-only, by design

The attribute is emitted **only in development** (`NODE_ENV !== 'production'`,
or set `PORTBAY_LOC=1` to force it). It never reaches a production bundle —
no DOM bloat, no leaking of local file paths.

## Install

See each package's README. Quick start for Vite:

```bash
pnpm add -D @portbay/vite-plugin-loc
```

```js
// vite.config.js
import { defineConfig } from 'vite'
import portbayLoc from '@portbay/vite-plugin-loc'

export default defineConfig({
  plugins: [portbayLoc(), /* your framework plugin(s) */],
})
```

## License

MIT © Tribal House LLC. Independent implementation; no third-party plugin code.

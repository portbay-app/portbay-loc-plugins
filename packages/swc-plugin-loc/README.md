# @portbay/swc-plugin-loc

Dev-time **SWC plugin** that stamps each rendered JSX host element with its
**authored source location** — `data-pb-loc="<file>:<line>:<col>"` — so
PortBay's visual editor can resolve a clicked element back to the exact span in
your source instead of guessing with a text search.

This is the **SWC/Turbopack** member of the family, for **Next.js App Router**
(which compiles with SWC by default, not Babel). It is byte-for-byte parity with
[`@portbay/babel-plugin-loc`](../babel-plugin-loc) and
[`@portbay/vite-plugin-loc`](../vite-plugin-loc):

```html
<button data-pb-loc="app/page.tsx:42:7">Get started</button>
```

- `file` — project-root-relative, POSIX.
- `line` — **1-based** line of the element's opening `<`.
- `col` — **1-based** column of that `<`.
- Host elements only (lowercase `<div>`, `<button>`, `<my-widget>`). Components
  (`<Hero>`), member (`<Foo.Bar>`) and namespaced names are never stamped.
- Idempotent, and appended **after** your attributes so a `{...spread}` can never
  override it. A single `.map()` element yields one source loc shared by every
  rendered copy.

## Dev-only, by design

The attribute is emitted only in development. PortBay wires the plugin into
`next dev` and removes it otherwise; the plugin also self-gates on `NODE_ENV`
(`production` ⇒ off) when no explicit `enabled` flag is passed. It never reaches
a production bundle — no DOM bloat, no leaking of local file paths.

> Unlike the Babel/Vite plugins, there is no `PORTBAY_LOC` env escape hatch: SWC
> plugins run in a sandboxed wasm VM with no access to `process.env`. Pass
> `enabled: true`/`false` through plugin config instead (PortBay does this for
> you).

## Install

```bash
pnpm add -D @portbay/swc-plugin-loc
```

The package ships a prebuilt `portbay_swc_plugin_loc.wasm`; **no Rust toolchain
is required to consume it**. (Building from source does — see below.)

### `next dev` (default, webpack + SWC)

```js
// next.config.js
const path = require('node:path');

/** @type {import('next').NextConfig} */
module.exports = {
  experimental: {
    swcPlugins: [
      [
        require.resolve('@portbay/swc-plugin-loc'),
        { root: __dirname, enabled: process.env.NODE_ENV !== 'production' },
      ],
    ],
  },
};
```

`require.resolve(...)` resolves to the package's `.wasm` (its `main`). `root`
**must** be the project root the emitted paths should be relative to (usually
`__dirname`); files outside it are skipped rather than stamped with a `..` path.

### `next dev --turbo` (Turbopack)

Turbopack reads the **same** `experimental.swcPlugins` entry — the config above
covers both pipelines. No separate loader is needed on a Next version whose
Turbopack supports wasm SWC plugins.

> **Version note (verify against your Next version):** wasm SWC plugin support
> in Turbopack landed during the Next 15 line and is still evolving. On a Next
> release where Turbopack ignores `swcPlugins`, precise editing silently falls
> back to the text-search resolver under `--turbo` (zero behavior change, just
> less precision). PortBay's `loc_detect` reports which pipeline is active.

## swc_core ↔ Next compatibility (important)

An SWC wasm plugin is ABI-coupled to the SWC compiler that loads it. The host
that matters is **`@next/swc`** — the compiler Next.js bundles for `next dev` /
`next build` — **not** the standalone `@swc/core` on npm (Next does not use it,
and it lags `@next/swc`). This blob is built against:

| Property | Value |
|---|---|
| `swc_core` | `71.0.3` |
| `swc_ecma_ast` | `25.0.0` |
| Plugin schema | `__plugin_transform_schema_v1` |

### Verified compatibility (2026-07-04)

| Host | Result |
|---|---|
| **Next.js 16.2.x** (webpack lane) | **Works** — `data-pb-loc` stamped in the rendered DOM, byte-accurate |
| Next.js 16.2.x default (Turbopack) | Path-resolution failure — see the Turbopack note above (out of scope) |
| Standalone `@swc/core` 1.15.x | Silent no-op — *"AST schema version is not compatible with host's"*, plugin skipped |
| Standalone `@swc/core` ≤ 1.14.x | Hard error — `failed to invoke plugin` |

### Two failure modes

- **Host older than the blob** → the compile fails loudly (`failed to invoke
  plugin`).
- **Host rejects the AST schema** (e.g. standalone `@swc/core`, or a Next that
  bundles an older `@next/swc`) → the plugin is **silently skipped**: no stamp,
  no error, `next` reports success. Precise editing then degrades to text search.
  PortBay's browser bar detects this (installed + configured but no
  `[data-pb-loc]` in the DOM) and shows a "wired but not active" warning.

When your Next version bundles an `@next/swc` outside this blob's range, rebuild
from source pinned to a matching `swc_core` (see <https://plugins.swc.rs/>), bump
the minor, and re-verify. PortBay pins and rebuilds this blob per supported Next
range; the version above is what ships in this package.

## Build from source

Requires the Rust toolchain and the wasm target:

```bash
rustup target add wasm32-wasip1
pnpm build      # cargo build --release + copy the .wasm to the package root
pnpm test       # cargo test — the parity suite (mirrors the babel vitest cases)
```

## License

MIT © Tribal House LLC. Independent implementation; no third-party plugin code.

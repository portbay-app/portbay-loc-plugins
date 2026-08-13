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
- Host elements (lowercase `<div>`, `<button>`, `<my-widget>`) get `data-pb-loc`.
  **Component** call sites (`<Hero title="…" />`) get `data-pb-comp` as a prop —
  React copies it into the component fiber's `memoizedProps`, so PortBay can edit
  a component's literal props at the JSX call site (see the
  [repo README](../../README.md#component-call-sites--data-pb-comp-reactjsx-only)).
  Member (`<Foo.Bar>`) and namespaced names are never stamped.
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

### Config — one entry, both bundlers

```js
// next.config.js
/** @type {import('next').NextConfig} */
module.exports = {
  experimental: {
    swcPlugins: [
      [
        '@portbay/swc-plugin-loc',
        { root: __dirname, enabled: process.env.NODE_ENV !== 'production' },
      ],
    ],
  },
};
```

The first element **must be the bare package specifier**, not
`require.resolve('@portbay/swc-plugin-loc')`. `require.resolve` returns an
absolute filesystem path, and Turbopack resolves the `swcPlugins` entry as a
*module specifier*: on Next 16.3.0 an absolute path fails the compile outright
with `Module not found: … server relative imports are not implemented yet`.
The bare specifier is resolved correctly by **both** Turbopack and webpack, so
there is no pipeline that wants the `require.resolve` form.

`root` **must** be the project root the emitted paths should be relative to
(usually `__dirname`); files outside it are skipped rather than stamped with a
`..` path.

### Turbopack (the `next dev` default since Next 16)

Turbopack reads the **same** `experimental.swcPlugins` entry — the config above
covers both pipelines. No separate loader is needed.

The two bundlers hand the plugin different filename shapes, and it handles both:

| Bundler | `Filename` context | Handling |
|---|---|---|
| webpack / `@swc/core` | absolute (`/proj/app/page.tsx`) | `root` stripped off the front |
| Turbopack | **already project-root-relative** (`app/page.tsx`) | used as-is |

Both produce identical coordinates. A path that escapes `root` — absolute and
outside it, or relative and climbing above it — is refused rather than stamped
with a `..` the resolver would reject.

> **Fixed after 0.1.0.** Before the fix, the relative form was fed to
> `Path::strip_prefix`, which failed, so the plugin stamped **nothing at all**
> under Turbopack — silently, with a successful build and an HTTP 200. That is
> why every bail now emits a warning (below).

> **Version note (verify against your Next version):** wasm SWC plugin support
> in Turbopack landed during the Next 15 line and is still evolving. On a Next
> release where Turbopack ignores `swcPlugins` entirely, precise editing falls
> back to the text-search resolver (zero behavior change, just less precision).
> PortBay's `loc_detect` reports which pipeline is active.

### When it decides not to stamp, it says so

If the plugin is enabled but cannot turn a file into a root-relative path, it
emits a warning naming the reason, the configured `root`, and the filename it
was handed — on the plugin's `HANDLER` diagnostic channel *and* on the wasm
sandbox's stderr. Two channels because whether a `HANDLER` warning is ever
printed is the host's decision (`@swc/core`'s Node binding buffers plugin
diagnostics and drops them when the transform succeeds; stderr gets through).

The commonest cause is a `root` that is not the directory the bundler's
filenames are relative to. The warning fires per refused file: swc instantiates
a fresh wasm module per transform, so no plugin-side state survives to
de-duplicate it.

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

### Verified compatibility

| Host | Result |
|---|---|
| **Next.js 16.3.0 default (Turbopack)** | **Works at 0.1.1** (2026-08-13) — verified end-to-end against a live `next dev`: served HTML carries byte-accurate `data-pb-loc`. Under **0.1.0** the same app served HTTP 200 with **zero** stamps |
| **Next.js 16.3.0** (`next dev --webpack`) | **Works at 0.1.1** (2026-08-13) — served HTML `data-pb-loc` set is identical to the Turbopack run |
| **Next.js 16.2.x** (webpack lane) | **Works** (2026-07-04) — `data-pb-loc` stamped in the rendered DOM, byte-accurate |
| **Standalone `@swc/core` 1.15.47** | **Works** (2026-07-31) — loads and stamps; the earlier "AST schema not compatible" no-op no longer reproduces on this version |
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

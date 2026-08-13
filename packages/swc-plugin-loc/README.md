# @portbay/swc-plugin-loc

An SWC plugin that stamps every rendered JSX host element with the place it was
written, `data-pb-loc="<file>:<line>:<col>"`, so PortBay's visual editor can open
the exact span you clicked. It runs in development only.

Next.js compiles with SWC rather than Babel, so this is the plugin Next projects
want. PortBay covers Vite, Astro, Vue and Svelte by injecting its own plugin into
the dev server it launches, with nothing for you to install.

```html
<button data-pb-loc="app/page.tsx:42:7">Get started</button>
```

`file` is project-root-relative and POSIX. `line` and `col` are 1-based and point
at the element's opening `<`.

Lowercase host elements (`<div>`, `<button>`, `<my-widget>`) get `data-pb-loc`.
Component call sites like `<Hero title="…" />` render no DOM node, so they get
`data-pb-comp` as a prop instead; React copies it into the fiber's
`memoizedProps`, which lets PortBay edit a component's literal props where you
wrote them. See the [repo README](../../README.md#component-call-sites-react-and-jsx-only).
Member tags (`<Foo.Bar>`) and namespaced names are never stamped.

The stamp is idempotent and lands after your own attributes, so a `{...spread}`
cannot overwrite it. One element inside `.map()` yields a single source location
shared by every copy it renders.

## Development only

PortBay wires the plugin into `next dev` and leaves it out otherwise. When you
pass no explicit `enabled` flag the plugin gates itself on `NODE_ENV`, treating
`production` as off. Nothing reaches a production bundle: no extra DOM, no source
paths in shipped HTML.

There is no `PORTBAY_LOC` escape hatch here, unlike the Blade stamper. SWC
plugins run inside a sandboxed wasm VM that cannot read `process.env`. Pass
`enabled: true` or `false` through plugin config, which PortBay does for you.

## Install

```bash
pnpm add -D @portbay/swc-plugin-loc
```

The package ships a prebuilt `portbay_swc_plugin_loc.wasm`, so consuming it needs
no Rust toolchain. Building from source does.

### Config, one entry for both bundlers

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

Pass the bare package specifier as the first element, never
`require.resolve('@portbay/swc-plugin-loc')`. `require.resolve` returns an
absolute filesystem path, and Turbopack reads that entry as a module specifier,
so on Next 16.3.0 an absolute path fails the compile outright with `Module not
found: … server relative imports are not implemented yet`. The bare specifier
resolves under both Turbopack and webpack, so no pipeline needs the
`require.resolve` form.

Set `root` to the project root your emitted paths should be relative to, usually
`__dirname`. Files outside it are skipped rather than stamped with a `..` path.

### Turbopack, the `next dev` default since Next 16

Turbopack reads the same `experimental.swcPlugins` entry, so the config above
covers both pipelines and you need no separate loader.

The two bundlers hand the plugin different filename shapes, and it handles both:

| Bundler | `Filename` context | Handling |
|---|---|---|
| webpack / `@swc/core` | absolute (`/proj/app/page.tsx`) | `root` stripped off the front |
| Turbopack | already project-root-relative (`app/page.tsx`) | used as-is |

Both produce identical coordinates. A path that escapes `root`, whether absolute
and outside it or relative and climbing above it, gets refused rather than
stamped with a `..` the resolver would reject.

> **Fixed after 0.1.0.** Earlier versions fed the relative form to
> `Path::strip_prefix`, which failed, so the plugin stamped nothing at all under
> Turbopack. The build succeeded, the page returned 200, and no warning appeared.
> Every bail now emits a diagnostic, which is why the section below exists.

> **Check this against your Next version.** Support for wasm SWC plugins in
> Turbopack landed during the Next 15 line and is still moving. On a release
> where Turbopack ignores `swcPlugins`, precise editing falls back to the
> text-search resolver with no change in behaviour, only less precision.
> PortBay's `loc_detect` reports which pipeline is active.

### It tells you when it declines to stamp

When the plugin is enabled but cannot turn a file into a root-relative path, it
names the reason, the configured `root` and the filename it was handed. It writes
that to the plugin's `HANDLER` diagnostic channel and to the wasm sandbox's
stderr, because the host decides whether a `HANDLER` warning is ever printed:
`@swc/core`'s Node binding buffers plugin diagnostics and drops them when the
transform succeeds, while stderr gets through.

Usually the cause is a `root` that is not the directory your bundler's filenames
are relative to. The warning fires once per refused file. SWC instantiates a
fresh wasm module per transform, so no state survives to deduplicate it.

## swc_core and Next compatibility

An SWC wasm plugin is ABI-coupled to the compiler that loads it. The host that
matters is `@next/swc`, which Next bundles for `next dev` and `next build`, not
the standalone `@swc/core` on npm. Next does not use that one, and it lags
`@next/swc`. This blob is built against:

| Property | Value |
|---|---|
| `swc_core` | `71.0.3` |
| `swc_ecma_ast` | `25.0.0` |
| Plugin schema | `__plugin_transform_schema_v1` |

### What we have verified

| Host | Result |
|---|---|
| Next.js 16.3.0, default Turbopack | Works at 0.1.1 (2026-08-13). Checked end-to-end against a live `next dev`: the served HTML carries byte-accurate `data-pb-loc`. Under 0.1.0 the same app returned 200 with zero stamps |
| Next.js 16.3.0, `next dev --webpack` | Works at 0.1.1 (2026-08-13). The served `data-pb-loc` set matches the Turbopack run exactly |
| Next.js 16.2.x, webpack lane | Works (2026-07-04). Byte-accurate stamps in the rendered DOM |
| Standalone `@swc/core` 1.15.47 | Works (2026-07-31). Loads and stamps; the earlier "AST schema not compatible" no-op no longer reproduces |
| Standalone `@swc/core` 1.14.x and below | Hard error, `failed to invoke plugin` |

### Two ways it can fail

A host older than the blob fails the compile loudly with `failed to invoke
plugin`.

A host that rejects the AST schema skips the plugin in silence. That covers
standalone `@swc/core` and any Next bundling an older `@next/swc`. You get no
stamp, no error, and a successful build, and precise editing quietly degrades to
text search. PortBay's browser bar catches this case, since the plugin is
installed and configured but no `[data-pb-loc]` appears in the DOM, and warns
that it is wired but not active.

If your Next version bundles an `@next/swc` outside this blob's range, rebuild
from source against a matching `swc_core` (see <https://plugins.swc.rs/>), bump
the minor version, and verify again. PortBay rebuilds and pins this blob per
supported Next range; the version above is what ships here.

## Build from source

You need the Rust toolchain and the wasm target:

```bash
rustup target add wasm32-wasip1
pnpm build      # cargo build --release, then copy the .wasm to the package root
pnpm test       # cargo test, the parity suite
```

## License

MIT © Tribal House LLC. Independent implementation, containing no third-party
plugin code.

# PortBay source-location plugins

Dev-time build instrumentation that stamps each rendered element with its
**authored source location** — `data-pb-loc="<file>:<line>:<col>"` — so PortBay's
visual editor can resolve a clicked element back to the exact span in your source
files instead of guessing with a text search.

| Package | Registry | Pipeline | Covers |
|---|---|---|---|
| [`@portbay/swc-plugin-loc`](packages/swc-plugin-loc) | npm | SWC / Turbopack | Next.js (JSX/TSX), webpack **and** Turbopack |
| [`portbay/blade-stamper`](packages/blade-stamper) | — (unpublished) | Laravel Blade compiler | `.blade.php` templates |

## Vite, Astro, Vue, Svelte — nothing to install

There is no `@portbay/vite-plugin-loc` on npm, and that is deliberate.

PortBay injects its own Vite loc plugin into the dev server it launches, loaded
by absolute path from a generated config. Your project — `package.json`,
`node_modules`, `vite.config` — is byte-identical before and after. That plugin
lives inside the PortBay application, not here, precisely so the promise holds:
**you install nothing.**

An earlier npm-shaped copy of it lived in this repo and was removed once the
injected version superseded it; a second implementation that nothing consumed was
only ever going to drift. Same for a Babel stamper: PortBay does not own
`babel.config.js`, so the Babel lane refuses by name and states what still works
rather than prescribing an install. Both remain in this repo's git history.

## What it does

Each **host** element carries its origin as a DOM attribute:

```html
<button data-pb-loc="src/components/Hero.jsx:42:7">Get started</button>
```

- `file` — project-root-relative, POSIX.
- `line` — **1-based** line of the element's opening `<` in the authored source.
- `col` — **1-based** column of that `<` (a tie-breaker; the resolver anchors on
  the line + tag name).

### Component call sites — `data-pb-comp` (React/JSX only)

A React **Component** (`<Hero title="…" />`) renders no DOM node of its own, so
it cannot carry a DOM attribute. Instead the JSX/TSX stamper stamps the **call
site** as a prop, `data-pb-comp="<relpath>:<line>:<col>"`, pointing at the
opening `<` of the `<Hero …>` tag in the authored source:

```jsx
// authored:  src/App.jsx, line 7
<Hero title="Welcome" count={3} />
// compiled (dev):
jsxDEV(Hero, { title: "Welcome", count: 3, "data-pb-comp": "src/App.jsx:7:3" }, …)
```

React copies enumerable props into the component fiber's `memoizedProps`
(verified against react@18.3.1 and react@19.2.5 dev bundles), so PortBay reads
the call-site coordinate off the resolved component fiber at runtime — a genuine
**source** coordinate, with no runtime sourcemap reversal. Because it is a
`data-*` prop, a wrapper that spreads `{...props}` onto a host element leaks it
to the DOM harmlessly and warning-free (React passes `data-*`/`aria-*` through
untouched).

Member (`<Foo.Bar>`) and namespaced component tags are a v1 gap.

PortBay's resolver opens the authored file at that coordinate, verifies identity
(the captured original text/class is still present), and patches the one element
— so React `className`, `.map()` / `v-for` repeated copies, and scoped-component
styles all resolve deterministically. When the attribute is absent, PortBay falls
back to its text-search resolver with **zero behavior change**.

## Dev-only, by design

The attribute is emitted **only in development** (`NODE_ENV !== 'production'`, or
set `PORTBAY_LOC=1` to force it). It never reaches a production bundle — no DOM
bloat, no leaking of local file paths.

## Install — Next.js

```bash
npm add -D @portbay/swc-plugin-loc
```

```js
// next.config.js — dev only; `next dev --turbopack` reads the same key
module.exports = {
  experimental: {
    swcPlugins: [
      ['@portbay/swc-plugin-loc', { root: process.cwd(), enabled: process.env.NODE_ENV !== 'production' }],
    ],
  },
}
```

The first element **must be the bare package specifier**. `require.resolve(...)`
returns an absolute filesystem path, and Turbopack resolves the `swcPlugins`
entry as a *module specifier* — an absolute path fails the compile outright with
`Module not found: … server relative imports are not implemented yet`. The bare
specifier resolves correctly under **both** Turbopack and webpack.

`root` must be the project root the emitted paths are relative to; files outside
it are skipped rather than stamped with a path that escapes the project.

## License

MIT © Tribal House LLC. Independent implementation; no third-party plugin code.

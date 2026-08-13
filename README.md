# PortBay source-location plugins

These plugins stamp every rendered element with the place it was written,
`data-pb-loc="<file>:<line>:<col>"`, so PortBay's visual editor can open the exact
span you clicked. Without them the editor searches your source for matching text,
which works but resolves fewer elements.

| Package | Registry | Pipeline | Covers |
|---|---|---|---|
| [`@portbay/swc-plugin-loc`](packages/swc-plugin-loc) | npm | SWC / Turbopack | Next.js (JSX/TSX), webpack and Turbopack |
| [`portbay/blade-stamper`](packages/blade-stamper) | not yet published | Laravel Blade compiler | `.blade.php` templates |

## Vite, Astro, Vue and Svelte need no install

You will not find `@portbay/vite-plugin-loc` on npm. PortBay ships that plugin
inside the application and loads it by absolute path from a config it generates,
so your `package.json`, `node_modules` and `vite.config` are byte-identical
before and after. You install nothing.

An npm-shaped copy of it once lived here. We removed it when the injected
version took over, because two implementations of one thing drift apart and
nobody notices until a coordinate is wrong. A Babel stamper went the same way:
PortBay does not own your `babel.config.js`, so the Babel lane now names what it
cannot do instead of prescribing an install that would not work. Both are still
in git history.

## The attribute

Every host element carries where it came from:

```html
<button data-pb-loc="src/components/Hero.jsx:42:7">Get started</button>
```

`file` is project-root-relative and POSIX. `line` is the 1-based line of the
element's opening `<`. `col` is the 1-based column of that same `<`, used to
break ties; the resolver matches on line plus tag name.

### Component call sites, React and JSX only

`<Hero title="…" />` renders no DOM node of its own, so it cannot carry an
attribute. The JSX stamper instead passes the call site as a prop,
`data-pb-comp="<relpath>:<line>:<col>"`, pointing at the opening `<` of the
`<Hero …>` tag you wrote:

```jsx
// you wrote, in src/App.jsx line 7:
<Hero title="Welcome" count={3} />
// dev build emits:
jsxDEV(Hero, { title: "Welcome", count: 3, "data-pb-comp": "src/App.jsx:7:3" }, …)
```

React copies enumerable props into the fiber's `memoizedProps`, which we checked
against the react@18.3.1 and react@19.2.5 dev bundles, so PortBay reads the
coordinate off the resolved fiber at runtime. That gives it a real source
position with no sourcemap reversal, and lets you edit a component's literal
props where you wrote them. Since it is a `data-*` prop, a wrapper that spreads
`{...props}` onto a host element passes it to the DOM harmlessly; React forwards
`data-*` and `aria-*` untouched.

Member tags like `<Foo.Bar>` and namespaced tags are not stamped yet.

PortBay opens the file at that coordinate, checks the element is still the one it
captured, and patches that single element. React `className`, repeated `.map()`
or `v-for` copies, and scoped component styles all resolve without guessing. Drop
the attribute and PortBay falls back to text search with no change in behaviour.

## Development only

The plugins stamp only outside production. They read `NODE_ENV`, and PortBay
passes an explicit `enabled` flag when it wires them, so nothing reaches a
production bundle: no extra DOM, no source paths in shipped HTML.

> **`PORTBAY_LOC=1` overrides that check.** It exists so you can debug the
> stamper in an environment that reports itself as production, and it is not safe
> to leave on. Anything you serve with it set publishes your file names, your
> directory layout and your line numbers to whoever loads the page. Use it on a
> machine you control, then unset it. The SWC plugin has no such variable, since
> wasm plugins cannot read `process.env`.

## Installing for Next.js

```bash
npm add -D @portbay/swc-plugin-loc
```

```js
// next.config.js. Dev only; `next dev --turbopack` reads the same key.
module.exports = {
  experimental: {
    swcPlugins: [
      ['@portbay/swc-plugin-loc', { root: process.cwd(), enabled: process.env.NODE_ENV !== 'production' }],
    ],
  },
}
```

Pass the bare package specifier as the first element. `require.resolve(...)`
hands back an absolute path, and Turbopack treats that entry as a module
specifier, so an absolute path breaks the compile with `Module not found: …
server relative imports are not implemented yet`. The bare specifier works under
both Turbopack and webpack.

Set `root` to the directory your emitted paths should be relative to. The plugin
skips files outside it rather than writing a path that climbs out of your
project.

## License

MIT © Tribal House LLC. Independent implementation, containing no third-party
plugin code.

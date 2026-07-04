# @portbay/babel-plugin-loc

Dev-time Babel plugin that stamps each JSX element with its authored source
location for [PortBay](https://portbay.app)'s visual editor:

- **host** elements (`<div>`, `<button>`) get `data-pb-loc="<relpath>:<line>:<col>"`;
- **Component** call sites (`<Hero title="…" />`) get `data-pb-comp="<relpath>:<line>:<col>"`
  as a prop, so PortBay can edit a component's literal props at the JSX call site
  (see the [repo README](../../README.md#component-call-sites--data-pb-comp-reactjsx-only)).

Use this when your build runs Babel directly (Create React App, a custom
webpack/Babel pipeline, or Next.js with the Babel pipeline). For Vite, use
[`@portbay/vite-plugin-loc`](../vite-plugin-loc).

## Install

```bash
npm i -D @portbay/babel-plugin-loc
```

```js
// babel.config.js — keep it in the DEV config only
module.exports = {
  plugins: [
    process.env.NODE_ENV !== 'production' && '@portbay/babel-plugin-loc',
  ].filter(Boolean),
};
```

With Next.js (Babel pipeline), add it to `.babelrc`:

```json
{ "presets": ["next/babel"], "plugins": ["@portbay/babel-plugin-loc"] }
```

## Options

| Option | Default | Meaning |
|---|---|---|
| `enabled` | `NODE_ENV !== 'production'` | Force the stamp on/off. |
| `root` | Babel `cwd` / `process.cwd()` | Root the `file` path is made relative to. |

`PORTBAY_LOC=1` forces stamping on (even in production builds); `PORTBAY_LOC=0`
forces it off.

## Contract

- Only lowercase intrinsic elements (`<div>`, `<li>`, …) are stamped — never
  Components (`<Hero>`), member names (`<Foo.Bar>`) or namespaced names.
- `line` is 1-based; `col` is the 1-based column of the opening `<`.
- The plugin is idempotent and skips files outside `root`.

MIT © Tribal House LLC.

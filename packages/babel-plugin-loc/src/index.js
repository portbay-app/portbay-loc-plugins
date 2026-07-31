// @portbay/babel-plugin-loc
//
// Stamps each JSX opening element with its authored source location, pointing
// at the opening `<` in the *authored* source file, so PortBay's visual editor
// can resolve a rendered node to its exact source span.
//
//   Host element (lowercase intrinsic <div>, <li>, <button>)
//     -> `data-pb-loc="<relpath>:<line>:<col>"`   (renders a DOM node)
//   Component call site (uppercase <Hero title="…" />)
//     -> `data-pb-comp="<relpath>:<line>:<col>"`  (the JSX call site)
//
// The two use DISTINCT attribute names on purpose. A host element's DOM node
// carries `data-pb-loc` directly. A component renders no DOM node of its own,
// so `data-pb-comp` rides as a *prop* on the component: React copies enumerable
// props into `element.props` -> the fiber's `memoizedProps` (verified against
// react@18.3.1 and react@19.2.5 dev bundles — jsxDEV builds props by iterating
// enumerable own-props, and React 19 reuses the config object directly when no
// `key` is present), so the editor reads the call-site coordinate off the
// resolved component fiber at runtime — a genuine SOURCE coordinate, needing no
// runtime sourcemap reversal. If a wrapper component spreads the prop onto a
// host (`<div {...props}/>`) the value leaks to the DOM as a `data-*`
// attribute, which is benign and warning-free (React passes `data-*`/`aria-*`
// through untouched — the same reason `data-pb-loc` on hosts is warning-free).
// See the repo README for the full contract.
//
// Dev-only: nothing is emitted in a production build unless PORTBAY_LOC=1.

import path from 'node:path';

const ATTR_HOST = 'data-pb-loc';
const ATTR_COMP = 'data-pb-comp';

/** Resolve the dev-only gate. Plugin `enabled` option wins, then PORTBAY_LOC,
 *  then NODE_ENV. */
function isEnabled(opts) {
  if (opts && typeof opts.enabled === 'boolean') return opts.enabled;
  if (process.env.PORTBAY_LOC === '1') return true;
  if (process.env.PORTBAY_LOC === '0') return false;
  return process.env.NODE_ENV !== 'production';
}

/** Project-root-relative POSIX path, or null when the file cannot be expressed
 *  as one (we never stamp a `..` path the resolver would reject).
 *
 *  `path.relative` resolves a RELATIVE second argument against `process.cwd()`,
 *  NOT against `root` — right only by accident when the two are the same
 *  directory, and silently a `..` path (=> no stamp, no diagnostic) when they
 *  are not. That is exactly what made the SWC port stamp nothing under
 *  Turbopack, which hands its plugins already-root-relative filenames. So a
 *  relative `filename` is taken as root-relative and used as-is.
 *
 *  MEASURED: Babel itself resolves `opts.filename` against `cwd` before any
 *  plugin runs, so through Babel's own API this branch is unreachable — the
 *  visitor always sees an absolute path (pinned by a test). The handling is
 *  kept because `relPosix` must not depend on that guarantee holding. */
function relPosix(root, filename) {
  if (!filename || !root) return null;
  const rel = path.isAbsolute(filename)
    ? path.relative(root, filename)
    : filename;
  // Normalise separators, drop `.` / empty segments, and resolve `..` against
  // what we have — refusing the moment one would climb above `root`.
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

/** The bail path used to be silent, which is how a dead stamper shipped
 *  unnoticed. Warn once per process — the visitor only runs on files that
 *  actually contain JSX, so a refusal here is always worth saying out loud. */
let warnedNoRel = false;
function warnNoRel(root, filename) {
  if (warnedNoRel) return;
  warnedNoRel = true;
  console.warn(
    `[@portbay/babel-plugin-loc] not stamping ${filename || '(no filename)'}: ` +
      `it does not resolve to a path under root=${JSON.stringify(root)}. ` +
      `No data-pb-loc will be emitted for it, so PortBay's visual editor ` +
      `cannot resolve it to source.`,
  );
}

export default function portbayBabelPluginLoc(babel) {
  const { types: t } = babel;
  return {
    name: '@portbay/babel-plugin-loc',
    visitor: {
      JSXOpeningElement(nodePath, state) {
        const opts = state.opts || {};
        if (!isEnabled(opts)) return;

        const root = opts.root || state.cwd || process.cwd();
        const filename =
          state.filename ||
          (state.file && state.file.opts && state.file.opts.filename);
        const rel = relPosix(root, filename);
        if (!rel) {
          warnNoRel(root, filename);
          return;
        }

        const node = nodePath.node;

        // Only a plain <Ident> tag. Member names (<Foo.Bar>) and namespaced
        // names (<svg:rect>) are skipped: member/namespaced components are an
        // explicit v1 gap, and namespaced SVG hosts are stamped by their
        // ancestor chain in practice.
        const name = node.name;
        if (!t.isJSXIdentifier(name)) return;
        const tag = name.name;

        // Lowercase intrinsic -> host DOM node -> data-pb-loc.
        // Anything else (uppercase Component) -> call site -> data-pb-comp.
        const isHost = /^[a-z]/.test(tag);
        const attr = isHost ? ATTR_HOST : ATTR_COMP;

        // Idempotent: never double-stamp the SAME attribute (host and comp are
        // distinct names, so they never collide).
        const already = node.attributes.some(
          (a) =>
            t.isJSXAttribute(a) &&
            t.isJSXIdentifier(a.name) &&
            a.name.name === attr,
        );
        if (already) return;

        const start = node.loc && node.loc.start;
        if (!start) return;
        const line = start.line; // Babel: 1-based
        const col = (start.column | 0) + 1; // Babel: 0-based -> 1-based of `<`
        const value = `${rel}:${line}:${col}`;

        // Appended last, so a `{...spread}` earlier in the tag can never
        // override the location we just stamped.
        node.attributes.push(
          t.jsxAttribute(t.jsxIdentifier(attr), t.stringLiteral(value)),
        );
      },
    },
  };
}

// @portbay/babel-plugin-loc
//
// Stamps every JSX *host* element (a lowercase intrinsic like <div>, <li>,
// <button> — never a Component) with `data-pb-loc="<relpath>:<line>:<col>"`,
// pointing at the opening `<` in the *authored* source file. PortBay's visual
// editor reads this attribute to resolve a rendered node to its exact source
// span. See the repo README for the contract.
//
// Dev-only: nothing is emitted in a production build unless PORTBAY_LOC=1.

import path from 'node:path';

const ATTR = 'data-pb-loc';

/** Resolve the dev-only gate. Plugin `enabled` option wins, then PORTBAY_LOC,
 *  then NODE_ENV. */
function isEnabled(opts) {
  if (opts && typeof opts.enabled === 'boolean') return opts.enabled;
  if (process.env.PORTBAY_LOC === '1') return true;
  if (process.env.PORTBAY_LOC === '0') return false;
  return process.env.NODE_ENV !== 'production';
}

/** Project-root-relative POSIX path, or null when the file sits outside root
 *  (we never stamp a `..` path the resolver would reject). */
function relPosix(root, filename) {
  if (!filename) return null;
  let rel = path.relative(root, filename);
  if (!rel || rel.startsWith('..') || path.isAbsolute(rel)) return null;
  return rel.split(path.sep).join('/');
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
        if (!rel) return;

        const node = nodePath.node;

        // Host elements only: a plain lowercase JSXIdentifier. Components
        // (uppercase), member names (<Foo.Bar>) and namespaced names produce
        // no DOM node of their own, so they get no loc.
        const name = node.name;
        if (!t.isJSXIdentifier(name)) return;
        const tag = name.name;
        if (!/^[a-z]/.test(tag)) return;

        // Idempotent: never double-stamp.
        const already = node.attributes.some(
          (a) =>
            t.isJSXAttribute(a) &&
            t.isJSXIdentifier(a.name) &&
            a.name.name === ATTR,
        );
        if (already) return;

        const start = node.loc && node.loc.start;
        if (!start) return;
        const line = start.line; // Babel: 1-based
        const col = (start.column | 0) + 1; // Babel: 0-based -> 1-based of `<`
        const value = `${rel}:${line}:${col}`;

        node.attributes.push(
          t.jsxAttribute(t.jsxIdentifier(ATTR), t.stringLiteral(value)),
        );
      },
    },
  };
}

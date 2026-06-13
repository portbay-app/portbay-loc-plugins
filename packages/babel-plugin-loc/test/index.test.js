import { describe, it, expect, afterEach } from 'vitest';
import { transformSync } from '@babel/core';
import path from 'node:path';
import plugin from '../src/index.js';

function run(code, { file = 'src/App.jsx', root = '/proj', ts = false, ...opts } = {}) {
  const res = transformSync(code, {
    filename: path.join(root, file),
    root,
    cwd: root,
    configFile: false,
    babelrc: false,
    parserOpts: { plugins: ts ? ['jsx', 'typescript'] : ['jsx'] },
    plugins: [[plugin, { enabled: true, root, ...opts }]],
  });
  return res.code;
}

/** Every emitted loc must point exactly at a `<` in the ORIGINAL source — the
 *  property PortBay's Rust resolver depends on. */
function assertLocAnchorsAtOpeningBracket(loc, source) {
  const m = /^(.*):(\d+):(\d+)$/.exec(loc);
  expect(m, `loc "${loc}" must be file:line:col`).not.toBeNull();
  const line = +m[2];
  const col = +m[3];
  const lines = source.split('\n');
  expect(lines[line - 1][col - 1]).toBe('<');
}

const allLocs = (out) =>
  [...out.matchAll(/data-pb-loc="([^"]+)"/g)].map((m) => m[1]);

afterEach(() => {
  delete process.env.NODE_ENV;
  delete process.env.PORTBAY_LOC;
});

describe('@portbay/babel-plugin-loc', () => {
  it('stamps a host element and anchors col at the opening `<`', () => {
    const code = `export default function App() {
  return <div className="card">Hi</div>;
}`;
    const out = run(code);
    const locs = allLocs(out);
    expect(locs).toHaveLength(1);
    expect(locs[0]).toMatch(/^src\/App\.jsx:2:\d+$/);
    assertLocAnchorsAtOpeningBracket(locs[0], code);
    // className is preserved (this is what unlocks React class write-back).
    expect(out).toContain('className="card"');
  });

  it('stamps host elements but never Components', () => {
    const code = `const App = () => (
  <Hero>
    <span>x</span>
  </Hero>
);`;
    const out = run(code);
    const locs = allLocs(out);
    // <span> only; <Hero> is a Component.
    expect(locs).toHaveLength(1);
    expect(out).toMatch(/<Hero>/); // untouched
    expect(out).toMatch(/<span data-pb-loc=/);
    locs.forEach((l) => assertLocAnchorsAtOpeningBracket(l, code));
  });

  it('stamps each nested host element independently', () => {
    const code = `const A = () => <ul><li>a</li><li>b</li></ul>;`;
    const out = run(code);
    const locs = allLocs(out);
    expect(locs).toHaveLength(3); // ul + 2 li
    locs.forEach((l) => assertLocAnchorsAtOpeningBracket(l, code));
  });

  it('a single .map() element yields one source loc (shared by N copies)', () => {
    const code = `const List = ({ items }) => (
  <ul>{items.map((it) => <li key={it.id}>{it.label}</li>)}</ul>
);`;
    const out = run(code);
    // One <li> in source -> one data-pb-loc; every rendered copy carries it.
    // (the attribute is appended, so `key` precedes it — that's intentional so
    // a {...spread} can never override the loc.)
    const li = [...out.matchAll(/<li[^>]*\bdata-pb-loc="([^"]+)"/g)];
    expect(li).toHaveLength(1);
    assertLocAnchorsAtOpeningBracket(li[0][1], code);
  });

  it('does not mistake TSX generics for elements', () => {
    const code = `const C = () => {
  const [v] = useState<Record<string, number>>({});
  return <div>{v.x}</div>;
};`;
    const out = run(code, { file: 'src/C.tsx', ts: true });
    const locs = allLocs(out);
    expect(locs).toHaveLength(1); // only <div>
    expect(locs[0]).toMatch(/^src\/C\.tsx:/);
    // the generic is left intact
    expect(out).toContain('useState');
  });

  it('is idempotent — re-running does not double-stamp', () => {
    const code = `const A = () => <div>x</div>;`;
    const once = run(code);
    const twice = run(once);
    expect(allLocs(twice)).toHaveLength(1);
  });

  it('emits nothing when disabled', () => {
    expect(allLocs(run(`const A = () => <div>x</div>;`, { enabled: false }))).toHaveLength(0);
  });

  it('honours NODE_ENV=production (no opt override)', () => {
    process.env.NODE_ENV = 'production';
    const out = transformSync(`const A = () => <div>x</div>;`, {
      filename: '/proj/src/App.jsx',
      root: '/proj',
      cwd: '/proj',
      configFile: false,
      babelrc: false,
      parserOpts: { plugins: ['jsx'] },
      plugins: [[plugin, { root: '/proj' }]],
    }).code;
    expect(allLocs(out)).toHaveLength(0);
  });

  it('PORTBAY_LOC=1 forces stamping even in production', () => {
    process.env.NODE_ENV = 'production';
    process.env.PORTBAY_LOC = '1';
    const out = transformSync(`const A = () => <div>x</div>;`, {
      filename: '/proj/src/App.jsx',
      root: '/proj',
      cwd: '/proj',
      configFile: false,
      babelrc: false,
      parserOpts: { plugins: ['jsx'] },
      plugins: [[plugin, { root: '/proj' }]],
    }).code;
    expect(allLocs(out)).toHaveLength(1);
  });

  it('skips files outside the project root (no `..` paths)', () => {
    const out = transformSync(`const A = () => <div>x</div>;`, {
      filename: '/elsewhere/App.jsx',
      root: '/proj',
      cwd: '/proj',
      configFile: false,
      babelrc: false,
      parserOpts: { plugins: ['jsx'] },
      plugins: [[plugin, { enabled: true, root: '/proj' }]],
    }).code;
    expect(allLocs(out)).toHaveLength(0);
  });
});

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

const allComps = (out) =>
  [...out.matchAll(/data-pb-comp="([^"]+)"/g)].map((m) => m[1]);

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

  it('stamps hosts with data-pb-loc and Components with data-pb-comp', () => {
    const code = `const App = () => (
  <Hero>
    <span>x</span>
  </Hero>
);`;
    const out = run(code);
    const locs = allLocs(out);
    const comps = allComps(out);
    // <span> is a host -> data-pb-loc; <Hero> is a Component -> data-pb-comp.
    expect(locs).toHaveLength(1);
    expect(comps).toHaveLength(1);
    // The two attributes are distinct — a Component never gets data-pb-loc and
    // a host never gets data-pb-comp.
    expect(out).toMatch(/<Hero data-pb-comp=/);
    expect(out).not.toMatch(/<Hero[^>]*data-pb-loc=/);
    expect(out).toMatch(/<span data-pb-loc=/);
    expect(out).not.toMatch(/<span[^>]*data-pb-comp=/);
    // Both coordinates anchor at their own opening `<`.
    assertLocAnchorsAtOpeningBracket(locs[0], code);
    assertLocAnchorsAtOpeningBracket(comps[0], code);
  });

  it('stamps a Component call site at the opening `<` of the Component', () => {
    const code = `export default function App() {
  return <Hero title="Welcome" count={3} disabled />;
}`;
    const out = run(code);
    const comps = allComps(out);
    expect(comps).toHaveLength(1);
    expect(comps[0]).toMatch(/^src\/App\.jsx:2:\d+$/);
    assertLocAnchorsAtOpeningBracket(comps[0], code);
    // The authored props are preserved verbatim — the call-site source that
    // the editor's prop classifier later reads is untouched.
    expect(out).toContain('title="Welcome"');
    expect(out).toContain('count={3}');
    expect(out).toMatch(/\bdisabled\b/);
  });

  it('data-pb-comp is appended last so a {...spread} cannot override it', () => {
    const code = `const A = () => <Hero {...rest} title="x" />;`;
    const out = run(code);
    const comps = allComps(out);
    expect(comps).toHaveLength(1);
    // The stamped attribute must sit AFTER the spread in emit order.
    const spreadIdx = out.indexOf('...rest');
    const compIdx = out.indexOf('data-pb-comp');
    expect(spreadIdx).toBeGreaterThanOrEqual(0);
    expect(compIdx).toBeGreaterThan(spreadIdx);
  });

  it('leaves member-expression and namespaced tags unstamped (v1 gap)', () => {
    const code = `const A = () => (
  <Foo.Bar>
    <ns:widget />
  </Foo.Bar>
);`;
    const out = run(code);
    expect(allLocs(out)).toHaveLength(0);
    expect(allComps(out)).toHaveLength(0);
    expect(out).toMatch(/<Foo\.Bar>/); // untouched
  });

  it('is idempotent for Components too', () => {
    const code = `const A = () => <Hero title="x" />;`;
    const once = run(code);
    const twice = run(once);
    expect(allComps(twice)).toHaveLength(1);
  });

  it('emits no data-pb-comp in production (no opt override)', () => {
    process.env.NODE_ENV = 'production';
    const out = transformSync(`const A = () => <Hero title="x" />;`, {
      filename: '/proj/src/App.jsx',
      root: '/proj',
      cwd: '/proj',
      configFile: false,
      babelrc: false,
      parserOpts: { plugins: ['jsx'] },
      plugins: [[plugin, { root: '/proj' }]],
    }).code;
    expect(allComps(out)).toHaveLength(0);
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

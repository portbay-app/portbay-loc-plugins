import { describe, it, expect } from 'vitest';
import plugin from '../src/index.js';

function setup({ command = 'serve', root = '/proj', enabled } = {}) {
  const p = plugin(enabled === undefined ? {} : { enabled });
  p.configResolved({ root, command });
  return p;
}

describe('@portbay/vite-plugin-loc', () => {
  it('declares enforce: pre so it runs before framework plugins', () => {
    expect(plugin().enforce).toBe('pre');
  });

  it('stamps JSX via babel and preserves className', async () => {
    const p = setup();
    const res = await p.transform(
      `export default () => <div className="x">hi</div>;`,
      '/proj/src/App.jsx',
    );
    expect(res).toBeTruthy();
    expect(res.code).toMatch(/data-pb-loc="src\/App\.jsx:1:\d+"/);
    expect(res.code).toContain('className');
  });

  it('stamps Vue templates via the markup scanner', async () => {
    const p = setup();
    const res = await p.transform(
      `<template><p>hi</p></template>`,
      '/proj/src/A.vue',
    );
    expect(res.code).toMatch(/<p data-pb-loc="src\/A\.vue:1:11">/);
  });

  it('does not run on a production build by default', async () => {
    const p = setup({ command: 'build' });
    const res = await p.transform(
      `export default () => <div/>;`,
      '/proj/src/App.jsx',
    );
    expect(res).toBeNull();
  });

  it('skips node_modules', async () => {
    const p = setup();
    const res = await p.transform(
      `<template><p>x</p></template>`,
      '/proj/node_modules/dep/A.vue',
    );
    expect(res).toBeNull();
  });

  it('ignores non-target extensions and files outside root', async () => {
    const p = setup();
    expect(await p.transform(`<p>x</p>`, '/proj/src/readme.md')).toBeNull();
    expect(await p.transform(`<p>x</p>`, '/other/A.vue')).toBeNull();
  });

  // ---- Id shape. `path.relative` resolves a RELATIVE second argument against
  // process.cwd(), not against Vite's `root` — right only by accident when the
  // two are the same directory. This is the same defect that made the SWC port
  // stamp nothing under Turbopack, where every filename arrives relative.

  it('stamps a root-relative id even when root is not the process cwd', async () => {
    const p = setup({ root: '/proj' });
    const res = await p.transform(
      `export default () => <div className="x">hi</div>;`,
      'src/App.jsx',
    );
    expect(res).toBeTruthy();
    expect(res.code).toMatch(/data-pb-loc="src\/App\.jsx:1:\d+"/);
  });

  it('absolute and root-relative ids stamp the same coordinates', async () => {
    const p = setup({ root: '/proj' });
    const code = `export default () => <div>hi</div>;`;
    const abs = await p.transform(code, '/proj/src/App.jsx');
    const rel = await p.transform(code, 'src/App.jsx');
    expect(rel.code).toBe(abs.code);
  });

  it('stamps a Vue template from a root-relative id', async () => {
    const p = setup({ root: '/proj' });
    const res = await p.transform(`<template><p>hi</p></template>`, 'src/A.vue');
    expect(res.code).toMatch(/<p data-pb-loc="src\/A\.vue:1:11">/);
  });

  it('refuses a relative id that climbs above root', async () => {
    const p = setup({ root: '/proj' });
    expect(
      await p.transform(`<template><p>x</p></template>`, '../A.vue'),
    ).toBeNull();
    expect(
      await p.transform(`<template><p>x</p></template>`, 'src/../../A.vue'),
    ).toBeNull();
  });

  it('never treats a virtual module id as a source path', async () => {
    const p = setup({ root: '/proj' });
    // Rollup/Vite mark virtual modules with a NUL prefix. They have no file on
    // disk, so a stamp would point the editor at nothing.
    expect(
      await p.transform(`export default () => <div>x</div>;`, '\0virtual.jsx'),
    ).toBeNull();
    expect(
      await p.transform(`<template><p>x</p></template>`, '\0plugin:a.vue'),
    ).toBeNull();
  });

  it('drops the query string before resolving a relative id', async () => {
    const p = setup({ root: '/proj' });
    const res = await p.transform(
      `<template><p>hi</p></template>`,
      'src/A.vue?vue&type=template',
    );
    expect(res.code).toMatch(/data-pb-loc="src\/A\.vue:1:11"/);
  });
});

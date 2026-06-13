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
});

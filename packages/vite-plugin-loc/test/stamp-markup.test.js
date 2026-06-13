import { describe, it, expect } from 'vitest';
import { stampMarkup } from '../src/stamp-markup.js';

const locs = (out) => [...out.matchAll(/data-pb-loc="([^"]+)"/g)].map((m) => m[1]);

/** Each loc must point exactly at a `<` in the ORIGINAL source. */
function anchors(loc, source) {
  const m = /^(.*):(\d+):(\d+)$/.exec(loc);
  expect(m, `loc "${loc}"`).not.toBeNull();
  const lines = source.split('\n');
  expect(lines[+m[2] - 1][+m[3] - 1]).toBe('<');
}

describe('stampMarkup — Vue', () => {
  const vue = `<template>
  <ul>
    <li class="item">A</li>
  </ul>
</template>

<script setup>
const x = "<not-a-tag>";
</script>

<style scoped>
.item { color: red; }
</style>`;

  it('stamps template host elements, never script/style/template/the value', () => {
    const out = stampMarkup(vue, 'src/List.vue', { mode: 'vue' });
    const found = locs(out);
    // <ul> and <li> only.
    expect(found).toHaveLength(2);
    expect(out).toMatch(/<ul data-pb-loc="src\/List\.vue:2:3"/);
    expect(out).toMatch(/<li data-pb-loc="src\/List\.vue:3:5" class="item"/);
    expect(out).not.toContain('<not-a-tag data-pb-loc'); // script body untouched
    expect(out).not.toMatch(/<template data-pb-loc/); // wrapper not stamped
    found.forEach((l) => anchors(l, vue));
  });
});

describe('stampMarkup — Svelte', () => {
  const svelte = `<script>
  let items = [1, 2, 3];
</script>

{#each items as n}
  <li class="row">{n}</li>
{/each}

<style>
  .row { padding: 4px; }
</style>`;

  it('stamps markup but skips script/style; one source loc per #each body', () => {
    const out = stampMarkup(svelte, 'src/Rows.svelte', { mode: 'svelte' });
    const found = locs(out);
    expect(found).toHaveLength(1); // the single <li> in the each body
    expect(out).toMatch(/<li data-pb-loc="src\/Rows\.svelte:6:3" class="row"/);
    anchors(found[0], svelte);
  });
});

describe('stampMarkup — Astro', () => {
  const astro = `---
const title = "<frontmatter not markup>";
import Card from './Card.astro';
---
<section>
  <Card />
  <p>{title}</p>
</section>`;

  it('skips the frontmatter fence and Components, stamps host elements', () => {
    const out = stampMarkup(astro, 'src/pages/index.astro', { mode: 'astro' });
    const found = locs(out);
    // <section> and <p>; <Card /> is a Component, frontmatter is skipped.
    expect(found).toHaveLength(2);
    expect(out).toContain('<Card />'); // untouched
    expect(out).not.toContain('frontmatter not markup" data-pb-loc');
    found.forEach((l) => anchors(l, astro));
  });
});

describe('stampMarkup — general', () => {
  it('handles void and self-closing elements', () => {
    const html = `<div><img src="a.png"><br/></div>`;
    const out = stampMarkup(html, 'x.html', { mode: 'html' });
    expect(out).toMatch(/<img data-pb-loc="x\.html:1:6" src="a\.png"/);
    expect(out).toMatch(/<br data-pb-loc="x\.html:1:\d+"\/>/);
  });

  it('does not stamp inside HTML comments', () => {
    const html = `<!-- <p>ghost</p> -->\n<p>real</p>`;
    const out = stampMarkup(html, 'x.html', { mode: 'html' });
    const found = locs(out);
    expect(found).toHaveLength(1);
    expect(found[0]).toBe('x.html:2:1');
  });

  it('is idempotent — a re-stamp is a no-op (returns null = no change)', () => {
    const once = stampMarkup(`<p>hi</p>`, 'x.html', { mode: 'html' });
    expect(locs(once)).toHaveLength(1);
    expect(stampMarkup(once, 'x.html', { mode: 'html' })).toBeNull();
  });

  it('returns null when there is nothing to stamp', () => {
    expect(stampMarkup(`const x = 1;`, 'x.js', { mode: 'html' })).toBeNull();
  });
});

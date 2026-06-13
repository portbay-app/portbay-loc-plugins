// Tolerant markup stamper for template-shaped sources (Vue SFC <template>,
// Svelte, Astro, plain HTML). It is NOT a parser — it is a single left-to-right
// scan over the markup shape every target shares (`<tag attr="…">…</tag>`),
// inserting `data-pb-loc="<relpath>:<line>:<col>"` after the tag name of each
// host element. JSX/TSX go through @portbay/babel-plugin-loc instead, because a
// regex scan cannot tell `useState<string>()` from a `<string>` element.

const ATTR = 'data-pb-loc';

// Wrapper elements that produce no DOM node of their own — skipped, but their
// children are still stamped.
const SKIP = new Set(['script', 'style', 'template', 'slot']);

const isLowerAlpha = (c) => c >= 'a' && c <= 'z';
const isNameChar = (c) =>
  (c >= 'a' && c <= 'z') ||
  (c >= 'A' && c <= 'Z') ||
  (c >= '0' && c <= '9') ||
  c === '-' ||
  c === '_' ||
  c === ':';

/**
 * Stamp host elements in `code` with their source location.
 * @param {string} code source text
 * @param {string} relpath project-root-relative POSIX path
 * @param {{mode?: 'vue'|'svelte'|'astro'|'html'}} [opts]
 * @returns {string|null} stamped code, or null when nothing changed
 */
export function stampMarkup(code, relpath, opts = {}) {
  const mode = opts.mode || 'html';
  const n = code.length;

  // Astro frontmatter fence: `---\n … \n---` at the very top is code, not markup.
  let frontmatterEnd = 0;
  if (mode === 'astro' && code.startsWith('---')) {
    const close = code.indexOf('\n---', 3);
    if (close !== -1) {
      const eol = code.indexOf('\n', close + 1);
      frontmatterEnd = eol === -1 ? n : eol + 1;
    }
  }

  let inScript = false;
  let inStyle = false;
  let templateDepth = 0;

  const active = (at) => {
    if (at < frontmatterEnd) return false;
    if (inScript || inStyle) return false;
    if (mode === 'vue') return templateDepth > 0;
    return true;
  };

  const inserts = [];
  let i = 0;
  let line = 1;
  let lineStart = 0;

  while (i < n) {
    const c = code[i];
    if (c === '\n') {
      line++;
      lineStart = i + 1;
      i++;
      continue;
    }
    if (c !== '<') {
      i++;
      continue;
    }

    // Comments / doctype / processing instructions.
    if (code.startsWith('<!--', i)) {
      const end = code.indexOf('-->', i + 4);
      // advance counting newlines
      const stop = end === -1 ? n : end + 3;
      while (i < stop) {
        if (code[i] === '\n') {
          line++;
          lineStart = i + 1;
        }
        i++;
      }
      continue;
    }
    if (code[i + 1] === '!' || code[i + 1] === '?') {
      i++;
      continue;
    }

    const isClose = code[i + 1] === '/';
    let j = i + 1 + (isClose ? 1 : 0);
    const nameStart = j;
    while (j < n && isNameChar(code[j])) j++;
    if (j === nameStart) {
      i++;
      continue;
    }
    const rawName = code.slice(nameStart, j);
    const name = rawName.toLowerCase();

    if (isClose) {
      if (name === 'script') inScript = false;
      else if (name === 'style') inStyle = false;
      else if (name === 'template' && templateDepth > 0) templateDepth--;
      i = j;
      continue;
    }

    // Opening tag — find its `>`, respecting quoted attribute values.
    const ltCol = i - lineStart + 1;
    const ltLine = line;
    let k = j;
    let quote = null;
    while (k < n) {
      const ck = code[k];
      if (quote) {
        if (ck === quote) quote = null;
      } else if (ck === '"' || ck === "'") {
        quote = ck;
      } else if (ck === '>') {
        break;
      }
      k++;
    }
    const selfClosing = k > j && code[k - 1] === '/';
    const openSlice = code.slice(i, k);
    const alreadyStamped = openSlice.includes(ATTR);
    const isHost =
      isLowerAlpha(rawName[0]) && !rawName.includes(':') && !SKIP.has(name);

    if (active(i) && isHost && !alreadyStamped) {
      inserts.push({ at: j, text: ` ${ATTR}="${relpath}:${ltLine}:${ltCol}"` });
    }

    // Entering a wrapper region affects this tag's CHILDREN, so flip after the
    // stamping decision for the tag itself.
    if (!selfClosing) {
      if (name === 'script') inScript = true;
      else if (name === 'style') inStyle = true;
      else if (name === 'template') templateDepth++;
    }

    // Advance to the `>` (or EOF), counting newlines inside the tag.
    const stop = k < n ? k + 1 : n;
    while (i < stop) {
      if (code[i] === '\n') {
        line++;
        lineStart = i + 1;
      }
      i++;
    }
  }

  if (!inserts.length) return null;
  inserts.sort((a, b) => a.at - b.at);
  let out = '';
  let prev = 0;
  for (const ins of inserts) {
    out += code.slice(prev, ins.at) + ins.text;
    prev = ins.at;
  }
  out += code.slice(prev);
  return out;
}

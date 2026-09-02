import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';

export const docsSiteRoot = process.cwd();
export const contentRoot = path.join(docsSiteRoot, 'content/docs');
export const publicRoot = path.join(docsSiteRoot, 'public');
export const repositoryRoot = path.resolve(docsSiteRoot, '..');
export const publicSiteOrigin = 'https://www.memory-layer.dev';

export function walk(dir, predicate = () => true) {
  const entries = [];

  for (const name of readdirSync(dir)) {
    const file = path.join(dir, name);
    const stat = statSync(file);

    if (stat.isDirectory()) {
      entries.push(...walk(file, predicate));
    } else if (predicate(file)) {
      entries.push(file);
    }
  }

  return entries;
}

function isInside(root, candidate) {
  const relative = path.relative(root, candidate);
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..');
}

export function includedSpecifiers(text) {
  const specifiers = [];
  const pattern = /<include(?:\s+[^>]*)?>\s*([^<\s][^<]*?)\s*<\/include>/g;

  for (const match of text.matchAll(pattern)) {
    specifiers.push(match[1].trim());
  }

  return specifiers;
}

export function collectDocsContent() {
  const records = [];
  const errors = [];
  const visited = new Set();

  function collect(file, includedBy = null) {
    const normalized = path.normalize(file);
    if (visited.has(normalized)) {
      return;
    }
    visited.add(normalized);

    if (!isInside(repositoryRoot, normalized)) {
      errors.push(`${path.relative(docsSiteRoot, includedBy ?? normalized)}: include escapes repository: ${normalized}`);
      return;
    }
    if (!existsSync(normalized)) {
      errors.push(`${path.relative(docsSiteRoot, includedBy ?? normalized)}: missing included file ${normalized}`);
      return;
    }
    if (!/\.mdx?$/i.test(normalized)) {
      errors.push(`${path.relative(docsSiteRoot, includedBy ?? normalized)}: included file must be Markdown: ${normalized}`);
      return;
    }

    const text = readFileSync(normalized, 'utf8');
    records.push({ file: normalized, text, includedBy });

    for (const specifier of includedSpecifiers(text)) {
      const target = specifier.split('#', 1)[0];
      if (!target) {
        errors.push(`${path.relative(docsSiteRoot, normalized)}: include needs a file path`);
        continue;
      }
      collect(path.resolve(path.dirname(normalized), target), normalized);
    }
  }

  for (const file of walk(contentRoot, (entry) => entry.endsWith('.mdx'))) {
    collect(file);
  }

  return { records, errors };
}

export function publicPathForReference(reference) {
  if (reference.startsWith(`${publicSiteOrigin}/`)) {
    return new URL(reference).pathname;
  }
  return reference;
}

export function isDocsSiteContent(file) {
  return isInside(contentRoot, file);
}

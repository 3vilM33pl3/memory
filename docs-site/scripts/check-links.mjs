import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const contentRoot = path.join(root, 'content/docs');
const publicRoot = path.join(root, 'public');
const validPrefixes = ['/docs', '/demo', '/images', '/api/search'];

function walk(dir, predicate = () => true) {
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

function contentPathForDocsUrl(url) {
  const clean = url.split('#')[0].replace(/\/$/, '');

  if (clean === '/docs') {
    return path.join(contentRoot, 'index.mdx');
  }

  if (!clean.startsWith('/docs/')) {
    return null;
  }

  const slug = clean.slice('/docs/'.length);
  return path.join(contentRoot, `${slug}.mdx`);
}

function existsForDocsUrl(url) {
  const file = contentPathForDocsUrl(url);

  if (!file) {
    return false;
  }

  if (existsSync(file)) {
    return true;
  }

  return existsSync(path.join(file.replace(/\.mdx$/, ''), 'index.mdx'));
}

const errors = [];

for (const file of walk(contentRoot, (entry) => entry.endsWith('.mdx'))) {
  const text = readFileSync(file, 'utf8');
  const rel = path.relative(root, file);
  const refs = [
    ...[...text.matchAll(/\[[^\]]+]\(([^)\s]+)\)/g)].map((match) => match[1]),
    ...[...text.matchAll(/href=["']([^"']+)["']/g)].map((match) => match[1]),
  ];

  for (const ref of refs) {
    if (/^(https?:|mailto:|#)/.test(ref)) {
      continue;
    }

    if (!ref.startsWith('/')) {
      errors.push(`${rel}: relative link should be absolute: ${ref}`);
      continue;
    }

    if (!validPrefixes.some((prefix) => ref === prefix || ref.startsWith(`${prefix}/`))) {
      errors.push(`${rel}: unsupported absolute link: ${ref}`);
      continue;
    }

    if (ref.startsWith('/docs') && !existsForDocsUrl(ref)) {
      errors.push(`${rel}: missing docs page ${ref}`);
    }

    if (ref.startsWith('/images') && !existsSync(path.join(publicRoot, ref))) {
      errors.push(`${rel}: missing image ${ref}`);
    }
  }
}

if (errors.length > 0) {
  console.error(errors.join('\n'));
  process.exit(1);
}

console.log('checked docs links');

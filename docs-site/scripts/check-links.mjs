import { existsSync } from 'node:fs';
import path from 'node:path';

import {
  collectDocsContent,
  contentRoot,
  docsSiteRoot,
  isDocsSiteContent,
  publicPathForReference,
  publicRoot,
} from './content-utils.mjs';

const root = docsSiteRoot;
const validPrefixes = ['/docs', '/demo', '/images', '/api/search'];

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
const { records, errors: contentErrors } = collectDocsContent();
errors.push(...contentErrors);

for (const { file, text, includedBy } of records) {
  const rel = path.relative(root, file);
  const refs = [
    ...[...text.matchAll(/\[[^\]]+]\(([^)\s]+)\)/g)].map((match) => match[1]),
    ...[...text.matchAll(/href=["']([^"']+)["']/g)].map((match) => match[1]),
  ];

  for (const rawRef of refs) {
    const ref = publicPathForReference(rawRef);
    if (/^(mailto:|#)/.test(ref)) {
      continue;
    }

    if (/^https?:/.test(ref)) {
      continue;
    }

    if (!ref.startsWith('/')) {
      if (!isDocsSiteContent(file) && !includedBy) {
        continue;
      }
      if (!isDocsSiteContent(file)) {
        errors.push(`${rel}: included content must use an absolute docs or image URL: ${rawRef}`);
        continue;
      }
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

    if (ref.startsWith('/images') && !existsSync(path.join(publicRoot, ref.slice(1)))) {
      errors.push(`${rel}: missing image ${ref}`);
    }
  }
}

if (errors.length > 0) {
  console.error(errors.join('\n'));
  process.exit(1);
}

console.log('checked docs links');

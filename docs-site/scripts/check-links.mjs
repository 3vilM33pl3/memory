import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

import {
  collectDocsContent,
  contentRoot,
  docsSiteRoot,
  isDocsSiteContent,
  publicPathForReference,
  publicRoot,
  repositoryRoot,
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

function markdownReferences(text) {
  return [...text.matchAll(/!?\[[^\]]*]\(([^)\s]+)(?:\s+"[^"]*")?\)/g)].map(
    (match) => match[1],
  );
}

function isInside(rootPath, candidate) {
  const relative = path.relative(rootPath, candidate);
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..');
}

function validateReadmeReferences() {
  const readme = path.join(repositoryRoot, 'README.md');

  for (const rawRef of markdownReferences(readFileSync(readme, 'utf8'))) {
    if (/^(https?:|mailto:|#)/.test(rawRef)) {
      continue;
    }

    const fileRef = rawRef.split(/[?#]/, 1)[0];
    if (!fileRef) {
      continue;
    }

    const target = path.resolve(repositoryRoot, fileRef);
    if (!isInside(repositoryRoot, target)) {
      errors.push(`README.md: local reference escapes repository: ${rawRef}`);
      continue;
    }
    if (!existsSync(target)) {
      errors.push(`README.md: missing local reference: ${rawRef}`);
    }
  }
}

validateReadmeReferences();

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

console.log('checked docs and README links');

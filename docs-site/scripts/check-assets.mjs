import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

import {
  collectDocsContent,
  docsSiteRoot,
  publicPathForReference,
  publicRoot,
  walk,
} from './content-utils.mjs';

const root = docsSiteRoot;
const imageRoot = path.join(publicRoot, 'images');
const codeRoots = ['app', 'components', 'lib'].map((dir) => path.join(root, dir));
const { records, errors: contentErrors } = collectDocsContent();
const contentFiles = [
  ...records.map(({ file }) => file),
  ...codeRoots
    .filter((dir) => existsSync(dir))
    .flatMap((dir) =>
      walk(dir, (file) => /\.(tsx?|jsx?)$/i.test(file)),
    ),
];
const imageRefs = new Set();
const errors = [...contentErrors];

for (const file of contentFiles) {
  const text = readFileSync(file, 'utf8');
  const rel = path.relative(root, file);
  const regexes = [
    /!\[[^\]]*]\(((?:https:\/\/www\.memory-layer\.dev)?\/images\/[^)\s]+)\)/g,
    /src=["']((?:https:\/\/www\.memory-layer\.dev)?\/images\/[^"']+)["']/g,
  ];

  for (const regex of regexes) {
    for (const match of text.matchAll(regex)) {
      const assetPath = decodeURIComponent(publicPathForReference(match[1]));
      imageRefs.add(assetPath);

      if (!existsSync(path.join(publicRoot, assetPath.slice(1)))) {
        errors.push(`${rel}: missing image ${assetPath}`);
      }
    }
  }
}

const images = walk(imageRoot, (file) => /\.(avif|gif|jpe?g|png|svg|webp)$/i.test(file));

for (const file of images) {
  const publicPath = `/${path.relative(publicRoot, file).replaceAll(path.sep, '/')}`;

  if (!imageRefs.has(publicPath)) {
    errors.push(`public asset is not referenced by MDX: ${publicPath}`);
  }
}

if (errors.length > 0) {
  console.error(errors.join('\n'));
  process.exit(1);
}

console.log(`checked ${imageRefs.size} image references`);

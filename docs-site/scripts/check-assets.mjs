import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';

const root = process.cwd();
const contentRoot = path.join(root, 'content/docs');
const publicRoot = path.join(root, 'public');
const imageRoot = path.join(publicRoot, 'images');
const codeRoots = ['app', 'components', 'lib'].map((dir) => path.join(root, dir));

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

const contentFiles = [
  ...walk(contentRoot, (file) => file.endsWith('.mdx')),
  ...codeRoots
    .filter((dir) => existsSync(dir))
    .flatMap((dir) =>
      walk(dir, (file) => /\.(tsx?|jsx?)$/i.test(file)),
    ),
];
const imageRefs = new Set();
const errors = [];

for (const file of contentFiles) {
  const text = readFileSync(file, 'utf8');
  const rel = path.relative(root, file);
  const regexes = [
    /!\[[^\]]*]\((\/images\/[^)\s]+)\)/g,
    /src=["'](\/images\/[^"']+)["']/g,
  ];

  for (const regex of regexes) {
    for (const match of text.matchAll(regex)) {
      const assetPath = decodeURIComponent(match[1]);
      imageRefs.add(assetPath);

      if (!existsSync(path.join(publicRoot, assetPath))) {
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

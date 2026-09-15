import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';

import {
  collectDocsContent,
  docsSiteRoot,
  includedSpecifiers,
  repositoryRoot,
} from './content-utils.mjs';

const { records, errors } = collectDocsContent();

const sharedGuidePairs = [
  ['content/docs/tui/index.mdx', 'docs/user/tui/README.md'],
  ...[
    'activity',
    'agents',
    'automations',
    'embeddings',
    'errors',
    'memories',
    'project',
    'query',
    'resume',
    'review',
    'skills',
    'watchers',
  ].map((name) => [`content/docs/tui/${name}.mdx`, `docs/user/tui/${name}.md`]),
  ['content/docs/web-ui.mdx', 'docs/user/web-ui.md'],
  ['content/docs/codex-plugin.mdx', 'docs/user/codex-desktop-plugin.md'],
];

for (const [siteRelative, canonicalRelative] of sharedGuidePairs) {
  const siteFile = path.join(docsSiteRoot, siteRelative);
  const canonicalFile = path.join(repositoryRoot, canonicalRelative);
  const label = siteRelative.replaceAll(path.sep, '/');

  if (!existsSync(siteFile)) {
    errors.push(`${label}: missing shared-guide wrapper`);
    continue;
  }
  if (!existsSync(canonicalFile)) {
    errors.push(`${label}: missing canonical source ${canonicalRelative}`);
    continue;
  }

  const includes = includedSpecifiers(readFileSync(siteFile, 'utf8')).map((specifier) =>
    path.resolve(path.dirname(siteFile), specifier.split('#', 1)[0]),
  );
  if (!includes.includes(canonicalFile)) {
    errors.push(`${label}: must include canonical source ${canonicalRelative}`);
  }
}

const workspaceManifest = readFileSync(path.join(repositoryRoot, 'Cargo.toml'), 'utf8');
const workspacePackage = workspaceManifest.match(
  /^\[workspace\.package\][\s\S]*?^version\s*=\s*"([^"]+)"/m,
);

if (!workspacePackage) {
  errors.push('Cargo.toml: could not resolve workspace.package version');
} else {
  const releaseVersion = workspacePackage[1];
  const releaseLabel = `v${releaseVersion}`;
  const docsPackage = JSON.parse(
    readFileSync(path.join(docsSiteRoot, 'package.json'), 'utf8'),
  );

  if (docsPackage.version !== releaseVersion) {
    errors.push(
      `package.json: version ${docsPackage.version} does not match Cargo workspace ${releaseVersion}`,
    );
  }

  const releaseReferences = [
    ['README.md', `[${releaseLabel}]`],
    ['CHANGELOG.md', `## ${releaseVersion} -`],
    [
      'docs/user/release-compatibility.md',
      `Memory Layer ${releaseLabel} is the current stable release.`,
    ],
    ['docs-site/content/docs/index.mdx', `Memory Layer ${releaseLabel} is available.`],
    [
      'docs-site/content/docs/help/known-limitations.mdx',
      `Memory Layer ${releaseLabel} is the current stable release.`,
    ],
    [
      'docs-site/content/docs/install/update.mdx',
      `Memory Layer ${releaseLabel} is the current stable release.`,
    ],
    ['docs-site/content/docs/install/index.mdx', `[${releaseLabel}]`],
    ['docs-site/lib/layout.shared.tsx', `text: '${releaseLabel}'`],
    ['docs-site/components/demo/demo-data.ts', `version: "${releaseVersion}"`],
  ];

  for (const [relativeFile, expectedText] of releaseReferences) {
    const absoluteFile = path.join(repositoryRoot, relativeFile);
    if (!existsSync(absoluteFile)) {
      errors.push(`${relativeFile}: missing release-aware source`);
      continue;
    }
    if (!readFileSync(absoluteFile, 'utf8').includes(expectedText)) {
      errors.push(
        `${relativeFile}: expected current release marker ${JSON.stringify(expectedText)}`,
      );
    }
  }
}

if (errors.length > 0) {
  console.error(errors.join('\n'));
  process.exit(1);
}

console.log(`checked ${records.length} documentation source file(s) and includes`);

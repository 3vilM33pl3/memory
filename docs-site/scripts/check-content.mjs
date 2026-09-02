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

if (errors.length > 0) {
  console.error(errors.join('\n'));
  process.exit(1);
}

console.log(`checked ${records.length} documentation source file(s) and includes`);

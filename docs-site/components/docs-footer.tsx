import { PageFooter } from 'fumadocs-ui/layouts/docs/page';

const footerGroups = [
  {
    title: 'Docs',
    links: [
      { label: 'Install', href: '/docs/install' },
      { label: 'CLI reference', href: '/docs/reference/cli' },
      { label: 'MCP', href: '/docs/mcp' },
      { label: 'Evaluations', href: '/docs/evals' },
    ],
  },
  {
    title: 'Project',
    links: [
      { label: 'GitHub', href: 'https://github.com/3vilM33pl3/memory' },
      { label: 'Releases', href: 'https://github.com/3vilM33pl3/memory/releases' },
      { label: 'Issues', href: 'https://github.com/3vilM33pl3/memory/issues' },
      {
        label: 'Contributing',
        href: 'https://github.com/3vilM33pl3/memory/blob/main/CONTRIBUTING.md',
      },
    ],
  },
  {
    title: 'Legal',
    links: [
      { label: 'AGPL-3.0', href: 'https://github.com/3vilM33pl3/memory/blob/main/LICENSE' },
      {
        label: 'Commercial license',
        href: 'https://github.com/3vilM33pl3/memory/blob/main/LICENSE-COMMERCIAL.md',
      },
      { label: 'Privacy & security', href: '/docs/operations#security-and-privacy' },
    ],
  },
];

function isExternalLink(href: string) {
  return href.startsWith('https://');
}

export function DocsFooter() {
  return (
    <>
      <PageFooter />
      <footer className="mt-10 border-t border-fd-border pt-8 text-sm text-fd-muted-foreground">
        <div className="grid gap-8 md:grid-cols-[minmax(0,1.4fr)_repeat(3,minmax(0,1fr))]">
          <div>
            <p className="font-medium text-fd-foreground">Memory Layer</p>
            <p className="mt-2 max-w-sm">Local-first memory for coding agents.</p>
          </div>
          {footerGroups.map((group) => (
            <div key={group.title}>
              <p className="font-medium text-fd-foreground">{group.title}</p>
              <ul className="mt-3 space-y-2">
                {group.links.map((link) => {
                  const external = isExternalLink(link.href);

                  return (
                    <li key={link.href}>
                      <a
                        href={link.href}
                        className="transition-colors hover:text-fd-foreground"
                        target={external ? '_blank' : undefined}
                        rel={external ? 'noreferrer noopener' : undefined}
                      >
                        {link.label}
                      </a>
                    </li>
                  );
                })}
              </ul>
            </div>
          ))}
        </div>
        <p className="mt-8 border-t border-fd-border pt-5 text-xs">
          &copy; 2026 Olivier Van Acker (3vilM33pl3). Memory Layer is AGPL-3.0-or-later with
          commercial licensing available.
        </p>
      </footer>
    </>
  );
}

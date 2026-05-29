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
      <footer className="mt-8 border-t border-fd-border py-5 text-xs text-fd-muted-foreground">
        <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="font-medium text-fd-foreground">Memory Layer</span>
            <span>Local-first memory for coding agents.</span>
          </div>
          {footerGroups.map((group) => (
            <nav key={group.title} className="flex flex-wrap items-center gap-x-2 gap-y-1">
              <span className="font-medium text-fd-foreground">{group.title}</span>
              <ul className="flex flex-wrap items-center gap-x-2 gap-y-1">
                {group.links.map((link) => {
                  const external = isExternalLink(link.href);

                  return (
                    <li key={link.href} className="flex items-center gap-x-2">
                      <span className="text-fd-muted-foreground/50" aria-hidden="true">
                        /
                      </span>
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
            </nav>
          ))}
        </div>
        <p className="mt-4 border-t border-fd-border pt-4">
          &copy; 2026 Olivier Van Acker (3vilM33pl3). Memory Layer is AGPL-3.0-or-later with
          commercial licensing available.
        </p>
      </footer>
    </>
  );
}

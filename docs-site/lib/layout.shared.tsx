import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';

function GitHubIcon() {
  return (
    <svg
      aria-hidden="true"
      className="h-4 w-4"
      viewBox="0 0 24 24"
      fill="currentColor"
    >
      <path d="M12 .5C5.65.5.5 5.65.5 12c0 5.08 3.29 9.39 7.86 10.92.58.11.79-.25.79-.56v-2.03c-3.2.7-3.88-1.36-3.88-1.36-.52-1.33-1.28-1.68-1.28-1.68-1.05-.72.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.76 2.7 1.25 3.36.96.1-.75.4-1.25.73-1.54-2.55-.29-5.24-1.28-5.24-5.69 0-1.26.45-2.29 1.19-3.1-.12-.29-.52-1.46.11-3.05 0 0 .98-.31 3.18 1.18A11.1 11.1 0 0 1 12 6.15c.98 0 1.97.13 2.89.39 2.2-1.49 3.17-1.18 3.17-1.18.64 1.59.24 2.76.12 3.05.74.81 1.19 1.84 1.19 3.1 0 4.42-2.69 5.4-5.26 5.68.42.36.79 1.08.79 2.18v3c0 .31.21.67.8.56A11.52 11.52 0 0 0 23.5 12C23.5 5.65 18.35.5 12 .5Z" />
    </svg>
  );
}

function ReleaseIcon() {
  return (
    <svg
      aria-hidden="true"
      className="h-4 w-4"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="2"
    >
      <path d="M20.59 13.41 13.42 20.58a2 2 0 0 1-2.83 0L3 13V3h10l7.59 7.59a2 2 0 0 1 0 2.82Z" />
      <path d="M7 7h.01" />
    </svg>
  );
}

function DiscordIcon() {
  return (
    <svg
      aria-hidden="true"
      className="h-4 w-4"
      viewBox="0 0 24 24"
      fill="currentColor"
    >
      <path d="M20.32 4.37A19.8 19.8 0 0 0 15.36 2.8a13.82 13.82 0 0 0-.63 1.31 18.34 18.34 0 0 0-5.46 0 12.84 12.84 0 0 0-.64-1.31 19.74 19.74 0 0 0-4.96 1.58C.54 9.04-.31 13.58.11 18.06a19.9 19.9 0 0 0 6.08 3.09c.49-.67.93-1.38 1.3-2.13-.72-.27-1.41-.6-2.06-.98.17-.13.34-.26.5-.4a14.13 14.13 0 0 0 12.14 0c.16.14.33.27.5.4-.65.38-1.34.71-2.06.98.37.75.81 1.46 1.3 2.13a19.86 19.86 0 0 0 6.08-3.09c.5-5.2-.84-9.7-3.57-13.69ZM8.02 15.31c-1.18 0-2.15-1.08-2.15-2.41s.95-2.42 2.15-2.42 2.17 1.09 2.15 2.42c0 1.33-.95 2.41-2.15 2.41Zm7.96 0c-1.18 0-2.15-1.08-2.15-2.41s.95-2.42 2.15-2.42 2.17 1.09 2.15 2.42c0 1.33-.95 2.41-2.15 2.41Z" />
    </svg>
  );
}

const socialLinks = [
  {
    label: 'GitHub repository',
    text: 'GitHub',
    href: 'https://github.com/3vilM33pl3/memory',
    icon: <GitHubIcon />,
  },
  {
    label: 'GitHub releases',
    text: 'Releases',
    href: 'https://github.com/3vilM33pl3/memory/releases',
    icon: <ReleaseIcon />,
  },
  {
    label: 'Discord',
    text: 'Discord',
    href: 'https://discord.gg/7ynrBfXSfU',
    icon: <DiscordIcon />,
  },
];

export function HeaderSocialLinks() {
  return (
    <nav
      aria-label="Project links"
      className="memory-social-links z-50 flex items-center gap-1 rounded-lg border bg-fd-background/85 p-1 text-fd-muted-foreground shadow-sm backdrop-blur"
    >
      {socialLinks.map((link) => (
        <a
          key={link.href}
          aria-label={link.label}
          className="inline-flex h-8 w-8 items-center justify-center rounded-md transition-colors hover:bg-fd-accent hover:text-fd-accent-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-fd-ring"
          href={link.href}
          rel="noreferrer noopener"
          target="_blank"
          title={link.label}
        >
          {link.icon}
        </a>
      ))}
    </nav>
  );
}

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: (
        <img
          src="/images/memory-layer-logo.png"
          alt="Memory Layer"
          className="h-8 w-8 object-contain"
        />
      ),
    },
    links: socialLinks.map((link) => ({
      type: 'main',
      text: link.text,
      url: link.href,
      external: true,
      icon: link.icon,
    })),
  };
}

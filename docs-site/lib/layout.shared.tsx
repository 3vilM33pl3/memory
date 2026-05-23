import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: 'Memory Layer',
    },
    links: [
      {
        text: 'GitHub',
        url: 'https://github.com/3vilM33pl3/memory',
        external: true,
      },
    ],
  };
}

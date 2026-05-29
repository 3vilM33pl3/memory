import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';

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
    links: [
      {
        text: 'GitHub',
        url: 'https://github.com/3vilM33pl3/memory',
        external: true,
      },
    ],
  };
}

import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: (
        <span className="flex items-center gap-2">
          <img
            src="/images/memory-layer-logo.png"
            alt=""
            className="h-7 w-7 rounded-sm object-cover"
          />
          <span>Memory Layer</span>
        </span>
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

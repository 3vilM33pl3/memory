import './global.css';

import type { Metadata } from 'next';
import type { ReactNode } from 'react';
import { RootProvider } from 'fumadocs-ui/provider/next';

export const metadata: Metadata = {
  title: {
    default: 'Memory Layer Docs',
    template: '%s | Memory Layer Docs',
  },
  description: 'Local-first memory for coding agents.',
  icons: {
    icon: '/images/memory-layer-logo.png',
    apple: '/images/memory-layer-logo.png',
  },
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className="flex min-h-screen flex-col">
        <RootProvider
          theme={{
            defaultTheme: 'system',
            enableSystem: true,
            storageKey: 'memory-layer-theme',
          }}
        >
          {children}
        </RootProvider>
      </body>
    </html>
  );
}

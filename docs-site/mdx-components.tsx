import Link from 'next/link';
import defaultMdxComponents from 'fumadocs-ui/mdx';
import type { MDXComponents } from 'mdx/types';

import { Mermaid } from '@/components/mdx/mermaid';

type CardProps = {
  title: string;
  href?: string;
  children?: React.ReactNode;
};

function CardGroup({
  children,
  cols = 2,
}: {
  children: React.ReactNode;
  cols?: number;
}) {
  return (
    <div
      className="not-prose my-6 grid gap-4"
      style={{
        gridTemplateColumns: `repeat(auto-fit, minmax(${cols >= 3 ? '180px' : '240px'}, 1fr))`,
      }}
    >
      {children}
    </div>
  );
}

function Card({ title, href, children }: CardProps) {
  const content = (
    <div className="rounded-lg border bg-fd-card p-4 text-fd-card-foreground transition-colors hover:bg-fd-accent/50">
      <div className="font-medium">{title}</div>
      {children ? (
        <div className="mt-2 text-sm text-fd-muted-foreground">{children}</div>
      ) : null}
    </div>
  );

  if (!href) {
    return content;
  }

  return (
    <Link className="no-underline" href={href}>
      {content}
    </Link>
  );
}

function Steps({ children }: { children: React.ReactNode }) {
  return <ol className="my-6 space-y-4 [counter-reset:step]">{children}</ol>;
}

function Step({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <li className="relative list-none border-l pl-6 [counter-increment:step]">
      <span className="absolute -left-3 flex h-6 w-6 items-center justify-center rounded-full border bg-fd-background text-xs font-medium before:content-[counter(step)]" />
      <p className="m-0 font-medium">{title}</p>
      <div className="mt-1 text-fd-muted-foreground">{children}</div>
    </li>
  );
}

function Warning({ children }: { children: React.ReactNode }) {
  return (
    <div className="my-6 rounded-lg border border-amber-500/40 bg-amber-500/10 p-4 text-sm">
      {children}
    </div>
  );
}

function PromptBox({ children }: { children: React.ReactNode }) {
  return <div className="memory-prompt-box">{children}</div>;
}

const tuiScreenshots = [
  ['Memories', 'memories-tab.png'],
  ['Query', 'query-tab.png'],
  ['Review', 'review-tab.png'],
  ['Agents', 'agents-tab.png'],
  ['Watchers', 'watchers-tab.png'],
  ['Activity', 'activity-tab.png'],
  ['Resume', 'resume-tab.png'],
  ['Embeddings', 'embeddings-tab.png'],
  ['Errors', 'errors-tab.png'],
  ['Project', 'project-tab.png'],
] as const;

function TuiScreenshotMosaic() {
  return (
    <div className="memory-screenshot-mosaic">
      {tuiScreenshots.map(([label, file]) => {
        const src = `/images/tui/${file}`;
        const slug = label.toLowerCase();

        return (
          <figure key={file}>
            <a href={`#tui-screenshot-${slug}`}>
              <img src={src} alt={`Memory Layer TUI ${label} tab`} />
            </a>
            <figcaption>{label}</figcaption>
            <a
              aria-label={`Return to TUI page from ${label} screenshot`}
              className="memory-screenshot-lightbox"
              href="/docs/tui"
              id={`tui-screenshot-${slug}`}
            >
              <img src={src} alt={`Large Memory Layer TUI ${label} tab screenshot`} />
            </a>
          </figure>
        );
      })}
    </div>
  );
}

export function getMDXComponents(components?: MDXComponents) {
  return {
    ...defaultMdxComponents,
    Card,
    CardGroup,
    Mermaid,
    PromptBox,
    Step,
    Steps,
    TuiScreenshotMosaic,
    Warning,
    ...components,
  } satisfies MDXComponents;
}

export const useMDXComponents = getMDXComponents;

declare global {
  type MDXProvidedComponents = ReturnType<typeof getMDXComponents>;
}

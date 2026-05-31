'use client';

import { useEffect, useState } from 'react';

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

type Screenshot = {
  label: string;
  slug: string;
  src: string;
};

const screenshots: Screenshot[] = tuiScreenshots.map(([label, file]) => ({
  label,
  slug: label.toLowerCase(),
  src: `/images/tui/${file}`,
}));

function selectedFromHash() {
  const slug = window.location.hash.replace(/^#tui-screenshot-/, '');
  return screenshots.find((screenshot) => screenshot.slug === slug) ?? null;
}

export function TuiScreenshotMosaic() {
  const [selected, setSelected] = useState<Screenshot | null>(null);

  useEffect(() => {
    const syncSelected = () => setSelected(selectedFromHash());

    syncSelected();
    window.addEventListener('hashchange', syncSelected);
    return () => window.removeEventListener('hashchange', syncSelected);
  }, []);

  return (
    <div className="memory-screenshot-mosaic">
      {screenshots.map((screenshot) => (
        <figure key={screenshot.slug}>
          <a
            href={`#tui-screenshot-${screenshot.slug}`}
            onClick={() => setSelected(screenshot)}
          >
            <img
              src={screenshot.src}
              alt={`Memory Layer TUI ${screenshot.label} tab`}
            />
          </a>
          <figcaption>{screenshot.label}</figcaption>
        </figure>
      ))}
      {selected ? (
        <a
          aria-label={`Return to TUI page from ${selected.label} screenshot`}
          className="memory-screenshot-lightbox memory-screenshot-lightbox-open"
          href="/docs/tui"
          id={`tui-screenshot-${selected.slug}`}
          role="dialog"
        >
          <img
            src={selected.src}
            alt={`Large Memory Layer TUI ${selected.label} tab screenshot`}
          />
        </a>
      ) : null}
    </div>
  );
}

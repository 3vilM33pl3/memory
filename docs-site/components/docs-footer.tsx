import { PageFooter } from 'fumadocs-ui/layouts/docs/page';

export function DocsFooter() {
  return (
    <>
      <PageFooter />
      <footer className="mt-8 border-t border-fd-border py-4 text-xs text-fd-muted-foreground">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <img
            src="/images/memory-layer-logo.png"
            alt=""
            className="h-5 w-5 shrink-0 object-contain brightness-0 dark:invert"
          />
          <p>
            &copy; 2026 Olivier Van Acker (3vilM33pl3). Memory Layer is AGPL-3.0-or-later with
            commercial licensing available.
          </p>
        </div>
      </footer>
    </>
  );
}

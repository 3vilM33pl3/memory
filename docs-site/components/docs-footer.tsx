import { PageFooter } from 'fumadocs-ui/layouts/docs/page';

export function DocsFooter() {
  return (
    <>
      <PageFooter />
      <footer className="mt-8 border-t border-fd-border py-4 text-xs text-fd-muted-foreground">
        <p>
          &copy; 2026 Olivier Van Acker (3vilM33pl3). Memory Layer is AGPL-3.0-or-later with
          commercial licensing available.
        </p>
      </footer>
    </>
  );
}

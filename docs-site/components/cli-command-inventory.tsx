import Link from 'next/link';

import inventory from '@/data/cli-command-inventory.json';

export function CliCommandInventory() {
  return (
    <section aria-labelledby="cli-command-inventory" className="not-prose my-8">
      <div className="rounded-xl border bg-fd-card p-5 text-fd-card-foreground">
        <h2 id="cli-command-inventory" className="m-0 text-xl font-semibold">
          Current command inventory
        </h2>
        <p className="mt-2 text-sm text-fd-muted-foreground">
          This reference is checked against the CLI&apos;s visible root commands in Rust tests. Use
          command help for flags and subcommands.
        </p>
        <div className="mt-4 overflow-x-auto">
          <table className="w-full min-w-[42rem] text-left text-sm">
            <thead className="border-b text-fd-muted-foreground">
              <tr>
                <th className="px-3 py-2 font-medium">Workflow</th>
                <th className="px-3 py-2 font-medium">Commands</th>
                <th className="px-3 py-2 font-medium">Use when</th>
              </tr>
            </thead>
            <tbody>
              {inventory.groups.map((group) => (
                <tr key={group.title} className="border-b last:border-0">
                  <th scope="row" className="px-3 py-3 align-top font-medium">
                    <Link
                      className="rounded-sm text-fd-foreground underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-fd-ring focus-visible:ring-offset-2 focus-visible:ring-offset-fd-card"
                      href={group.href}
                    >
                      {group.title}
                    </Link>
                  </th>
                  <td className="px-3 py-3 align-top font-mono text-xs leading-6">
                    {group.commands.join(', ')}
                  </td>
                  <td className="px-3 py-3 align-top text-fd-muted-foreground">
                    {group.description}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <details className="mt-4 text-sm">
          <summary className="cursor-pointer rounded-sm font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-fd-ring focus-visible:ring-offset-2 focus-visible:ring-offset-fd-card">
            Show all {inventory.visibleRootCommands.length} visible root commands
          </summary>
          <p className="mt-3 rounded-md bg-fd-muted px-3 py-2 font-mono text-xs leading-6 text-fd-muted-foreground">
            {inventory.visibleRootCommands.join(' · ')}
          </p>
        </details>
      </div>
    </section>
  );
}

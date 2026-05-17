import type { ReactNode } from "react";

export function RichText({ text }: { text: string }) {
  const lines = text.split(/\r?\n/);
  const blocks: ReactNode[] = [];
  let listItems: string[] = [];
  let codeLines: string[] = [];
  let inCode = false;

  const flushList = () => {
    if (!listItems.length) return;
    const items = listItems;
    listItems = [];
    blocks.push(<ul key={`list-${blocks.length}`} className="rich-list">{items.map((item) => <li key={item}>{item}</li>)}</ul>);
  };
  const flushCode = () => {
    if (!codeLines.length) return;
    const code = codeLines.join("\n");
    codeLines = [];
    blocks.push(<pre key={`code-${blocks.length}`}>{code}</pre>);
  };

  for (const raw of lines) {
    const line = raw.trimEnd();
    if (line.trim().startsWith("```")) {
      if (inCode) {
        inCode = false;
        flushCode();
      } else {
        flushList();
        inCode = true;
      }
      continue;
    }
    if (inCode) {
      codeLines.push(line);
      continue;
    }
    if (!line.trim()) {
      flushList();
      continue;
    }
    const heading = line.match(/^(#{1,4})\s+(.+)$/);
    if (heading) {
      flushList();
      blocks.push(<h4 key={`heading-${blocks.length}`}>{heading[2]}</h4>);
      continue;
    }
    const bullet = line.match(/^\s*[-*]\s+(.+)$/);
    if (bullet) {
      listItems.push(bullet[1]);
      continue;
    }
    flushList();
    blocks.push(<p key={`p-${blocks.length}`}>{line}</p>);
  }
  flushList();
  flushCode();

  return <div className="rich-text">{blocks.length ? blocks : <p className="muted">No text.</p>}</div>;
}

import type { Metadata } from "next";
import Link from "next/link";

import { WebUiDemoApp } from "@/components/demo/demo-app";

export const metadata: Metadata = {
  title: "Web UI Demo",
  description: "Explore a self-contained Memory Layer Web UI demo with static snapshot data.",
};

export default function DemoPage() {
  return (
    <div className="demo-page">
      <div className="demo-page-nav">
        <Link href="/docs">Docs</Link>
        <Link href="/docs/web-ui">Browser UI guide</Link>
        <a href="https://github.com/3vilM33pl3/memory" rel="noreferrer noopener" target="_blank">
          GitHub
        </a>
      </div>
      <WebUiDemoApp />
    </div>
  );
}

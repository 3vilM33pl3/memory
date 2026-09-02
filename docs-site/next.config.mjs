import { createMDX } from 'fumadocs-mdx/next';
import { fileURLToPath } from 'node:url';

const repositoryRoot = fileURLToPath(new URL('..', import.meta.url));

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  turbopack: {
    // The canonical user guides live next to docs-site, and Fumadocs includes
    // them while compiling the thin route wrappers.
    root: repositoryRoot,
  },
};

const withMDX = createMDX();

export default withMDX(config);

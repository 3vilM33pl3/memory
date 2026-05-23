import { createMDX } from 'fumadocs-mdx/next';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('.', import.meta.url));

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  turbopack: {
    root,
  },
};

const withMDX = createMDX();

export default withMDX(config);

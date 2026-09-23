export const appName = 'uze';
// The hero's own line. It is the site's title too: a browser tab reading just
// "uze" says nothing to someone with twenty tabs open.
export const appTagline = 'Agents come and go, your work stays';
export const appDescription =
  'A compatibility and distribution layer for agent tooling: one plugin and one AGENTS.md reach every harness natively, and one terminal runs them side by side.';
// The deployment sets no NEXT_PUBLIC_SITE_URL, and a localhost fallback went
// out as every page's og:image; the variable only needs setting to preview elsewhere.
export const siteUrl = process.env.NEXT_PUBLIC_SITE_URL ?? 'https://uze.hiukky.com';
export const docsRoute = '/docs';
export const docsImageRoute = '/og/docs';
export const docsContentRoute = '/llms.mdx/docs';

export const gitConfig = {
  user: 'hiukky',
  repo: 'uze',
  branch: 'main',
};
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { createMDX } from 'fumadocs-mdx/next';

const withMDX = createMDX();

// The one version source is `[workspace.package].version` in the root
// Cargo.toml, which every crate inherits. Read at build time so the header
// badge cannot drift from the binary it names.
// `version.workspace = true` in [package] has a dot before the `=`, so the
// first thing this matches is [workspace.package]'s literal value.
// Missing entirely (a checkout of `web/` alone) just drops the badge.
let version = '';
try {
  const cargoToml = readFileSync(fileURLToPath(new URL('../Cargo.toml', import.meta.url)), 'utf8');
  version = cargoToml.match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1] ?? '';
} catch {
  version = '';
}

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  env: {
    NEXT_PUBLIC_UZE_VERSION: version,
  },
  // A published URL is a promise to whoever linked it. `theming` became
  // `appearance` when appearance stopped being one choice — the palette and
  // the glyph set are chosen apart now, and only one of them is a theme.
  async redirects() {
    return [{ source: '/docs/theming', destination: '/docs/appearance', permanent: true }];
  },
};

export default withMDX(config);

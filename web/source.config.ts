import { defineConfig } from 'fumadocs-mdx/config';
import { remarkMdxMermaid } from 'fumadocs-core/mdx-plugins';

// Additive: merged into fumadocs' default remark/rehype pipeline (GFM
// tables, headings, etc.), not a replacement — see fumadocs-mdx's
// GlobalConfig.mdxOptions vs. the per-collection option in lib/source.ts,
// which *does* replace the defaults and is deliberately left untouched.
export default defineConfig({
  mdxOptions: {
    remarkPlugins: [remarkMdxMermaid],
  },
});

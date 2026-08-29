'use client';

import { useEffect, useId, useState } from 'react';
import { useTheme } from 'next-themes';

function cssVar(name: string, fallback: string) {
  if (typeof window === 'undefined') return fallback;
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
}

export function Mermaid({ chart }: { chart: string }) {
  const id = useId().replace(/[^a-zA-Z0-9]/g, '');
  const [svg, setSvg] = useState<string | null>(null);
  const { resolvedTheme } = useTheme();

  useEffect(() => {
    let cancelled = false;

    void import('mermaid').then(async ({ default: mermaid }) => {
      const dark = resolvedTheme === 'dark';
      mermaid.initialize({
        startOnLoad: false,
        theme: 'base',
        fontFamily: 'var(--font-ui-mono)',
        themeVariables: {
          fontSize: '13px',
          background: cssVar('--color-paper', dark ? '#0a0c0d' : '#f2f0ea'),
          primaryColor: cssVar('--color-surface', dark ? '#161e1a' : '#e5ece6'),
          primaryBorderColor: cssVar('--color-line', dark ? '#1e1f20' : '#ddd8cd'),
          primaryTextColor: cssVar('--color-ink', dark ? '#f2f0ea' : '#0a0c0d'),
          lineColor: cssVar('--color-accent', dark ? '#8fd19e' : '#3d7a52'),
          textColor: cssVar('--color-ink', dark ? '#f2f0ea' : '#0a0c0d'),
        },
      });
      const { svg: rendered } = await mermaid.render(`mermaid-${id}`, chart);
      if (!cancelled) setSvg(rendered);
    });

    return () => {
      cancelled = true;
    };
  }, [chart, id, resolvedTheme]);

  if (!svg) {
    return (
      <div className="my-4 rounded-md border border-fd-border p-4 text-sm text-fd-muted-foreground">
        Rendering diagram…
      </div>
    );
  }

  return (
    // biome-ignore lint: mermaid output is trusted, generated at build/render time from our own MDX
    <div className="my-4 flex justify-center [&_svg]:max-w-full" dangerouslySetInnerHTML={{ __html: svg }} />
  );
}

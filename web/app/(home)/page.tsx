import Link from 'next/link';

const harnesses = [
  { name: 'Claude Code', delivery: 'native plugin' },
  { name: 'Codex', delivery: 'native plugin' },
  { name: 'OpenCode', delivery: 'native skills + bridge' },
  { name: 'Antigravity', delivery: 'native plugin' },
];

const rowY = [30, 104, 178, 252];

const spec = [
  {
    term: 'STORE',
    title: 'One store, four surfaces',
    body: 'Plugin bytes live once in the Store. Each harness receives them through its own most native mechanism — a real plugin where one exists, a safe adapter only as a last resort.',
    href: '/docs/concepts',
  },
  {
    term: 'SEMANTICS',
    title: 'Semantics survive delivery',
    body: 'Invocation policy, hooks, and capabilities are canonical. Integrations translate the policy into each vendor’s encoding instead of dropping it.',
    href: '/docs/concepts/capabilities',
  },
  {
    term: 'CONTEXT',
    title: 'One project context',
    body: 'AGENTS.md is the portable baseline. Every harness reads it natively or through a managed bridge — never four instruction files to maintain.',
    href: '/docs/concepts/context',
  },
];

export default function HomePage() {
  return (
    <main className="flex flex-col items-center flex-1 px-6 font-sans">
      {/* Hero */}
      <section className="grid md:grid-cols-[1fr_1fr] gap-12 md:gap-8 items-center w-full max-w-4xl py-20 md:py-28">
        <div>
          <div className="inline-flex items-center gap-2 text-[11px] font-mono uppercase tracking-[0.2em] text-muted">
            <span className="size-1.5 bg-accent" aria-hidden />
            Rust CLI — alpha
          </div>
          <h1 className="mt-5 font-mono font-bold leading-[1.05] tracking-tight text-ink text-4xl sm:text-5xl">
            Install once.
            <br />
            <span className="text-accent">Native everywhere.</span>
          </h1>
          <p className="mt-6 max-w-md text-[15px] leading-relaxed text-muted">
            uze is a compatibility and distribution layer for agent tooling. A single plugin and
            project context, delivered to Claude Code, Codex, OpenCode, and Antigravity through
            each harness&apos;s own native surface.
          </p>
          <div className="flex flex-row flex-wrap gap-3 mt-8">
            <Link
              href="/docs"
              className="inline-flex items-center gap-2 px-5 py-2.5 rounded-[3px] text-xs font-mono font-medium uppercase tracking-[0.12em] bg-fd-primary text-fd-primary-foreground hover:opacity-90 transition-opacity"
            >
              Read the docs <span aria-hidden>→</span>
            </Link>
            <Link
              href="https://github.com/hiukky/uze"
              className="inline-flex items-center gap-2 px-5 py-2.5 rounded-[3px] text-xs font-mono font-medium uppercase tracking-[0.12em] border border-line text-ink hover:bg-surface transition-colors"
            >
              <svg viewBox="0 0 16 16" className="size-3.5" fill="currentColor" aria-hidden="true">
                <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
              </svg>
              Source
            </Link>
          </div>
        </div>

        {/* Signature diagram — one canonical package, four native surfaces */}
        <div className="flex flex-col gap-2">
          <svg
            viewBox="0 0 480 300"
            className="uze-diagram w-full h-auto max-w-md mx-auto"
            role="img"
            aria-label="uze routes one plugin package to Claude Code, Codex, OpenCode, and Antigravity, each through its own native surface"
          >
            {/* corner ticks */}
            <path d="M0 10 V0 H10" fill="none" stroke="var(--color-line)" strokeWidth="1" />
            <path
              d="M470 290 V300 H460"
              fill="none"
              stroke="var(--color-line)"
              strokeWidth="1"
            />

            {/* source node */}
            <rect
              x="4"
              y="125"
              width="100"
              height="32"
              rx="2"
              fill="none"
              stroke="var(--color-line)"
            />
            <text
              x="54"
              y="145"
              textAnchor="middle"
              fontFamily="var(--font-mono)"
              fontWeight="700"
              fontSize="12"
              fill="var(--color-ink)"
            >
              STORE
            </text>
            <text
              x="54"
              y="170"
              textAnchor="middle"
              fontFamily="var(--font-mono)"
              fontSize="7.5"
              letterSpacing="0.5"
              fill="var(--color-muted)"
            >
              PLUGIN BYTES
            </text>

            {/* trunk + spine + branches */}
            <line
              className="uze-line"
              x1="104"
              y1="141"
              x2="170"
              y2="141"
              stroke="var(--color-line)"
              strokeWidth="1.5"
              pathLength={1}
            />
            <line
              className="uze-line"
              x1="170"
              y1="30"
              x2="170"
              y2="252"
              stroke="var(--color-line)"
              strokeWidth="1.5"
              pathLength={1}
            />
            {rowY.map((y) => (
              <line
                key={y}
                className="uze-line"
                x1="170"
                y1={y}
                x2="250"
                y2={y}
                stroke="var(--color-line)"
                strokeWidth="1.5"
                pathLength={1}
              />
            ))}

            <circle className="uze-dot" cx="170" cy="141" r="3" fill="var(--color-accent)" />

            {/* leaf nodes */}
            {harnesses.map((h, i) => {
              const y = rowY[i];
              return (
                <g key={h.name}>
                  <rect
                    x="250"
                    y={y - 16}
                    width="210"
                    height="32"
                    rx="2"
                    fill="none"
                    stroke="var(--color-line)"
                  />
                  <text
                    x="262"
                    y={y - 2}
                    fontFamily="var(--font-mono)"
                    fontWeight="700"
                    fontSize="11"
                    fill="var(--color-ink)"
                  >
                    {h.name}
                  </text>
                  <text
                    x="262"
                    y={y + 11}
                    fontFamily="var(--font-mono)"
                    fontSize="7.5"
                    letterSpacing="0.5"
                    fill="var(--color-muted)"
                  >
                    {h.delivery.toUpperCase()}
                  </text>
                  <circle className="uze-dot" cx="250" cy={y} r="3" fill="var(--color-accent)" />
                </g>
              );
            })}
          </svg>
          <p className="text-[11px] font-mono uppercase tracking-[0.15em] text-muted text-right max-w-md mx-auto w-full">
            detected + verified per harness
          </p>
        </div>
      </section>

      {/* Spec rows */}
      <section className="w-full max-w-3xl border-t border-line pb-24">
        {spec.map((item) => (
          <Link
            key={item.term}
            href={item.href}
            className="group flex flex-col sm:flex-row gap-2 sm:gap-8 py-6 px-2 -mx-2 border-b border-line hover:bg-surface/60 transition-colors"
          >
            <span className="shrink-0 sm:w-32 font-mono text-xs uppercase tracking-[0.15em] text-accent pt-0.5">
              {item.term}
            </span>
            <span className="flex-1">
              <h2 className="font-mono font-semibold text-ink">{item.title}</h2>
              <p className="mt-1.5 text-sm text-muted leading-relaxed">{item.body}</p>
            </span>
            <span
              className="shrink-0 self-center font-mono text-muted group-hover:text-accent group-hover:translate-x-1 transition-all"
              aria-hidden
            >
              →
            </span>
          </Link>
        ))}
      </section>
    </main>
  );
}

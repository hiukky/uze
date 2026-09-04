import Link from 'next/link';
import { Check } from 'lucide-react';
import { InstallCommand } from '@/components/install-command';
import { TrademarkNotice } from '@/components/trademark-notice';
import matrix from '@/lib/harness-matrix.json';

type Capability = 'context' | 'skills' | 'mcp' | 'agents' | 'hooks' | 'package';

const columns: [Capability, string][] = [
  ['context', 'AGENTS.md'],
  ['skills', 'Skills'],
  ['mcp', 'MCP'],
  ['agents', 'Agents'],
  ['hooks', 'Hooks'],
  ['package', 'Plugin'],
];

// The question a reader is actually asking of this table is "does it work
// here", so that is what the mark answers: every delivered route gets the
// same check, and the route word beside it is the detail. Dimming `bridge`
// and `adapted` against `native` answered a different question and made two
// working routes look broken.
function Route({ value }: { value: string }) {
  if (value === 'none') {
    return (
      <span className="font-mono text-xs text-muted/60" title="not delivered through this route">
        —
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-1.5 font-mono text-xs whitespace-nowrap text-ink">
      <Check className="size-3.5 shrink-0 text-accent" aria-hidden strokeWidth={3} />
      {value}
    </span>
  );
}

// Icon sources: Claude Code / OpenCode are simple-icons paths (recolored to
// the theme, since an <image>-embedded SVG renders in its own document and
// can't inherit currentColor); Codex / Antigravity have no distinct mark of
// their own — these are OpenAI's and Google Antigravity's actual favicons,
// fetched from their own sites (public/harnesses/, not redistributed by a
// third party), shown at their real brand colors.
const harnesses = [
  {
    name: 'Claude Code',
    icon: {
      type: 'path' as const,
      d: 'M21 10.5h3v3h-3v3h-1.5v3H18v-3h-1.5v3H15v-3H9v3H7.5v-3H6v3H4.5v-3H3v-3H0v-3h3v-6h18Zm-15 0h1.5v-3H6Zm10.5 0H18v-3h-1.5z',
    },
  },
  { name: 'Codex', icon: { type: 'image' as const, href: '/harnesses/codex.png' } },
  {
    name: 'OpenCode',
    icon: { type: 'path' as const, d: 'M22 24H2V0h20zM17 4.8H7v14.4h10z' },
  },
  {
    name: 'Antigravity',
    icon: { type: 'image' as const, href: '/harnesses/antigravity.png' },
  },
];

const pillars = [
  {
    title: 'One package, four native surfaces',
    body: 'The Store owns a plugin’s bytes and writes nothing a harness reads. Each integration delivers them through the most native mechanism that harness has — a real plugin where one exists, a safe adapter only as a last resort.',
    href: '/docs/concepts',
    link: 'How delivery is decided',
  },
  {
    title: 'Semantics survive the trip',
    body: 'A skill’s invocation policy, a hook’s effect, an agent’s frontmatter — each is translated into the vendor’s own encoding, or reported as adapted. uze never claims a route is native without a passing real-harness scenario.',
    href: '/docs/concepts/capabilities',
    link: 'What travels, and how',
  },
  {
    title: 'One project context',
    body: 'AGENTS.md is the portable baseline. Every harness reads it natively or through the one bridge uze maintains, inside regions it owns — never four instruction files drifting apart.',
    href: '/docs/concepts/context',
    link: 'How context reaches each harness',
  },
  {
    title: 'Agents that don’t collide',
    body: 'Run several at once in one terminal. Each starts in an isolated checkout on a branch of its own, readiness is read from Git rather than announced, and finished work comes home through a delivery you trigger.',
    href: '/docs/workspace',
    link: 'Inside the workspace',
  },
];

export default function HomePage() {
  return (
    <main className="flex flex-col items-center flex-1 px-6 font-sans">
      {/* Hero. Holds the first screen on its own — the banner and the h-14
          header are the only chrome above it — so the recording below is
          something you arrive at by scrolling, not something competing with
          the headline for the same view. */}
      <section className="flex w-full max-w-5xl flex-col justify-center min-h-[calc(100dvh_-_var(--uze-banner-height)_-_3.5rem)] py-14 text-center">
        <h1 className="mx-auto max-w-[18ch] font-mono font-bold tracking-tight text-ink text-[2.5rem] leading-[1.02] sm:text-6xl lg:text-[4.25rem]">
          Install once.
          <br />
          <span className="text-accent">Native everywhere.</span>
        </h1>
        <p className="mx-auto mt-6 max-w-[58ch] text-lg leading-relaxed text-muted">
          One plugin and one project context reach Claude Code, Codex, OpenCode and Antigravity
          through each one&apos;s own native surface — and one terminal runs them side by side,
          each agent in a checkout of its own.
        </p>

        <div className="mx-auto mt-9 flex max-w-xl flex-col items-stretch gap-3 sm:flex-row">
          <div className="flex-1 text-left">
            <InstallCommand command="curl -fsSL https://uze.hiukky.com/i | sh" />
          </div>
          <Link
            href="/docs/getting-started"
            className="inline-flex shrink-0 items-center justify-center border border-ink bg-ink px-5 py-2.5 font-mono text-[13px] text-paper transition-opacity hover:opacity-85"
          >
            Get started
          </Link>
        </div>
        <p className="mt-3 text-xs text-muted">
          Linux, x86_64 or aarch64, checksum verified.{' '}
          <Link href="/docs/getting-started" className="text-ink underline underline-offset-4 hover:text-accent transition-colors">
            Build from source
          </Link>{' '}
          on anything else.
        </p>

      </section>

      {/* The recording. `prefers-reduced-motion` gets a still frame instead,
          and <source media> means only the matched file is ever fetched. */}
      <section className="w-full max-w-6xl pt-12 pb-24 sm:pb-28">
        <figure className="m-0">
          <div className="uze-demo-frame border border-line" style={{ background: '#0a0c0d' }}>
            <picture>
              <source srcSet="/uze-demo-poster.png" media="(prefers-reduced-motion: reduce)" />
              <img
                src="/uze-demo.gif"
                width={1298}
                height={725}
                alt="The uze terminal: one project running a Claude Code agent and an OpenCode agent at once, each on its own branch in its own checkout, with the commit timeline of one beside them."
                className="block h-auto w-full"
              />
            </picture>
          </div>
          <figcaption className="mt-3 text-center font-mono text-xs text-muted">
            Run <span className="text-ink">uze</span> with no arguments. Ctrl+O switches between the
            workspace and the machine view.
          </figcaption>
        </figure>
      </section>

      {/* Who it delivers to. */}
      <section className="w-full max-w-5xl border-t border-line py-20 sm:py-24">
        <h2 className="text-center font-mono text-xs text-muted">Delivers natively to</h2>
        <ul className="mt-10 grid grid-cols-2 gap-x-6 gap-y-10 sm:grid-cols-4">
          {harnesses.map((harness) => (
            <li key={harness.name} className="flex flex-col items-center gap-2.5 text-center">
              <svg viewBox="0 0 24 24" className="size-7" aria-hidden>
                {harness.icon.type === 'path' ? (
                  <path d={harness.icon.d} fill="var(--color-ink)" />
                ) : (
                  <image href={harness.icon.href} width="24" height="24" />
                )}
              </svg>
              <span className="font-mono text-sm font-semibold text-ink">{harness.name}</span>
            </li>
          ))}
        </ul>
      </section>

      {/* What it actually does. */}
      <section className="w-full max-w-5xl border-t border-line py-20 sm:py-24">
        <ul className="grid gap-x-16 gap-y-14 sm:grid-cols-2">
          {pillars.map((pillar) => (
            <li key={pillar.title}>
              <h3 className="font-mono text-lg font-semibold leading-snug text-ink">{pillar.title}</h3>
              <p className="mt-2.5 text-sm leading-relaxed text-muted">{pillar.body}</p>
              <Link
                href={pillar.href}
                className="mt-3.5 inline-block border-b border-accent/50 pb-0.5 font-mono text-xs text-ink transition-colors hover:border-accent hover:text-accent"
              >
                {pillar.link}
              </Link>
            </li>
          ))}
        </ul>
      </section>

      {/* What each harness receives. Generated from the integration code
          (web/lib/harness-matrix.json) — the same source the docs matrix is
          built from, so the landing page cannot claim a route the code
          stopped taking. */}
      <section className="w-full max-w-5xl border-t border-line py-20 sm:py-24">
        <h2 className="font-mono font-semibold text-ink">What each harness receives</h2>
        <p className="mt-1.5 max-w-[68ch] text-sm leading-relaxed text-muted">
          A check means the capability is delivered. The word beside it is how:{' '}
          <span className="text-ink">native</span> through the harness&apos;s own mechanism,{' '}
          <span className="text-ink">bridge</span> or <span className="text-ink">adapted</span>{' '}
          where uze preserves the semantics another way and reports that it did. Every route is
          derived from the integration that implements it.
        </p>

        <div className="mt-10 overflow-x-auto">
          <table className="w-full min-w-[38rem] border-collapse text-left">
            <thead>
              <tr className="border-b border-line">
                <th className="py-3 pe-4 font-mono text-xs font-normal text-muted">Harness</th>
                {columns.map(([key, label]) => (
                  <th key={key} className="px-3 py-3 font-mono text-xs font-normal text-muted">
                    {label}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {matrix.harnesses.map((harness) => (
                <tr key={harness.name} className="border-b border-line/70">
                  <th scope="row" className="py-4 pe-4 font-normal">
                    <span className="flex items-center gap-2.5">
                      {harness.icon ? (
                        <img src={harness.icon} alt="" className="size-4 shrink-0 rounded-[2px]" />
                      ) : null}
                      <span className="font-mono text-sm font-semibold text-ink">
                        {harness.name}
                      </span>
                    </span>
                  </th>
                  {columns.map(([key]) => (
                    <td key={key} className="px-3 py-4">
                      <Route value={harness[key]} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <p className="mt-8 text-center text-sm text-muted">
          A dash means that route does not exist for the harness, and the capability arrives another
          way. {matrix.planned.join(', ')} are on the roadmap — cells appear when the integration
          lands.{' '}
          <Link
            href="/docs/harnesses"
            className="text-ink underline underline-offset-4 hover:text-accent transition-colors"
          >
            The full matrix, per capability
          </Link>
          .
        </p>
      </section>

      <section className="w-full max-w-5xl border-t border-line py-24 sm:py-28 text-center">
        <h2 className="font-mono text-2xl font-bold tracking-tight text-ink">
          Set it up once, on this machine.
        </h2>
        <p className="mx-auto mt-3 max-w-[52ch] text-sm leading-relaxed text-muted">
          uze detects the coding agents you already have, provisions the ones you don&apos;t through
          each vendor&apos;s own installer, and reports what it could not do rather than guessing.
        </p>
        <div className="mt-10 flex flex-wrap items-center justify-center gap-4 font-mono text-xs">
          <Link
            href="/docs/getting-started"
            className="border border-ink bg-ink px-5 py-2.5 text-paper transition-opacity hover:opacity-85"
          >
            Get started
          </Link>
          <Link
            href="/docs/creating-a-plugin"
            className="border border-line px-5 py-2.5 text-ink transition-colors hover:bg-surface"
          >
            Write a plugin
          </Link>
          <Link
            href="https://github.com/hiukky/uze"
            className="inline-flex items-center gap-2 border border-line px-5 py-2.5 text-ink transition-colors hover:bg-surface"
          >
            <svg viewBox="0 0 16 16" className="size-3.5" fill="currentColor" aria-hidden="true">
              <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
            </svg>
            Browse the source
          </Link>
        </div>
      </section>

      <footer className="w-full max-w-5xl border-t border-line py-14 text-center">
        <p className="inline-flex items-center gap-2 text-[11px] font-mono text-muted">
          <span className="size-1.5 bg-accent" aria-hidden />
          Built with 🖤 by{' '}
          <a href="https://hiukky.com" className="text-ink hover:text-accent transition-colors">
            Romullo (@hiukky)
          </a>
        </p>
        <div className="mt-6">
          <TrademarkNotice />
        </div>
      </footer>
    </main>
  );
}

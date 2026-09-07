import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import { appName, gitConfig } from './shared';

// Injected from the workspace Cargo.toml at build time (see next.config.mjs).
const version = process.env.NEXT_PUBLIC_UZE_VERSION;

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: (
        <span className="font-mono font-semibold tracking-tight text-fd-foreground">
          <svg
            viewBox="0 0 32 32"
            className="mr-2 inline-block size-[0.75em] align-middle text-accent"
            aria-hidden
          >
            <path d="M16 2 L30 16 L16 30 L2 16 Z" fill="currentColor" />
            <path
              d="M3 3 L29 29 M29 3 L3 29"
              stroke="var(--color-paper)"
              strokeWidth="2.6"
            />
          </svg>
          {appName}
          {version ? (
            <span className="ml-2 text-[11px] font-normal text-fd-muted-foreground">
              v{version}
            </span>
          ) : null}
        </span>
      ),
    },
    githubUrl: `https://github.com/${gitConfig.user}/${gitConfig.repo}`,
  };
}

import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import { appName, gitConfig } from './shared';

// Injected from the workspace Cargo.toml at build time (see next.config.mjs).
const version = process.env.NEXT_PUBLIC_UZE_VERSION;

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: (
        <span className="inline-flex items-baseline gap-2 font-mono font-semibold tracking-tight text-fd-foreground">
          <span className="size-1.5 self-center bg-accent" aria-hidden />
          {appName}
          {version ? (
            <span className="text-[11px] font-normal text-fd-muted-foreground">v{version}</span>
          ) : null}
        </span>
      ),
    },
    githubUrl: `https://github.com/${gitConfig.user}/${gitConfig.repo}`,
  };
}

import { RootProvider } from 'fumadocs-ui/provider/next';
import { Banner } from 'fumadocs-ui/components/banner';
import './global.css';
import localFont from 'next/font/local';
import type { Metadata } from 'next';
import { appDescription, appName, appTagline, siteUrl } from '@/lib/shared';

// IBM's own release of Plex, not Google's: Google serves an unhinted build to
// any client but a Windows browser, and `next/font/google` fetches at build
// time — so every visitor got the unhinted outlines, which Windows renders
// with strokes eaten away. IBM's woff2 carry their hinting. OFL-1.1, beside them.
const plexSans = localFont({
  src: [
    { path: './fonts/IBMPlexSans-Regular.woff2', weight: '400' },
    { path: './fonts/IBMPlexSans-Medium.woff2', weight: '500' },
    { path: './fonts/IBMPlexSans-SemiBold.woff2', weight: '600' },
  ],
  variable: '--font-body',
});

const plexMono = localFont({
  src: [
    { path: './fonts/IBMPlexMono-Regular.woff2', weight: '400' },
    { path: './fonts/IBMPlexMono-Medium.woff2', weight: '500' },
    { path: './fonts/IBMPlexMono-SemiBold.woff2', weight: '600' },
    { path: './fonts/IBMPlexMono-Bold.woff2', weight: '700' },
  ],
  variable: '--font-ui-mono',
});

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title: {
    default: `${appName} · ${appTagline}`,
    template: `%s · ${appName}`,
  },
  description: appDescription,
};

export default function Layout({ children }: LayoutProps<'/'>) {
  return (
    <html
      lang="en"
      className={`${plexSans.variable} ${plexMono.variable} scrollbar-thin scrollbar-thumb-muted scrollbar-track-transparent scrollbar-thumb-rounded-full`}
      suppressHydrationWarning
    >
      <body className="flex flex-col min-h-screen font-sans">
        <Banner
          id="alpha-2026-09"
          height="var(--uze-banner-height)"
          className="gap-x-2 px-12 text-center font-mono text-xs tracking-tight"
        >
          <span className="text-accent">Alpha</span>
          <span className="text-fd-muted-foreground">·</span>
          {/* One line at every width: the banner's height feeds the docs
              grid's sticky offsets, so text that wraps is text that gets
              clipped. */}
          <span className="sm:hidden">every release is a pre-release</span>
          <span className="max-sm:hidden">APIs and harness behavior are still changing</span>
          <span className="text-fd-muted-foreground max-sm:hidden">·</span>
          <span className="max-sm:hidden">every release until v1 is a pre-release</span>
        </Banner>
        <RootProvider>{children}</RootProvider>
      </body>
    </html>
  );
}

import { RootProvider } from 'fumadocs-ui/provider/next';
import { Banner } from 'fumadocs-ui/components/banner';
import './global.css';
import { IBM_Plex_Mono, IBM_Plex_Sans } from 'next/font/google';
import type { Metadata } from 'next';
import { appDescription, appName } from '@/lib/shared';

const plexSans = IBM_Plex_Sans({
  subsets: ['latin'],
  weight: ['400', '500', '600'],
  variable: '--font-body',
});

const plexMono = IBM_Plex_Mono({
  subsets: ['latin'],
  weight: ['400', '500', '700'],
  variable: '--font-ui-mono',
});

export const metadata: Metadata = {
  metadataBase: new URL(process.env.NEXT_PUBLIC_SITE_URL ?? 'http://localhost:3000'),
  title: {
    default: appName,
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
          id="alpha-2026-08"
          height="auto"
          className="relative flex-wrap gap-x-2 gap-y-1 px-12 py-2.5 text-center font-mono text-xs tracking-tight"
        >
          <span className="text-accent">Alpha</span>
          <span className="mx-2 text-fd-muted-foreground">·</span>
          APIs and harness behavior are still changing
          <span className="mx-2 hidden text-fd-muted-foreground sm:inline">·</span>
          <span className="hidden sm:inline">no packaged installer yet, build from source</span>
        </Banner>
        <RootProvider>{children}</RootProvider>
      </body>
    </html>
  );
}

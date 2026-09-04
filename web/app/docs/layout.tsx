import { source } from '@/lib/source';
import { DocsLayout } from 'fumadocs-ui/layouts/docs';
import { baseOptions } from '@/lib/layout.shared';
import { TrademarkNotice } from '@/components/trademark-notice';

export default function Layout({ children }: LayoutProps<'/docs'>) {
  return (
    <DocsLayout tree={source.getPageTree()} {...baseOptions()}>
      {children}
      <footer className="border-t border-line px-4 py-8 text-center">
        <TrademarkNotice />
      </footer>
    </DocsLayout>
  );
}

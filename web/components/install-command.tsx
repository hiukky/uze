'use client';

import { useEffect, useState } from 'react';

export function InstallCommand({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 2000);
    return () => clearTimeout(timer);
  }, [copied]);

  async function copy() {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
    } catch {
      // Clipboard access can be refused (insecure origin, denied permission).
      // The command is selectable text either way, so there is nothing to
      // recover from and nothing worth interrupting the reader about.
    }
  }

  return (
    <div className="flex items-stretch border border-line bg-surface/40 font-mono text-[12.5px]">
      {/* Wraps rather than scrolls: a horizontal scrollbar under a command
          is a thing to fight, and the copy button already removes the reason
          to select it by hand. */}
      <code className="flex-1 px-3 py-2.5 leading-6 text-ink whitespace-pre-wrap break-words">
        <span className="text-accent select-none">$ </span>
        {command}
      </code>
      <button
        type="button"
        onClick={copy}
        className="shrink-0 border-s border-line px-3 text-xs text-muted hover:text-ink focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-accent transition-colors"
      >
        {copied ? 'copied' : 'copy'}
      </button>
    </div>
  );
}

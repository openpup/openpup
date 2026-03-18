import React, { useEffect, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import 'highlight.js/styles/github-dark.css';
import { invoke } from '@tauri-apps/api/core';

// ── Text extraction helper ─────────────────────────────────────────────────────
// rehype-highlight converts code children into React element trees (spans).
// String() on those gives "[object Object]" — we need to walk the tree.

function nodeText(node: React.ReactNode): string {
  if (node == null) return '';
  if (typeof node === 'string' || typeof node === 'number') return String(node);
  if (Array.isArray(node)) return node.map(nodeText).join('');
  if (React.isValidElement(node))
    return nodeText((node.props as { children?: React.ReactNode }).children);
  return '';
}

// ── Mermaid block ─────────────────────────────────────────────────────────────

const MermaidBlock: React.FC<{ code: string }> = ({ code }) => {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!ref.current) return;
    let cancelled = false;
    const el = ref.current;

    import('mermaid').then(({ default: mermaid }) => {
      if (cancelled) return;
      mermaid.initialize({ startOnLoad: false, theme: 'dark', suppressErrorRendering: true });
      const id = `mmd-${Math.random().toString(36).slice(2)}`;
      mermaid
        .render(id, code)
        .then(({ svg }) => {
          if (cancelled) return;
          // Mermaid v11 can return an error SVG instead of rejecting — detect it
          if (svg.includes('syntax-error') || svg.includes('Syntax error')) {
            el.innerHTML = `<pre style="color:#f87171;font-size:12px;white-space:pre-wrap;text-align:left">${escHtml(code)}</pre>`;
          } else {
            el.innerHTML = svg;
          }
        })
        .catch(() => {
          if (!cancelled)
            el.innerHTML = `<pre style="color:#f87171;font-size:12px;white-space:pre-wrap;text-align:left">${escHtml(code)}</pre>`;
        });
    });

    return () => { cancelled = true; };
  }, [code]);

  return (
    <div
      ref={ref}
      className="my-2 overflow-x-auto rounded-lg bg-stone-900 p-4 text-center min-h-[60px] flex items-center justify-center"
    >
      <span className="text-stone-500 text-xs animate-pulse">rendering diagram…</span>
    </div>
  );
};

function escHtml(s: string) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

// ── Code block wrapper with copy button ───────────────────────────────────────

const PreBlock: React.FC<React.HTMLAttributes<HTMLPreElement>> = ({ children, ...rest }) => {
  const [copied, setCopied] = useState(false);

  // Extract code text and language from the <code> child rendered by rehype-highlight.
  // After rehype-highlight, children may be React element trees — use nodeText().
  const codeEl = React.Children.toArray(children).find(
    (c): c is React.ReactElement<{ className?: string; children?: React.ReactNode }> =>
      React.isValidElement(c),
  );
  const className = codeEl?.props?.className ?? '';
  // class is e.g. "hljs language-javascript" or "language-mermaid hljs"
  const langMatch = className.match(/language-(\S+)/);
  const language = langMatch ? langMatch[1] : '';
  const codeText = nodeText(codeEl?.props?.children).replace(/\n$/, '');

  if (language === 'mermaid') {
    return <MermaidBlock code={codeText} />;
  }

  const copy = () => {
    void navigator.clipboard.writeText(codeText).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  return (
    <div className="relative group my-2 rounded-lg overflow-hidden">
      <button
        onClick={copy}
        className="absolute top-2 right-2 z-10 opacity-0 group-hover:opacity-100 transition-opacity
                   text-[10px] px-2 py-0.5 rounded bg-stone-600 text-stone-300 hover:bg-stone-500 select-none"
      >
        {copied ? '✓ copied' : 'copy'}
      </button>
      <pre {...rest} className="!my-0 !rounded-lg overflow-x-auto">
        {children}
      </pre>
    </div>
  );
};

// ── Public component ──────────────────────────────────────────────────────────

interface Props {
  children: string;
}

export const MarkdownRenderer: React.FC<Props> = ({ children }) => (
  <ReactMarkdown
    remarkPlugins={[remarkGfm]}
    rehypePlugins={[[rehypeHighlight, { ignoreMissing: true, detect: false }]]}
    components={{
      pre: PreBlock,
      // Links: open in system browser instead of navigating the webview
      a({ href, children: c }) {
        return (
          <a
            href={href}
            onClick={(e) => {
              if (href) {
                e.preventDefault();
                void invoke('open_url', { url: href });
              }
            }}
            className="text-amber-400 hover:text-amber-300 underline underline-offset-2 cursor-pointer"
          >
            {c}
          </a>
        );
      },
      // Tables: horizontal scroll wrapper
      table({ children: c }) {
        return (
          <div className="overflow-x-auto my-2">
            <table className="border-collapse w-full text-sm">{c}</table>
          </div>
        );
      },
      th({ children: c }) {
        return (
          <th className="border border-stone-600 px-3 py-1.5 bg-stone-700/60 text-left font-semibold text-stone-200 text-xs">
            {c}
          </th>
        );
      },
      td({ children: c }) {
        return <td className="border border-stone-700 px-3 py-1.5 text-stone-300 text-xs">{c}</td>;
      },
    }}
  >
    {children}
  </ReactMarkdown>
);

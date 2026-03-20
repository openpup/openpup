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
      style={{ margin: '8px 0', overflowX: 'auto', borderRadius: '8px', background: 'var(--color-background-secondary)', padding: '16px', textAlign: 'center', minHeight: '60px', display: 'flex', alignItems: 'center', justifyContent: 'center' }}
    >
      <span style={{ fontSize: '12px', color: 'var(--color-text-tertiary)' }} className="animate-pulse">rendering diagram…</span>
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
        style={{ position: 'absolute', top: '8px', right: '8px', zIndex: 10, opacity: 0, transition: 'opacity 0.15s', fontSize: '11px', padding: '2px 7px', borderRadius: '4px', background: 'var(--color-border-primary)', color: 'var(--color-background-primary)', border: 'none', cursor: 'pointer', userSelect: 'none' }}
        onMouseEnter={e => (e.currentTarget.style.opacity = '1')}
        onMouseLeave={e => (e.currentTarget.style.opacity = '0')}
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

// Hoisted outside component — stable reference, no new object on each render
const MD_COMPONENTS: React.ComponentPropsWithoutRef<typeof ReactMarkdown>['components'] = {
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
        style={{ color: 'var(--color-text-info)', textDecoration: 'underline', textUnderlineOffset: '2px', cursor: 'pointer' }}
      >
        {c}
      </a>
    );
  },
  // Tables: horizontal scroll wrapper
  table({ children: c }) {
    return (
      <div className="overflow-x-auto my-2">
        <table style={{ borderCollapse: 'collapse', width: '100%', fontSize: '13px' }}>{c}</table>
      </div>
    );
  },
  th({ children: c }) {
    return (
      <th style={{ border: '0.5px solid var(--color-border-secondary)', padding: '6px 12px', background: 'var(--color-background-secondary)', textAlign: 'left', fontWeight: 500, color: 'var(--color-text-primary)', fontSize: '12px' }}>
        {c}
      </th>
    );
  },
  td({ children: c }) {
    return <td style={{ border: '0.5px solid var(--color-border-tertiary)', padding: '6px 12px', color: 'var(--color-text-secondary)', fontSize: '12px' }}>{c}</td>;
  },
};

const REMARK_PLUGINS: React.ComponentPropsWithoutRef<typeof ReactMarkdown>['remarkPlugins'] = [remarkGfm];
const REHYPE_PLUGINS: React.ComponentPropsWithoutRef<typeof ReactMarkdown>['rehypePlugins'] = [
  [rehypeHighlight, { ignoreMissing: true, detect: false }],
];

export const MarkdownRenderer: React.FC<Props> = React.memo(({ children }) => (
  <ReactMarkdown
    remarkPlugins={REMARK_PLUGINS}
    rehypePlugins={REHYPE_PLUGINS}
    components={MD_COMPONENTS}
  >
    {children}
  </ReactMarkdown>
), (prev, next) => prev.children === next.children);

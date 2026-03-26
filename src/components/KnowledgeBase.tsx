import React, { useState, useRef, useEffect, useCallback } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { useLang, t } from '../i18n';
import { useKnowledgeBase, KbSearchResult } from '../hooks/useKnowledgeBase';

interface KgRelationInfo {
  relation: string;
  other_name: string;
  other_type: string;
  direction: string;
  confidence: number;
}

interface KgEntityInfo {
  id: string;
  name: string;
  entity_type: string;
  description: string | null;
  relations: KgRelationInfo[];
}

const inputStyle: React.CSSProperties = {
  width: '100%',
  borderRadius: 8,
  background: 'var(--color-background-primary)',
  border: '1px solid var(--color-border-secondary)',
  padding: '6px 12px',
  fontSize: 12,
  color: 'var(--color-text-primary)',
  outline: 'none',
};

const btnStyle: React.CSSProperties = {
  borderRadius: 6,
  border: '1px solid var(--color-border-secondary)',
  background: 'var(--color-background-secondary)',
  color: 'var(--color-text-primary)',
  padding: '5px 12px',
  fontSize: 12,
  cursor: 'pointer',
};

const entityTypeColors: Record<string, string> = {
  person: '#3B82F6',
  project: '#8B5CF6',
  concept: '#1D9E75',
  tool: '#BA7517',
  org: '#EC4899',
  file: '#6B7280',
  other: 'var(--color-text-tertiary)',
};

export const KnowledgeBase: React.FC = () => {
  const { lang } = useLang();
  const {
    sources,
    searchResults,
    loading,
    ingesting,
    ingestFile,
    deleteSource,
    search,
  } = useKnowledgeBase();

  const [searchQuery, setSearchQuery] = useState('');
  const [mode, setMode] = useState<'sources' | 'search' | 'graph'>('sources');
  const searchTimer = useRef<ReturnType<typeof setTimeout>>();

  // Graph state
  const [entities, setEntities] = useState<KgEntityInfo[]>([]);
  const [graphLoading, setGraphLoading] = useState(false);
  const [expandedEntity, setExpandedEntity] = useState<string | null>(null);

  const loadEntities = useCallback(async () => {
    setGraphLoading(true);
    try {
      const list = await invoke<KgEntityInfo[]>('kg_list_entities', { entityType: null });
      setEntities(list);
    } catch (e) {
      console.error('kg_list_entities failed:', e);
    } finally {
      setGraphLoading(false);
    }
  }, []);

  useEffect(() => {
    if (mode === 'graph') {
      loadEntities();
    }
  }, [mode, loadEntities]);

  const handleAddFile = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: 'Documents', extensions: ['md', 'txt', 'markdown', 'pdf'] }],
    });
    if (selected) {
      await ingestFile(selected as string);
    }
  };

  const handleSearch = (query: string) => {
    setSearchQuery(query);
    if (searchTimer.current) clearTimeout(searchTimer.current);
    searchTimer.current = setTimeout(() => {
      search(query);
    }, 300);
  };

  const statusDot = (status: string) => {
    const colors: Record<string, string> = {
      indexed: '#1D9E75',
      indexing: '#BA7517',
      pending: 'var(--color-text-tertiary)',
      failed: '#E5484D',
    };
    return colors[status] || 'var(--color-text-tertiary)';
  };

  // Group entities by type
  const entityGroups: Record<string, KgEntityInfo[]> = {};
  for (const e of entities) {
    (entityGroups[e.entity_type] ??= []).push(e);
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', gap: 12 }}>
      {/* Header */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexShrink: 0 }}>
        <h3 style={{ fontSize: 14, fontWeight: 600, margin: 0, color: 'var(--color-text-primary)' }}>
          {t('kb_title', lang)}
        </h3>
        <div style={{ flex: 1 }} />
        <button style={btnStyle} onClick={handleAddFile}>
          + {t('kb_import', lang)}
        </button>
      </div>

      {/* Mode tabs */}
      <div style={{ display: 'flex', gap: 4, flexShrink: 0 }}>
        {(['sources', 'search', 'graph'] as const).map(tab => (
          <button
            key={tab}
            onClick={() => setMode(tab)}
            style={{
              ...btnStyle,
              background: mode === tab ? 'var(--color-background-secondary)' : 'transparent',
              fontWeight: mode === tab ? 600 : 400,
            }}
          >
            {tab === 'sources' && `${t('kb_sources', lang)} (${sources.length})`}
            {tab === 'search' && t('kb_search_tab', lang)}
            {tab === 'graph' && `${t('kb_graph_tab', lang)}${entities.length > 0 ? ` (${entities.length})` : ''}`}
          </button>
        ))}
      </div>

      {/* Ingesting progress */}
      {ingesting && (
        <div style={{
          padding: '8px 12px',
          borderRadius: 8,
          background: 'rgba(186,117,23,0.08)',
          border: '1px solid rgba(186,117,23,0.2)',
          fontSize: 12,
          color: 'var(--color-text-secondary)',
        }}>
          {t('kb_importing', lang)}: {ingesting.source_title}
          {ingesting.chunks_total > 0 && (
            <span> — {ingesting.chunks_done}/{ingesting.chunks_total} chunks</span>
          )}
          {ingesting.error && (
            <div style={{ color: '#E5484D', marginTop: 4 }}>{ingesting.error}</div>
          )}
        </div>
      )}

      {/* Sources tab */}
      {mode === 'sources' && (
        <div style={{ flex: 1, overflowY: 'auto' }}>
          {sources.length === 0 ? (
            <div style={{ textAlign: 'center', padding: '40px 20px', color: 'var(--color-text-tertiary)', fontSize: 13 }}>
              {t('kb_empty', lang)}
            </div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
              {sources.map(src => (
                <div
                  key={src.id}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 8,
                    padding: '8px 12px',
                    borderRadius: 8,
                    background: 'var(--color-background-secondary)',
                    fontSize: 12,
                  }}
                >
                  <span style={{
                    width: 7,
                    height: 7,
                    borderRadius: '50%',
                    background: statusDot(src.status),
                    flexShrink: 0,
                  }} />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{
                      color: 'var(--color-text-primary)',
                      fontWeight: 500,
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                    }}>
                      {src.title}
                    </div>
                    <div style={{ color: 'var(--color-text-tertiary)', fontSize: 11, marginTop: 2 }}>
                      {src.chunk_count} chunks · {src.status}
                      {src.error_msg && <span style={{ color: '#E5484D' }}> · {src.error_msg}</span>}
                    </div>
                  </div>
                  <button
                    onClick={() => deleteSource(src.id)}
                    style={{
                      background: 'none',
                      border: 'none',
                      color: 'var(--color-text-tertiary)',
                      cursor: 'pointer',
                      fontSize: 14,
                      padding: '2px 6px',
                    }}
                    title={t('kb_delete', lang)}
                  >
                    ×
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Search tab */}
      {mode === 'search' && (
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 8, overflow: 'hidden' }}>
          <input
            style={inputStyle}
            placeholder={t('kb_search_placeholder', lang)}
            value={searchQuery}
            onChange={e => handleSearch(e.target.value)}
            autoFocus
          />
          <div style={{ flex: 1, overflowY: 'auto' }}>
            {loading && (
              <div style={{ textAlign: 'center', padding: 20, color: 'var(--color-text-tertiary)', fontSize: 12 }}>
                {t('kb_searching', lang)}...
              </div>
            )}
            {!loading && searchQuery && searchResults.length === 0 && (
              <div style={{ textAlign: 'center', padding: 20, color: 'var(--color-text-tertiary)', fontSize: 12 }}>
                {t('kb_no_results', lang)}
              </div>
            )}
            {!loading && searchResults.length > 0 && (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                {searchResults.map((r: KbSearchResult) => (
                  <div
                    key={r.chunk_id}
                    style={{
                      padding: '10px 12px',
                      borderRadius: 8,
                      background: 'var(--color-background-secondary)',
                      fontSize: 12,
                    }}
                  >
                    <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4 }}>
                      <span style={{ color: 'var(--color-text-primary)', fontWeight: 500 }}>
                        {r.source_title}
                      </span>
                      {r.heading_path && (
                        <span style={{ color: 'var(--color-text-tertiary)' }}>
                          › {r.heading_path}
                        </span>
                      )}
                      <span style={{ marginLeft: 'auto', color: 'var(--color-text-tertiary)', fontSize: 11 }}>
                        {Math.round(r.score * 100)}%
                      </span>
                    </div>
                    <div style={{
                      color: 'var(--color-text-secondary)',
                      lineHeight: '1.5',
                      whiteSpace: 'pre-wrap',
                      maxHeight: 120,
                      overflow: 'hidden',
                    }}>
                      {r.content.slice(0, 400)}
                      {r.content.length > 400 && '…'}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      )}

      {/* Graph tab */}
      {mode === 'graph' && (
        <div style={{ flex: 1, overflowY: 'auto' }}>
          {graphLoading && (
            <div style={{ textAlign: 'center', padding: 20, color: 'var(--color-text-tertiary)', fontSize: 12 }}>
              {t('kb_searching', lang)}...
            </div>
          )}
          {!graphLoading && entities.length === 0 && (
            <div style={{ textAlign: 'center', padding: '40px 20px', color: 'var(--color-text-tertiary)', fontSize: 13 }}>
              {t('kb_graph_empty', lang)}
            </div>
          )}
          {!graphLoading && entities.length > 0 && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              {Object.entries(entityGroups).map(([type, group]) => (
                <div key={type}>
                  <div style={{
                    fontSize: 11,
                    fontWeight: 600,
                    textTransform: 'uppercase',
                    color: entityTypeColors[type] || 'var(--color-text-tertiary)',
                    letterSpacing: '0.05em',
                    marginBottom: 4,
                    padding: '0 4px',
                  }}>
                    {type} ({group.length})
                  </div>
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                    {group.map(entity => {
                      const isExpanded = expandedEntity === entity.id;
                      return (
                        <div key={entity.id}>
                          <button
                            onClick={() => setExpandedEntity(isExpanded ? null : entity.id)}
                            style={{
                              display: 'flex',
                              alignItems: 'center',
                              gap: 6,
                              width: '100%',
                              padding: '6px 8px',
                              borderRadius: 6,
                              background: isExpanded ? 'var(--color-background-secondary)' : 'transparent',
                              border: 'none',
                              cursor: 'pointer',
                              fontSize: 12,
                              color: 'var(--color-text-primary)',
                              textAlign: 'left',
                            }}
                          >
                            <span style={{ fontSize: 10, opacity: 0.5 }}>{isExpanded ? '▾' : '▸'}</span>
                            <span style={{ fontWeight: 500 }}>{entity.name}</span>
                            {entity.description && (
                              <span style={{
                                color: 'var(--color-text-tertiary)',
                                fontSize: 11,
                                overflow: 'hidden',
                                textOverflow: 'ellipsis',
                                whiteSpace: 'nowrap',
                                flex: 1,
                              }}>
                                — {entity.description}
                              </span>
                            )}
                            {entity.relations.length > 0 && (
                              <span style={{
                                color: 'var(--color-text-tertiary)',
                                fontSize: 11,
                                flexShrink: 0,
                              }}>
                                {entity.relations.length} rel
                              </span>
                            )}
                          </button>
                          {isExpanded && entity.relations.length > 0 && (
                            <div style={{ paddingLeft: 24, paddingBottom: 4 }}>
                              {entity.relations.map((rel, i) => (
                                <div
                                  key={i}
                                  style={{
                                    fontSize: 11,
                                    color: 'var(--color-text-secondary)',
                                    padding: '3px 0',
                                    display: 'flex',
                                    gap: 4,
                                  }}
                                >
                                  <span style={{ color: 'var(--color-text-tertiary)' }}>
                                    {rel.direction === 'out' ? '→' : '←'}
                                  </span>
                                  <span style={{ color: entityTypeColors[rel.other_type] || 'var(--color-text-tertiary)' }}>
                                    {rel.relation}
                                  </span>
                                  <span style={{ fontWeight: 500 }}>{rel.other_name}</span>
                                  <span style={{ color: 'var(--color-text-tertiary)', marginLeft: 'auto' }}>
                                    {Math.round(rel.confidence * 100)}%
                                  </span>
                                </div>
                              ))}
                            </div>
                          )}
                        </div>
                      );
                    })}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
};

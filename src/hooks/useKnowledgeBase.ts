import { useState, useCallback, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface KnowledgeSource {
  id: string;
  title: string;
  source_type: string;
  source_path: string | null;
  mime_type: string | null;
  byte_size: number | null;
  tags: string | null;
  chunk_count: number;
  status: string;
  error_msg: string | null;
  created_at: number;
  updated_at: number;
}

export interface KbSearchResult {
  chunk_id: string;
  source_id: string;
  source_title: string;
  source_path: string | null;
  content: string;
  heading_path: string | null;
  score: number;
  chunk_index: number;
}

interface IngestProgressEvent {
  source_id: string;
  source_title: string;
  status: string;
  chunks_done: number;
  chunks_total: number;
  error: string | null;
}

export function useKnowledgeBase() {
  const [sources, setSources] = useState<KnowledgeSource[]>([]);
  const [searchResults, setSearchResults] = useState<KbSearchResult[]>([]);
  const [loading, setLoading] = useState(false);
  const [ingesting, setIngesting] = useState<IngestProgressEvent | null>(null);

  const refreshSources = useCallback(async () => {
    try {
      const list = await invoke<KnowledgeSource[]>('kb_list_sources');
      setSources(list);
    } catch (e) {
      console.error('kb_list_sources failed:', e);
    }
  }, []);

  const ingestFile = useCallback(async (path: string, title?: string, tags: string[] = []) => {
    try {
      await invoke<string>('kb_ingest_file', { path, title, tags });
      // Progress events update the UI; refresh sources when done
    } catch (e) {
      console.error('kb_ingest_file failed:', e);
    }
  }, []);

  const deleteSource = useCallback(async (sourceId: string) => {
    try {
      await invoke('kb_delete_source', { sourceId });
      setSources(prev => prev.filter(s => s.id !== sourceId));
    } catch (e) {
      console.error('kb_delete_source failed:', e);
    }
  }, []);

  const search = useCallback(async (query: string, limit?: number) => {
    if (!query.trim()) {
      setSearchResults([]);
      return;
    }
    setLoading(true);
    try {
      const results = await invoke<KbSearchResult[]>('kb_search', { query, limit });
      setSearchResults(results);
    } catch (e) {
      console.error('kb_search failed:', e);
      setSearchResults([]);
    } finally {
      setLoading(false);
    }
  }, []);

  // Listen for ingest progress events
  useEffect(() => {
    const unlisten = listen<IngestProgressEvent>('kb_ingest_progress', (event) => {
      const evt = event.payload;
      setIngesting(evt);

      if (evt.status === 'indexed' || evt.status === 'failed') {
        // Refresh sources list and clear ingesting state after a short delay
        setTimeout(() => {
          setIngesting(null);
          refreshSources();
        }, 500);
      }
    });

    return () => { unlisten.then(fn => fn()); };
  }, [refreshSources]);

  // Load sources on mount
  useEffect(() => {
    refreshSources();
  }, [refreshSources]);

  return {
    sources,
    searchResults,
    loading,
    ingesting,
    ingestFile,
    deleteSource,
    search,
    refreshSources,
  };
}

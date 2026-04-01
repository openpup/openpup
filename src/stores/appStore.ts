import { create } from 'zustand';

export interface PupConfig {
  key: string;
  display_name: string;
  description: string;
  enabled: boolean;
  is_custom: boolean;
}

export interface MemoryChip {
  content: string;
  memory_type: string;
  importance: number;
}

export interface ContextStats {
  pup_key: string;
  message_count: number;
  context_tokens: number;
  context_limit: number;
  compression_status: {
    is_compressed: boolean;
    last_compression_row: number;
  };
}

interface AppState {
  onboardingDone: boolean | null;
  pups: PupConfig[];
  memoryChips: MemoryChip[];
  kbSourceCount: number;
  kbAutoIngest: boolean;
  permissionRequest: PermissionRequest | null;
  contextStats: ContextStats | null;
  execMode: 'leashed' | 'free_run';

  // Settings panel transient state
  exporting: boolean;
  importing: boolean;
  importPath: string;
  settingsMsg: string | null;
  settingsErr: string | null;

  setOnboardingDone: (done: boolean | null) => void;
  setPups: (pups: PupConfig[]) => void;
  setMemoryChips: (chips: MemoryChip[]) => void;
  setKbSourceCount: (count: number) => void;
  setKbAutoIngest: (ingest: boolean) => void;
  setPermissionRequest: (req: PermissionRequest | null) => void;
  setContextStats: (stats: ContextStats | null) => void;
  setExecMode: (mode: 'leashed' | 'free_run') => void;
  setExporting: (exporting: boolean) => void;
  setImporting: (importing: boolean) => void;
  setImportPath: (path: string) => void;
  setSettingsMsg: (msg: string | null) => void;
  setSettingsErr: (err: string | null) => void;
}

// Re-export from the canonical definition
export type { PermissionRequest } from '../components/PermissionDialog';
import type { PermissionRequest } from '../components/PermissionDialog';

export const useAppStore = create<AppState>((set) => ({
  onboardingDone: null,
  pups: [],
  memoryChips: [],
  kbSourceCount: 0,
  kbAutoIngest: true,
  permissionRequest: null,
  contextStats: null,
  execMode: 'leashed',
  exporting: false,
  importing: false,
  importPath: '',
  settingsMsg: null,
  settingsErr: null,

  setOnboardingDone: (done) => set({ onboardingDone: done }),
  setPups: (pups) => set({ pups }),
  setMemoryChips: (chips) => set({ memoryChips: chips }),
  setKbSourceCount: (count) => set({ kbSourceCount: count }),
  setKbAutoIngest: (ingest) => set({ kbAutoIngest: ingest }),
  setPermissionRequest: (req) => set({ permissionRequest: req }),
  setContextStats: (stats) => set({ contextStats: stats }),
  setExecMode: (mode) => set({ execMode: mode }),
  setExporting: (exporting) => set({ exporting }),
  setImporting: (importing) => set({ importing }),
  setImportPath: (path) => set({ importPath: path }),
  setSettingsMsg: (msg) => set({ settingsMsg: msg }),
  setSettingsErr: (err) => set({ settingsErr: err }),
}));

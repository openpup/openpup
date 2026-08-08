import { create } from 'zustand';

export type NavItem =
  | 'chat'
  | 'channel'
  | 'groups'
  | 'memories'
  | 'timeline'
  | 'skills'
  | 'pups'
  | 'tasks'
  | 'mcp'
  | 'bridge'
  | 'settings'
  | 'knowledge';

type Theme = 'dark' | 'light';

interface UIState {
  theme: Theme;
  activeNav: NavItem;
  channelDetailMode: boolean;
  sidebarCollapsed: boolean;
  membersExpanded: boolean;
  toolsExpanded: boolean;
  configExpanded: boolean;
  isMaximized: boolean;
  selectedPupKey: string;
  memoriesTab: 'long_term' | 'diary';

  setTheme: (theme: Theme) => void;
  setActiveNav: (nav: NavItem) => void;
  setChannelDetailMode: (mode: boolean) => void;
  setSidebarCollapsed: (collapsed: boolean) => void;
  setMembersExpanded: (expanded: boolean) => void;
  setToolsExpanded: (expanded: boolean) => void;
  setConfigExpanded: (expanded: boolean) => void;
  setIsMaximized: (maximized: boolean) => void;
  setSelectedPupKey: (key: string) => void;
  setMemoriesTab: (tab: 'long_term' | 'diary') => void;
}

export const useUIStore = create<UIState>((set) => ({
  theme: (localStorage.getItem('openpup_theme') === 'light' ? 'light' : 'dark') as Theme,
  activeNav: 'chat',
  channelDetailMode: false,
  sidebarCollapsed: false,
  membersExpanded: true,
  toolsExpanded: false,
  configExpanded: false,
  isMaximized: false,
  selectedPupKey: 'alpha',
  memoriesTab: 'long_term',

  setTheme: (theme) => set({ theme }),
  setActiveNav: (nav) => set({ activeNav: nav }),
  setChannelDetailMode: (mode) => set({ channelDetailMode: mode }),
  setSidebarCollapsed: (collapsed) => set({ sidebarCollapsed: collapsed }),
  setMembersExpanded: (expanded) => set({ membersExpanded: expanded }),
  setToolsExpanded: (expanded) => set({ toolsExpanded: expanded }),
  setConfigExpanded: (expanded) => set({ configExpanded: expanded }),
  setIsMaximized: (maximized) => set({ isMaximized: maximized }),
  setSelectedPupKey: (key) => set({ selectedPupKey: key }),
  setMemoriesTab: (tab) => set({ memoriesTab: tab }),
}));

import { create } from 'zustand';

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  pup_key?: string;
  pup_name?: string;
  timestamp?: number;
  finance_artifact?: FinanceArtifactPayload;
}

export interface FinanceArtifactPayload {
  scenarioPreset?: string;
  sourcePupKey?: string;
  sourcePupName?: string;
  stageLabels: string[];
  intents: FinanceIntentPayload[];
  rawLines: string[];
}

export interface FinanceIntentPayload {
  symbol?: string;
  market?: string;
  direction?: string;
  thesis?: string;
  confidence?: string;
  entryRule?: string;
  exitRule?: string;
  maxPositionPct?: string;
  timeHorizon?: string;
  validUntil?: string;
  riskNotes?: string;
  approvalStatus?: string;
  adjustedPositionPct?: string;
  rawLines: string[];
}

export interface StreamingPupState {
  key: string;
  name: string;
}

export interface ActivityStep {
  kind: 'routing' | 'skill' | 'shell' | 'file_read' | 'file_write' | 'http' | 'memory' | 'task' | 'mcp' | 'tool_call' | string;
  label: string;
}

export interface TokenUsage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
}

interface ChatState {
  messages: ChatMessage[];
  streamingContent: string;
  streamingReasoningContent: string;
  streamingPup: StreamingPupState | null;
  streamingSteps: ActivityStep[];
  input: string;
  sending: boolean;
  tokenUsage: TokenUsage | null;

  setMessages: (msgs: ChatMessage[] | ((prev: ChatMessage[]) => ChatMessage[])) => void;
  appendMessage: (msg: ChatMessage) => void;
  setStreamingContent: (content: string | ((prev: string) => string)) => void;
  setStreamingReasoningContent: (content: string | ((prev: string) => string)) => void;
  setStreamingPup: (pup: StreamingPupState | null) => void;
  setStreamingSteps: (steps: ActivityStep[] | ((prev: ActivityStep[]) => ActivityStep[])) => void;
  setInput: (input: string | ((prev: string) => string)) => void;
  setSending: (sending: boolean) => void;
  setTokenUsage: (usage: TokenUsage | null) => void;
  resetStreaming: () => void;
}

export const useChatStore = create<ChatState>((set) => ({
  messages: [],
  streamingContent: '',
  streamingReasoningContent: '',
  streamingPup: null,
  streamingSteps: [],
  input: '',
  sending: false,
  tokenUsage: null,

  setMessages: (msgs) =>
    set((s) => ({ messages: typeof msgs === 'function' ? msgs(s.messages) : msgs })),
  appendMessage: (msg) =>
    set((s) => ({ messages: [...s.messages, msg] })),
  setStreamingContent: (content) =>
    set((s) => ({
      streamingContent: typeof content === 'function' ? content(s.streamingContent) : content,
    })),
  setStreamingReasoningContent: (content) =>
    set((s) => ({
      streamingReasoningContent:
        typeof content === 'function' ? content(s.streamingReasoningContent) : content,
    })),
  setStreamingPup: (pup) => set({ streamingPup: pup }),
  setStreamingSteps: (steps) =>
    set((s) => ({
      streamingSteps: typeof steps === 'function' ? steps(s.streamingSteps) : steps,
    })),
  setInput: (input) =>
    set((s) => ({ input: typeof input === 'function' ? input(s.input) : input })),
  setSending: (sending) => set({ sending }),
  setTokenUsage: (usage) => set({ tokenUsage: usage }),
  resetStreaming: () =>
    set({
      streamingContent: '',
      streamingReasoningContent: '',
      streamingPup: null,
      streamingSteps: [],
    }),
}));

export interface ChannelMessageRecord {
  id: string;
  channel_id: string;
  sender: string;
  content: string;
  msg_type: string;
  artifact_name?: string | null;
  status_val?: string | null;
  mentions?: string[];
  timestamp: number;
}

export interface ChannelRecord {
  id: string;
  task_id: string;
  title: string;
  status: string;
  created_at: number;
  completed_at?: number | null;
  updated_at: number;
  members: string[];
}

export interface Subtask {
  pup: string;
  description: string;
  depends_on: string[];
}

export interface DelegationPlan {
  channel_id: string;
  channel_title: string;
  subtasks: Subtask[];
}

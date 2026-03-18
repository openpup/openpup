import React from 'react';
import { useLang, t } from '../i18n';

export interface PermissionRequest {
  request_id: string;
  skill_name: string;
  action_description: string;
  risk_level: 'low' | 'medium' | 'high';
  details: {
    affected_files?: string[];
    network_destinations?: string[];
    estimated_cost?: number;
  };
}

interface Props {
  request: PermissionRequest;
  onApprove: (remember: boolean) => void;
  onDeny: () => void;
}

const RISK_CONFIG = {
  high: {
    border: 'border-t-2 border-red-500',
    badge: 'bg-red-500/20 text-red-400 border border-red-500/40',
    label: '高风险',
    approveLabel: '允许发布',
    approveClass: 'bg-red-600 hover:bg-red-500 text-white',
  },
  medium: {
    border: 'border-t-2 border-amber-500',
    badge: 'bg-amber-500/20 text-amber-400 border border-amber-500/40',
    label: '中风险',
    approveLabel: '允许',
    approveClass: 'bg-amber-500 hover:bg-amber-400 text-stone-950',
  },
  low: {
    border: 'border-t-2 border-emerald-500',
    badge: 'bg-emerald-500/20 text-emerald-400 border border-emerald-500/40',
    label: '低风险',
    approveLabel: '允许',
    approveClass: 'bg-emerald-600 hover:bg-emerald-500 text-white',
  },
};

export const PermissionDialog: React.FC<Props> = ({ request, onApprove, onDeny }) => {
  const { lang } = useLang();
  const [remember, setRemember] = React.useState(false);
  const cfg = RISK_CONFIG[request.risk_level] ?? RISK_CONFIG.medium;
  const isHigh = request.risk_level === 'high';

  return (
    <div className="fixed inset-0 z-50 flex items-end sm:items-center justify-center bg-black/60 backdrop-blur-sm p-4">
      <div className={`w-full max-w-sm rounded-2xl bg-stone-900 border border-stone-700 shadow-2xl overflow-hidden ${cfg.border}`}>
        {/* Header */}
        <div className="px-5 pt-5 pb-3 flex items-start justify-between gap-3">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1.5">
              <span className="text-sm font-semibold text-stone-100">🐕 {request.skill_name} 需要你的许可</span>
            </div>
            <p className="text-xs text-stone-400 leading-relaxed">{request.action_description}</p>
          </div>
          <span className={`shrink-0 text-[11px] font-medium px-2 py-0.5 rounded-full ${cfg.badge}`}>
            {cfg.label}
          </span>
        </div>

        {/* Details */}
        {(request.details.affected_files?.length || request.details.network_destinations?.length || typeof request.details.estimated_cost === 'number') && (
          <div className="mx-5 mb-3 rounded-xl bg-stone-800/60 border border-stone-700/60 px-3 py-2.5 text-[11px] text-stone-500 space-y-1">
            {request.details.affected_files?.length ? (
              <div><span className="text-stone-400">文件：</span>{request.details.affected_files.join(', ')}</div>
            ) : null}
            {request.details.network_destinations?.length ? (
              <div><span className="text-stone-400">网络：</span>{request.details.network_destinations.join(', ')}</div>
            ) : null}
            {typeof request.details.estimated_cost === 'number' ? (
              <div><span className="text-stone-400">预计费用：</span>${request.details.estimated_cost}</div>
            ) : null}
          </div>
        )}

        {/* Remember checkbox — only for low risk */}
        {!isHigh && (
          <label className="mx-5 mb-3 flex items-center gap-2.5 text-xs text-stone-400 cursor-pointer select-none">
            <input
              type="checkbox"
              className="w-3.5 h-3.5 rounded border-stone-600 bg-stone-800 accent-emerald-500"
              checked={remember}
              onChange={(e) => setRemember(e.target.checked)}
            />
            记住，下次不再询问
          </label>
        )}

        {/* Actions */}
        <div className="px-5 pb-5 flex gap-2.5 justify-end">
          <button
            className="px-4 py-2 rounded-xl bg-stone-800 text-stone-300 text-xs font-medium hover:bg-stone-700 transition-colors"
            onClick={onDeny}
          >
            拒绝
          </button>
          <button
            className={`px-4 py-2 rounded-xl text-xs font-medium transition-colors ${cfg.approveClass}`}
            onClick={() => onApprove(isHigh ? false : remember)}
          >
            {cfg.approveLabel}
          </button>
        </div>
      </div>
    </div>
  );
};

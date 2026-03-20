import React from 'react';

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

const RISK: Record<string, { bar: string; badge: string; badgeText: string; approveLabel: string; approveBg: string; approveColor: string }> = {
  high:   { bar: '#E24B4A', badge: 'rgba(226,75,74,0.12)', badgeText: '#E24B4A', approveLabel: '允许', approveBg: 'var(--color-text-primary)', approveColor: 'var(--color-background-primary)' },
  medium: { bar: '#BA7517', badge: 'rgba(186,117,23,0.12)', badgeText: '#BA7517', approveLabel: '允许', approveBg: '#BA7517', approveColor: '#fff' },
  low:    { bar: '#1D9E75', badge: 'rgba(29,158,117,0.12)', badgeText: '#1D9E75', approveLabel: '允许', approveBg: '#1D9E75', approveColor: '#fff' },
};

const RISK_LABEL: Record<string, string> = { high: '高风险', medium: '中风险', low: '低风险' };

export const PermissionDialog: React.FC<Props> = ({ request, onApprove, onDeny }) => {
  const [remember, setRemember] = React.useState(false);
  const cfg = RISK[request.risk_level] ?? RISK.medium;
  const isHigh = request.risk_level === 'high';

  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 50,
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        background: 'rgba(0,0,0,0.3)',
      }}
    >
      <div style={{
        width: '320px',
        background: 'var(--color-background-primary)',
        border: '0.5px solid var(--color-border-secondary)',
        borderRadius: '16px',
        overflow: 'hidden',
      }}>
        {/* 3px color bar */}
        <div style={{ height: '3px', background: cfg.bar }} />

        {/* Header */}
        <div style={{ padding: '16px 18px 12px', display: 'flex', alignItems: 'flex-start', gap: '10px' }}>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontSize: '13px', fontWeight: 500, color: 'var(--color-text-primary)', marginBottom: '4px' }}>
              {request.skill_name} 需要你的许可
            </div>
            <div style={{ fontSize: '11.5px', color: 'var(--color-text-secondary)', lineHeight: 1.7 }}>
              {request.action_description}
            </div>
          </div>
          <span style={{
            flexShrink: 0,
            fontSize: '11px', fontWeight: 500,
            padding: '2px 8px', borderRadius: '10px',
            background: cfg.badge, color: cfg.badgeText,
            border: `0.5px solid ${cfg.bar}`,
          }}>
            {RISK_LABEL[request.risk_level] ?? '中风险'}
          </span>
        </div>

        {/* Details */}
        {(request.details.affected_files?.length || request.details.network_destinations?.length || typeof request.details.estimated_cost === 'number') && (
          <div style={{
            margin: '0 18px 12px',
            background: 'var(--color-background-secondary)',
            border: '0.5px solid var(--color-border-tertiary)',
            borderRadius: '8px',
            padding: '8px 10px',
            fontSize: '12px',
            color: 'var(--color-text-secondary)',
            lineHeight: 1.6,
          }}>
            {request.details.affected_files?.length ? (
              <div><span style={{ color: 'var(--color-text-tertiary)' }}>目标文件 · </span>{request.details.affected_files.join(', ')}</div>
            ) : null}
            {request.details.network_destinations?.length ? (
              <div><span style={{ color: 'var(--color-text-tertiary)' }}>目标平台 · </span>{request.details.network_destinations.join(', ')}</div>
            ) : null}
            {typeof request.details.estimated_cost === 'number' ? (
              <div><span style={{ color: 'var(--color-text-tertiary)' }}>预计费用 · </span>${request.details.estimated_cost}</div>
            ) : null}
          </div>
        )}

        {/* Remember — only for non-high risk */}
        {!isHigh && (
          <label style={{
            display: 'flex', alignItems: 'center', gap: '8px',
            margin: '0 18px 12px',
            fontSize: '13px', color: 'var(--color-text-secondary)',
            cursor: 'pointer', userSelect: 'none',
          }}>
            <input
              type="checkbox"
              checked={remember}
              onChange={(e) => setRemember(e.target.checked)}
              style={{ width: '13px', height: '13px', cursor: 'pointer', accentColor: '#1D9E75' }}
            />
            记住选择，下次不再询问
          </label>
        )}

        {/* Actions */}
        <div style={{ padding: '0 18px 18px', display: 'flex', gap: '8px', justifyContent: 'flex-end' }}>
          <button
            onClick={onDeny}
            style={{
              padding: '7px 16px', borderRadius: '8px', fontSize: '14px', cursor: 'pointer',
              background: 'transparent', color: 'var(--color-text-secondary)',
              border: '0.5px solid var(--color-border-secondary)',
            }}
          >
            拒绝
          </button>
          <button
            onClick={() => onApprove(isHigh ? false : remember)}
            style={{
              padding: '7px 16px', borderRadius: '8px', fontSize: '14px', cursor: 'pointer',
              background: cfg.approveBg, color: cfg.approveColor,
              border: 'none',
            }}
          >
            {cfg.approveLabel}
          </button>
        </div>
      </div>
    </div>
  );
};

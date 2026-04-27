import React from 'react';
import { useLang, t } from '../i18n';

export interface PermissionRequest {
  request_id: string;
  skill_name: string;
  action_description: string;
  risk_level: 'low' | 'medium' | 'high';
  actor_kind?: string;
  actor_name?: string;
  tool_name?: string;
  effect_kind?: string;
  scope?: unknown;
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

const RISK: Record<string, { bar: string; badge: string; badgeText: string; approveBg: string; approveColor: string }> = {
  high:   { bar: '#E24B4A', badge: 'rgba(226,75,74,0.12)', badgeText: '#E24B4A', approveBg: 'var(--color-text-primary)', approveColor: 'var(--color-background-primary)' },
  medium: { bar: '#BA7517', badge: 'rgba(186,117,23,0.12)', badgeText: '#BA7517', approveBg: '#BA7517', approveColor: '#fff' },
  low:    { bar: '#1D9E75', badge: 'rgba(29,158,117,0.12)', badgeText: '#1D9E75', approveBg: '#1D9E75', approveColor: '#fff' },
};

export const PermissionDialog: React.FC<Props> = ({ request, onApprove, onDeny }) => {
  const { lang } = useLang();
  const [remember, setRemember] = React.useState(false);
  const cfg = RISK[request.risk_level] ?? RISK.medium;
  const isHigh = request.risk_level === 'high';
  const riskLabel = request.risk_level === 'high'
    ? t('permission_risk_high', lang)
    : request.risk_level === 'low'
      ? t('permission_risk_low', lang)
      : t('permission_risk_medium', lang);

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
              {request.skill_name} {t('permission_requires_approval', lang)}
            </div>
            <div style={{ fontSize: '11.5px', color: 'var(--color-text-secondary)', lineHeight: 1.7 }}>
              {request.action_description}
            </div>
            {(request.tool_name || request.effect_kind) && (
              <div style={{ marginTop: '6px', fontSize: '10.5px', color: 'var(--color-text-tertiary)', lineHeight: 1.5 }}>
                {[request.actor_name, request.tool_name, request.effect_kind].filter(Boolean).join(' · ')}
              </div>
            )}
          </div>
          <span style={{
            flexShrink: 0,
            fontSize: '11px', fontWeight: 500,
            padding: '2px 8px', borderRadius: '10px',
            background: cfg.badge, color: cfg.badgeText,
            border: `0.5px solid ${cfg.bar}`,
          }}>
            {riskLabel}
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
              <div><span style={{ color: 'var(--color-text-tertiary)' }}>{t('permission_affected_files', lang)} · </span>{request.details.affected_files.join(', ')}</div>
            ) : null}
            {request.details.network_destinations?.length ? (
              <div><span style={{ color: 'var(--color-text-tertiary)' }}>{t('permission_network_destinations', lang)} · </span>{request.details.network_destinations.join(', ')}</div>
            ) : null}
            {typeof request.details.estimated_cost === 'number' ? (
              <div><span style={{ color: 'var(--color-text-tertiary)' }}>{t('permission_estimated_cost', lang)} · </span>${request.details.estimated_cost}</div>
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
            {t('permission_remember', lang)}
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
            {t('permission_deny', lang)}
          </button>
          <button
            onClick={() => onApprove(isHigh ? false : remember)}
            style={{
              padding: '7px 16px', borderRadius: '8px', fontSize: '14px', cursor: 'pointer',
              background: cfg.approveBg, color: cfg.approveColor,
              border: 'none',
            }}
          >
            {t('permission_allow', lang)}
          </button>
        </div>
      </div>
    </div>
  );
};

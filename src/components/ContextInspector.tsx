import React from 'react';
import type { DelegationPlan } from '../types/channel';
import { useLang, t } from '../i18n';

type InspectorTab = 'plan' | 'members' | 'artifacts';
type InspectorTone = 'idle' | 'running' | 'done';

interface MemberState {
  member: string;
  label: string;
  tone: InspectorTone;
}

interface ArtifactItem {
  id: string;
  artifact_name?: string | null;
  content: string;
  timestamp: number;
}

interface ContextInspectorProps {
  plan: DelegationPlan | null;
  members: MemberState[];
  artifacts: ArtifactItem[];
  getPupAccent: (name: string) => string;
  formatRelativeTime: (timestamp: number) => string;
}

function statusTone(tone: InspectorTone) {
  switch (tone) {
    case 'running':
      return { color: '#1D9E75', background: 'rgba(29,158,117,0.12)' };
    case 'done':
      return { color: '#BA7517', background: 'rgba(186,117,23,0.12)' };
    default:
      return { color: 'var(--color-text-tertiary)', background: 'var(--color-background-secondary)' };
  }
}

export const ContextInspector: React.FC<ContextInspectorProps> = ({
  plan,
  members,
  artifacts,
  getPupAccent,
  formatRelativeTime,
}) => {
  const { lang } = useLang();
  const [tab, setTab] = React.useState<InspectorTab>('plan');
  const [collapsed, setCollapsed] = React.useState(true);

  return (
    <div style={{
      width: collapsed ? '44px' : '320px',
      borderLeft: '0.5px solid var(--color-border-tertiary)',
      background: 'color-mix(in srgb, var(--color-background-primary) 82%, var(--color-background-secondary) 18%)',
      display: 'flex',
      flexDirection: 'column',
      minWidth: 0,
      transition: 'width 160ms ease',
    }}>
      {collapsed ? (
        <div style={{ paddingTop: '18px', display: 'flex', justifyContent: 'center' }}>
          <button
            onClick={() => setCollapsed(false)}
            style={{
              width: '28px',
              height: '28px',
              borderRadius: '999px',
              border: '0.5px solid var(--color-border-tertiary)',
              background: 'var(--color-background-primary)',
              color: 'var(--color-text-tertiary)',
              cursor: 'pointer',
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
            }}
            title={t('ctx_open', lang)}
          >
            ‹
          </button>
        </div>
      ) : (
        <>
          <div style={{ padding: '18px', borderBottom: '0.5px solid var(--color-border-tertiary)' }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '10px', marginBottom: '10px' }}>
              <div style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.08em' }}>
                {t('ctx_title', lang)}
              </div>
              <button
                onClick={() => setCollapsed(true)}
                style={{ color: 'var(--color-text-tertiary)', background: 'none', border: 'none', cursor: 'pointer', padding: 0, lineHeight: 1, fontSize: '14px' }}
                title={t('ctx_close', lang)}
              >
                ›
              </button>
            </div>
            <div style={{ display: 'flex', gap: '6px' }}>
              {[
                ['plan', t('ctx_tab_plan', lang)],
                ['members', t('ctx_tab_members', lang)],
                ['artifacts', t('ctx_tab_artifacts', lang)],
              ].map(([key, label]) => (
                <button
                  key={key}
                  onClick={() => setTab(key as InspectorTab)}
                  style={{
                    padding: '6px 10px',
                    borderRadius: '999px',
                    border: tab === key ? '0.5px solid rgba(29,158,117,0.3)' : '0.5px solid var(--color-border-tertiary)',
                    background: tab === key ? 'rgba(29,158,117,0.08)' : 'transparent',
                    color: tab === key ? '#1D9E75' : 'var(--color-text-secondary)',
                    fontSize: '12px',
                    fontWeight: 600,
                    cursor: 'pointer',
                  }}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>

          <div style={{ flex: 1, overflowY: 'auto', padding: '16px 18px 24px' }}>
            {tab === 'plan' && (
              <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                {plan ? plan.subtasks.map((subtask, idx) => (
                  <div key={`${subtask.pup}-${idx}`} style={{
                    borderRadius: '16px',
                    padding: '14px',
                    border: '0.5px solid var(--color-border-tertiary)',
                    background: 'var(--color-background-primary)',
                  }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '8px' }}>
                      <span style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', minWidth: '20px' }}>{idx + 1}.</span>
                      <span style={{ fontSize: '13px', fontWeight: 600, color: getPupAccent(subtask.pup) }}>
                        {subtask.pup}
                      </span>
                    </div>
                    <div style={{ fontSize: '12px', color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                      {subtask.description}
                    </div>
                    <div style={{ marginTop: '10px', fontSize: '11px', color: 'var(--color-text-tertiary)', lineHeight: 1.5 }}>
                      {subtask.depends_on.length > 0 ? `${t('ctx_depends_on', lang)}: ${subtask.depends_on.join(', ')}` : t('ctx_no_upstream', lang)}
                    </div>
                  </div>
                )) : (
                  <div style={{ fontSize: '13px', color: 'var(--color-text-tertiary)', lineHeight: 1.7 }}>
                    {t('ctx_no_plan', lang)}
                  </div>
                )}
              </div>
            )}

            {tab === 'members' && (
              <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                {members.map(({ member, label, tone }) => (
                  <div key={member} style={{
                    borderRadius: '16px',
                    padding: '14px',
                    border: '0.5px solid var(--color-border-tertiary)',
                    background: 'var(--color-background-primary)',
                  }}>
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '10px' }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                        <span style={{ width: '10px', height: '10px', borderRadius: '50%', background: getPupAccent(member), flexShrink: 0 }} />
                        <span style={{ fontSize: '13px', fontWeight: 600, color: 'var(--color-text-primary)' }}>{member}</span>
                      </div>
                      <span style={{
                        fontSize: '11px',
                        fontWeight: 700,
                        padding: '4px 8px',
                        borderRadius: '999px',
                        color: statusTone(tone).color,
                        background: statusTone(tone).background,
                      }}>
                        {label}
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            )}

            {tab === 'artifacts' && (
              <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
                {artifacts.length > 0 ? artifacts.map((item) => (
                  <div key={item.id} style={{
                    borderRadius: '16px',
                    padding: '14px',
                    border: '0.5px solid var(--color-border-tertiary)',
                    background: 'var(--color-background-primary)',
                  }}>
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: '8px', marginBottom: '8px' }}>
                      <span style={{ fontSize: '13px', fontWeight: 600, color: 'var(--color-text-primary)' }}>
                        {item.artifact_name || t('ctx_unnamed_artifact', lang)}
                      </span>
                      <span style={{ fontSize: '11px', color: 'var(--color-text-tertiary)' }}>{formatRelativeTime(item.timestamp)}</span>
                    </div>
                    <div style={{ fontSize: '12px', color: 'var(--color-text-secondary)', lineHeight: 1.55, whiteSpace: 'pre-wrap' }}>
                      {item.content.split('\n').slice(0, 4).join('\n')}
                    </div>
                  </div>
                )) : (
                  <div style={{ fontSize: '13px', color: 'var(--color-text-tertiary)', lineHeight: 1.7 }}>
                    {t('ctx_no_artifacts', lang)}
                  </div>
                )}
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
};

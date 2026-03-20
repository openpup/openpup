import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useLang, t } from '../i18n';

interface InstalledSkill {
  name: string;
  description: string;
  category: string;
  source: string;
  enabled: boolean;
}

const SOURCE_BADGE: Record<string, React.CSSProperties> = {
  builtin: { background: 'var(--color-background-info)', color: 'var(--color-text-info)' },
  core:    { background: 'var(--color-background-info)', color: 'var(--color-text-info)' },
};

const DEFAULT_BADGE_STYLE: React.CSSProperties = {
  background: 'var(--color-background-secondary)',
  color: 'var(--color-text-secondary)',
};

const SOURCE_LABEL: Record<string, string> = {
  git:   'ClawHub',
  local: 'Local',
};

export const SkillClaw: React.FC = () => {
  const { lang } = useLang();
  const [skills, setSkills] = useState<InstalledSkill[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');

  const loadSkills = async () => {
    setLoading(true);
    try { setSkills(await invoke<InstalledSkill[]>('list_skills')); }
    catch (e) { setError(String(e)); }
    finally { setLoading(false); }
  };

  useEffect(() => { void loadSkills(); }, []);

  const toggleEnabled = async (skill: InstalledSkill) => {
    try {
      await invoke('set_skill_enabled', { name: skill.name, enabled: !skill.enabled });
      setSkills((prev) => prev.map((s) => s.name === skill.name ? { ...s, enabled: !s.enabled } : s));
    } catch (e) { setError(String(e)); }
  };

  const filtered = skills.filter((s) =>
    !search ||
    s.name.toLowerCase().includes(search.toLowerCase()) ||
    s.description.toLowerCase().includes(search.toLowerCase())
  );

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '12px', height: '100%' }}>
      {error && (
        <div style={{
          fontSize: '13px',
          color: 'var(--color-text-danger)',
          background: 'var(--color-background-danger)',
          padding: '8px 12px',
          borderRadius: '8px',
          flexShrink: 0,
        }}>
          {error}
        </div>
      )}

      <input
        style={{
          width: '100%',
          borderRadius: '8px',
          background: 'var(--color-background-secondary)',
          border: '1px solid var(--color-border-secondary)',
          padding: '10px 12px',
          fontSize: '15px',
          color: 'var(--color-text-primary)',
          outline: 'none',
          flexShrink: 0,
          boxSizing: 'border-box',
        }}
        placeholder={t('skills_search_ph', lang)}
        value={search}
        onChange={(e) => setSearch(e.target.value)}
      />

      <div style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: '6px', minHeight: 0 }}>
        {loading && (
          <p style={{ fontSize: '13px', color: 'var(--color-text-tertiary)', textAlign: 'center', padding: '16px 0', margin: 0 }}>
            {t('diary_loading', lang)}
          </p>
        )}
        {!loading && filtered.length === 0 && (
          <p style={{ fontSize: '13px', color: 'var(--color-text-tertiary)', textAlign: 'center', padding: '16px 0', margin: 0 }}>
            {skills.length === 0 ? t('skills_empty', lang) : t('skills_no_match', lang)}
          </p>
        )}
        {filtered.map((skill) => (
          <div
            key={skill.name}
            style={{
              display: 'flex',
              alignItems: 'flex-start',
              justifyContent: 'space-between',
              borderRadius: '12px',
              border: '1px solid var(--color-border-tertiary)',
              background: 'var(--color-background-secondary)',
              padding: '12px 16px',
              fontSize: '15px',
              gap: '8px',
            }}
          >
            <div style={{ minWidth: 0, flex: 1 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
                <span style={{
                  fontWeight: 500,
                  color: skill.enabled ? 'var(--color-text-primary)' : 'var(--color-text-tertiary)',
                }}>
                  {skill.name}
                </span>
                {SOURCE_LABEL[skill.source] && (
                  <span style={{
                    fontSize: '12px',
                    padding: '2px 6px',
                    borderRadius: '9999px',
                    ...(SOURCE_BADGE[skill.source] ?? DEFAULT_BADGE_STYLE),
                  }}>
                    {SOURCE_LABEL[skill.source]}
                  </span>
                )}
                {skill.category && (
                  <span style={{
                    fontSize: '12px',
                    padding: '2px 6px',
                    borderRadius: '9999px',
                    background: 'var(--color-background-secondary)',
                    color: 'var(--color-text-secondary)',
                  }}>
                    {skill.category}
                  </span>
                )}
              </div>
              {skill.description && (
                <p style={{
                  color: 'var(--color-text-tertiary)',
                  marginTop: '2px',
                  lineHeight: 1.5,
                  display: '-webkit-box',
                  WebkitLineClamp: 2,
                  WebkitBoxOrient: 'vertical',
                  overflow: 'hidden',
                  margin: '2px 0 0 0',
                  fontSize: '13px',
                }}>
                  {skill.description}
                </p>
              )}
            </div>
            <button
              style={{
                padding: '6px 12px',
                borderRadius: '8px',
                fontSize: '13px',
                border: 'none',
                cursor: 'pointer',
                flexShrink: 0,
                fontWeight: 500,
                background: 'var(--color-background-secondary)',
                color: skill.enabled ? '#1D9E75' : 'var(--color-text-tertiary)',
              }}
              onClick={() => void toggleEnabled(skill)}
            >
              {skill.enabled ? t('skills_enable', lang) : t('skills_disable', lang)}
            </button>
          </div>
        ))}
      </div>
    </div>
  );
};

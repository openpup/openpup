import React, { useDeferredValue, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useLang, t } from '../i18n';

interface InstalledSkill {
  name: string;
  description: string;
  category: string;
  source: string;
  enabled: boolean;
}

type SkillFilter = 'all' | 'enabled' | 'disabled';

const inputStyle: React.CSSProperties = {
  borderRadius: 8,
  background: 'var(--color-background-primary)',
  border: '1px solid var(--color-border-secondary)',
  padding: '8px 12px',
  fontSize: 12,
  color: 'var(--color-text-primary)',
  outline: 'none',
  width: '100%',
  boxSizing: 'border-box',
};

const cardStyle: React.CSSProperties = {
  borderRadius: 12,
  border: '1px solid var(--color-border-tertiary)',
  background: 'var(--color-background-secondary)',
  padding: '12px 16px',
};

const pillStyle: React.CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 6,
  padding: '3px 8px',
  borderRadius: 999,
  fontSize: 11,
  fontWeight: 600,
  background: 'var(--color-background-primary)',
  border: '1px solid var(--color-border-tertiary)',
  color: 'var(--color-text-secondary)',
};

const activeButtonStyle: React.CSSProperties = {
  padding: '4px 8px',
  borderRadius: 8,
  background: 'var(--color-background-success)',
  color: 'var(--color-text-success)',
  border: 'none',
  cursor: 'pointer',
  fontSize: 12,
};

const inactiveButtonStyle: React.CSSProperties = {
  padding: '4px 8px',
  borderRadius: 8,
  background: 'var(--color-background-primary)',
  color: 'var(--color-text-tertiary)',
  border: '1px solid var(--color-border-secondary)',
  cursor: 'pointer',
  fontSize: 12,
};

const sourceLabel = (source: string) => {
  if (source === 'git') return 'ClawHub';
  if (source === 'local') return 'Local';
  if (source === 'builtin') return 'Built-in';
  if (source === 'core') return 'Core';
  return source;
};

export const SkillClaw: React.FC = () => {
  const { lang } = useLang();
  const [skills, setSkills] = useState<InstalledSkill[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [filter, setFilter] = useState<SkillFilter>('all');
  const deferredSearch = useDeferredValue(search.trim().toLowerCase());

  const loadSkills = async (refresh = false) => {
    setLoading(true);
    setError(null);
    try {
      setSkills(await invoke<InstalledSkill[]>(refresh ? 'refresh_skills' : 'list_skills'));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadSkills();
  }, []);

  const toggleEnabled = async (skill: InstalledSkill) => {
    try {
      await invoke('set_skill_enabled', { name: skill.name, enabled: !skill.enabled });
      setSkills((prev) => prev.map((item) => item.name === skill.name ? { ...item, enabled: !item.enabled } : item));
    } catch (e) {
      setError(String(e));
    }
  };

  const filtered = useMemo(() => {
    return skills.filter((skill) => {
      if (filter === 'enabled' && !skill.enabled) return false;
      if (filter === 'disabled' && skill.enabled) return false;
      if (!deferredSearch) return true;
      return `${skill.name} ${skill.description} ${skill.category} ${skill.source}`.toLowerCase().includes(deferredSearch);
    });
  }, [skills, filter, deferredSearch]);

  const enabledCount = skills.filter((skill) => skill.enabled).length;
  const disabledCount = skills.length - enabledCount;
  const sourceCount = new Set(skills.map((skill) => skill.source)).size;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12, height: '100%', fontSize: 12 }}>
      {error && (
        <div style={{
          fontSize: '13px',
          color: 'var(--color-text-danger)',
          background: 'var(--color-background-danger)',
          padding: '6px 12px',
          borderRadius: '8px',
          flexShrink: 0,
        }}>
          {error}
        </div>
      )}

      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', flexShrink: 0 }}>
        <span style={pillStyle}>{t('skills_stat_total', lang)} · {skills.length}</span>
        <span style={pillStyle}>{t('skills_stat_enabled', lang)} · {enabledCount}</span>
        <span style={pillStyle}>{t('skills_stat_disabled', lang)} · {disabledCount}</span>
        <span style={pillStyle}>{t('skills_stat_sources', lang)} · {sourceCount}</span>
      </div>

      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', flexShrink: 0 }}>
        <input
          style={{ ...inputStyle, flex: '1 1 260px' }}
          placeholder={t('skills_search_ph', lang)}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <button onClick={() => setFilter('all')} style={filter === 'all' ? activeButtonStyle : inactiveButtonStyle}>{t('tab_all', lang)}</button>
        <button onClick={() => setFilter('enabled')} style={filter === 'enabled' ? activeButtonStyle : inactiveButtonStyle}>{t('mcp_enabled', lang)}</button>
        <button onClick={() => setFilter('disabled')} style={filter === 'disabled' ? activeButtonStyle : inactiveButtonStyle}>{t('mcp_disabled', lang)}</button>
        <button onClick={() => void loadSkills(true)} style={inactiveButtonStyle} disabled={loading}>{t('skills_refresh', lang)}</button>
      </div>

      <div style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: '8px', minHeight: 0 }}>
        {loading && (
          <p style={{ fontSize: '13px', color: 'var(--color-text-tertiary)', textAlign: 'center', padding: '16px 0', margin: 0 }}>
            {t('diary_loading', lang)}
          </p>
        )}
        {!loading && filtered.length === 0 && (
          <p style={{ fontSize: '13px', color: 'var(--color-text-tertiary)', textAlign: 'center', padding: '16px 0', margin: 0 }}>
            {skills.length === 0 ? t('skills_empty', lang) : t('skills_no_match_body', lang)}
          </p>
        )}
        {filtered.map((skill) => (
          <div
            key={skill.name}
            style={{
              ...cardStyle,
              display: 'flex',
              alignItems: 'flex-start',
              justifyContent: 'space-between',
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
                <span style={{
                  ...pillStyle,
                  background: skill.enabled ? 'var(--color-background-success)' : 'var(--color-background-primary)',
                  color: skill.enabled ? 'var(--color-text-success)' : 'var(--color-text-tertiary)',
                  border: skill.enabled ? 'none' : '1px solid var(--color-border-secondary)',
                }}>
                  {skill.enabled ? t('mcp_enabled', lang) : t('mcp_disabled', lang)}
                </span>
                <span style={pillStyle}>{sourceLabel(skill.source)}</span>
                {skill.category && <span style={pillStyle}>{skill.category}</span>}
              </div>
              {skill.description && (
                <p style={{
                  color: 'var(--color-text-tertiary)',
                  marginTop: '4px',
                  lineHeight: 1.5,
                  margin: '4px 0 0 0',
                  fontSize: '13px',
                }}>
                  {skill.description}
                </p>
              )}
            </div>
            <button
              style={skill.enabled ? inactiveButtonStyle : activeButtonStyle}
              onClick={() => void toggleEnabled(skill)}
            >
              {skill.enabled ? t('skills_disable', lang) : t('skills_enable', lang)}
            </button>
          </div>
        ))}
      </div>
    </div>
  );
};

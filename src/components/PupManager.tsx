import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useLang, t } from '../i18n';

interface PupConfig {
  key: string;
  display_name: string;
  description: string;
  system_prompt_override: string;
  enabled: boolean;
  is_custom: boolean;
}

const inputStyle: React.CSSProperties = {
  borderRadius: 8,
  background: 'var(--color-background-primary)',
  border: '1px solid var(--color-border-secondary)',
  padding: '6px 12px',
  fontSize: 12,
  color: 'var(--color-text-primary)',
  outline: 'none',
  width: '100%',
};

export const PupManager: React.FC = () => {
  const { lang } = useLang();
  const [pups, setPups] = useState<PupConfig[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [draftPrompt, setDraftPrompt] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const [addKey, setAddKey] = useState('');
  const [addName, setAddName] = useState('');
  const [addDesc, setAddDesc] = useState('');
  const [addPrompt, setAddPrompt] = useState('');
  const [adding, setAdding] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const load = () =>
    invoke<PupConfig[]>('list_pups').then(setPups).catch((e) => setError(String(e)));

  useEffect(() => { void load(); }, []);

  const flash = (m: string) => { setMsg(m); setTimeout(() => setMsg(null), 2000); };

  const save = async (key: string, enabled: boolean) => {
    setError(null);
    try {
      await invoke('update_pup', { key, systemPromptOverride: draftPrompt, enabled });
      setEditing(null);
      await load();
      flash(lang === 'zh' ? '已保存' : 'Saved');
    } catch (e) { setError(String(e)); }
  };

  const toggle = async (pup: PupConfig) => {
    setError(null);
    try {
      await invoke('update_pup', {
        key: pup.key,
        systemPromptOverride: pup.system_prompt_override,
        enabled: !pup.enabled,
      });
      await load();
    } catch (e) { setError(String(e)); }
  };

  const removePup = async (key: string) => {
    try {
      await invoke('remove_custom_pup', { key });
      setConfirmDelete(null);
      await load();
      flash(lang === 'zh' ? '已删除' : 'Removed');
    } catch (e) { setError(String(e)); }
  };

  const addPup = async () => {
    if (!addKey.trim() || !addName.trim() || !addPrompt.trim()) return;
    setAdding(true);
    setError(null);
    try {
      await invoke('add_custom_pup', {
        key: addKey.trim().toLowerCase().replace(/\s+/g, '_'),
        displayName: addName.trim(),
        description: addDesc.trim(),
        systemPrompt: addPrompt.trim(),
      });
      setAddKey(''); setAddName(''); setAddDesc(''); setAddPrompt('');
      await load();
      flash(lang === 'zh' ? '已添加' : 'Added');
    } catch (e) { setError(String(e)); }
    finally { setAdding(false); }
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12, fontSize: 12 }}>
      <h3 style={{ fontSize: 14, fontWeight: 500, color: 'var(--color-text-primary)', margin: 0 }}>
        {t('pup_mgr_title', lang)}
      </h3>
      {error && (
        <p style={{
          color: 'var(--color-text-danger)',
          background: 'var(--color-background-danger)',
          padding: '6px 12px',
          borderRadius: 8,
          margin: 0,
        }}>
          {error}
        </p>
      )}
      {msg && (
        <p style={{
          color: 'var(--color-text-success)',
          background: 'var(--color-background-success)',
          padding: '6px 12px',
          borderRadius: 8,
          margin: 0,
        }}>
          {msg}
        </p>
      )}

      {/* Pup list */}
      {pups.map((pup) => (
        <div
          key={pup.key}
          style={{
            borderRadius: 12,
            border: '1px solid var(--color-border-tertiary)',
            background: 'var(--color-background-secondary)',
            padding: '12px 16px',
            display: 'flex',
            flexDirection: 'column',
            gap: 8,
            opacity: pup.enabled ? 1 : 0.6,
          }}
        >
          <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 8 }}>
            <div style={{ flex: 1, minWidth: 0 }}>
              <span style={{ fontWeight: 500, color: 'var(--color-text-primary)' }}>{pup.display_name}</span>
              {pup.is_custom && (
                <span style={{
                  marginLeft: 6,
                  fontSize: 10,
                  padding: '1px 6px',
                  borderRadius: 999,
                  background: 'var(--color-background-info)',
                  color: 'var(--color-text-info)',
                }}>
                  {t('pup_custom', lang)}
                </span>
              )}
              <div style={{ color: 'var(--color-text-tertiary)', marginTop: 2 }}>{pup.description}</div>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexShrink: 0 }}>
              <button
                onClick={() => {
                  setEditing(editing === pup.key ? null : pup.key);
                  setDraftPrompt(pup.system_prompt_override);
                }}
                style={{
                  padding: '4px 8px',
                  borderRadius: 8,
                  background: 'var(--color-background-primary)',
                  border: '1px solid var(--color-border-secondary)',
                  color: 'var(--color-text-secondary)',
                  cursor: 'pointer',
                  fontSize: 12,
                }}
              >
                {editing === pup.key ? t('pup_collapse', lang) : t('pup_edit', lang)}
              </button>
              <button
                onClick={() => void toggle(pup)}
                style={pup.enabled ? {
                  padding: '4px 8px',
                  borderRadius: 8,
                  background: 'var(--color-background-success)',
                  color: 'var(--color-text-success)',
                  border: 'none',
                  cursor: 'pointer',
                  fontSize: 12,
                } : {
                  padding: '4px 8px',
                  borderRadius: 8,
                  background: 'var(--color-background-secondary)',
                  color: 'var(--color-text-tertiary)',
                  border: 'none',
                  cursor: 'pointer',
                  fontSize: 12,
                }}
              >
                {pup.enabled ? t('pup_enabled', lang) : t('pup_disabled', lang)}
              </button>
              {pup.is_custom && (
                confirmDelete === pup.key ? (
                  <>
                    <button
                      onClick={() => void removePup(pup.key)}
                      style={{ padding: '4px 8px', borderRadius: 8, background: 'var(--color-background-danger)', color: 'var(--color-text-danger)', border: 'none', cursor: 'pointer', fontSize: 12 }}
                    >
                      {lang === 'zh' ? '确认删除' : 'Confirm'}
                    </button>
                    <button
                      onClick={() => setConfirmDelete(null)}
                      style={{ padding: '4px 8px', borderRadius: 8, background: 'var(--color-background-secondary)', color: 'var(--color-text-secondary)', border: 'none', cursor: 'pointer', fontSize: 12 }}
                    >
                      {lang === 'zh' ? '取消' : 'Cancel'}
                    </button>
                  </>
                ) : (
                  <button
                    onClick={() => setConfirmDelete(pup.key)}
                    style={{ padding: '4px 8px', borderRadius: 8, background: 'var(--color-background-danger)', color: 'var(--color-text-danger)', border: 'none', cursor: 'pointer', fontSize: 12 }}
                  >
                    {t('pup_remove', lang)}
                  </button>
                )
              )}
            </div>
          </div>

          {editing === pup.key && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8, paddingTop: 4 }}>
              <p style={{ color: 'var(--color-text-tertiary)', margin: 0 }}>{t('pup_prompt_label', lang)}</p>
              <textarea
                style={{
                  ...inputStyle,
                  height: 112,
                  resize: 'vertical',
                  fontFamily: 'inherit',
                }}
                placeholder={pup.is_custom ? t('pup_prompt_custom', lang) : t('pup_prompt_builtin', lang)}
                value={draftPrompt}
                onChange={(e) => setDraftPrompt(e.target.value)}
              />
              <div style={{ display: 'flex', gap: 8 }}>
                <button
                  onClick={() => void save(pup.key, pup.enabled)}
                  style={{
                    padding: '6px 12px',
                    borderRadius: 8,
                    background: 'var(--color-text-primary)',
                    color: 'var(--color-background-primary)',
                    border: 'none',
                    cursor: 'pointer',
                    fontSize: 12,
                    fontWeight: 500,
                  }}
                >
                  {t('pup_save', lang)}
                </button>
                <button
                  onClick={() => setEditing(null)}
                  style={{
                    padding: '6px 12px',
                    borderRadius: 8,
                    background: 'var(--color-background-primary)',
                    border: '1px solid var(--color-border-secondary)',
                    color: 'var(--color-text-secondary)',
                    cursor: 'pointer',
                    fontSize: 12,
                  }}
                >
                  {t('pup_cancel', lang)}
                </button>
              </div>
            </div>
          )}
        </div>
      ))}

      {/* Add custom pup */}
      <div style={{
        borderRadius: 12,
        border: '1px solid var(--color-border-tertiary)',
        background: 'var(--color-background-secondary)',
        padding: '12px 16px',
        display: 'flex',
        flexDirection: 'column',
        gap: 10,
      }}>
        <div style={{ fontWeight: 500, color: 'var(--color-text-secondary)' }}>{t('pup_add_title', lang)}</div>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8 }}>
          <input
            style={inputStyle}
            placeholder={t('pup_key_ph', lang)}
            value={addKey}
            onChange={(e) => setAddKey(e.target.value)}
          />
          <input
            style={inputStyle}
            placeholder={t('pup_name_ph', lang)}
            value={addName}
            onChange={(e) => setAddName(e.target.value)}
          />
          <input
            style={{ ...inputStyle, gridColumn: '1 / -1' }}
            placeholder={t('pup_desc_ph', lang)}
            value={addDesc}
            onChange={(e) => setAddDesc(e.target.value)}
          />
        </div>
        <textarea
          style={{
            ...inputStyle,
            height: 96,
            resize: 'vertical',
            fontFamily: 'inherit',
          }}
          placeholder={t('pup_prompt_ph', lang)}
          value={addPrompt}
          onChange={(e) => setAddPrompt(e.target.value)}
        />
        <button
          style={{
            padding: '8px 12px',
            borderRadius: 8,
            background: 'var(--color-text-primary)',
            color: 'var(--color-background-primary)',
            border: 'none',
            cursor: 'pointer',
            fontSize: 12,
            fontWeight: 500,
            opacity: (adding || !addKey.trim() || !addName.trim() || !addPrompt.trim()) ? 0.5 : 1,
          }}
          disabled={adding || !addKey.trim() || !addName.trim() || !addPrompt.trim()}
          onClick={() => void addPup()}
        >
          {adding ? t('pup_adding', lang) : t('pup_add_btn', lang)}
        </button>
      </div>
    </div>
  );
};

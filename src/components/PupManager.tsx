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

  const INPUT = 'rounded-lg bg-stone-800 border border-stone-700 px-3 py-2 text-xs text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-1 focus:ring-amber-500/50';

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
    if (!window.confirm(`删除自定义 Pup "${key}"？`)) return;
    try {
      await invoke('remove_custom_pup', { key });
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
    <div className="space-y-3 text-xs">
      <h3 className="text-sm font-semibold text-stone-100">{t('pup_mgr_title', lang)}</h3>
      {error && <p className="text-red-400 bg-red-900/20 px-3 py-2 rounded-lg">{error}</p>}
      {msg && <p className="text-emerald-400 bg-emerald-900/20 px-3 py-2 rounded-lg">{msg}</p>}

      {/* Pup list */}
      {pups.map((pup) => (
        <div
          key={pup.key}
          className={`rounded-xl border px-4 py-3 space-y-2 transition-colors ${
            pup.enabled ? 'border-stone-700 bg-stone-900/40' : 'border-stone-800 opacity-60'
          }`}
        >
          <div className="flex items-start justify-between gap-2">
            <div className="flex-1 min-w-0">
              <span className="font-medium text-stone-100">{pup.display_name}</span>
              {pup.is_custom && (
                <span className="ml-1.5 text-[10px] px-1.5 py-0.5 rounded-full bg-violet-900/60 text-violet-300">
                  {t('pup_custom', lang)}
                </span>
              )}
              <div className="text-stone-500 mt-0.5">{pup.description}</div>
            </div>
            <div className="flex items-center gap-1.5 shrink-0">
              <button
                onClick={() => {
                  setEditing(editing === pup.key ? null : pup.key);
                  setDraftPrompt(pup.system_prompt_override);
                }}
                className="px-2 py-1 rounded-lg bg-stone-700 text-stone-300 hover:bg-stone-600 transition-colors"
              >
                {editing === pup.key ? t('pup_collapse', lang) : t('pup_edit', lang)}
              </button>
              <button
                onClick={() => void toggle(pup)}
                className={`px-2 py-1 rounded-lg transition-colors ${
                  pup.enabled
                    ? 'bg-emerald-900/60 text-emerald-300 hover:bg-emerald-900/80'
                    : 'bg-stone-700 text-stone-400 hover:bg-stone-600'
                }`}
              >
                {pup.enabled ? t('pup_enabled', lang) : t('pup_disabled', lang)}
              </button>
              {pup.is_custom && (
                <button
                  onClick={() => void removePup(pup.key)}
                  className="px-2 py-1 rounded-lg bg-red-900/40 text-red-300 hover:bg-red-900/60 transition-colors"
                >
                  {t('pup_remove', lang)}
                </button>
              )}
            </div>
          </div>

          {editing === pup.key && (
            <div className="space-y-2 pt-1">
              <p className="text-stone-500">{t('pup_prompt_label', lang)}</p>
              <textarea
                className="w-full h-28 rounded-lg bg-stone-800 border border-stone-700 px-3 py-2 text-xs text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-1 focus:ring-amber-500/50 resize-y"
                placeholder={pup.is_custom ? t('pup_prompt_custom', lang) : t('pup_prompt_builtin', lang)}
                value={draftPrompt}
                onChange={(e) => setDraftPrompt(e.target.value)}
              />
              <div className="flex gap-2">
                <button
                  onClick={() => void save(pup.key, pup.enabled)}
                  className="px-3 py-1.5 rounded-lg bg-amber-500 text-stone-950 font-medium hover:bg-amber-400 transition-colors"
                >
                  {t('pup_save', lang)}
                </button>
                <button
                  onClick={() => setEditing(null)}
                  className="px-3 py-1.5 rounded-lg bg-stone-700 text-stone-300 hover:bg-stone-600 transition-colors"
                >
                  {t('pup_cancel', lang)}
                </button>
              </div>
            </div>
          )}
        </div>
      ))}

      {/* Add custom pup */}
      <div className="rounded-xl border border-stone-700 bg-stone-900/40 px-4 py-3 space-y-2.5">
        <div className="font-medium text-stone-300 mb-1">{t('pup_add_title', lang)}</div>
        <div className="grid grid-cols-2 gap-2">
          <input
            className={INPUT}
            placeholder={t('pup_key_ph', lang)}
            value={addKey}
            onChange={(e) => setAddKey(e.target.value)}
          />
          <input
            className={INPUT}
            placeholder={t('pup_name_ph', lang)}
            value={addName}
            onChange={(e) => setAddName(e.target.value)}
          />
          <input
            className={INPUT + ' col-span-2'}
            placeholder={t('pup_desc_ph', lang)}
            value={addDesc}
            onChange={(e) => setAddDesc(e.target.value)}
          />
        </div>
        <textarea
          className="w-full h-24 rounded-lg bg-stone-800 border border-stone-700 px-3 py-2 text-xs text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-1 focus:ring-amber-500/50 resize-y"
          placeholder={t('pup_prompt_ph', lang)}
          value={addPrompt}
          onChange={(e) => setAddPrompt(e.target.value)}
        />
        <button
          className="px-3 py-2 rounded-lg bg-violet-600 text-white text-xs font-medium disabled:opacity-50 hover:bg-violet-500 transition-colors"
          disabled={adding || !addKey.trim() || !addName.trim() || !addPrompt.trim()}
          onClick={() => void addPup()}
        >
          {adding ? t('pup_adding', lang) : t('pup_add_btn', lang)}
        </button>
      </div>
    </div>
  );
};

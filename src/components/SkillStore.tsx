import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useLang, t } from '../i18n';

interface InstalledSkill {
  name: string;
  description: string;
  category: string;
  source: string;
  repo_url?: string | null;
  installed_at: number;
  enabled: boolean;
}

interface VettingReport {
  risk_level: string;
  summary: string;
  flags: string[];
  recommendation: string;
}

interface PendingInstall {
  url: string;
  subdir: string | null;
  report: VettingReport;
}

const RISK_COLORS: Record<string, string> = {
  safe: 'text-emerald-400',
  low: 'text-sky-400',
  medium: 'text-yellow-400',
  high: 'text-orange-400',
  critical: 'text-red-400',
};

const INPUT = 'w-full rounded-lg bg-stone-800 border border-stone-700 px-3 py-2 text-xs text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-1 focus:ring-amber-500/50';

export const SkillStore: React.FC = () => {
  const { lang } = useLang();
  const [skills, setSkills] = useState<InstalledSkill[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [repoUrl, setRepoUrl] = useState('');
  const [subdir, setSubdir] = useState('');
  const [installing, setInstalling] = useState<string | null>(null);
  const [vetting, setVetting] = useState<string | null>(null);
  const [pendingInstall, setPendingInstall] = useState<PendingInstall | null>(null);

  const loadInstalled = async () => {
    setLoading(true);
    setError(null);
    try { setSkills(await invoke<InstalledSkill[]>('list_skills')); }
    catch (e) { setError(String(e)); }
    finally { setLoading(false); }
  };

  useEffect(() => {
    void loadInstalled();
    // Hot-reload: any skill install (git, LLM-generated, suggestion banner) fires this
    const unlisten = listen('skill_installed', () => { void loadInstalled(); });
    return () => { void unlisten.then((f) => f()); };
  }, []);

  const toggleEnabled = async (skill: InstalledSkill) => {
    try {
      await invoke('set_skill_enabled', { name: skill.name, enabled: !skill.enabled });
      setSkills((prev) => prev.map((s) => s.name === skill.name ? { ...s, enabled: !s.enabled } : s));
    } catch (e) { setError(String(e)); }
  };

  const uninstall = async (name: string) => {
    if (!window.confirm(`卸载 skill "${name}"？`)) return;
    try {
      await invoke('uninstall_skill', { name });
      setSkills((prev) => prev.filter((s) => s.name !== name));
    } catch (e) { setError(String(e)); }
  };

  const doInstall = async (url: string, sub: string | null) => {
    setInstalling(url);
    setError(null);
    try {
      await invoke('install_skill_from_git', { repoUrl: url, subdir: sub });
      if (url === repoUrl.trim()) { setRepoUrl(''); setSubdir(''); }
      await loadInstalled();
    } catch (e) { setError(String(e)); }
    finally { setInstalling(null); }
  };

  const installFromGit = async () => {
    const targetUrl = repoUrl.trim();
    if (!targetUrl) return;
    const targetSubdir = subdir.trim() || null;
    setVetting(targetUrl);
    setError(null);
    try {
      let manifestText: string;
      try {
        manifestText = await invoke<string>('fetch_skill_manifest', {
          repoUrl: targetUrl,
          subdir: targetSubdir,
        });
      } catch {
        await doInstall(targetUrl, targetSubdir);
        return;
      }
      let reportRaw: string;
      try {
        reportRaw = await invoke<string>('run_skill', { name: 'skill_vetting', input: manifestText });
      } catch {
        await doInstall(targetUrl, targetSubdir);
        return;
      }
      try {
        const clean = reportRaw.trim()
          .replace(/^```json\s*/i, '').replace(/^```/, '').replace(/```$/, '').trim();
        const report: VettingReport = JSON.parse(clean);
        if (report.recommendation === 'install_safe') {
          await doInstall(targetUrl, targetSubdir);
        } else {
          setPendingInstall({ url: targetUrl, subdir: targetSubdir, report });
        }
      } catch {
        setPendingInstall({
          url: targetUrl,
          subdir: targetSubdir,
          report: {
            risk_level: 'unknown',
            summary: '无法解析安全报告，请谨慎安装。',
            flags: [],
            recommendation: 'install_with_caution',
          },
        });
      }
    } finally {
      setVetting(null);
    }
  };

  return (
    <div className="flex flex-col gap-3 h-full">
      {/* Vetting dialog */}
      {pendingInstall && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
          <div className="bg-stone-900 border border-stone-700 rounded-2xl shadow-2xl p-5 max-w-sm w-full mx-4 space-y-3 text-xs">
            <div className="flex items-center gap-2">
              <span className="text-base">🔍</span>
              <span className="font-semibold text-sm text-stone-100">{t('vetting_title', lang)}</span>
              <span className={`ml-auto font-bold uppercase text-xs ${RISK_COLORS[pendingInstall.report.risk_level] ?? 'text-stone-400'}`}>
                {pendingInstall.report.risk_level}
              </span>
            </div>
            <p className="text-stone-300">{pendingInstall.report.summary}</p>
            {pendingInstall.report.flags.length > 0 && (
              <ul className="space-y-1 pl-3">
                {pendingInstall.report.flags.map((f, i) => (
                  <li key={i} className="text-orange-300 list-disc">{f}</li>
                ))}
              </ul>
            )}
            <div className="flex gap-2 pt-1">
              <button
                className="flex-1 px-3 py-1.5 rounded-lg bg-red-900/60 text-red-300 hover:bg-red-900/80 transition-colors"
                onClick={() => setPendingInstall(null)}
              >
                {t('vetting_cancel', lang)}
              </button>
              {pendingInstall.report.recommendation !== 'do_not_install' && (
                <button
                  className="flex-1 px-3 py-1.5 rounded-lg bg-amber-500 text-stone-950 font-medium hover:bg-amber-400 transition-colors"
                  onClick={async () => {
                    const { url, subdir: sub } = pendingInstall;
                    setPendingInstall(null);
                    await doInstall(url, sub);
                  }}
                >
                  {pendingInstall.report.recommendation === 'install_with_caution'
                    ? t('vetting_caution', lang)
                    : t('vetting_confirm', lang)}
                </button>
              )}
            </div>
          </div>
        </div>
      )}

      {error && <div className="text-xs text-red-400 bg-red-900/20 px-3 py-2 rounded-lg">{error}</div>}

      {/* Install from Git */}
      <div className="rounded-xl border border-stone-800 bg-stone-900/40 px-4 py-3 space-y-2.5">
        <div className="text-xs font-medium text-stone-300">{t('skills_git_title', lang)}</div>
        <input className={INPUT}
          placeholder={t('skills_repo', lang)}
          value={repoUrl} onChange={(e) => setRepoUrl(e.target.value)} />
        <input className={INPUT}
          placeholder={t('skills_subdir', lang)}
          value={subdir} onChange={(e) => setSubdir(e.target.value)} />
        <button
          className="px-3 py-2 rounded-lg bg-amber-500 text-stone-950 text-xs font-medium disabled:opacity-50 hover:bg-amber-400 transition-colors"
          disabled={!!installing || !!vetting || !repoUrl.trim()}
          onClick={() => void installFromGit()}
        >
          {vetting ? t('skills_vetting', lang) : installing ? t('skills_installing', lang) : t('skills_install_btn', lang)}
        </button>
      </div>

      {/* Installed skills */}
      <div className="text-xs font-medium text-stone-400 px-1">{t('skills_installed', lang)}</div>

      {loading && <p className="text-xs text-stone-500">加载中…</p>}
      {!loading && skills.length === 0 && <p className="text-xs text-stone-500">{t('skills_empty', lang)}</p>}

      {skills.map((skill) => (
        <div key={skill.name} className="flex items-start justify-between rounded-xl border border-stone-800 bg-stone-900/40 px-4 py-3 text-xs gap-2 hover:border-stone-700 transition-colors">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="font-medium text-stone-100">{skill.name}</span>
              {skill.category && <span className="px-1.5 py-0.5 rounded-full bg-stone-700 text-stone-300 text-[10px]">{skill.category}</span>}
              {skill.source === 'builtin' && <span className="px-1.5 py-0.5 rounded-full bg-amber-900/40 text-amber-300 text-[10px]">{t('skills_builtin', lang)}</span>}
            </div>
            <p className="text-stone-500 mt-0.5">{skill.description}</p>
          </div>
          <div className="flex items-center gap-1.5 shrink-0">
            <button
              className={`px-2 py-1 rounded-lg text-[11px] transition-colors ${skill.enabled ? 'bg-emerald-900/60 text-emerald-300 hover:bg-emerald-900/80' : 'bg-stone-700 text-stone-300 hover:bg-stone-600'}`}
              onClick={() => void toggleEnabled(skill)}
            >
              {skill.enabled ? t('skills_disable', lang) : t('skills_enable', lang)}
            </button>
            {skill.source !== 'builtin' && (
              <button className="px-2 py-1 rounded-lg bg-red-900/40 text-red-400 text-[11px] hover:bg-red-900/60 transition-colors"
                onClick={() => void uninstall(skill.name)}>{t('skills_uninstall', lang)}</button>
            )}
          </div>
        </div>
      ))}
    </div>
  );
};

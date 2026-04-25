import React from 'react';
import { invoke } from '@tauri-apps/api/core';
import { t, useLang } from '../i18n';
import { useAppStore } from '../stores/appStore';
import type { KbSummaryFrequency } from '../stores/appStore';

interface KbSettingsInfo {
  auto_ingest_summaries: boolean;
  auto_ingest_artifacts: boolean;
  summary_frequency: KbSummaryFrequency;
}

interface LocalKbSettings {
  autoIngestSummaries: boolean;
  autoIngestArtifacts: boolean;
  summaryFrequency: KbSummaryFrequency;
}

const KB_SUMMARY_FREQUENCY_OPTIONS: KbSummaryFrequency[] = ['frequent', 'standard', 'conservative'];

export const KnowledgeSettings: React.FC = () => {
  const { lang } = useLang();
  const { setSettingsErr } = useAppStore();
  const [settings, setSettings] = React.useState<LocalKbSettings>({
    autoIngestSummaries: true,
    autoIngestArtifacts: true,
    summaryFrequency: 'standard',
  });
  const [loadFailed, setLoadFailed] = React.useState(false);
  const [saveState, setSaveState] = React.useState<'idle' | 'saving' | 'saved'>('idle');
  const saveTimerRef = React.useRef<number | null>(null);

  const showSaved = React.useCallback(() => {
    if (saveTimerRef.current !== null) {
      window.clearTimeout(saveTimerRef.current);
    }
    setSaveState('saved');
    saveTimerRef.current = window.setTimeout(() => {
      setSaveState('idle');
      saveTimerRef.current = null;
    }, 1800);
  }, []);

  React.useEffect(() => {
    return () => {
      if (saveTimerRef.current !== null) {
        window.clearTimeout(saveTimerRef.current);
      }
    };
  }, []);

  React.useEffect(() => {
    invoke<KbSettingsInfo>('kb_get_settings')
      .then((loaded) => {
        setSettings({
          autoIngestSummaries: loaded.auto_ingest_summaries,
          autoIngestArtifacts: loaded.auto_ingest_artifacts,
          summaryFrequency: loaded.summary_frequency,
        });
      })
      .catch(() => {
        setLoadFailed(true);
      });
  }, []);

  const saveKnowledgeSettings = async (nextSettings: LocalKbSettings, previousSettings: LocalKbSettings) => {
    setSettings(nextSettings);
    setSaveState('saving');
    try {
      const saved = await invoke<KbSettingsInfo>('kb_save_settings', {
        autoIngestSummaries: nextSettings.autoIngestSummaries,
        autoIngestArtifacts: nextSettings.autoIngestArtifacts,
        frequency: nextSettings.summaryFrequency,
      });
      setSettings({
        autoIngestSummaries: saved.auto_ingest_summaries,
        autoIngestArtifacts: saved.auto_ingest_artifacts,
        summaryFrequency: saved.summary_frequency,
      });
      showSaved();
    } catch (e: unknown) {
      setSettings(previousSettings);
      setSaveState('idle');
      setSettingsErr(String(e));
    }
  };

  const { autoIngestSummaries, autoIngestArtifacts, summaryFrequency } = settings;

  return (
    <section>
      <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '10px' }}>
        <h2 className="mb-0" style={{ fontSize: '14px', fontWeight: 500, color: 'var(--color-text-primary)' }}>
          {t('kb_settings_title', lang)}
        </h2>
        <div
          style={{
            fontSize: '11px',
            color: saveState === 'saved' ? 'var(--color-text-success)' : 'var(--color-text-tertiary)',
            transition: 'color 160ms ease, opacity 160ms ease',
            opacity: saveState === 'idle' ? 0 : 1,
          }}
        >
          {saveState === 'saving'
            ? t('kb_settings_saving', lang)
            : saveState === 'saved'
              ? t('kb_settings_saved', lang)
              : ' '}
        </div>
      </div>
      {loadFailed ? (
        <p
          style={{
            margin: '0 0 10px 0',
            color: 'var(--color-text-danger)',
            background: 'var(--color-background-danger)',
            padding: '8px 10px',
            borderRadius: '8px',
            fontSize: '12px',
          }}
        >
          {t('kb_settings_load_failed', lang)}
        </p>
      ) : null}
      <div style={{ display: 'grid', gap: '7px' }}>
        <label
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: '8px',
            minHeight: '26px',
            cursor: 'pointer',
          }}
        >
          <input
            type="checkbox"
            checked={autoIngestSummaries}
            onChange={(e) => {
              const previousSettings = settings;
              const nextSettings = { ...settings, autoIngestSummaries: e.target.checked };
              void saveKnowledgeSettings(nextSettings, previousSettings);
            }}
            style={{ accentColor: '#1D9E75' }}
          />
          <span style={{ fontSize: '13px', color: 'var(--color-text-secondary)' }}>
            {t('kb_auto_ingest_summaries_label', lang)}
          </span>
        </label>

        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: '8px',
            marginLeft: '23px',
            minHeight: '26px',
            opacity: autoIngestSummaries ? 1 : 0.45,
          }}
        >
          <span style={{ width: '82px', fontSize: '12px', color: 'var(--color-text-tertiary)', flexShrink: 0 }}>
            {t('kb_summary_frequency_label', lang)}
          </span>
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(3, minmax(0, 1fr))',
              width: '172px',
              padding: '2px',
              borderRadius: '7px',
              background: 'var(--color-background-secondary)',
              border: '0.5px solid var(--color-border-tertiary)',
            }}
          >
            {KB_SUMMARY_FREQUENCY_OPTIONS.map((frequency) => {
              const active = summaryFrequency === frequency;
              return (
                <button
                  key={frequency}
                  disabled={!autoIngestSummaries}
                  onClick={() => {
                    const previousSettings = settings;
                    const nextSettings = { ...settings, summaryFrequency: frequency };
                    void saveKnowledgeSettings(nextSettings, previousSettings);
                  }}
                  style={{
                    border: 'none',
                    borderRadius: '5px',
                    height: '22px',
                    fontSize: '11px',
                    fontWeight: active ? 600 : 500,
                    color: active ? 'var(--color-text-primary)' : 'var(--color-text-secondary)',
                    background: active ? 'var(--color-background-primary)' : 'transparent',
                    cursor: autoIngestSummaries ? 'pointer' : 'not-allowed',
                  }}
                >
                  {t(`kb_summary_frequency_${frequency}` as Parameters<typeof t>[0], lang)}
                </button>
              );
            })}
          </div>
        </div>

        <label
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: '8px',
            minHeight: '26px',
            cursor: 'pointer',
            marginTop: '2px',
          }}
        >
          <input
            type="checkbox"
            checked={autoIngestArtifacts}
            onChange={(e) => {
              const previousSettings = settings;
              const nextSettings = { ...settings, autoIngestArtifacts: e.target.checked };
              void saveKnowledgeSettings(nextSettings, previousSettings);
            }}
            style={{ accentColor: '#1D9E75' }}
          />
          <span style={{ fontSize: '13px', color: 'var(--color-text-secondary)' }}>
            {t('kb_auto_ingest_artifacts_label', lang)}
          </span>
        </label>
      </div>
    </section>
  );
};

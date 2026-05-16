import React from 'react';
import { invoke } from '@tauri-apps/api/core';
import { t, useLang } from '../i18n';
import { useAppStore } from '../stores/appStore';

interface DesktopBehaviorSettings {
  minimizeToTrayOnClose: boolean;
  launchAtStartup: boolean;
  trayAvailable: boolean;
  autostartSupported: boolean;
}

export const DesktopSettings: React.FC = () => {
  const { lang } = useLang();
  const { setSettingsErr } = useAppStore();
  const [settings, setSettings] = React.useState<DesktopBehaviorSettings | null>(null);
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
    invoke<DesktopBehaviorSettings>('get_desktop_behavior_settings')
      .then(setSettings)
      .catch(() => setLoadFailed(true));
  }, []);

  const saveDesktopSettings = async (nextSettings: DesktopBehaviorSettings, previousSettings: DesktopBehaviorSettings) => {
    setSettings(nextSettings);
    setSaveState('saving');
    try {
      const saved = await invoke<DesktopBehaviorSettings>('save_desktop_behavior_settings', {
        minimizeToTrayOnClose: nextSettings.minimizeToTrayOnClose,
        launchAtStartup: nextSettings.launchAtStartup,
      });
      setSettings(saved);
      showSaved();
    } catch (e: unknown) {
      setSettings(previousSettings);
      setSaveState('idle');
      setSettingsErr(String(e));
    }
  };

  if (!settings) {
    return (
      <section>
        <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '10px' }}>
          <h2 className="mb-0" style={{ fontSize: '14px', fontWeight: 500, color: 'var(--color-text-primary)' }}>
            {t('desktop_settings_title', lang)}
          </h2>
        </div>
        {loadFailed ? (
          <p
            style={{
              margin: 0,
              color: 'var(--color-text-danger)',
              background: 'var(--color-background-danger)',
              padding: '8px 10px',
              borderRadius: '8px',
              fontSize: '12px',
            }}
          >
            {t('desktop_settings_load_failed', lang)}
          </p>
        ) : null}
      </section>
    );
  }

  return (
    <section>
      <div style={{ display: 'flex', alignItems: 'center', gap: '10px', marginBottom: '10px' }}>
        <h2 className="mb-0" style={{ fontSize: '14px', fontWeight: 500, color: 'var(--color-text-primary)' }}>
          {t('desktop_settings_title', lang)}
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
            ? t('desktop_settings_saving', lang)
            : saveState === 'saved'
              ? t('desktop_settings_saved', lang)
              : ' '}
        </div>
      </div>

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
            checked={settings.minimizeToTrayOnClose}
            onChange={(e) => {
              const previous = settings;
              const next = { ...settings, minimizeToTrayOnClose: e.target.checked };
              void saveDesktopSettings(next, previous);
            }}
            style={{ accentColor: '#1D9E75' }}
          />
          <span style={{ fontSize: '13px', color: 'var(--color-text-secondary)' }}>
            {t('desktop_minimize_to_tray_on_close', lang)}
          </span>
        </label>

        <label
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: '8px',
            minHeight: '26px',
            cursor: settings.autostartSupported ? 'pointer' : 'not-allowed',
            opacity: settings.autostartSupported ? 1 : 0.55,
          }}
        >
          <input
            type="checkbox"
            checked={settings.launchAtStartup}
            disabled={!settings.autostartSupported}
            onChange={(e) => {
              const previous = settings;
              const next = { ...settings, launchAtStartup: e.target.checked };
              void saveDesktopSettings(next, previous);
            }}
            style={{ accentColor: '#1D9E75' }}
          />
          <span style={{ fontSize: '13px', color: 'var(--color-text-secondary)' }}>
            {t('desktop_launch_at_startup', lang)}
          </span>
        </label>

        {!settings.trayAvailable && (
          <div style={{ marginLeft: '23px', fontSize: '12px', color: 'var(--color-text-tertiary)' }}>
            {t('desktop_tray_unavailable_hint', lang)}
          </div>
        )}
      </div>
    </section>
  );
};

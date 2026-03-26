import React, { useState, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useLang, type Lang, t } from '../i18n';

interface OnboardingData {
  name: string;
  boundaries: string;
  pain_points: string;
  language: string;
  work_schedule: string;
  tools: string;
}

interface LlmConfig {
  api_key: string;
  model: string;
  api_base: string; // empty = use provider default
}

const QUESTIONS_BY_LANG: Record<Lang, {
  key: keyof OnboardingData;
  pup_text: string;
  placeholder: string;
  section: string;
  skippable?: boolean;
}[]> = {
  zh: [
    {
      key: 'name',
      pup_text: '我是你的 Alpha Pup 🐾 很高兴认识你！\n\n你叫什么名字？或者你希望我怎么称呼你？',
      placeholder: '例如：小明，或者叫我 Alex 就好',
      section: '## Name',
    },
    {
      key: 'boundaries',
      pup_text: '很高兴认识你！\n\n你最不希望我在没有你确认的情况下替你做的事是什么？',
      placeholder: '例如：不自动发送任何公开消息，不删除文件',
      section: '## Boundaries',
    },
    {
      key: 'pain_points',
      pup_text: '你每周最浪费时间的重复性工作是什么？',
      placeholder: '例如：整理 GitHub issues，写周报，回复重复邮件',
      section: '## Pain Points',
      skippable: true,
    },
    {
      key: 'language',
      pup_text: '你偏好用哪种语言和我交流？代码和注释呢？',
      placeholder: '例如：对话：中文，代码：English',
      section: '## Language',
      skippable: true,
    },
    {
      key: 'work_schedule',
      pup_text: '你通常几点开始工作，几点结束？有不想被打扰的时段吗？',
      placeholder: '例如：9:00–18:00 PST，晚上 22:00 后请勿打扰',
      section: '## Work Schedule',
      skippable: true,
    },
    {
      key: 'tools',
      pup_text: '你最常用什么工具？（GitHub / Notion / Calendar / 邮件 等）',
      placeholder: '例如：GitHub、Notion、Google Calendar、Gmail',
      section: '## Tools',
      skippable: true,
    },
  ],
  en: [
    {
      key: 'name',
      pup_text: "I'm your Alpha Pup 🐾 Nice to meet you!\n\nWhat should I call you?",
      placeholder: 'For example: Alex, or just call me Ben',
      section: '## Name',
    },
    {
      key: 'boundaries',
      pup_text: "Nice to meet you!\n\nWhat's something you never want me to do without your confirmation?",
      placeholder: 'For example: do not send public messages automatically, do not delete files',
      section: '## Boundaries',
    },
    {
      key: 'pain_points',
      pup_text: 'What repetitive work wastes the most time for you each week?',
      placeholder: 'For example: triaging GitHub issues, writing status updates, replying to repetitive email',
      section: '## Pain Points',
      skippable: true,
    },
    {
      key: 'language',
      pup_text: 'Which language do you prefer for conversation? What about code and comments?',
      placeholder: 'For example: chat: Chinese, code: English',
      section: '## Language',
      skippable: true,
    },
    {
      key: 'work_schedule',
      pup_text: "What are your usual working hours? Any times when you don't want to be interrupted?",
      placeholder: 'For example: 9:00-18:00 PST, please do not ping me after 22:00',
      section: '## Work Schedule',
      skippable: true,
    },
    {
      key: 'tools',
      pup_text: 'What tools do you use the most? (GitHub / Notion / Calendar / Email / etc.)',
      placeholder: 'For example: GitHub, Notion, Google Calendar, Gmail',
      section: '## Tools',
      skippable: true,
    },
  ],
};

// Step indices
const TOTAL_PROFILE = QUESTIONS_BY_LANG.zh.length; // 0..5
const STEP_LLM = TOTAL_PROFILE;                // 6
const STEP_CONFIRM = TOTAL_PROFILE + 1;        // 7

function buildOwnerMd(answers: Partial<OnboardingData>, lang: Lang): string {
  const questions = QUESTIONS_BY_LANG[lang];
  const lines: string[] = ['# Owner Profile', ''];
  for (const q of questions) {
    const val = answers[q.key];
    lines.push(q.section);
    lines.push(val ? val.trim() : t('onboarding_field_empty', lang));
    lines.push('');
  }
  return lines.join('\n');
}

function presetModels(lang: Lang) {
  return [
    { label: 'GPT-4o (OpenAI)', value: 'gpt-4o', base: '' },
    { label: 'GPT-4o-mini (OpenAI)', value: 'gpt-4o-mini', base: '' },
    { label: t('onboarding_custom_model', lang), value: '__custom__', base: '' },
  ];
}

interface Props {
  onComplete: () => void;
}

export const Onboarding: React.FC<Props> = ({ onComplete }) => {
  const { lang } = useLang();
  const questions = QUESTIONS_BY_LANG[lang];
  const models = presetModels(lang);
  const [step, setStep] = useState(0);
  const [answers, setAnswers] = useState<Partial<OnboardingData>>({});
  const [currentInput, setCurrentInput] = useState('');
  const [saving, setSaving] = useState(false);
  const [editableOwnerMd, setEditableOwnerMd] = useState('');
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const scrollEndRef = useRef<HTMLDivElement>(null);

  // LLM config state
  const [llmPreset, setLlmPreset] = useState(models[0].value);
  const [llmConfig, setLlmConfig] = useState<LlmConfig>({ api_key: '', model: 'gpt-4o', api_base: '' });
  const [llmError, setLlmError] = useState('');

  useEffect(() => {
    if (step < TOTAL_PROFILE) inputRef.current?.focus();
  }, [step]);

  useEffect(() => {
    scrollEndRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' });
  }, [step]);

  const ownerMdPreview = buildOwnerMd(answers, lang);
  const progressPct = Math.min((step / (STEP_CONFIRM)) * 100, 100);

  // ── Profile questions navigation ──────────────────────────────────────────

  const advance = (value: string) => {
    const key = questions[step].key;
    const newAnswers = { ...answers, [key]: value };
    setAnswers(newAnswers);
    setCurrentInput('');
    if (step < TOTAL_PROFILE - 1) {
      setStep(step + 1);
    } else {
      setStep(STEP_LLM);
    }
  };

  const handleNext = () => {
    if (!currentInput.trim()) return;
    advance(currentInput.trim());
  };

  const handleSkip = () => {
    advance('');
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleNext(); }
  };

  // ── LLM preset selection ──────────────────────────────────────────────────

  const handlePresetChange = (value: string) => {
    setLlmPreset(value);
    if (value !== '__custom__') {
      const preset = models.find((p) => p.value === value);
      if (preset) {
        setLlmConfig((prev) => ({ ...prev, model: preset.value, api_base: preset.base }));
      }
    }
  };

  const handleLlmNext = () => {
    if (!llmConfig.api_key.trim()) { setLlmError(t('onboarding_llm_required', lang)); return; }
    if (!llmConfig.model.trim()) { setLlmError(t('onboarding_model_required', lang)); return; }
    setLlmError('');
    setEditableOwnerMd(buildOwnerMd(answers, lang));
    setStep(STEP_CONFIRM);
  };

  // ── Save ──────────────────────────────────────────────────────────────────

  const handleSave = async () => {
    setSaving(true);
    try {
      // 1. Save LLM config first — so the embed API key is available when
      //    save_onboarding_data seeds long-term memory (prevents timeout hang).
      if (llmConfig.api_key.trim()) {
        await invoke('set_llm_provider', {
          provider: 'openai',
          model: llmConfig.model.trim(),
          miniModel: llmConfig.model.trim(),
          embedModel: null,
          apiKey: llmConfig.api_key.trim(),
          apiBase: llmConfig.api_base.trim() || null,
        });
      }

      // 2. Save OWNER.md profile + seed memory (embedding now has a valid key)
      await invoke('save_onboarding_data', {
        data: {
          name: answers.name ?? '',
          boundaries: answers.boundaries ?? '',
          pain_points: answers.pain_points ?? '',
          language: answers.language ?? '',
          work_schedule: answers.work_schedule ?? '',
          tools: answers.tools ?? '',
        },
      });

      // 3. Done — transition immediately
      onComplete();
    } catch (e) {
      console.error('Onboarding save error:', e);
      setSaving(false);
      setLlmError(`${t('onboarding_save_failed_prefix', lang)}${e}`);
    }
  };

  // ── Render ────────────────────────────────────────────────────────────────

  const isCustomPreset = llmPreset === '__custom__';
  const stepLabel = step < TOTAL_PROFILE
    ? `${step + 1} / ${TOTAL_PROFILE}`
    : step === STEP_LLM ? t('onboarding_step_ai', lang) : t('onboarding_step_confirm', lang);

  const currentQuestion = step < TOTAL_PROFILE ? questions[step] : null;

  // Shared input style
  const inputStyle: React.CSSProperties = {
    width: '100%',
    borderRadius: '8px',
    background: 'var(--color-background-primary)',
    border: '1px solid var(--color-border-secondary)',
    padding: '10px 12px',
    fontSize: '15px',
    color: 'var(--color-text-primary)',
    outline: 'none',
    transition: 'border-color 0.15s',
    boxSizing: 'border-box',
  };

  const labelStyle: React.CSSProperties = {
    display: 'block',
    fontSize: '12px',
    color: 'var(--color-text-tertiary)',
    marginBottom: '6px',
  };

  return (
    <div style={{ height: '100%', background: 'var(--color-background-primary)', color: 'var(--color-text-primary)', display: 'flex', flexDirection: 'column' }}>
      {/* Sub-header with step info + progress */}
      <div style={{ borderBottom: '1px solid var(--color-border-primary)', padding: '8px 20px', display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexShrink: 0, background: 'var(--color-background-secondary)' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
          <img src="/openpup-icon.svg" alt="OpenPup" style={{ width: '20px', height: '20px' }} />
          <span style={{ fontSize: '12px', fontWeight: 400, color: 'var(--color-text-tertiary)', userSelect: 'none' }}>
            {t('onboarding_tagline', lang)}
          </span>
        </div>
        <div style={{ fontSize: '12px', color: 'var(--color-text-tertiary)', fontWeight: 500, fontVariantNumeric: 'tabular-nums' }}>{stepLabel}</div>
      </div>

      {/* Progress bar */}
      <div style={{ height: '1px', background: 'var(--color-border-primary)', flexShrink: 0, position: 'relative' }}>
        <div
          style={{ height: '100%', background: '#1D9E75', transition: 'width 0.7s ease-out', width: `${progressPct}%` }}
        />
      </div>

      <div style={{ display: 'flex', flex: 1, overflow: 'hidden' }}>
        {/* Left: Conversation / LLM config */}
        <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
          {/* Scrollable messages area */}
          <div style={{ flex: 1, overflow: 'auto', padding: '32px', background: 'var(--color-background-primary)' }}>

            {/* Answered profile questions */}
            {questions.slice(0, step < TOTAL_PROFILE ? step : TOTAL_PROFILE).map((q, idx) => (
              <div key={q.key} className="animate-in fade-in slide-in-from-bottom-2 duration-500" style={{ marginBottom: '32px', animationDelay: `${idx * 50}ms` }}>
                {/* Pup bubble */}
                <div style={{ display: 'flex', alignItems: 'flex-start', gap: '12px', marginBottom: '12px' }}>
                  <div style={{ width: '28px', height: '28px', borderRadius: '50%', background: '#1D9E75', display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: '13px', flexShrink: 0 }}>
                    🐾
                  </div>
                  <div style={{ background: 'var(--color-background-primary)', border: '1px solid var(--color-border-primary)', borderLeft: '2px solid #1D9E75', borderRadius: '12px', padding: '12px 16px', fontSize: '15px', color: 'var(--color-text-primary)', whiteSpace: 'pre-line', maxWidth: '672px' }}>
                    {q.pup_text}
                  </div>
                </div>
                {/* User reply */}
                {answers[q.key] ? (
                  <div className="animate-in fade-in slide-in-from-right-2 duration-300" style={{ display: 'flex', justifyContent: 'flex-end' }}>
                    <div style={{ background: 'var(--color-background-info)', color: 'var(--color-text-info)', fontSize: '15px', padding: '12px 16px', borderRadius: '12px', fontWeight: 400, maxWidth: '672px', lineHeight: 1.6 }}>
                      {answers[q.key]}
                    </div>
                  </div>
                ) : (
                  <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
                    <span style={{ fontSize: '12px', color: 'var(--color-text-tertiary)', fontStyle: 'italic', paddingRight: '4px' }}>{t('onboarding_skipped', lang)}</span>
                  </div>
                )}
              </div>
            ))}

            {/* Current profile question */}
            {step < TOTAL_PROFILE && (
              <div className="animate-in fade-in slide-in-from-bottom-2 duration-500" style={{ marginBottom: '24px' }}>
                <div style={{ display: 'flex', alignItems: 'flex-start', gap: '12px', marginBottom: '16px' }}>
                  <div style={{ width: '28px', height: '28px', borderRadius: '50%', background: '#1D9E75', display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: '13px', flexShrink: 0 }}>
                    🐾
                  </div>
                  <div style={{ background: 'var(--color-background-primary)', border: '1px solid var(--color-border-primary)', borderLeft: '2px solid #1D9E75', borderRadius: '12px', padding: '12px 16px', fontSize: '15px', color: 'var(--color-text-primary)', whiteSpace: 'pre-line', maxWidth: '672px' }}>
                    {questions[step].pup_text}
                  </div>
                </div>
              </div>
            )}

            {/* LLM config step */}
            {step === STEP_LLM && (
              <div className="animate-in fade-in slide-in-from-bottom-2 duration-500" style={{ maxWidth: '672px' }}>
                <div style={{ display: 'flex', alignItems: 'flex-start', gap: '12px', marginBottom: '20px' }}>
                  <div style={{ width: '28px', height: '28px', borderRadius: '50%', background: '#1D9E75', display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: '13px', flexShrink: 0 }}>
                    🐾
                  </div>
                  <div style={{ background: 'var(--color-background-primary)', border: '1px solid var(--color-border-primary)', borderLeft: '2px solid #1D9E75', borderRadius: '12px', padding: '12px 16px', fontSize: '15px', color: 'var(--color-text-primary)', maxWidth: '448px' }}>
                    {t('onboarding_llm_intro_title', lang)}<br /><br />
                    {t('onboarding_llm_intro_body', lang)}
                  </div>
                </div>

                <div style={{ paddingLeft: '40px', display: 'flex', flexDirection: 'column', gap: '12px' }}>
                  {/* Preset picker */}
                  <div>
                    <label style={labelStyle}>{t('onboarding_select_model', lang)}</label>
                    <select
                      style={{ ...inputStyle, cursor: 'pointer' }}
                      value={llmPreset}
                      onChange={(e) => handlePresetChange(e.target.value)}
                    >
                      {models.map((p) => (
                        <option key={p.value} value={p.value}>{p.label}</option>
                      ))}
                    </select>
                  </div>

                  {/* Custom model name */}
                  {isCustomPreset && (
                    <div className="animate-in fade-in slide-in-from-top-2 duration-300">
                      <label style={labelStyle}>{t('onboarding_model_name', lang)}</label>
                      <input
                        style={inputStyle}
                        placeholder={t('onboarding_model_name_ph', lang)}
                        value={llmConfig.model}
                        onChange={(e) => setLlmConfig((prev) => ({ ...prev, model: e.target.value }))}
                      />
                    </div>
                  )}

                  {/* API Key */}
                  <div>
                    <label style={labelStyle}>API Key</label>
                    <input
                      style={inputStyle}
                      type="password"
                      placeholder="sk-..."
                      value={llmConfig.api_key}
                      onChange={(e) => setLlmConfig((prev) => ({ ...prev, api_key: e.target.value }))}
                    />
                    <p style={{ marginTop: '6px', fontSize: '11px', color: 'var(--color-text-tertiary)' }}>
                      {t('onboarding_api_key_hint', lang)}
                    </p>
                  </div>

                  {/* Base URL */}
                  {(isCustomPreset || llmConfig.api_base) && (
                    <div className="animate-in fade-in slide-in-from-top-2 duration-300">
                      <label style={labelStyle}>
                        {t('onboarding_base_url_optional', lang)}
                      </label>
                      <input
                        style={inputStyle}
                        placeholder="https://api.openai.com/v1"
                        value={llmConfig.api_base}
                        onChange={(e) => setLlmConfig((prev) => ({ ...prev, api_base: e.target.value }))}
                      />
                    </div>
                  )}

                  {llmError && (
                    <div className="animate-in fade-in duration-300" style={{ fontSize: '13px', color: 'var(--color-text-danger)', background: 'var(--color-background-danger)', border: '1px solid var(--color-border-tertiary)', padding: '10px 12px', borderRadius: '8px' }}>
                      {llmError}
                    </div>
                  )}

                  <button
                    style={{ width: '100%', marginTop: '4px', padding: '10px 16px', borderRadius: '8px', background: '#1D9E75', color: '#fff', fontSize: '15px', fontWeight: 500, border: 'none', cursor: 'pointer', transition: 'opacity 0.15s' }}
                    onClick={handleLlmNext}
                  >
                    {t('onboarding_next', lang)}
                  </button>
                </div>
              </div>
            )}

            {/* Confirm step */}
            {step === STEP_CONFIRM && (
              <div className="animate-in fade-in slide-in-from-bottom-2 duration-500" style={{ display: 'flex', alignItems: 'flex-start', gap: '12px', marginBottom: '24px' }}>
                <div style={{ width: '28px', height: '28px', borderRadius: '50%', background: '#1D9E75', display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: '13px', flexShrink: 0 }}>
                  🐾
                </div>
                <div style={{ background: 'var(--color-background-primary)', border: '1px solid var(--color-border-primary)', borderLeft: '2px solid #1D9E75', borderRadius: '12px', padding: '12px 16px', fontSize: '15px', color: 'var(--color-text-primary)', maxWidth: '672px' }}>
                  <>
                    {answers.name
                      ? `${t('onboarding_confirm_with_name_prefix', lang)}${answers.name}${t('onboarding_confirm_with_name_suffix', lang)}`
                      : t('onboarding_confirm_without_name', lang)}
                    <br /><br />
                    {t('onboarding_confirm_saved', lang)}{' '}
                    <code style={{ fontFamily: 'var(--font-mono)', color: 'var(--color-text-secondary)', fontSize: '13px', background: 'var(--color-background-secondary)', padding: '1px 6px', borderRadius: '4px' }}>~/.openpup/OWNER.md</code>{' '}
                    {t('onboarding_confirm_saved_suffix', lang)}
                    <br /><br />
                    {t('onboarding_confirm_preview', lang)}
                  </>
                </div>
              </div>
            )}
            <div ref={scrollEndRef} />
          </div>

          {/* Fixed input area at bottom */}
          {step < TOTAL_PROFILE && (
            <div style={{ flexShrink: 0, borderTop: '1px solid var(--color-border-primary)', padding: '16px 32px', background: 'var(--color-background-primary)', display: 'flex', gap: '12px' }}>
              <textarea
                ref={inputRef}
                style={{ flex: 1, resize: 'none', borderRadius: '8px', background: 'var(--color-background-primary)', border: '1px solid var(--color-border-secondary)', padding: '12px 16px', fontSize: '15px', color: 'var(--color-text-primary)', outline: 'none', transition: 'border-color 0.15s', fontFamily: 'inherit' }}
                rows={3}
                placeholder={questions[step].placeholder}
                value={currentInput}
                onChange={(e) => setCurrentInput(e.target.value)}
                onKeyDown={handleKeyDown}
              />
              <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', justifyContent: 'flex-end' }}>
                <button
                  style={{ padding: '10px 20px', borderRadius: '8px', background: '#1D9E75', color: '#fff', fontSize: '15px', fontWeight: 500, border: 'none', cursor: currentInput.trim() ? 'pointer' : 'not-allowed', opacity: currentInput.trim() ? 1 : 0.4, transition: 'opacity 0.15s' }}
                  onClick={handleNext}
                  disabled={!currentInput.trim()}
                >
                  {t('onboarding_next', lang)}
                </button>
                {currentQuestion?.skippable && (
                  <button
                    style={{ fontSize: '12px', color: 'var(--color-text-tertiary)', border: 'none', background: 'transparent', cursor: 'pointer', textAlign: 'center', transition: 'color 0.15s' }}
                    onClick={handleSkip}
                  >
                    {t('onboarding_skip', lang)}
                  </button>
                )}
              </div>
            </div>
          )}
        </div>

        {/* Right: OWNER.md live preview — hidden below ~800px window width */}
        <div className="hidden lg:flex" style={{ width: '320px', borderLeft: '1px solid var(--color-border-primary)', padding: '20px', flexDirection: 'column', background: 'var(--color-background-secondary)', flexShrink: 0 }}>
          <div style={{ fontSize: '11px', fontWeight: 500, color: 'var(--color-text-tertiary)', marginBottom: '12px', textTransform: 'uppercase', letterSpacing: '0.12em' }}>
            {t('onboarding_preview_title', lang)}
          </div>

          {step === 0 ? (
            /* Step 0: Explain what OWNER.md is before any answers exist */
            <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: '16px' }}>
              <div style={{ borderRadius: '12px', border: '1px solid var(--color-border-primary)', background: 'var(--color-background-primary)', padding: '16px', display: 'flex', flexDirection: 'column', gap: '12px' }}>
                <p style={{ fontSize: '13px', color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                  {t('onboarding_preview_intro_prefix', lang)}
                  <code style={{ fontFamily: 'var(--font-mono)', color: 'var(--color-text-secondary)', fontSize: '12px', margin: '0 4px', background: 'var(--color-background-secondary)', padding: '1px 4px', borderRadius: '4px' }}>~/.openpup/OWNER.md</code>
                  {t('onboarding_preview_intro_suffix', lang)}
                </p>
                <p style={{ fontSize: '13px', color: 'var(--color-text-tertiary)', lineHeight: 1.6 }}>
                  {t('onboarding_preview_explanation', lang)}
                </p>
                <p style={{ fontSize: '13px', color: 'var(--color-text-tertiary)', lineHeight: 1.6 }}>
                  {t('onboarding_preview_edit_hint', lang)}
                </p>
              </div>
              <div style={{ borderRadius: '12px', border: '1px solid var(--color-border-primary)', background: 'var(--color-background-primary)', padding: '12px 16px' }}>
                <p style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: '8px' }}>{t('onboarding_preview_example_title', lang)}</p>
                <pre style={{ fontSize: '12px', color: 'var(--color-text-tertiary)', fontFamily: 'var(--font-mono)', lineHeight: 1.6, whiteSpace: 'pre-wrap' }}>{t('onboarding_preview_example_content', lang)}</pre>
              </div>
            </div>
          ) : (
            /* Steps 1+: Show growing preview */
            step < STEP_CONFIRM ? (
              <pre style={{ flex: 1, background: 'var(--color-background-primary)', border: '1px solid var(--color-border-primary)', borderRadius: '12px', padding: '16px', fontSize: '12px', color: 'var(--color-text-secondary)', fontFamily: 'var(--font-mono)', lineHeight: 1.6, overflow: 'auto', whiteSpace: 'pre-wrap' }}>
                {ownerMdPreview}
              </pre>
            ) : (
              <>
                <textarea
                  style={{ flex: 1, background: 'var(--color-background-primary)', border: '1px solid var(--color-border-primary)', borderRadius: '12px', padding: '16px', fontSize: '12px', color: 'var(--color-text-secondary)', fontFamily: 'var(--font-mono)', lineHeight: 1.6, resize: 'none', outline: 'none', overflow: 'auto', transition: 'border-color 0.15s' }}
                  value={editableOwnerMd}
                  onChange={(e) => setEditableOwnerMd(e.target.value)}
                  spellCheck={false}
                />
                <button
                  style={{ marginTop: '12px', width: '100%', padding: '12px', borderRadius: '12px', background: '#1D9E75', color: '#fff', fontSize: '15px', fontWeight: 500, border: 'none', cursor: saving ? 'not-allowed' : 'pointer', opacity: saving ? 0.4 : 1, transition: 'opacity 0.15s' }}
                  onClick={() => void handleSave()}
                  disabled={saving}
                >
                  {saving ? t('onboarding_saving', lang) : t('onboarding_save_and_start', lang)}
                </button>
              </>
            )
          )}
        </div>
      </div>
    </div>
  );
};

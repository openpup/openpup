import React, { useState, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

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

const QUESTIONS: {
  key: keyof OnboardingData;
  pup_text: string;
  placeholder: string;
  section: string;
  skippable?: boolean;
}[] = [
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
];

// Step indices
const TOTAL_PROFILE = QUESTIONS.length;        // 0..5
const STEP_LLM = TOTAL_PROFILE;                // 6
const STEP_CONFIRM = TOTAL_PROFILE + 1;        // 7

function buildOwnerMd(answers: Partial<OnboardingData>): string {
  const lines: string[] = ['# Owner Profile', ''];
  for (const q of QUESTIONS) {
    const val = answers[q.key];
    lines.push(q.section);
    lines.push(val ? val.trim() : '_（未填写）_');
    lines.push('');
  }
  return lines.join('\n');
}

const PRESET_MODELS = [
  { label: 'GPT-4o (OpenAI)', value: 'gpt-4o', base: '' },
  { label: 'GPT-4o-mini (OpenAI)', value: 'gpt-4o-mini', base: '' },
  { label: '自定义…', value: '__custom__', base: '' },
];

interface Props {
  onComplete: () => void;
}

export const Onboarding: React.FC<Props> = ({ onComplete }) => {
  const [step, setStep] = useState(0);
  const [answers, setAnswers] = useState<Partial<OnboardingData>>({});
  const [currentInput, setCurrentInput] = useState('');
  const [saving, setSaving] = useState(false);
  const [editableOwnerMd, setEditableOwnerMd] = useState('');
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const scrollEndRef = useRef<HTMLDivElement>(null);

  // LLM config state
  const [llmPreset, setLlmPreset] = useState(PRESET_MODELS[0].value);
  const [llmConfig, setLlmConfig] = useState<LlmConfig>({ api_key: '', model: 'gpt-4o', api_base: '' });
  const [llmError, setLlmError] = useState('');

  useEffect(() => {
    if (step < TOTAL_PROFILE) inputRef.current?.focus();
  }, [step]);

  useEffect(() => {
    scrollEndRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' });
  }, [step]);

  const ownerMdPreview = buildOwnerMd(answers);
  const progressPct = Math.min((step / (STEP_CONFIRM)) * 100, 100);

  // ── Profile questions navigation ──────────────────────────────────────────

  const advance = (value: string) => {
    const key = QUESTIONS[step].key;
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
      const preset = PRESET_MODELS.find((p) => p.value === value);
      if (preset) {
        setLlmConfig((prev) => ({ ...prev, model: preset.value, api_base: preset.base }));
      }
    }
  };

  const handleLlmNext = () => {
    if (!llmConfig.api_key.trim()) { setLlmError('API Key 为必需项，请填写后继续'); return; }
    if (!llmConfig.model.trim()) { setLlmError('请填写模型名称'); return; }
    setLlmError('');
    setEditableOwnerMd(buildOwnerMd(answers));
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
      setLlmError(`保存失败：${e}`);
    }
  };

  // ── Render ────────────────────────────────────────────────────────────────

  const isCustomPreset = llmPreset === '__custom__';
  const stepLabel = step < TOTAL_PROFILE
    ? `${step + 1} / ${TOTAL_PROFILE}`
    : step === STEP_LLM ? 'AI 配置' : '确认';

  const currentQuestion = step < TOTAL_PROFILE ? QUESTIONS[step] : null;

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
    <div style={{ minHeight: '100vh', background: 'var(--color-background-primary)', color: 'var(--color-text-primary)', display: 'flex', flexDirection: 'column' }}>
      {/* Header */}
      <div style={{ borderBottom: '1px solid var(--color-border-primary)', padding: '12px 20px', display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexShrink: 0, background: 'var(--color-background-secondary)' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
          <span style={{ width: '8px', height: '8px', borderRadius: '50%', background: '#1D9E75', flexShrink: 0, display: 'inline-block' }} />
          <span style={{ fontSize: '13px', fontWeight: 400, color: 'var(--color-text-secondary)', userSelect: 'none' }}>
            open<span style={{ color: '#1D9E75' }}>pup</span>
            <span style={{ marginLeft: '10px', color: 'var(--color-text-tertiary)' }}>“用 ChatGPT 三个月，它还是不知道你喜欢下雨天。OpenPup 记得。”</span>
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
            {QUESTIONS.slice(0, step < TOTAL_PROFILE ? step : TOTAL_PROFILE).map((q, idx) => (
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
                    <span style={{ fontSize: '12px', color: 'var(--color-text-tertiary)', fontStyle: 'italic', paddingRight: '4px' }}>（跳过）</span>
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
                    {QUESTIONS[step].pup_text}
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
                    最后一步——我需要 AI 接口才能工作。<br /><br />
                    请选择你的模型供应商并填入 API Key。
                  </div>
                </div>

                <div style={{ paddingLeft: '40px', display: 'flex', flexDirection: 'column', gap: '12px' }}>
                  {/* Preset picker */}
                  <div>
                    <label style={labelStyle}>选择模型</label>
                    <select
                      style={{ ...inputStyle, cursor: 'pointer' }}
                      value={llmPreset}
                      onChange={(e) => handlePresetChange(e.target.value)}
                    >
                      {PRESET_MODELS.map((p) => (
                        <option key={p.value} value={p.value}>{p.label}</option>
                      ))}
                    </select>
                  </div>

                  {/* Custom model name */}
                  {isCustomPreset && (
                    <div className="animate-in fade-in slide-in-from-top-2 duration-300">
                      <label style={labelStyle}>模型名称</label>
                      <input
                        style={inputStyle}
                        placeholder="例如：gpt-4o、claude-3-5-sonnet-20241022"
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
                    <p style={{ marginTop: '6px', fontSize: '11px', color: 'var(--color-text-tertiary)' }}>API Key 仅存储在本地，不会上传至任何服务器。</p>
                  </div>

                  {/* Base URL */}
                  {(isCustomPreset || llmConfig.api_base) && (
                    <div className="animate-in fade-in slide-in-from-top-2 duration-300">
                      <label style={labelStyle}>
                        Base URL <span style={{ color: 'var(--color-text-tertiary)' }}>（可选）</span>
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
                    下一步 →
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
                  好了，{answers.name ? `${answers.name}！` : ''}我对你有了初步了解 🐾
                  <br /><br />
                  这些都会写在{' '}
                  <code style={{ fontFamily: 'var(--font-mono)', color: 'var(--color-text-secondary)', fontSize: '13px', background: 'var(--color-background-secondary)', padding: '1px 6px', borderRadius: '4px' }}>~/.openpup/OWNER.md</code>{' '}
                  里，你随时可以打开直接修改。
                  <br /><br />
                  右侧是预览，确认无误后点击「确认并开始」。
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
                placeholder={QUESTIONS[step].placeholder}
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
                  {step < TOTAL_PROFILE - 1 ? '下一步 →' : '下一步 →'}
                </button>
                {currentQuestion?.skippable && (
                  <button
                    style={{ fontSize: '12px', color: 'var(--color-text-tertiary)', border: 'none', background: 'transparent', cursor: 'pointer', textAlign: 'center', transition: 'color 0.15s' }}
                    onClick={handleSkip}
                  >
                    跳过
                  </button>
                )}
              </div>
            </div>
          )}
        </div>

        {/* Right: OWNER.md live preview — hidden below ~800px window width */}
        <div className="hidden lg:flex" style={{ width: '320px', borderLeft: '1px solid var(--color-border-primary)', padding: '20px', flexDirection: 'column', background: 'var(--color-background-secondary)', flexShrink: 0 }}>
          <div style={{ fontSize: '11px', fontWeight: 500, color: 'var(--color-text-tertiary)', marginBottom: '12px', textTransform: 'uppercase', letterSpacing: '0.12em' }}>
            OWNER.md 预览
          </div>

          {step === 0 ? (
            /* Step 0: Explain what OWNER.md is before any answers exist */
            <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: '16px' }}>
              <div style={{ borderRadius: '12px', border: '1px solid var(--color-border-primary)', background: 'var(--color-background-primary)', padding: '16px', display: 'flex', flexDirection: 'column', gap: '12px' }}>
                <p style={{ fontSize: '13px', color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                  你的回答会保存到
                  <code style={{ fontFamily: 'var(--font-mono)', color: 'var(--color-text-secondary)', fontSize: '12px', margin: '0 4px', background: 'var(--color-background-secondary)', padding: '1px 4px', borderRadius: '4px' }}>~/.openpup/OWNER.md</code>
                  文件中。
                </p>
                <p style={{ fontSize: '13px', color: 'var(--color-text-tertiary)', lineHeight: 1.6 }}>
                  每次对话前，你的 pup 都会先读这份档案——这样它们就能记住你的名字、工作习惯和边界。
                </p>
                <p style={{ fontSize: '13px', color: 'var(--color-text-tertiary)', lineHeight: 1.6 }}>
                  随时可以直接编辑这个文件来更新你的偏好。
                </p>
              </div>
              <div style={{ borderRadius: '12px', border: '1px solid var(--color-border-primary)', background: 'var(--color-background-primary)', padding: '12px 16px' }}>
                <p style={{ fontSize: '11px', color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.08em', marginBottom: '8px' }}>之后会长这样</p>
                <pre style={{ fontSize: '12px', color: 'var(--color-text-tertiary)', fontFamily: 'var(--font-mono)', lineHeight: 1.6, whiteSpace: 'pre-wrap' }}>{`# Owner Profile

## Name
Alex

## Boundaries
不自动发消息，不删文件

## Pain Points
整理 issues，写周报…`}</pre>
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
                  {saving ? '保存中…' : '确认并开始 →'}
                </button>
              </>
            )
          )}
        </div>
      </div>
    </div>
  );
};

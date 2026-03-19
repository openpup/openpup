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
  },
  {
    key: 'language',
    pup_text: '你偏好用哪种语言和我交流？代码和注释呢？',
    placeholder: '例如：对话：中文，代码：English',
    section: '## Language',
  },
  {
    key: 'work_schedule',
    pup_text: '你通常几点开始工作，几点结束？有不想被打扰的时段吗？',
    placeholder: '例如：9:00–18:00 PST，晚上 22:00 后请勿打扰',
    section: '## Work Schedule',
  },
  {
    key: 'tools',
    pup_text: '你最常用什么工具？（GitHub / Notion / Calendar / 邮件 等）',
    placeholder: '例如：GitHub、Notion、Google Calendar、Gmail',
    section: '## Tools',
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

  const handleNext = () => {
    if (!currentInput.trim()) return;
    const key = QUESTIONS[step].key;
    const newAnswers = { ...answers, [key]: currentInput.trim() };
    setAnswers(newAnswers);
    setCurrentInput('');
    if (step < TOTAL_PROFILE - 1) {
      setStep(step + 1);
    } else {
      setStep(STEP_LLM);
    }
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

  return (
    <div className="min-h-screen bg-stone-950 text-stone-100 flex flex-col">
      {/* Header */}
      <div className="border-b border-stone-800/50 px-6 py-4 flex items-center justify-between shrink-0 bg-gradient-to-r from-stone-950 via-stone-950 to-stone-900/50">
        <div className="flex items-center gap-3">
          <span className="text-2xl">🐾</span>
          <div>
            <div className="font-bold text-sm tracking-tight">openpup</div>
            <div className="text-stone-500 text-xs mt-0.5">初次见面</div>
          </div>
        </div>
        <div className="text-xs text-stone-400 font-medium">{stepLabel}</div>
      </div>

      {/* Progress */}
      <div className="h-1 bg-stone-900 shrink-0 relative overflow-hidden">
        <div className="h-full bg-gradient-to-r from-amber-500 via-amber-400 to-amber-500 transition-all duration-700 ease-out shadow-lg shadow-amber-500/20" style={{ width: `${progressPct}%` }} />
      </div>

      <div className="flex flex-1 overflow-hidden">
        {/* Left: Conversation / LLM config */}
        <div className="flex-1 flex flex-col overflow-hidden">
          {/* Scrollable messages area */}
          <div className="flex-1 overflow-auto p-8 bg-gradient-to-b from-stone-950 via-stone-950 to-stone-900/30">

          {/* Answered profile questions */}
          {QUESTIONS.slice(0, step < TOTAL_PROFILE ? step : TOTAL_PROFILE).map((q, idx) => (
            <div key={q.key} className="mb-8 animate-in fade-in slide-in-from-bottom-2 duration-500" style={{ animationDelay: `${idx * 50}ms` }}>
              <div className="flex items-start gap-3 mb-3">
                <div className="w-8 h-8 rounded-full bg-gradient-to-br from-emerald-500 to-emerald-600 flex items-center justify-center text-sm font-bold shrink-0 shadow-lg shadow-emerald-500/30 ring-2 ring-emerald-500/20">
                  🐾
                </div>
                <div className="bg-gradient-to-br from-stone-800/80 to-stone-800/40 backdrop-blur-sm border border-stone-700/50 rounded-3xl px-5 py-3.5 text-sm text-stone-200 whitespace-pre-line max-w-2xl shadow-lg hover:shadow-xl transition-shadow duration-300">
                  {q.pup_text}
                </div>
              </div>
              {answers[q.key] && (
                <div className="flex justify-end animate-in fade-in slide-in-from-right-2 duration-300">
                  <div className="bg-gradient-to-br from-amber-600/90 to-amber-700/80 backdrop-blur-sm border border-amber-500/30 rounded-3xl px-5 py-3.5 text-sm text-white max-w-2xl shadow-lg">
                    {answers[q.key]}
                  </div>
                </div>
              )}
            </div>
          ))}

          {/* Current profile question */}
          {step < TOTAL_PROFILE && (
            <div className="mb-6 animate-in fade-in slide-in-from-bottom-2 duration-500">
              <div className="flex items-start gap-3 mb-4">
                <div className="w-8 h-8 rounded-full bg-gradient-to-br from-emerald-500 to-emerald-600 flex items-center justify-center text-sm font-bold shrink-0 shadow-lg shadow-emerald-500/30 ring-2 ring-emerald-500/20">
                  🐾
                </div>
                <div className="bg-gradient-to-br from-stone-800/80 to-stone-800/40 backdrop-blur-sm border border-stone-700/50 rounded-3xl px-5 py-3.5 text-sm text-stone-200 whitespace-pre-line max-w-2xl shadow-lg">
                  {QUESTIONS[step].pup_text}
                </div>
              </div>
            </div>
          )}

          {/* LLM config step */}
          {step === STEP_LLM && (
            <div className="max-w-2xl space-y-6 animate-in fade-in slide-in-from-bottom-2 duration-500">
              <div className="flex items-start gap-3 mb-6">
                <div className="w-8 h-8 rounded-full bg-gradient-to-br from-emerald-500 to-emerald-600 flex items-center justify-center text-sm font-bold shrink-0 shadow-lg shadow-emerald-500/30 ring-2 ring-emerald-500/20">
                  🐾
                </div>
                <div className="bg-gradient-to-br from-stone-800/80 to-stone-800/40 backdrop-blur-sm border border-stone-700/50 rounded-3xl px-5 py-3.5 text-sm text-stone-200 max-w-md shadow-lg">
                  最后一步——我需要 AI 接口才能工作。<br /><br />
                  请选择你的模型供应商并填入 API Key。
                </div>
              </div>

              <div className="space-y-4 pl-8">
                {/* Preset picker */}
                <div>
                  <label className="block text-xs font-semibold text-stone-300 mb-2.5 uppercase tracking-wide">选择模型</label>
                  <select
                    className="w-full rounded-xl bg-stone-900/50 backdrop-blur-sm border border-stone-700/50 px-4 py-3 text-sm text-stone-100 focus:outline-none focus:ring-2 focus:ring-amber-500/50 focus:border-amber-500/50 transition-all duration-300"
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
                    <label className="block text-xs font-semibold text-stone-300 mb-2.5 uppercase tracking-wide">模型名称</label>
                    <input
                      className="w-full rounded-xl bg-stone-900/50 backdrop-blur-sm border border-stone-700/50 px-4 py-3 text-sm text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-2 focus:ring-amber-500/50 focus:border-amber-500/50 transition-all duration-300"
                      placeholder="例如：gpt-4o、claude-3-5-sonnet-20241022"
                      value={llmConfig.model}
                      onChange={(e) => setLlmConfig((prev) => ({ ...prev, model: e.target.value }))}
                    />
                  </div>
                )}

                {/* API Key */}
                <div>
                  <label className="block text-xs font-semibold text-stone-300 mb-2.5 uppercase tracking-wide">API Key</label>
                  <input
                    className="w-full rounded-xl bg-stone-900/50 backdrop-blur-sm border border-stone-700/50 px-4 py-3 text-sm text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-2 focus:ring-amber-500/50 focus:border-amber-500/50 transition-all duration-300"
                    type="password"
                    placeholder="sk-..."
                    value={llmConfig.api_key}
                    onChange={(e) => setLlmConfig((prev) => ({ ...prev, api_key: e.target.value }))}
                  />
                </div>

                {/* Base URL */}
                {(isCustomPreset || llmConfig.api_base) && (
                  <div className="animate-in fade-in slide-in-from-top-2 duration-300">
                    <label className="block text-xs font-semibold text-stone-300 mb-2.5 uppercase tracking-wide">
                      Base URL <span className="text-stone-500 normal-case">（可选）</span>
                    </label>
                    <input
                      className="w-full rounded-xl bg-stone-900/50 backdrop-blur-sm border border-stone-700/50 px-4 py-3 text-sm text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-2 focus:ring-amber-500/50 focus:border-amber-500/50 transition-all duration-300"
                      placeholder="https://api.openai.com/v1"
                      value={llmConfig.api_base}
                      onChange={(e) => setLlmConfig((prev) => ({ ...prev, api_base: e.target.value }))}
                    />
                  </div>
                )}

                {llmError && (
                  <div className="text-xs text-red-300 bg-red-900/30 border border-red-800/50 px-4 py-3 rounded-xl backdrop-blur-sm animate-in fade-in duration-300">
                    ⚠️ {llmError}
                  </div>
                )}

                <button
                  className="w-full mt-3 px-4 py-3.5 rounded-xl bg-gradient-to-br from-amber-500 to-amber-600 text-stone-950 text-sm font-semibold hover:shadow-lg hover:shadow-amber-500/30 hover:scale-105 transition-all duration-300 shadow-md"
                  onClick={handleLlmNext}
                >
                  下一步 →
                </button>
              </div>
            </div>
          )}

          {/* Confirm step */}
          {step === STEP_CONFIRM && (
            <div className="flex items-start gap-3 mb-6 animate-in fade-in slide-in-from-bottom-2 duration-500">
              <div className="w-8 h-8 rounded-full bg-gradient-to-br from-emerald-500 to-emerald-600 flex items-center justify-center text-sm font-bold shrink-0 shadow-lg shadow-emerald-500/30 ring-2 ring-emerald-500/20">
                🐾
              </div>
              <div className="bg-gradient-to-br from-stone-800/80 to-stone-800/40 backdrop-blur-sm border border-stone-700/50 rounded-3xl px-5 py-3.5 text-sm text-stone-200 max-w-2xl shadow-lg">
                好了，{answers.name ? `${answers.name}！` : ''}我对你有了初步了解 🐾
                <br /><br />
                这些都会写在{' '}
                <span className="font-mono text-amber-300 text-xs bg-stone-900/50 px-2 py-1 rounded">~/.openpup/OWNER.md</span>{' '}
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
            <div className="flex-shrink-0 border-t border-stone-800/50 px-8 py-4 bg-gradient-to-t from-stone-950 via-stone-950/80 to-stone-950/50 flex gap-3">
              <textarea
                ref={inputRef}
                className="flex-1 resize-none rounded-2xl bg-stone-900/50 backdrop-blur-sm border border-stone-700/50 px-4 py-3 text-sm text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-2 focus:ring-amber-500/50 focus:border-amber-500/50 shadow-sm"
                rows={3}
                placeholder={QUESTIONS[step].placeholder}
                value={currentInput}
                onChange={(e) => setCurrentInput(e.target.value)}
                onKeyDown={handleKeyDown}
              />
              <button
                className="self-end px-5 py-3 rounded-xl bg-gradient-to-br from-amber-500 to-amber-600 text-stone-950 text-sm font-semibold disabled:opacity-40 hover:shadow-lg hover:shadow-amber-500/30 hover:scale-105 disabled:hover:scale-100 transition-all duration-300 shadow-md"
                onClick={handleNext}
                disabled={!currentInput.trim()}
              >
                {step < TOTAL_PROFILE - 1 ? '下一步 →' : '下一步 →'}
              </button>
            </div>
          )}
        </div>

        {/* Right: OWNER.md live preview */}
        <div className="w-96 border-l border-stone-800/50 p-6 flex flex-col bg-gradient-to-b from-stone-900/50 via-stone-900/30 to-stone-900/10 shrink-0 backdrop-blur-sm">
          <div className="text-xs font-semibold text-stone-400 mb-3 uppercase tracking-wider">预览 OWNER.md</div>
          {step < STEP_CONFIRM ? (
            <pre className="flex-1 bg-stone-900/60 backdrop-blur border border-stone-700/50 rounded-2xl p-4 text-xs text-stone-300 font-mono leading-relaxed overflow-auto whitespace-pre-wrap shadow-inner">
              {ownerMdPreview}
            </pre>
          ) : (
            <>
              <textarea
                className="flex-1 bg-stone-900/60 backdrop-blur border border-stone-700/50 rounded-2xl p-4 text-xs text-stone-300 font-mono leading-relaxed resize-none focus:outline-none focus:ring-2 focus:ring-amber-500/50 overflow-auto shadow-inner transition-all duration-300"
                value={editableOwnerMd}
                onChange={(e) => setEditableOwnerMd(e.target.value)}
                spellCheck={false}
              />
              <button
                className="mt-4 w-full py-3.5 rounded-xl bg-gradient-to-br from-amber-500 to-amber-600 text-stone-950 text-sm font-semibold disabled:opacity-40 hover:shadow-lg hover:shadow-amber-500/30 hover:scale-105 disabled:hover:scale-100 transition-all duration-300 shadow-md"
                onClick={() => void handleSave()}
                disabled={saving}
              >
                {saving ? '保存中…' : '确认并开始 →'}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
};

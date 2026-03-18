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
  { label: 'DeepSeek V3 (api.deepseek.com)', value: 'deepseek-chat', base: 'https://api.deepseek.com/v1' },
  { label: 'DeepSeek Reasoner (api.deepseek.com)', value: 'deepseek-reasoner', base: 'https://api.deepseek.com/v1' },
  { label: 'DeepSeek V3 (SiliconFlow)', value: 'deepseek-ai/DeepSeek-V3', base: 'https://api.siliconflow.cn/v1' },
  { label: '自定义…', value: '__custom__', base: '' },
];

const INPUT_CLS = 'w-full rounded-xl bg-stone-900 border border-stone-700 px-3.5 py-2.5 text-sm text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-1 focus:ring-amber-500/50 transition-colors';

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

  // LLM config state
  const [llmPreset, setLlmPreset] = useState(PRESET_MODELS[0].value);
  const [llmConfig, setLlmConfig] = useState<LlmConfig>({ api_key: '', model: 'gpt-4o', api_base: '' });
  const [llmError, setLlmError] = useState('');

  useEffect(() => {
    if (step < TOTAL_PROFILE) inputRef.current?.focus();
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
    if (!llmConfig.api_key.trim()) { setLlmError('请填写 API Key'); return; }
    if (!llmConfig.model.trim()) { setLlmError('请填写模型名称'); return; }
    setLlmError('');
    setEditableOwnerMd(buildOwnerMd(answers));
    setStep(STEP_CONFIRM);
  };

  // ── Save ──────────────────────────────────────────────────────────────────

  const handleSave = async () => {
    setSaving(true);
    try {
      // 1. Save OWNER.md profile
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

      // 2. Save LLM config
      if (llmConfig.api_key.trim()) {
        await invoke('set_llm_provider', {
          provider: 'openai',
          model: llmConfig.model.trim(),
          miniModel: null,
          embedModel: null,
          apiKey: llmConfig.api_key.trim(),
          apiBase: llmConfig.api_base.trim() || null,
        });
      }

      onComplete();
    } catch (e) {
      console.error(e);
      setSaving(false);
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
      <div className="border-b border-stone-800 px-6 py-3 flex items-center justify-between shrink-0">
        <div className="flex items-center gap-2">
          <span className="text-base">🐾</span>
          <span className="font-bold text-sm tracking-tight">openpup</span>
          <span className="text-stone-600 text-xs">· 初次见面</span>
        </div>
        <div className="text-xs text-stone-500">{stepLabel}</div>
      </div>

      {/* Progress */}
      <div className="h-0.5 bg-stone-800 shrink-0">
        <div className="h-full bg-amber-500 transition-all duration-500" style={{ width: `${progressPct}%` }} />
      </div>

      <div className="flex flex-1 overflow-hidden">
        {/* Left: Conversation / LLM config */}
        <div className="flex-1 flex flex-col p-6 overflow-auto">

          {/* Answered profile questions */}
          {QUESTIONS.slice(0, step < TOTAL_PROFILE ? step : TOTAL_PROFILE).map((q) => (
            <div key={q.key} className="mb-5">
              <div className="flex items-start gap-3 mb-2">
                <div className="w-7 h-7 rounded-full bg-emerald-600 flex items-center justify-center text-xs font-bold shrink-0 shadow-sm shadow-emerald-500/30">
                  🐾
                </div>
                <div className="bg-stone-800 rounded-2xl rounded-tl-sm px-4 py-2.5 text-sm text-stone-200 whitespace-pre-line max-w-sm shadow-sm">
                  {q.pup_text}
                </div>
              </div>
              {answers[q.key] && (
                <div className="flex justify-end">
                  <div className="bg-amber-700 rounded-2xl rounded-tr-sm px-4 py-2.5 text-sm text-white max-w-sm shadow-sm">
                    {answers[q.key]}
                  </div>
                </div>
              )}
            </div>
          ))}

          {/* Current profile question */}
          {step < TOTAL_PROFILE && (
            <div className="mb-4">
              <div className="flex items-start gap-3 mb-4">
                <div className="w-7 h-7 rounded-full bg-emerald-600 flex items-center justify-center text-xs font-bold shrink-0 shadow-sm shadow-emerald-500/30">
                  🐾
                </div>
                <div className="bg-stone-800 rounded-2xl rounded-tl-sm px-4 py-2.5 text-sm text-stone-200 whitespace-pre-line max-w-sm shadow-sm">
                  {QUESTIONS[step].pup_text}
                </div>
              </div>
              <div className="flex gap-2 mt-2">
                <textarea
                  ref={inputRef}
                  className="flex-1 resize-none rounded-xl bg-stone-800 border border-stone-700 px-3.5 py-2.5 text-sm text-stone-100 placeholder:text-stone-500 focus:outline-none focus:ring-1 focus:ring-amber-500/50 transition-colors"
                  rows={2}
                  placeholder={QUESTIONS[step].placeholder}
                  value={currentInput}
                  onChange={(e) => setCurrentInput(e.target.value)}
                  onKeyDown={handleKeyDown}
                />
                <button
                  className="self-end px-4 py-2.5 rounded-xl bg-amber-500 text-stone-950 text-sm font-medium disabled:opacity-40 hover:bg-amber-400 transition-colors shadow-sm shadow-amber-500/20"
                  onClick={handleNext}
                  disabled={!currentInput.trim()}
                >
                  {step < TOTAL_PROFILE - 1 ? '下一步 →' : '下一步 →'}
                </button>
              </div>
            </div>
          )}

          {/* LLM config step */}
          {step === STEP_LLM && (
            <div className="max-w-md space-y-4">
              <div className="flex items-start gap-3 mb-2">
                <div className="w-7 h-7 rounded-full bg-emerald-600 flex items-center justify-center text-xs font-bold shrink-0 shadow-sm shadow-emerald-500/30">
                  🐾
                </div>
                <div className="bg-stone-800 rounded-2xl rounded-tl-sm px-4 py-2.5 text-sm text-stone-200 max-w-sm shadow-sm">
                  最后一步——我需要 AI 接口才能工作。<br /><br />
                  请选择你的模型供应商并填入 API Key。
                </div>
              </div>

              <div className="space-y-3 pl-10">
                {/* Preset picker */}
                <div>
                  <label className="block text-xs text-stone-400 mb-1.5">选择模型</label>
                  <select
                    className={INPUT_CLS + ' bg-stone-900'}
                    value={llmPreset}
                    onChange={(e) => handlePresetChange(e.target.value)}
                  >
                    {PRESET_MODELS.map((p) => (
                      <option key={p.value} value={p.value}>{p.label}</option>
                    ))}
                  </select>
                </div>

                {/* Custom model name — shown only in custom mode */}
                {isCustomPreset && (
                  <div>
                    <label className="block text-xs text-stone-400 mb-1.5">模型名称</label>
                    <input
                      className={INPUT_CLS}
                      placeholder="例如：gpt-4o、claude-3-5-sonnet-20241022"
                      value={llmConfig.model}
                      onChange={(e) => setLlmConfig((prev) => ({ ...prev, model: e.target.value }))}
                    />
                  </div>
                )}

                {/* API Key */}
                <div>
                  <label className="block text-xs text-stone-400 mb-1.5">API Key</label>
                  <input
                    className={INPUT_CLS}
                    type="password"
                    placeholder="sk-..."
                    value={llmConfig.api_key}
                    onChange={(e) => setLlmConfig((prev) => ({ ...prev, api_key: e.target.value }))}
                  />
                </div>

                {/* Base URL — shown in custom mode or if preset has a non-default base */}
                {(isCustomPreset || llmConfig.api_base) && (
                  <div>
                    <label className="block text-xs text-stone-400 mb-1.5">
                      Base URL <span className="text-stone-600">（可选，留空使用 OpenAI 默认）</span>
                    </label>
                    <input
                      className={INPUT_CLS}
                      placeholder="https://api.openai.com/v1"
                      value={llmConfig.api_base}
                      onChange={(e) => setLlmConfig((prev) => ({ ...prev, api_base: e.target.value }))}
                    />
                  </div>
                )}

                {llmError && (
                  <p className="text-xs text-red-400 bg-red-900/20 px-3 py-2 rounded-lg">{llmError}</p>
                )}

                <div className="flex gap-2 pt-1">
                  <button
                    className="text-xs text-stone-500 hover:text-stone-400 transition-colors"
                    onClick={() => { setLlmError(''); setStep(STEP_CONFIRM); setEditableOwnerMd(buildOwnerMd(answers)); }}
                  >
                    跳过，稍后配置
                  </button>
                  <button
                    className="ml-auto px-4 py-2.5 rounded-xl bg-amber-500 text-stone-950 text-sm font-medium hover:bg-amber-400 transition-colors shadow-sm shadow-amber-500/20"
                    onClick={handleLlmNext}
                  >
                    下一步 →
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* Confirm step */}
          {step === STEP_CONFIRM && (
            <div className="flex items-start gap-3 mb-4">
              <div className="w-7 h-7 rounded-full bg-emerald-600 flex items-center justify-center text-xs font-bold shrink-0 shadow-sm shadow-emerald-500/30">
                🐾
              </div>
              <div className="bg-stone-800 rounded-2xl rounded-tl-sm px-4 py-2.5 text-sm text-stone-200 max-w-sm shadow-sm">
                好了，{answers.name ? `${answers.name}！` : ''}我对你有了初步了解 🐾
                <br /><br />
                这些都会写在{' '}
                <span className="font-mono text-stone-400 text-xs">~/.openpup/OWNER.md</span>{' '}
                里，你随时可以打开直接修改。
                <br /><br />
                右侧是预览，确认无误后点击「确认并开始」。
              </div>
            </div>
          )}
        </div>

        {/* Right: OWNER.md live preview */}
        <div className="w-80 border-l border-stone-800 p-4 flex flex-col bg-stone-900/30 shrink-0">
          <div className="text-xs text-stone-500 mb-2.5 font-medium">自动生成 OWNER.md 预览</div>
          {step < STEP_CONFIRM ? (
            <pre className="flex-1 bg-stone-900 border border-stone-800 rounded-xl p-3.5 text-xs text-stone-300 font-mono leading-relaxed overflow-auto whitespace-pre-wrap">
              {ownerMdPreview}
            </pre>
          ) : (
            <>
              <textarea
                className="flex-1 bg-stone-900 border border-stone-800 rounded-xl p-3.5 text-xs text-stone-300 font-mono leading-relaxed resize-none focus:outline-none focus:ring-1 focus:ring-amber-500/50 overflow-auto"
                value={editableOwnerMd}
                onChange={(e) => setEditableOwnerMd(e.target.value)}
                spellCheck={false}
              />
              <button
                className="mt-3 w-full py-2.5 rounded-xl bg-amber-500 text-stone-950 text-sm font-semibold disabled:opacity-40 hover:bg-amber-400 transition-colors shadow-sm shadow-amber-500/20"
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

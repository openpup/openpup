import React, { createContext, useContext, useState } from 'react';

export type Lang = 'zh' | 'en';

const T = {
  zh: {
    app_name: 'openpup',
    nav_chat: '对话', nav_pack_channel: '频道', nav_memories: '记忆', nav_timeline: '时间线',
    nav_tasks: '任务', nav_skills: '技能', nav_pups: '伙伴',
    nav_mcp: 'MCP', nav_settings: '设置',
    pup_section: '狗群', pup_back_to_alpha: '切换回 Alpha',
    pup_talking_to: '正在与', pup_talking_to_suffix: '对话',
    chat_placeholder_alpha: '告诉 Alpha…', chat_thinking: '思考中…',
    chat_welcome: '汪！我已经准备好了。今天想先做什么？🐾',
    tab_long_term: '长期记忆', tab_diary: '每日日志',
    mem_search: '搜索记忆…', mem_search_btn: '搜索',
    mem_prev: '上一页', mem_next: '下一页', mem_empty: '暂无记忆',
    mem_edit: '编辑', mem_delete: '删除', mem_save: '保存', mem_cancel: '取消',
    mem_edit_title: '编辑记忆', mem_type_placeholder: '类型，如 preference / fact',
    mem_confirm_delete: '确定要删除这条记忆吗？此操作不可恢复。',
    mem_importance: '重要性',
    timeline_search: '搜索对话历史…', timeline_search_btn: '搜索',
    timeline_clear: '清除', timeline_refresh: '刷新',
    timeline_empty: '暂无记录', timeline_no_results: '未找到匹配记录',
    tab_all: '全部', tab_alpha: 'Alpha', tab_you: '我', tab_skills_run: '⚡ 技能',
    task_title: '任务追踪', task_new: '新建任务',
    task_desc_placeholder: '任务描述…', task_pup_placeholder: '分配给（可选，如 dev）',
    task_add: '添加', task_adding: '添加中…', task_empty: '暂无任务',
    task_active: '进行中', task_done_section: '已完成',
    task_pending: '待处理', task_in_progress: '进行中',
    task_done: '已完成', task_failed: '失败',
    task_start: '开始', task_complete: '完成', task_reopen: '重开', task_delete: '删除',
    task_created_at: '创建', task_completed_at: '完成',
    skills_installed: '已安装', skills_discover: '发现', skills_registries: '注册源',
    skills_git_title: '从 Git 安装', skills_repo: 'Git 仓库地址',
    skills_subdir: '子目录（可选）', skills_install_btn: '审查并安装',
    skills_installing: '安装中…', skills_vetting: '审查中…',
    skills_empty: '尚无已安装的技能', skills_builtin: '内置',
    skills_enable: '启用', skills_disable: '禁用', skills_uninstall: '卸载',
    skills_refresh: '刷新', skills_discovering: '搜索中…',
    skills_no_results: '没有发现可用技能',
    skills_add_registry: '添加注册源', skills_registry_name: '名称',
    skills_registry_url: 'JSON URL',
    vetting_title: '安全审查报告', vetting_cancel: '取消安装',
    vetting_confirm: '确认安装', vetting_caution: '⚠️ 仍然安装',
    pup_mgr_title: '伙伴配置', pup_custom: '自定义',
    pup_edit: '编辑', pup_collapse: '收起', pup_enabled: '启用', pup_disabled: '禁用',
    pup_remove: '删除', pup_prompt_label: '系统提示词（留空使用内置默认）：',
    pup_prompt_custom: '系统提示词（必填）', pup_prompt_builtin: '覆盖内置提示词（可选）',
    pup_save: '保存', pup_cancel: '取消', pup_add_title: '添加自定义伙伴',
    pup_key_ph: '标识符（如 finance）', pup_name_ph: '显示名称',
    pup_desc_ph: '描述', pup_prompt_ph: '系统提示词（必填）',
    pup_add_btn: '添加', pup_adding: '添加中…',
    mcp_title: 'MCP 服务器', mcp_builtin: '内置本地工具', mcp_always_on: '始终启用',
    mcp_add_title: '添加 MCP 服务器', mcp_name_ph: '名称', mcp_url_ph: 'Base URL',
    mcp_token_ph: 'Token（可选）', mcp_desc_ph: '描述（可选）',
    mcp_add_btn: '添加', mcp_adding: '添加中…',
    mcp_tools_title: '发现的工具', mcp_refresh: '刷新工具', mcp_refreshing: '刷新中…',
    mcp_tools_empty: '暂无工具，点击刷新连接服务器',
    mcp_enabled: '已启用', mcp_disabled: '已禁用', mcp_delete: '删除',
    settings_exec: '执行模式', settings_leashed: '🔒 牵绳', settings_free: '🐕 放养',
    settings_leashed_note: '危险操作需每次确认', settings_free_note: '受信任技能可自动执行',
    settings_theme: '界面主题',
    settings_backup: 'Workspace 备份', settings_export: '导出 workspace',
    settings_exporting: '导出中…', settings_restore: '从备份恢复',
    settings_restore_ph: '备份文件路径', settings_import: '从备份导入',
    settings_importing: '导入中…', settings_lang: '语言 / Language',
    diary_empty: '还没有日记记录。', diary_select: '← 选择一天查看日记',
    diary_loading: '加载中…',
    skill_install_suggestion: '安装', skill_dismiss_suggestion: '忽略',
    mode_leashed: '🔒 牵绳', mode_free: '🐕 放养',
  },
  en: {
    app_name: 'openpup',
    nav_chat: 'Chat', nav_pack_channel: 'Channel', nav_memories: 'Memory', nav_timeline: 'Timeline',
    nav_tasks: 'Tasks', nav_skills: 'Skills', nav_pups: 'Pups',
    nav_mcp: 'MCP', nav_settings: 'Settings',
    pup_section: 'Pack', pup_back_to_alpha: 'Back to Alpha',
    pup_talking_to: 'Talking to', pup_talking_to_suffix: '',
    chat_placeholder_alpha: 'Tell Alpha…', chat_thinking: 'Thinking…',
    chat_welcome: "Woof! Ready to help. What's on your mind? 🐾",
    tab_long_term: 'Long-term Memory', tab_diary: 'Daily Log',
    mem_search: 'Search memories…', mem_search_btn: 'Search',
    mem_prev: 'Prev', mem_next: 'Next', mem_empty: 'No memories yet',
    mem_edit: 'Edit', mem_delete: 'Delete', mem_save: 'Save', mem_cancel: 'Cancel',
    mem_edit_title: 'Edit Memory', mem_type_placeholder: 'Type, e.g. preference / fact',
    mem_confirm_delete: 'Delete this memory? This cannot be undone.',
    mem_importance: 'importance',
    timeline_search: 'Search history…', timeline_search_btn: 'Search',
    timeline_clear: 'Clear', timeline_refresh: 'Refresh',
    timeline_empty: 'No records yet', timeline_no_results: 'No matching records',
    tab_all: 'All', tab_alpha: 'Alpha', tab_you: 'Me', tab_skills_run: '⚡ Skills',
    task_title: 'Task Tracker', task_new: 'New Task',
    task_desc_placeholder: 'Task description…', task_pup_placeholder: 'Assign to pup (optional)',
    task_add: 'Add', task_adding: 'Adding…', task_empty: 'No tasks yet',
    task_active: 'Active', task_done_section: 'Completed',
    task_pending: 'Pending', task_in_progress: 'In Progress',
    task_done: 'Done', task_failed: 'Failed',
    task_start: 'Start', task_complete: 'Done', task_reopen: 'Reopen', task_delete: 'Delete',
    task_created_at: 'Created', task_completed_at: 'Completed',
    skills_installed: 'Installed', skills_discover: 'Discover', skills_registries: 'Registries',
    skills_git_title: 'Install from Git', skills_repo: 'Git repo URL',
    skills_subdir: 'Subdirectory (optional)', skills_install_btn: 'Review & Install',
    skills_installing: 'Installing…', skills_vetting: 'Reviewing…',
    skills_empty: 'No installed skills yet', skills_builtin: 'builtin',
    skills_enable: 'Enable', skills_disable: 'Disable', skills_uninstall: 'Uninstall',
    skills_refresh: 'Refresh', skills_discovering: 'Searching…',
    skills_no_results: 'No skills found',
    skills_add_registry: 'Add Registry', skills_registry_name: 'Name',
    skills_registry_url: 'JSON URL',
    vetting_title: 'Security Review', vetting_cancel: 'Cancel Install',
    vetting_confirm: 'Confirm Install', vetting_caution: '⚠️ Install Anyway',
    pup_mgr_title: 'Pup Configuration', pup_custom: 'custom',
    pup_edit: 'Edit', pup_collapse: 'Collapse', pup_enabled: 'Enabled', pup_disabled: 'Disabled',
    pup_remove: 'Remove', pup_prompt_label: 'System prompt override (leave empty for built-in):',
    pup_prompt_custom: 'System prompt (required)', pup_prompt_builtin: 'Override built-in (optional)',
    pup_save: 'Save', pup_cancel: 'Cancel', pup_add_title: 'Add Custom Pup',
    pup_key_ph: 'Identifier (e.g. finance)', pup_name_ph: 'Display name',
    pup_desc_ph: 'Description', pup_prompt_ph: 'System prompt (required)',
    pup_add_btn: 'Add Pup', pup_adding: 'Adding…',
    mcp_title: 'MCP Servers', mcp_builtin: 'Built-in local tools', mcp_always_on: 'Always on',
    mcp_add_title: 'Add MCP Server', mcp_name_ph: 'Name', mcp_url_ph: 'Base URL',
    mcp_token_ph: 'Token (optional)', mcp_desc_ph: 'Description (optional)',
    mcp_add_btn: 'Add', mcp_adding: 'Adding…',
    mcp_tools_title: 'Discovered Tools', mcp_refresh: 'Refresh Tools', mcp_refreshing: 'Refreshing…',
    mcp_tools_empty: 'No tools yet. Click refresh to connect.',
    mcp_enabled: 'Enabled', mcp_disabled: 'Disabled', mcp_delete: 'Remove',
    settings_exec: 'Execution Mode', settings_leashed: '🔒 Leashed', settings_free: '🐕 Free Run',
    settings_leashed_note: 'Dangerous actions require confirmation',
    settings_free_note: 'Trusted skills run automatically',
    settings_theme: 'Theme',
    settings_backup: 'Workspace Backup', settings_export: 'Export workspace',
    settings_exporting: 'Exporting…', settings_restore: 'Restore from Backup',
    settings_restore_ph: 'Path to backup file', settings_import: 'Import from backup',
    settings_importing: 'Importing…', settings_lang: '语言 / Language',
    diary_empty: 'No diary entries yet.', diary_select: '← Select a date to read',
    diary_loading: 'Loading…',
    skill_install_suggestion: 'Install', skill_dismiss_suggestion: 'Dismiss',
    mode_leashed: '🔒 Leashed', mode_free: '🐕 Free Run',
  },
} as const;

export type TKey = keyof typeof T.zh;

export function t(key: TKey, lang: Lang): string {
  return ((T[lang] as Record<string, string>)[key as string]) ??
         ((T.zh as Record<string, string>)[key as string]) ?? key;
}

// ─── Language context ─────────────────────────────────────────────────────────

export const LangContext = createContext<{ lang: Lang; setLang: (l: Lang) => void }>({
  lang: 'zh',
  setLang: () => {},
});

export const LangProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [lang, setLangState] = useState<Lang>(
    () => (localStorage.getItem('openpup_lang') as Lang | null) ?? 'zh',
  );
  const setLang = (l: Lang) => {
    localStorage.setItem('openpup_lang', l);
    setLangState(l);
  };
  return React.createElement(LangContext.Provider, { value: { lang, setLang } }, children);
};

export function useLang() {
  return useContext(LangContext);
}

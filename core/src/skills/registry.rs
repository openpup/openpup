use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::warn;

// ── Skill manifest (TOML) ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    pub description: String,
    #[serde(default)]
    pub category: String,
    /// Keywords / phrases that trigger this skill from natural language.
    #[serde(default)]
    pub triggers: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SkillPermissions {
    #[serde(default)]
    pub shell: bool,
    #[serde(default)]
    pub sandbox_shell: bool,
    #[serde(default)]
    pub file_read: bool,
    #[serde(default)]
    pub file_write: bool,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub mcp: bool,
    #[serde(default)]
    pub dangerous: bool,
    #[serde(default)]
    pub dangerous_operations: bool,
}

impl Default for SkillPermissions {
    fn default() -> Self {
        Self {
            shell: false,
            sandbox_shell: false,
            file_read: false,
            file_write: false,
            network: false,
            mcp: false,
            dangerous: false,
            dangerous_operations: false,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct SkillPrompt {
    pub system: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SkillManifest {
    pub metadata: SkillMetadata,
    #[serde(default)]
    pub permissions: SkillPermissions,
    #[serde(default)]
    pub prompt: Option<SkillPrompt>,
}

#[derive(Debug, Clone)]
enum SkillSource {
    BuiltinToml(String),
    TomlFile(PathBuf),
    SkillHubDir(PathBuf),
}

#[derive(Debug, Clone)]
struct RegisteredSkill {
    metadata: SkillMetadata,
    permissions: SkillPermissions,
    source: SkillSource,
    bundle: Option<SkillBundle>,
}

#[derive(Debug, Clone)]
struct SkillBundle {
    resource_index: SkillResourceIndex,
    path_validation: SkillPathValidation,
}

#[derive(Debug, Clone)]
pub struct SkillResourceIndex {
    pub skill_root: PathBuf,
    pub entry_prompt_path: PathBuf,
    pub files_by_relpath: HashMap<String, PathBuf>,
    pub dirs_by_relpath: HashMap<String, PathBuf>,
}

impl SkillResourceIndex {
    pub fn file_paths(&self) -> Vec<String> {
        let mut files: Vec<String> = self.files_by_relpath.keys().cloned().collect();
        files.sort();
        files
    }

    pub fn dir_paths(&self) -> Vec<String> {
        let mut dirs: Vec<String> = self.dirs_by_relpath.keys().cloned().collect();
        dirs.sort();
        dirs
    }

    pub fn resolve_relpath(&self, relpath: &str) -> Option<PathBuf> {
        self.files_by_relpath
            .get(relpath)
            .cloned()
            .or_else(|| self.dirs_by_relpath.get(relpath).cloned())
    }

    pub fn render_resource_listing(&self, limit: usize) -> String {
        let files = self.file_paths();
        let dirs = self.dir_paths();
        let visible_files: Vec<String> = files.iter().take(limit).cloned().collect();
        let visible_dirs: Vec<String> = dirs.iter().take(limit).cloned().collect();
        let mut lines = vec![
            format!("skill_root: {}", self.skill_root.display()),
            format!("entry_prompt: {}", self.entry_prompt_path.display()),
            format!("indexed_file_count: {}", files.len()),
            format!("indexed_dir_count: {}", dirs.len()),
        ];

        if visible_dirs.is_empty() {
            lines.push("dirs: (none)".to_string());
        } else {
            lines.push(format!("dirs: {}", visible_dirs.join(", ")));
            if dirs.len() > visible_dirs.len() {
                lines.push(format!(
                    "more_dirs_omitted: {}",
                    dirs.len() - visible_dirs.len()
                ));
            }
        }

        if visible_files.is_empty() {
            lines.push("files: (none)".to_string());
        } else {
            lines.push(format!("files: {}", visible_files.join(", ")));
            if files.len() > visible_files.len() {
                lines.push(format!(
                    "more_files_omitted: {}",
                    files.len() - visible_files.len()
                ));
            }
        }

        lines.join("\n")
    }
}

#[derive(Debug, Clone, Default)]
pub struct SkillPathValidation {
    pub referenced_paths: Vec<String>,
    pub resolved_paths: Vec<String>,
    pub unresolved_paths: Vec<String>,
    pub near_matches: Vec<SkillPathNearMatch>,
}

#[derive(Debug, Clone)]
pub struct SkillPathNearMatch {
    pub referenced_path: String,
    pub matched_path: String,
}

#[derive(Debug, Clone)]
pub struct LoadedSkillPrompt {
    pub metadata: SkillMetadata,
    pub permissions: SkillPermissions,
    pub prompt: String,
    pub resource_index: Option<SkillResourceIndex>,
    pub path_validation: Option<SkillPathValidation>,
}

impl LoadedSkillPrompt {
    pub fn render_with_preamble(&self) -> String {
        if let Some(index) = &self.resource_index {
            let files = index.file_paths();
            let dirs = index.dir_paths();
            let visible_files: Vec<String> = files.iter().take(12).cloned().collect();
            let mut sections = vec![
                "## Claude Skill Context".to_string(),
                format!("- Active skill: {}", self.metadata.name),
                format!("- Skill root: {}", index.skill_root.display()),
                format!("- Entry prompt: {}", index.entry_prompt_path.display()),
                format!("- Indexed files: {}", files.len()),
                format!("- Indexed directories: {}", dirs.len()),
                String::new(),
                "## Skill path rules".to_string(),
                "- Relative paths in this skill are relative to the skill root, never the workspace root.".to_string(),
                "- Never guess, invent, autocorrect, singularize, or pluralize a path.".to_string(),
                "- Use only: indexed skill resources, exact user-provided paths, or explicit tool discovery results.".to_string(),
                "- If a path is unresolved, report it. Do not silently substitute a near match.".to_string(),
                "- Do not pass unresolved paths to `file_read`, `file_write`, or `skill_read_resource`.".to_string(),
                "- Use `skill_list_resources` before reading when the exact indexed path is uncertain.".to_string(),
                "- Use `skill_read_resource` for indexed skill files whenever possible.".to_string(),
                String::new(),
                "## Indexed resource summary".to_string(),
            ];

            if visible_files.is_empty() {
                sections.push("- files: (none)".to_string());
            } else {
                sections.push(format!("- files: {}", visible_files.join(", ")));
                if files.len() > visible_files.len() {
                    sections.push(format!(
                        "- more_files_omitted: {}",
                        files.len() - visible_files.len()
                    ));
                }
            }

            if let Some(validation) = &self.path_validation {
                sections.push(String::new());
                sections.push("## Referenced path validation".to_string());
                if validation.referenced_paths.is_empty() {
                    sections.push(
                        "- No explicit relative path references were extracted from SKILL.md."
                            .to_string(),
                    );
                } else {
                    sections.push(format!(
                        "- Referenced paths: {}",
                        validation.referenced_paths.join(", ")
                    ));
                    if validation.resolved_paths.is_empty() {
                        sections.push("- Resolved paths: none".to_string());
                    } else {
                        sections.push(format!(
                            "- Resolved paths: {}",
                            validation.resolved_paths.join(", ")
                        ));
                    }
                    if validation.unresolved_paths.is_empty() {
                        sections.push("- Unresolved paths: none".to_string());
                    } else {
                        sections.push(format!(
                            "- Unresolved paths: {}",
                            validation.unresolved_paths.join(", ")
                        ));
                        sections.push("- Guardrail: unresolved paths must not be passed to `file_read`, `file_write`, or `skill_read_resource` unless the user explicitly confirms the exact path or a later tool result resolves it.".to_string());
                        sections.push("- Guardrail: near matches are diagnostic only. Do not auto-select them, even for obvious typos such as `script/` vs `scripts/`.".to_string());
                    }
                    if validation.near_matches.is_empty() {
                        sections.push("- Near matches: none".to_string());
                    } else {
                        sections.push(
                            "- Near matches (for reporting only; do not auto-use):".to_string(),
                        );
                        sections.extend(validation.near_matches.iter().map(|item| {
                            format!("  - {} -> {}", item.referenced_path, item.matched_path)
                        }));
                    }
                }
            }

            sections.push(String::new());
            sections.push("## Skill instructions".to_string());
            sections.push(self.prompt.clone());
            sections.join("\n")
        } else {
            self.prompt.clone()
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct SkillHubFrontmatter {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    metadata: Option<serde_yaml::Value>,
}

// ── Installed skill record ────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstalledSkill {
    pub name: String,
    pub description: String,
    pub category: String,
    pub source: String,
    pub repo_url: Option<String>,
    pub installed_at: i64,
    pub enabled: bool,
}

// ── Registry ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SkillRegistry {
    skills: Arc<RwLock<HashMap<String, RegisteredSkill>>>,
    installed: Arc<RwLock<HashMap<String, InstalledSkill>>>,
    persist_path: PathBuf,
    scan_roots: Arc<RwLock<Vec<(PathBuf, String)>>>,
    generation: Arc<std::sync::atomic::AtomicU64>,
}

impl SkillRegistry {
    pub fn new(persist_path: PathBuf) -> Self {
        let registry = Self {
            skills: Arc::new(RwLock::new(HashMap::new())),
            installed: Arc::new(RwLock::new(HashMap::new())),
            persist_path,
            scan_roots: Arc::new(RwLock::new(Vec::new())),
            generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        };
        registry.load_installed_snapshot();
        registry
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn bump_generation(&self) {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub async fn register_builtin(&self, toml: &str) {
        let manifest: SkillManifest = match toml::from_str(toml) {
            Ok(m) => m,
            Err(e) => {
                warn!("[registry] failed to parse builtin skill: {e}");
                return;
            }
        };
        let now = chrono::Utc::now().timestamp();
        self.register_skill(
            RegisteredSkill {
                metadata: manifest.metadata,
                permissions: manifest.permissions,
                source: SkillSource::BuiltinToml(toml.to_string()),
                bundle: None,
            },
            "builtin",
            now,
        )
        .await;
    }

    /// Check if a path is a skill TOML file.
    ///
    /// Only files matching `*.skill.toml` are recognised as skill manifests.
    /// This avoids false positives from pyproject.toml, Cargo.toml, config.toml, etc.
    fn is_skill_toml(path: &Path) -> bool {
        path.file_name()
            .and_then(|s| s.to_str())
            .map(|name| name.ends_with(".skill.toml"))
            .unwrap_or(false)
    }

    pub async fn add_scan_root(&self, path: PathBuf, source: impl Into<String>) {
        let source = source.into();
        let mut roots = self.scan_roots.write().await;
        if !roots.iter().any(|(p, _)| p == &path) {
            roots.push((path, source));
        }
    }

    pub async fn refresh(&self) {
        self.clear_dynamic_skills().await;
        let roots = self.scan_roots.read().await.clone();
        for (path, source) in roots {
            if let Err(e) = self.register_from_dir(&path, &source).await {
                warn!("skill refresh {}: {e}", path.display());
            }
        }
        self.reconcile_installed_with_registered().await;
        self.save_installed_snapshot();
        self.bump_generation();
    }

    pub async fn enabled_skills_for_tools(&self) -> Vec<(String, String, Vec<String>)> {
        let installed = self.installed.read().await;
        let skills = self.skills.read().await;
        installed
            .values()
            .filter(|s| s.enabled)
            .filter_map(|s| {
                let manifest = skills.get(&s.name)?;
                Some((
                    s.name.clone(),
                    manifest.metadata.description.clone(),
                    manifest.metadata.triggers.clone(),
                ))
            })
            .collect()
    }

    pub async fn enabled_skills_for_tools_tagged(
        &self,
    ) -> Vec<(String, String, Vec<String>, bool)> {
        let installed = self.installed.read().await;
        let skills = self.skills.read().await;
        installed
            .values()
            .filter(|s| s.enabled)
            .filter_map(|s| {
                let manifest = skills.get(&s.name)?;
                let is_builtin = matches!(manifest.source, SkillSource::BuiltinToml(_));
                Some((
                    s.name.clone(),
                    manifest.metadata.description.clone(),
                    manifest.metadata.triggers.clone(),
                    is_builtin,
                ))
            })
            .collect()
    }

    pub async fn load_from_dir(&self, root: impl AsRef<Path>) -> Result<()> {
        let root = root.as_ref();
        if !root.exists() {
            return Ok(());
        }
        for entry in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if Self::is_skill_toml(path) {
                self.load_one(path).await?;
            }
        }
        Ok(())
    }

    pub async fn register_from_dir(&self, root: impl AsRef<Path>, source: &str) -> Result<()> {
        let root = root.as_ref();
        if !root.exists() {
            return Ok(());
        }
        let now = chrono::Utc::now().timestamp();
        let mut any = false;

        for entry in walkdir::WalkDir::new(root)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            if Self::is_skill_toml(path) {
                let text = match fs::read_to_string(path) {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("read {}: {e}", path.display());
                        continue;
                    }
                };
                let manifest: SkillManifest = match toml::from_str(&text) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("parse {}: {e}", path.display());
                        continue;
                    }
                };
                self.register_skill(
                    RegisteredSkill {
                        metadata: manifest.metadata,
                        permissions: manifest.permissions,
                        source: SkillSource::TomlFile(path.to_path_buf()),
                        bundle: None,
                    },
                    source,
                    now,
                )
                .await;
                any = true;
                continue;
            }

            if path.is_dir() {
                let skill_md = path.join("SKILL.md");
                if skill_md.exists() {
                    match Self::parse_skill_hub_dir(path) {
                        Ok(skill) => {
                            self.register_skill(skill, source, now).await;
                            any = true;
                        }
                        Err(e) => warn!("parse SkillHub {}: {e}", path.display()),
                    }
                }
            }
        }

        if any {
            self.save_installed_snapshot();
        }
        Ok(())
    }

    async fn register_skill(&self, skill: RegisteredSkill, source: &str, now: i64) {
        let name = skill.metadata.name.clone();
        let description = skill.metadata.description.clone();
        let category = skill.metadata.category.clone();
        self.skills.write().await.insert(name.clone(), skill);
        let mut installed = self.installed.write().await;
        installed
            .entry(name.clone())
            .and_modify(|meta| {
                meta.description = description.clone();
                meta.category = category.clone();
                meta.source = source.to_string();
            })
            .or_insert_with(|| InstalledSkill {
                name,
                description,
                category,
                source: source.to_string(),
                repo_url: None,
                installed_at: now,
                enabled: true,
            });
        self.bump_generation();
    }

    async fn clear_dynamic_skills(&self) {
        let mut skills = self.skills.write().await;
        skills.retain(|_, skill| matches!(skill.source, SkillSource::BuiltinToml(_)));
    }

    pub async fn reconcile_installed_with_registered(&self) {
        let registered = self.skills.read().await;
        let mut installed = self.installed.write().await;
        installed.retain(|name, _| registered.contains_key(name));
        for (name, skill) in registered.iter() {
            let description = skill.metadata.description.clone();
            let category = skill.metadata.category.clone();
            let source = match skill.source {
                SkillSource::BuiltinToml(_) => "builtin",
                SkillSource::TomlFile(_) | SkillSource::SkillHubDir(_) => "local",
            }
            .to_string();

            if let Some(meta) = installed.get_mut(name) {
                meta.description = description;
                meta.category = category;
                meta.source = source;
            }
        }
    }

    fn parse_skill_hub_dir(dir: &Path) -> Result<RegisteredSkill> {
        let skill_md_path = dir.join("SKILL.md");
        let md_text = fs::read_to_string(&skill_md_path)?;
        let (frontmatter, body) = Self::parse_skill_markdown(&md_text)?;

        let version = dir
            .join("_meta.json")
            .exists()
            .then(|| fs::read_to_string(dir.join("_meta.json")).ok())
            .flatten()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["version"].as_str().map(String::from))
            .unwrap_or_else(|| "1.0.0".to_string());

        let meta_value = frontmatter
            .metadata
            .clone()
            .and_then(|value| serde_json::to_value(value).ok());

        let mut shell = false;
        let mut network = false;

        if let Some(ref mv) = meta_value {
            let requires: Option<&serde_json::Value> =
                if let Some(req) = mv.pointer("/clawdbot/requires") {
                    Some(req)
                } else {
                    mv.get("requires")
                };
            if let Some(req) = requires {
                if req["bins"]
                    .as_array()
                    .map(|a| !a.is_empty())
                    .unwrap_or(false)
                {
                    shell = true;
                }
                if req["network"].as_bool().unwrap_or(false) {
                    network = true;
                }
            }
        }

        if body.contains("curl ") || body.contains("https://") || body.contains("http://") {
            shell = true;
            network = true;
        }

        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let description = if frontmatter.description.trim().is_empty() {
            frontmatter.name.trim().to_string()
        } else {
            frontmatter.description.trim().to_string()
        };
        let triggers: Vec<String> = std::iter::once(name.replace(['_', '-'], " "))
            .chain(name.split(['_', '-']).map(String::from))
            .filter(|s| s.len() > 2)
            .collect();

        let resource_index = Self::build_resource_index(dir, &skill_md_path)?;
        let path_validation = Self::validate_referenced_paths(&body, &resource_index);

        Ok(RegisteredSkill {
            metadata: SkillMetadata {
                name,
                version,
                author: String::new(),
                description,
                category: "skillhub".to_string(),
                triggers,
            },
            permissions: SkillPermissions {
                shell,
                network,
                ..Default::default()
            },
            source: SkillSource::SkillHubDir(dir.to_path_buf()),
            bundle: Some(SkillBundle {
                resource_index,
                path_validation,
            }),
        })
    }

    async fn load_one(&self, path: &Path) -> Result<()> {
        let text = fs::read_to_string(path)?;
        let manifest: SkillManifest = toml::from_str(&text)?;
        self.skills.write().await.insert(
            manifest.metadata.name.clone(),
            RegisteredSkill {
                metadata: manifest.metadata,
                permissions: manifest.permissions,
                source: SkillSource::TomlFile(path.to_path_buf()),
                bundle: None,
            },
        );
        Ok(())
    }

    pub async fn get(&self, name: &str) -> Option<SkillManifest> {
        self.ensure_skill(name).await.ok()
    }

    pub async fn list(&self) -> Vec<SkillManifest> {
        let names: Vec<String> = self.skills.read().await.keys().cloned().collect();
        let mut manifests = Vec::new();
        for name in names {
            if let Ok(manifest) = self.ensure_skill(&name).await {
                manifests.push(manifest);
            }
        }
        manifests
    }

    pub async fn ensure_skill(&self, name: &str) -> Result<SkillManifest> {
        let loaded = self.load_skill_prompt(name).await?;
        Ok(SkillManifest {
            metadata: loaded.metadata,
            permissions: loaded.permissions,
            prompt: Some(SkillPrompt {
                system: loaded.prompt,
            }),
        })
    }

    pub async fn load_skill_prompt(&self, name: &str) -> Result<LoadedSkillPrompt> {
        let skill = self
            .skills
            .read()
            .await
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("skill not found: {name}"))?;
        let prompt = self.load_prompt(&skill)?;
        Ok(LoadedSkillPrompt {
            metadata: skill.metadata,
            permissions: skill.permissions,
            prompt: prompt.system,
            resource_index: skill
                .bundle
                .as_ref()
                .map(|bundle| bundle.resource_index.clone()),
            path_validation: skill
                .bundle
                .as_ref()
                .map(|bundle| bundle.path_validation.clone()),
        })
    }

    pub async fn resolve_skill_resource_path(
        &self,
        skill_name: &str,
        relpath: &str,
    ) -> Result<PathBuf> {
        let skill = self
            .skills
            .read()
            .await
            .get(skill_name)
            .cloned()
            .ok_or_else(|| anyhow!("skill not found: {skill_name}"))?;
        let bundle = skill
            .bundle
            .ok_or_else(|| anyhow!("skill '{skill_name}' has no indexed resource bundle"))?;
        bundle
            .resource_index
            .resolve_relpath(relpath)
            .ok_or_else(|| anyhow!("skill resource not found: {skill_name}:{relpath}"))
    }

    pub async fn list_skill_resources(&self, skill_name: &str, limit: usize) -> Result<String> {
        let skill = self
            .skills
            .read()
            .await
            .get(skill_name)
            .cloned()
            .ok_or_else(|| anyhow!("skill not found: {skill_name}"))?;
        let bundle = skill
            .bundle
            .ok_or_else(|| anyhow!("skill '{skill_name}' has no indexed resource bundle"))?;
        Ok(bundle.resource_index.render_resource_listing(limit))
    }

    pub async fn set_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        let mut guard = self.installed.write().await;
        if let Some(meta) = guard.get_mut(name) {
            meta.enabled = enabled;
            drop(guard);
            self.save_installed_snapshot();
            self.bump_generation();
            Ok(())
        } else {
            Err(anyhow!("skill not installed: {name}"))
        }
    }

    pub async fn list_installed(&self) -> Vec<InstalledSkill> {
        self.reconcile_installed_with_registered().await;
        let guard = self.installed.read().await;
        let mut v: Vec<InstalledSkill> = guard.values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub async fn enabled_skill_names_and_triggers(&self) -> Vec<(String, Vec<String>)> {
        let installed = self.installed.read().await;
        let skills = self.skills.read().await;
        installed
            .values()
            .filter(|s| s.enabled)
            .map(|s| {
                let triggers = skills
                    .get(&s.name)
                    .map(|m| m.metadata.triggers.clone())
                    .unwrap_or_default();
                (s.name.clone(), triggers)
            })
            .collect()
    }

    fn load_installed_snapshot(&self) {
        if !self.persist_path.exists() {
            return;
        }
        if let Ok(text) = fs::read_to_string(&self.persist_path) {
            if let Ok(map) = serde_json::from_str::<HashMap<String, InstalledSkill>>(&text) {
                if let Ok(mut guard) = self.installed.try_write() {
                    *guard = map;
                }
            }
        }
    }

    fn save_installed_snapshot(&self) {
        if let Ok(guard) = self.installed.try_read() {
            if let Ok(text) = serde_json::to_string_pretty(&*guard) {
                let _ = fs::create_dir_all(
                    self.persist_path.parent().unwrap_or_else(|| Path::new(".")),
                );
                let _ = fs::write(&self.persist_path, text);
            }
        }
    }

    fn load_prompt(&self, skill: &RegisteredSkill) -> Result<SkillPrompt> {
        match &skill.source {
            SkillSource::BuiltinToml(text) => {
                let manifest: SkillManifest = toml::from_str(text)?;
                manifest.prompt.ok_or_else(|| {
                    anyhow!("skill '{}' has no [prompt] section", skill.metadata.name)
                })
            }
            SkillSource::TomlFile(path) => {
                let text = fs::read_to_string(path)?;
                let manifest: SkillManifest = toml::from_str(&text)?;
                manifest.prompt.ok_or_else(|| {
                    anyhow!("skill '{}' has no [prompt] section", skill.metadata.name)
                })
            }
            SkillSource::SkillHubDir(dir) => {
                let md_text = fs::read_to_string(dir.join("SKILL.md"))?;
                let (_frontmatter, body) = Self::parse_skill_markdown(&md_text)?;
                Ok(SkillPrompt {
                    system: body.trim().to_string(),
                })
            }
        }
    }

    fn parse_skill_markdown(md_text: &str) -> Result<(SkillHubFrontmatter, String)> {
        let md = md_text.trim();
        let md = md
            .strip_prefix("---")
            .ok_or_else(|| anyhow!("missing opening ---"))?;
        let (frontmatter, body) = md
            .split_once("\n---")
            .ok_or_else(|| anyhow!("missing closing ---"))?;
        let frontmatter: SkillHubFrontmatter = serde_yaml::from_str(frontmatter)?;
        Ok((frontmatter, body.trim().to_string()))
    }

    fn build_resource_index(dir: &Path, entry_prompt_path: &Path) -> Result<SkillResourceIndex> {
        let skill_root = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let entry_prompt_path =
            fs::canonicalize(entry_prompt_path).unwrap_or_else(|_| entry_prompt_path.to_path_buf());
        let mut files_by_relpath = HashMap::new();
        let mut dirs_by_relpath = HashMap::new();

        for entry in walkdir::WalkDir::new(&skill_root)
            .into_iter()
            .filter_entry(|entry| !Self::is_ignored_skill_entry(entry.path()))
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path == skill_root {
                continue;
            }
            let rel = match path.strip_prefix(&skill_root) {
                Ok(rel) => rel,
                Err(_) => continue,
            };
            let rel = Self::normalize_relpath(rel);
            if rel.is_empty() {
                continue;
            }
            if path.is_dir() {
                dirs_by_relpath.insert(rel, path.to_path_buf());
            } else if path.is_file() {
                files_by_relpath.insert(rel, path.to_path_buf());
            }
        }

        Ok(SkillResourceIndex {
            skill_root,
            entry_prompt_path,
            files_by_relpath,
            dirs_by_relpath,
        })
    }

    fn validate_referenced_paths(body: &str, index: &SkillResourceIndex) -> SkillPathValidation {
        let referenced_paths = Self::extract_path_candidates(body);
        let mut resolved_paths = Vec::new();
        let mut unresolved_paths = Vec::new();
        let mut near_matches = Vec::new();

        for path in &referenced_paths {
            if index.files_by_relpath.contains_key(path) || index.dirs_by_relpath.contains_key(path)
            {
                resolved_paths.push(path.clone());
                continue;
            }
            unresolved_paths.push(path.clone());
            if let Some(matched_path) = Self::find_near_match(path, index) {
                near_matches.push(SkillPathNearMatch {
                    referenced_path: path.clone(),
                    matched_path,
                });
            }
        }

        SkillPathValidation {
            referenced_paths,
            resolved_paths,
            unresolved_paths,
            near_matches,
        }
    }

    fn extract_path_candidates(body: &str) -> Vec<String> {
        let mut result = Vec::new();
        for raw in body.split_whitespace() {
            let candidate = raw
                .trim_matches(|c: char| {
                    matches!(
                        c,
                        '`' | '"'
                            | '\''
                            | ','
                            | '.'
                            | ';'
                            | ':'
                            | '('
                            | ')'
                            | '['
                            | ']'
                            | '{'
                            | '}'
                            | '<'
                            | '>'
                    )
                })
                .trim();
            if Self::looks_like_skill_path(candidate)
                && !result.iter().any(|existing| existing == candidate)
            {
                result.push(candidate.to_string());
            }
        }
        result
    }

    fn looks_like_skill_path(candidate: &str) -> bool {
        if candidate.len() < 3 || candidate.starts_with('/') || candidate.starts_with("~/") {
            return false;
        }
        if candidate.contains("://") || candidate.contains('\\') || candidate.contains(' ') {
            return false;
        }
        if !candidate.contains('/') {
            return false;
        }
        let mut segments = candidate.split('/');
        let first = segments.next().unwrap_or_default();
        let last = candidate.rsplit('/').next().unwrap_or_default();
        if first.is_empty() || last.is_empty() || candidate.ends_with('/') {
            return false;
        }
        if first.starts_with('.') || first.starts_with('#') {
            return false;
        }
        last.contains('.') || candidate.chars().filter(|c| *c == '/').count() >= 1
    }

    fn find_near_match(path: &str, index: &SkillResourceIndex) -> Option<String> {
        let mut candidates: Vec<String> = index.file_paths();
        candidates.extend(index.dir_paths());
        let target = path.to_ascii_lowercase();

        let mut best: Option<(usize, String)> = None;
        for candidate in candidates {
            let distance = Self::levenshtein(&target, &candidate.to_ascii_lowercase());
            if distance <= 3 {
                match &best {
                    Some((best_distance, _)) if distance >= *best_distance => {}
                    _ => best = Some((distance, candidate)),
                }
            }
        }
        best.map(|(_, candidate)| candidate)
    }

    fn levenshtein(a: &str, b: &str) -> usize {
        if a == b {
            return 0;
        }
        if a.is_empty() {
            return b.chars().count();
        }
        if b.is_empty() {
            return a.chars().count();
        }

        let b_chars: Vec<char> = b.chars().collect();
        let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
        let mut curr = vec![0; b_chars.len() + 1];

        for (i, a_char) in a.chars().enumerate() {
            curr[0] = i + 1;
            for (j, b_char) in b_chars.iter().enumerate() {
                let cost = usize::from(a_char != *b_char);
                curr[j + 1] =
                    std::cmp::min(std::cmp::min(curr[j] + 1, prev[j + 1] + 1), prev[j] + cost);
            }
            prev.clone_from_slice(&curr);
        }

        prev[b_chars.len()]
    }

    fn normalize_relpath(path: &Path) -> String {
        path.components()
            .map(|component| component.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/")
    }

    fn is_ignored_skill_entry(path: &Path) -> bool {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| {
                matches!(
                    name,
                    ".git" | "target" | "node_modules" | "dist" | "__pycache__"
                )
            })
            .unwrap_or(false)
    }
}

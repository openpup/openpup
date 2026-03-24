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
    // ── New primitive-level flags (zeroclaw-style) ─────────────────────────────
    /// Allow the skill to run shell commands.
    #[serde(default)]
    pub shell: bool,
    /// Allow file_read and file_write tools.
    #[serde(default)]
    pub filesystem: bool,
    /// Allow http_get tool.
    #[serde(default)]
    pub network: bool,
    /// Allow calling registered MCP server tools.
    #[serde(default)]
    pub mcp: bool,
    /// Marks the skill as dangerous (requires user confirmation).
    #[serde(default)]
    pub dangerous: bool,

    /// Legacy alias — treated same as `dangerous`.
    #[serde(default)]
    pub dangerous_operations: bool,
}

impl Default for SkillPermissions {
    fn default() -> Self {
        Self {
            shell: false,
            filesystem: false,
            network: false,
            mcp: false,
            dangerous: false,
            dangerous_operations: false,
        }
    }
}

/// Skill prompt — the system prompt injected at the start of the tool-call loop.
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
    /// Directories to re-scan on refresh() — populated at startup.
    scan_roots: Arc<RwLock<Vec<(PathBuf, String)>>>,
}

impl SkillRegistry {
    pub fn new(persist_path: PathBuf) -> Self {
        let registry = Self {
            skills: Arc::new(RwLock::new(HashMap::new())),
            installed: Arc::new(RwLock::new(HashMap::new())),
            persist_path,
            scan_roots: Arc::new(RwLock::new(Vec::new())),
        };
        registry.load_installed_snapshot();
        registry
    }

    /// Register a built-in skill from an embedded TOML string.
    /// Uses `or_insert_with` so user enable/disable state is preserved across restarts.
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
            },
            "builtin",
            now,
        )
        .await;
    }

    /// Register a directory as a scan root so `refresh()` can re-discover skills written there.
    pub async fn add_scan_root(&self, path: PathBuf, source: impl Into<String>) {
        let source = source.into();
        let mut roots = self.scan_roots.write().await;
        if !roots.iter().any(|(p, _)| p == &path) {
            roots.push((path, source));
        }
    }

    /// Re-scan all registered directories and pick up any new skill files.
    /// Called after file_write / shell_exec tool calls so LLM-written skills are live immediately.
    pub async fn refresh(&self) {
        let roots = self.scan_roots.read().await.clone();
        for (path, source) in roots {
            if let Err(e) = self.register_from_dir(&path, &source).await {
                warn!("skill refresh {}: {e}", path.display());
            }
        }
    }

    /// Returns enabled skills as (name, description, triggers) for injection into LLM tool lists.
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

    // ── CRUD ───────────────────────────────────────────────────────────────────

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
            if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                self.load_one(path).await?;
            }
        }
        Ok(())
    }

    /// Load skill manifests from `root` and register them as installed.
    ///
    /// Supports two formats, detected automatically:
    /// - **TOML** (`*.toml`): native openpup manifest
    /// - **SkillHub / ClawHub** (`SKILL.md` + optional `_meta.json`): community hub format
    ///
    /// Uses `or_insert_with` so existing enabled/disabled state is preserved.
    pub async fn register_from_dir(&self, root: impl AsRef<Path>, source: &str) -> Result<()> {
        let root = root.as_ref();
        if !root.exists() {
            return Ok(());
        }
        let now = chrono::Utc::now().timestamp();
        let mut any = false;

        // Collect immediate subdirectories (SkillHub packs) and TOML files.
        // We walk depth=2 so that a flat dir of .toml files and a dir of skill
        // subdirectories (each containing SKILL.md) both work.
        for entry in walkdir::WalkDir::new(root)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // ── TOML format ──────────────────────────────────────────────────
            if path.extension().and_then(|s| s.to_str()) == Some("toml") {
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
                    },
                    source,
                    now,
                )
                .await;
                any = true;
                continue;
            }

            // ── SkillHub / ClawHub format ─────────────────────────────────────
            // Detect: directory containing SKILL.md
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

    /// Register a parsed skill into both `skills` and `installed` maps.
    async fn register_skill(&self, skill: RegisteredSkill, source: &str, now: i64) {
        let name = skill.metadata.name.clone();
        let description = skill.metadata.description.clone();
        let category = skill.metadata.category.clone();
        self.skills.write().await.insert(name.clone(), skill);
        self.installed
            .write()
            .await
            .entry(name.clone())
            .or_insert_with(|| InstalledSkill {
                name,
                description,
                category,
                source: source.to_string(),
                repo_url: None,
                installed_at: now,
                enabled: true,
            });
    }

    /// Parse a SkillHub / ClawHub skill directory (contains `SKILL.md` and
    /// optionally `_meta.json`) into a native `SkillManifest`.
    ///
    /// SKILL.md format:
    /// ```
    /// ---
    /// name: my_skill
    /// description: What it does.
    /// metadata: {"clawdbot":{"requires":{"bins":["curl"],"network":true}}}
    /// ---
    /// # Body used as system prompt
    /// ```
    fn parse_skill_hub_dir(dir: &Path) -> Result<RegisteredSkill> {
        // ── Parse SKILL.md ────────────────────────────────────────────────────
        let md_text = fs::read_to_string(dir.join("SKILL.md"))?;
        let md = md_text.trim();

        // Extract YAML frontmatter between leading `---` and next `---`
        let md = md
            .strip_prefix("---")
            .ok_or_else(|| anyhow!("missing opening ---"))?;
        let (frontmatter, body) = md
            .split_once("\n---")
            .ok_or_else(|| anyhow!("missing closing ---"))?;
        let body = body.trim();

        // Parse frontmatter: simple `key: rest-of-line` (value may contain colons)
        let mut name = String::new();
        let mut description = String::new();
        let mut meta_value: Option<serde_json::Value> = None;

        for line in frontmatter.lines() {
            if let Some((k, v)) = line.split_once(':') {
                let k = k.trim();
                let v = v.trim();
                match k {
                    "name" => name = v.to_string(),
                    "description" => description = v.to_string(),
                    "metadata" => {
                        // Value is a JSON object inline in YAML
                        // Reconstruct full value including any colons after the key
                        let full_val = line[k.len() + 1..].trim();
                        meta_value = serde_json::from_str(full_val).ok();
                    }
                    _ => {}
                }
            }
        }

        if name.is_empty() {
            // Fall back to directory name
            name = dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
        }

        // ── Parse _meta.json for version ──────────────────────────────────────
        let version = dir
            .join("_meta.json")
            .exists()
            .then(|| fs::read_to_string(dir.join("_meta.json")).ok())
            .flatten()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["version"].as_str().map(String::from))
            .unwrap_or_else(|| "1.0.0".to_string());

        // ── Derive permissions ────────────────────────────────────────────────
        let mut shell = false;
        let mut network = false;

        // From clawdbot / generic `requires` block in metadata
        if let Some(ref mv) = meta_value {
            // Support both {"clawdbot":{...}} and {"requires":{...}} layouts
            let requires = mv
                .pointer("/clawdbot/requires")
                .or_else(|| mv.get("requires"));
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

        // Heuristic: body mentions curl / http → needs shell + network
        if body.contains("curl ") || body.contains("https://") || body.contains("http://") {
            shell = true;
            network = true;
        }

        // ── Derive triggers from name words ───────────────────────────────────
        let triggers: Vec<String> = std::iter::once(name.replace('_', " "))
            .chain(name.split('_').map(String::from))
            .filter(|s| s.len() > 2)
            .collect();

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
        let skill = self
            .skills
            .read()
            .await
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("skill not found: {name}"))?;
        let prompt = self.load_prompt(&skill)?;
        Ok(SkillManifest {
            metadata: skill.metadata,
            permissions: skill.permissions,
            prompt: Some(prompt),
        })
    }

    pub async fn set_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        let mut guard = self.installed.write().await;
        if let Some(meta) = guard.get_mut(name) {
            meta.enabled = enabled;
            drop(guard);
            self.save_installed_snapshot();
            Ok(())
        } else {
            Err(anyhow!("skill not installed: {name}"))
        }
    }

    pub async fn list_installed(&self) -> Vec<InstalledSkill> {
        let guard = self.installed.read().await;
        let mut v: Vec<InstalledSkill> = guard.values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// Returns names of enabled installed skills plus their trigger keywords.
    /// Used by Alpha Pup for classify_intent.
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

    // ── Private helpers ────────────────────────────────────────────────────────

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
                let md = md_text.trim();
                let md = md
                    .strip_prefix("---")
                    .ok_or_else(|| anyhow!("missing opening ---"))?;
                let (_frontmatter, body) = md
                    .split_once("\n---")
                    .ok_or_else(|| anyhow!("missing closing ---"))?;
                Ok(SkillPrompt {
                    system: body.trim().to_string(),
                })
            }
        }
    }
}

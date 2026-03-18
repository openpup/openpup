use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

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

  // ── Legacy fields (kept for backward compat with old-style TOML skills) ────
  #[serde(default)]
  pub required_scopes: Vec<String>,
  #[serde(default)]
  pub data_access: Vec<String>,
  #[serde(default)]
  pub network_access: Vec<String>,
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
      required_scopes: vec![],
      data_access: vec![],
      network_access: vec![],
      dangerous_operations: false,
    }
  }
}

/// New-style skill prompt — the system prompt injected at the start of the
/// tool-call loop.  Skills that declare `[prompt]` use the primitive tool
/// loop; skills that declare `[implementation]` use the legacy prompt_chain.
#[derive(Debug, Deserialize, Clone)]
pub struct SkillPrompt {
  pub system: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ExecutionConfig {
  pub mode: String,
}

impl Default for ExecutionConfig {
  fn default() -> Self { Self { mode: "leashed".to_string() } }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Dependencies {
  #[serde(default)]
  pub mcp_servers: Vec<String>,
  #[serde(default)]
  pub local_tools: Vec<String>,
}

impl Default for Dependencies {
  fn default() -> Self { Self { mcp_servers: vec![], local_tools: vec![] } }
}

#[derive(Debug, Deserialize, Clone)]
pub struct PromptChainStep {
  pub action: String,
  #[serde(default)]
  pub params: serde_json::Value,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Implementation {
  pub r#type: String,
  #[serde(default)]
  pub steps: Vec<PromptChainStep>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct SkillScheduleConfig {
  /// Cron expression (5-field "min hour day month weekday" or 6-field with seconds).
  pub cron: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SkillManifest {
  pub metadata: SkillMetadata,
  #[serde(default)]
  pub permissions: SkillPermissions,
  #[serde(default)]
  pub execution: ExecutionConfig,
  #[serde(default)]
  pub dependencies: Dependencies,
  /// New-style: `[prompt]` section drives the primitive tool-call loop.
  #[serde(default)]
  pub prompt: Option<SkillPrompt>,
  /// Legacy: `[implementation]` section drives the prompt_chain executor.
  #[serde(default)]
  pub implementation: Option<Implementation>,
  #[serde(default)]
  pub schedule: Option<SkillScheduleConfig>,
}

// ── Skill registry sources (remote discovery) ─────────────────────────────────

/// A remote registry that lists discoverable (not-yet-installed) skills.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRegistrySource {
  pub name: String,
  pub url: String,
  pub enabled: bool,
}

/// A skill visible in a remote registry (not yet installed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoverableSkill {
  pub name: String,
  pub description: String,
  pub author: String,
  pub category: String,
  pub version: String,
  pub repo_url: String,
  pub registry: String,
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
  skills: Arc<RwLock<HashMap<String, SkillManifest>>>,
  installed: Arc<RwLock<HashMap<String, InstalledSkill>>>,
  sources: Arc<RwLock<Vec<SkillRegistrySource>>>,
  persist_path: PathBuf,
  sources_path: PathBuf,
}

impl SkillRegistry {
  pub fn new(persist_path: PathBuf) -> Self {
    let sources_path = persist_path
      .parent()
      .unwrap_or_else(|| Path::new("."))
      .join("skill_registries.json");

    let registry = Self {
      skills: Arc::new(RwLock::new(HashMap::new())),
      installed: Arc::new(RwLock::new(HashMap::new())),
      sources: Arc::new(RwLock::new(Vec::new())),
      persist_path,
      sources_path,
    };
    registry.load_installed_snapshot();
    registry.load_sources_snapshot();
    registry
  }

  // ── Built-in skills ────────────────────────────────────────────────────────

  /// Register a skill from embedded TOML text (built-ins bundled at compile time).
  pub async fn register_builtin(&self, toml_content: &str) {
    match toml::from_str::<SkillManifest>(toml_content) {
      Ok(manifest) => {
        let name = manifest.metadata.name.clone();
        self.skills.write().await.insert(name.clone(), manifest.clone());
        let mut guard = self.installed.write().await;
        guard.entry(name.clone()).or_insert_with(|| InstalledSkill {
          name,
          description: manifest.metadata.description.clone(),
          category: manifest.metadata.category.clone(),
          source: "builtin".to_string(),
          repo_url: None,
          installed_at: 0,
          enabled: true,
        });
      }
      Err(e) => eprintln!("failed to parse builtin skill: {e}"),
    }
  }

  // ── CRUD ───────────────────────────────────────────────────────────────────

  pub async fn load_from_dir(&self, root: impl AsRef<Path>) -> Result<()> {
    let root = root.as_ref();
    if !root.exists() { return Ok(()); }
    for entry in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
      let path = entry.path();
      if path.extension().and_then(|s| s.to_str()) == Some("toml") {
        self.load_one(path).await?;
      }
    }
    Ok(())
  }

  async fn load_one(&self, path: &Path) -> Result<()> {
    let text = fs::read_to_string(path)?;
    let manifest: SkillManifest = toml::from_str(&text)?;
    self.skills.write().await.insert(manifest.metadata.name.clone(), manifest);
    Ok(())
  }

  pub async fn get(&self, name: &str) -> Option<SkillManifest> {
    self.skills.read().await.get(name).cloned()
  }

  pub async fn list(&self) -> Vec<SkillManifest> {
    self.skills.read().await.values().cloned().collect()
  }

  pub async fn ensure_skill(&self, name: &str) -> Result<SkillManifest> {
    self.get(name).await.ok_or_else(|| anyhow!("skill not found: {name}"))
  }

  pub async fn install_from_dir(&self, root: impl AsRef<Path>, source: &str) -> Result<Vec<String>> {
    let root = root.as_ref();
    self.load_from_dir(root).await?;
    let manifests = self.list().await;
    let mut names = Vec::new();
    let mut guard = self.installed.write().await;
    let now = chrono::Utc::now().timestamp();
    for m in manifests {
      let name = m.metadata.name.clone();
      names.push(name.clone());
      guard.insert(name.clone(), InstalledSkill {
        name,
        description: m.metadata.description.clone(),
        category: m.metadata.category.clone(),
        source: source.to_string(),
        repo_url: None,
        installed_at: now,
        enabled: true,
      });
    }
    drop(guard);
    self.save_installed_snapshot();
    Ok(names)
  }

  pub async fn install_from_git(&self, repo_url: &str, subdir: Option<&str>) -> Result<Vec<String>> {
    let cache_root = Self::skills_cache_root();
    fs::create_dir_all(&cache_root)?;
    let safe_name = repo_url.replace("://", "_").replace(['/', '\\', ':'], "_");
    let target_dir = cache_root.join(safe_name);
    if target_dir.exists() { fs::remove_dir_all(&target_dir)?; }

    let status = Command::new("git")
      .args(["clone", "--depth", "1", repo_url])
      .arg(&target_dir)
      .status()?;
    if !status.success() { return Err(anyhow!("git clone failed for {repo_url}")); }

    let skills_root = if let Some(rel) = subdir { target_dir.join(rel) } else { target_dir };
    self.load_from_dir(&skills_root).await?;

    let manifests = self.list().await;
    let mut names = Vec::new();
    let mut guard = self.installed.write().await;
    let now = chrono::Utc::now().timestamp();
    for m in manifests {
      let name = m.metadata.name.clone();
      names.push(name.clone());
      guard.insert(name.clone(), InstalledSkill {
        name,
        description: m.metadata.description.clone(),
        category: m.metadata.category.clone(),
        source: "git".to_string(),
        repo_url: Some(repo_url.to_string()),
        installed_at: now,
        enabled: true,
      });
    }
    drop(guard);
    self.save_installed_snapshot();
    Ok(names)
  }

  pub async fn uninstall(&self, name: &str) -> Result<()> {
    self.skills.write().await.remove(name);
    self.installed.write().await.remove(name);
    self.save_installed_snapshot();
    Ok(())
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

  // ── Registry sources (remote discovery) ────────────────────────────────────

  pub async fn add_registry_source(&self, source: SkillRegistrySource) {
    let mut guard = self.sources.write().await;
    guard.retain(|s| s.name != source.name);
    guard.push(source);
    drop(guard);
    self.save_sources_snapshot();
  }

  pub async fn remove_registry_source(&self, name: &str) {
    self.sources.write().await.retain(|s| s.name != name);
    self.save_sources_snapshot();
  }

  pub async fn toggle_registry_source(&self, name: &str, enabled: bool) {
    let mut guard = self.sources.write().await;
    if let Some(s) = guard.iter_mut().find(|s| s.name == name) { s.enabled = enabled; }
    drop(guard);
    self.save_sources_snapshot();
  }

  pub async fn list_registry_sources(&self) -> Vec<SkillRegistrySource> {
    self.sources.read().await.clone()
  }

  /// Seed default registry sources on first run (when none are configured).
  /// Called from main.rs after loading the registry.
  pub async fn seed_default_sources_if_empty(&self) {
    if !self.sources.read().await.is_empty() {
      return;
    }
    let defaults = vec![
      SkillRegistrySource {
        name: "ClaWHub".to_string(),
        url: "github://repo:PhenixStar/skill-hub".to_string(),
        enabled: true,
      },
    ];
    let mut guard = self.sources.write().await;
    *guard = defaults;
    drop(guard);
    self.save_sources_snapshot();
    eprintln!("[registry] seeded default ClaWHub registry source");
  }

  /// Fetch discoverable skills from all enabled remote registries.
  /// Registry JSON format: `{"skills": [{name, description, author, category, version, repo_url}]}`
  pub async fn fetch_discoverable(&self) -> Vec<DiscoverableSkill> {
    let sources = self.sources.read().await.clone();
    let mut result = Vec::new();
    let client = reqwest::Client::builder()
      .timeout(std::time::Duration::from_secs(8))
      .build()
      .unwrap_or_default();

    for source in sources.iter().filter(|s| s.enabled) {
      // ── GitHub topic search: github://topic:<topic-name> ───────────────────
      if let Some(topic) = source.url.strip_prefix("github://topic:") {
        let api_url = format!(
          "https://api.github.com/search/repositories?q=topic:{}&per_page=30&sort=stars",
          topic
        );
        if let Ok(resp) = client
          .get(&api_url)
          .header("User-Agent", "openpup/0.1")
          .header("Accept", "application/vnd.github.v3+json")
          .send()
          .await
        {
          if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(items) = json.get("items").and_then(|v| v.as_array()) {
              for item in items {
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if name.is_empty() { continue; }
                result.push(DiscoverableSkill {
                  name,
                  description: item.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                  author: item.get("owner").and_then(|o| o.get("login")).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                  category: "github".to_string(),
                  version: "latest".to_string(),
                  repo_url: item.get("clone_url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                  registry: source.name.clone(),
                });
              }
            }
          }
        }
        continue;
      }

      // ── GitHub repo scan: github://repo:<owner>/<repo>[/<subdir>] ──────────
      // Uses the Git Trees API to list all .toml files in the repo, then fetches
      // and parses each one as a SkillManifest.  repo_url points to the whole
      // repo so `install_from_git` can install all skills in one step.
      if let Some(repo_path) = source.url.strip_prefix("github://repo:") {
        let (repo_slug, subdir) = match repo_path.find('/') {
          None => (repo_path, ""),
          Some(pos) => {
            // owner/repo  or  owner/repo/subdir
            let after_first = &repo_path[pos + 1..];
            match after_first.find('/') {
              None => (repo_path, ""),   // no subdir
              Some(p2) => (&repo_path[..pos + 1 + p2], &after_first[p2 + 1..]),
            }
          }
        };
        let tree_url = format!(
          "https://api.github.com/repos/{repo_slug}/git/trees/main?recursive=1"
        );
        let repo_clone_url = format!("https://github.com/{repo_slug}");
        let (owner, repo_name) = repo_slug
          .split_once('/')
          .unwrap_or(("unknown", repo_slug));

        let tree_resp = client
          .get(&tree_url)
          .header("User-Agent", "openpup/0.1")
          .header("Accept", "application/vnd.github.v3+json")
          .send()
          .await;

        if let Ok(resp) = tree_resp {
          if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(tree) = json.get("tree").and_then(|v| v.as_array()) {
              // Collect all .toml paths (optionally filtered by subdir)
              let toml_paths: Vec<String> = tree
                .iter()
                .filter_map(|node| {
                  let path = node.get("path").and_then(|v| v.as_str())?;
                  let typ = node.get("type").and_then(|v| v.as_str()).unwrap_or("");
                  if typ == "blob" && path.ends_with(".toml") {
                    if subdir.is_empty() || path.starts_with(subdir) {
                      return Some(path.to_string());
                    }
                  }
                  None
                })
                .collect();

              for toml_path in toml_paths {
                let raw_url = format!(
                  "https://raw.githubusercontent.com/{repo_slug}/main/{toml_path}"
                );
                if let Ok(r) = client
                  .get(&raw_url)
                  .header("User-Agent", "openpup/0.1")
                  .send()
                  .await
                {
                  if let Ok(text) = r.text().await {
                    if let Ok(manifest) = toml::from_str::<SkillManifest>(&text) {
                      let m = &manifest.metadata;
                      result.push(DiscoverableSkill {
                        name: m.name.clone(),
                        description: m.description.clone(),
                        author: if m.author.is_empty() {
                          owner.to_string()
                        } else {
                          m.author.clone()
                        },
                        category: if m.category.is_empty() {
                          repo_name.to_string()
                        } else {
                          m.category.clone()
                        },
                        version: m.version.clone(),
                        repo_url: repo_clone_url.clone(),
                        registry: source.name.clone(),
                      });
                    }
                  }
                }
              }
            }
          }
        }
        continue;
      }

      // ── Standard JSON registry ─────────────────────────────────────────────
      if let Ok(resp) = client.get(&source.url).send().await {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
          if let Some(arr) = json.get("skills").and_then(|v| v.as_array()) {
            for item in arr {
              let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
              if name.is_empty() { continue; }
              result.push(DiscoverableSkill {
                name,
                description: item.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                author: item.get("author").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                category: item.get("category").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                version: item.get("version").and_then(|v| v.as_str()).unwrap_or("0.1.0").to_string(),
                repo_url: item.get("repo_url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                registry: source.name.clone(),
              });
            }
          }
        }
      }
    }
    result
  }

  // ── Private helpers ────────────────────────────────────────────────────────

  fn skills_cache_root() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".openpup").join("skills_cache")
  }

  fn load_installed_snapshot(&self) {
    if !self.persist_path.exists() { return; }
    if let Ok(text) = fs::read_to_string(&self.persist_path) {
      if let Ok(map) = serde_json::from_str::<HashMap<String, InstalledSkill>>(&text) {
        if let Ok(mut guard) = self.installed.try_write() { *guard = map; }
      }
    }
  }

  fn save_installed_snapshot(&self) {
    if let Ok(guard) = self.installed.try_read() {
      if let Ok(text) = serde_json::to_string_pretty(&*guard) {
        let _ = fs::create_dir_all(self.persist_path.parent().unwrap_or_else(|| Path::new(".")));
        let _ = fs::write(&self.persist_path, text);
      }
    }
  }

  fn load_sources_snapshot(&self) {
    if !self.sources_path.exists() { return; }
    if let Ok(text) = fs::read_to_string(&self.sources_path) {
      if let Ok(list) = serde_json::from_str::<Vec<SkillRegistrySource>>(&text) {
        if let Ok(mut guard) = self.sources.try_write() { *guard = list; }
      }
    }
  }

  fn save_sources_snapshot(&self) {
    if let Ok(guard) = self.sources.try_read() {
      if let Ok(text) = serde_json::to_string_pretty(&*guard) {
        let _ = fs::create_dir_all(self.sources_path.parent().unwrap_or_else(|| Path::new(".")));
        let _ = fs::write(&self.sources_path, text);
      }
    }
  }
}

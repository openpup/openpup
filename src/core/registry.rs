//! 本地 registry：SubAgent 与 Node 的声明存储（~/.openpup/agents.toml, nodes.toml）。

use anyhow::{Context, Result};
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn openpup_dir() -> Result<PathBuf> {
    let home = home_dir().context("failed to locate home directory")?;
    Ok(home.join(".openpup"))
}

/// 子 Agent 规格：由 `openpup spawn` 写入，供多 Agent 协作提示或未来进程拉起使用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentSpec {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
}

/// 磁盘格式：name -> SubAgentSpec
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AgentsFile {
    #[serde(default)]
    pub agents: HashMap<String, SubAgentSpec>,
}

pub fn agents_path() -> Result<PathBuf> {
    Ok(openpup_dir()?.join("agents.toml"))
}

pub fn load_agents() -> Result<AgentsFile> {
    let path = agents_path()?;
    if !path.exists() {
        return Ok(AgentsFile::default());
    }
    let s = fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    toml::from_str(&s).with_context(|| format!("failed to parse agents file {:?}", path))
}

pub fn save_agents(file: &AgentsFile) -> Result<()> {
    let dir = openpup_dir()?;
    fs::create_dir_all(&dir)?;
    let path = agents_path()?;
    let s = toml::to_string_pretty(file).context("failed to serialize agents")?;
    fs::write(&path, s).with_context(|| format!("failed to write {:?}", path))?;
    Ok(())
}

pub fn register_sub_agent(spec: SubAgentSpec) -> Result<()> {
    let mut file = load_agents()?;
    file.agents.insert(spec.name.clone(), spec);
    save_agents(&file)
}

pub fn list_sub_agents() -> Result<Vec<SubAgentSpec>> {
    let file = load_agents()?;
    Ok(file.agents.into_values().collect())
}

/// 节点信息：由 `openpup node spawn` 写入，供多节点部署与未来 HTTP 控制面使用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub last_seen_ts: i64,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct NodesFile {
    #[serde(default)]
    pub nodes: HashMap<String, NodeInfo>,
}

pub fn nodes_path() -> Result<PathBuf> {
    Ok(openpup_dir()?.join("nodes.toml"))
}

pub fn load_nodes() -> Result<NodesFile> {
    let path = nodes_path()?;
    if !path.exists() {
        return Ok(NodesFile::default());
    }
    let s = fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    toml::from_str(&s).with_context(|| format!("failed to parse nodes file {:?}", path))
}

pub fn save_nodes(file: &NodesFile) -> Result<()> {
    let dir = openpup_dir()?;
    fs::create_dir_all(&dir)?;
    let path = nodes_path()?;
    let s = toml::to_string_pretty(file).context("failed to serialize nodes")?;
    fs::write(&path, s).with_context(|| format!("failed to write {:?}", path))?;
    Ok(())
}

pub fn register_node(info: NodeInfo) -> Result<()> {
    let mut file = load_nodes()?;
    file.nodes.insert(info.name.clone(), info);
    save_nodes(&file)
}

pub fn list_nodes() -> Result<Vec<NodeInfo>> {
    let file = load_nodes()?;
    Ok(file.nodes.into_values().collect())
}

pub fn update_node_heartbeat(node_id: &str, status: &str) -> Result<()> {
    let mut file = load_nodes()?;
    if let Some(node) = file.nodes.get_mut(node_id) {
        node.last_seen_ts = crate::core::memory::now_unix_ts();
        node.status = status.to_string();
    }
    save_nodes(&file)
}

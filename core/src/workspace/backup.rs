use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use chrono::Utc;
use flate2::write::GzEncoder;
use flate2::Compression;
use tar::Builder;
use walkdir::WalkDir;

fn workspace_root() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("cannot determine home directory"))?;
    Ok(home.join(".openpup"))
}

/// Export the current workspace at ~/.openpup into a tar.gz file
/// under ~/Downloads with a timestamped filename.
pub async fn export_workspace_default() -> Result<String> {
    let root = workspace_root()?;
    if !root.exists() {
        return Err(anyhow!("workspace directory does not exist: {:?}", root));
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow!("cannot determine home directory"))?;
    let downloads = home.join("Downloads");
    fs::create_dir_all(&downloads)?;

    let ts = Utc::now().format("%Y%m%d-%H%M%S");
    let target = downloads.join(format!("openpup-backup-{}.tar.gz", ts));

    create_tar_gz(&root, &target)?;

    Ok(target
        .to_str()
        .ok_or_else(|| anyhow!("invalid target path"))?
        .to_string())
}

fn create_tar_gz(root: &Path, target: &Path) -> Result<()> {
    let file = fs::File::create(target)?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = Builder::new(enc);

    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path == root {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|_| anyhow!("failed to strip prefix for {:?}", path))?;

        if path.is_dir() {
            tar.append_dir(rel, path)?;
        } else {
            tar.append_path_with_name(path, rel)?;
        }
    }

    Ok(())
}

/// Import a workspace backup from the given tar.gz file path.
/// This will replace the current ~/.openpup directory after creating
/// a timestamped backup of the existing workspace.
pub async fn import_workspace_from_path(backup_path: &str) -> Result<()> {
    let backup_path = PathBuf::from(backup_path);
    if !backup_path.exists() {
        return Err(anyhow!("backup file not found: {:?}", backup_path));
    }

    let root = workspace_root()?;
    let parent = root
        .parent()
        .ok_or_else(|| anyhow!("invalid workspace root: {:?}", root))?;

    let ts = Utc::now().format("%Y%m%d-%H%M%S");
    let tmp_dir = parent.join(format!(".openpup-import-{}", ts));
    fs::create_dir_all(&tmp_dir)?;

    // Unpack archive into temporary directory
    let file = fs::File::open(&backup_path)?;
    let dec = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(dec);
    archive.unpack(&tmp_dir)?;

    // Basic structural validation
    for required in ["OWNER.md", "PUPS.md", "RULES.md", "database.db"] {
        if !tmp_dir.join(required).exists() {
            return Err(anyhow!(
                "backup is missing required file {} in top-level directory",
                required
            ));
        }
    }

    // Move current workspace to a backup directory if it exists
    if root.exists() {
        let backup_dir = parent.join(format!(".openpup-backup-{}", ts));
        fs::rename(&root, &backup_dir)?;
    }

    // Replace workspace with imported content
    fs::rename(&tmp_dir, &root)?;

    Ok(())
}

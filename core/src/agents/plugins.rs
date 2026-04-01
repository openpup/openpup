use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use libloading::Library;

use crate::agents::specialist::SpecialistPup;

#[allow(improper_ctypes_definitions)]
type CreatePupFn = unsafe extern "C" fn() -> *mut dyn SpecialistPup;

fn plugins_root() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".openpup").join("plugins")
}

/// Load dynamic SpecialistPup plugins from ~/.openpup/plugins.
/// This is a Phase 3 skeleton: we load cdylibs exposing
/// `#[no_mangle] extern "C" fn create_pup() -> Box<dyn SpecialistPup>`.
pub fn load_dynamic_pups() -> Vec<Arc<dyn SpecialistPup>> {
    let root = plugins_root();
    if !root.exists() {
        return Vec::new();
    }

    let mut pups: Vec<Arc<dyn SpecialistPup>> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_dynamic_lib(&path) {
                continue;
            }

            match load_one_plugin(&path) {
                Ok(pup) => pups.push(pup),
                Err(_e) => {
                    // For now, ignore plugin load failures in skeleton.
                }
            }
        }
    }

    pups
}

fn is_dynamic_lib(path: &PathBuf) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        // macOS: .dylib, Linux: .so, Windows: .dll
        matches!(ext, "dylib" | "so" | "dll")
    } else {
        false
    }
}

fn load_one_plugin(path: &PathBuf) -> Result<Arc<dyn SpecialistPup>> {
    // Leak the library so that loaded function pointers and trait objects
    // remain valid for the duration of the process.
    let lib = unsafe { Library::new(path)? };
    let lib = Box::leak(Box::new(lib));

    unsafe {
        let constructor: libloading::Symbol<CreatePupFn> = lib.get(b"create_pup")?;
        let raw = constructor();
        if raw.is_null() {
            anyhow::bail!("create_pup returned null");
        }
        let boxed: Box<dyn SpecialistPup> = Box::from_raw(raw);
        Ok(Arc::from(boxed))
    }
}

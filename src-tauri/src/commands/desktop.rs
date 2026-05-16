use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
#[cfg(target_os = "windows")]
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
#[cfg(target_os = "windows")]
use winreg::RegKey;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::DesktopBehaviorState;

const AUTOSTART_IDENTIFIER: &str = "com.openpup.app";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopBehaviorSettings {
    pub minimize_to_tray_on_close: bool,
    pub launch_at_startup: bool,
    pub tray_available: bool,
    pub autostart_supported: bool,
}

#[tauri::command]
pub fn get_desktop_behavior_settings(
    app: AppHandle,
    desktop_state: State<'_, Arc<DesktopBehaviorState>>,
) -> Result<DesktopBehaviorSettings, String> {
    let cfg = crate::config::load();
    let launch_at_startup = query_autostart_enabled(&app).unwrap_or(cfg.app.launch_at_startup);
    Ok(DesktopBehaviorSettings {
        minimize_to_tray_on_close: cfg.app.minimize_to_tray_on_close,
        launch_at_startup,
        tray_available: desktop_state.tray_available.load(Ordering::Relaxed),
        autostart_supported: autostart_supported(),
    })
}

#[tauri::command]
pub fn save_desktop_behavior_settings(
    app: AppHandle,
    desktop_state: State<'_, Arc<DesktopBehaviorState>>,
    minimize_to_tray_on_close: bool,
    launch_at_startup: bool,
) -> Result<DesktopBehaviorSettings, String> {
    if autostart_supported() {
        set_autostart_enabled(&app, launch_at_startup)?;
    }

    let mut cfg = crate::config::load();
    cfg.app.minimize_to_tray_on_close = minimize_to_tray_on_close;
    cfg.app.launch_at_startup = launch_at_startup;
    crate::config::save(&cfg).map_err(|e| e.to_string())?;

    Ok(DesktopBehaviorSettings {
        minimize_to_tray_on_close,
        launch_at_startup,
        tray_available: desktop_state.tray_available.load(Ordering::Relaxed),
        autostart_supported: autostart_supported(),
    })
}

fn autostart_supported() -> bool {
    cfg!(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows"
    ))
}

fn current_executable() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("无法获取当前程序路径: {e}"))
}

#[cfg(target_os = "macos")]
fn autostart_target_path() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| {
            home.join("Library/LaunchAgents")
                .join(format!("{AUTOSTART_IDENTIFIER}.plist"))
        })
        .ok_or_else(|| "无法确定 LaunchAgents 目录".to_string())
}

#[cfg(target_os = "linux")]
fn autostart_target_path() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(".config/autostart/openpup.desktop"))
        .ok_or_else(|| "无法确定 autostart 目录".to_string())
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "linux")]
fn desktop_exec_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace(' ', "\\ ")
}

#[cfg(target_os = "macos")]
fn set_autostart_enabled(_app: &AppHandle, enabled: bool) -> Result<(), String> {
    let target = autostart_target_path()?;
    if !enabled {
        if target.exists() {
            fs::remove_file(&target).map_err(|e| format!("移除开机启动配置失败: {e}"))?;
        }
        return Ok(());
    }

    let exe = current_executable()?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建 LaunchAgents 目录失败: {e}"))?;
    }
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <false/>
</dict>
</plist>
"#,
        AUTOSTART_IDENTIFIER,
        xml_escape(&exe.to_string_lossy())
    );
    fs::write(&target, plist).map_err(|e| format!("写入开机启动配置失败: {e}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_autostart_enabled(_app: &AppHandle, enabled: bool) -> Result<(), String> {
    let target = autostart_target_path()?;
    if !enabled {
        if target.exists() {
            fs::remove_file(&target).map_err(|e| format!("移除开机启动配置失败: {e}"))?;
        }
        return Ok(());
    }

    let exe = current_executable()?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建 autostart 目录失败: {e}"))?;
    }
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName=OpenPup\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        desktop_exec_escape(&exe.to_string_lossy())
    );
    fs::write(&target, desktop).map_err(|e| format!("写入开机启动配置失败: {e}"))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn set_autostart_enabled(_app: &AppHandle, enabled: bool) -> Result<(), String> {
    let value_name = "OpenPup";
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu
        .create_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            KEY_READ | KEY_SET_VALUE,
        )
        .map_err(|e| format!("打开开机启动注册表失败: {e}"))?;
    if enabled {
        let exe = current_executable()?;
        let quoted = format!("\"{}\"", exe.to_string_lossy());
        run_key
            .set_value(value_name, &quoted)
            .map_err(|e| format!("写入开机启动注册表失败: {e}"))?;
    } else {
        match run_key.delete_value(value_name) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(format!("移除开机启动注册表失败: {err}"));
            }
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn set_autostart_enabled(_app: &AppHandle, _enabled: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn query_autostart_enabled(_app: &AppHandle) -> Result<bool, String> {
    Ok(autostart_target_path()?.exists())
}

#[cfg(target_os = "linux")]
fn query_autostart_enabled(_app: &AppHandle) -> Result<bool, String> {
    Ok(autostart_target_path()?.exists())
}

#[cfg(target_os = "windows")]
fn query_autostart_enabled(_app: &AppHandle) -> Result<bool, String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run_key = hkcu
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", KEY_READ)
        .map_err(|e| format!("读取开机启动注册表失败: {e}"))?;
    match run_key.get_value::<String, _>("OpenPup") {
        Ok(value) => Ok(!value.trim().is_empty()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(format!("读取开机启动注册表失败: {err}")),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn query_autostart_enabled(_app: &AppHandle) -> Result<bool, String> {
    Ok(false)
}

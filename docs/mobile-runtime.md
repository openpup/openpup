# Mobile Runtime Notes

This repository now contains Rust mobile runtimes and Tauri platform-specific configuration for:

- Android via `openpup-runtime-android`
- iOS via `openpup-runtime-ios`

## What is already implemented

- `src-tauri/src/main.rs` selects the runtime by target platform:
  - desktop -> `openpup-runtime-desktop`
  - android -> `openpup-runtime-android`
  - ios -> `openpup-runtime-ios`
- Mobile runtimes use a sandbox-friendly workspace root:
  - defaults to `OPENPUP_MOBILE_WORKSPACE_ROOT`
  - otherwise falls back to the platform local data directory
- Unsupported mobile capabilities are explicit:
  - shell/process execution: unsupported
  - dynamic plugins: unsupported
- Supported mobile capability bridges are file-backed until native bridge wiring is added:
  - scheduler queue
  - notifier queue
  - background task queue
  - secure store placeholder

## Repository configuration

- `src-tauri/tauri.android.conf.json`
- `src-tauri/tauri.ios.conf.json`

These files are merged automatically by the Tauri CLI for their target platforms.

## Host generation status

The Rust/runtime side is ready, but the native host projects were not generated automatically on this machine because local tooling is missing:

- Android init requires Android SDK + NDK and `ANDROID_HOME` / `NDK_HOME`
- iOS init requires Xcode tooling plus a working `xcodegen` installation

Once those prerequisites are installed, generate the native hosts with:

```bash
npx tauri android init
npx tauri ios init
```

If you want deterministic workspace placement during mobile development:

```bash
export OPENPUP_MOBILE_WORKSPACE_ROOT="$PWD/.openpup-mobile"
```

## Recommended next step

After native host generation succeeds, wire the file-backed mobile queues to native services:

- Android:
  - WorkManager / foreground service for background tasks
  - NotificationManager for notifications
  - SAF/content URI imports for external files
- iOS:
  - BGTaskScheduler for background work
  - UNUserNotificationCenter for notifications
  - app group or sandbox document import flow as needed

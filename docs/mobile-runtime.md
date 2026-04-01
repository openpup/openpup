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

## Android CI build and signing

The Android GitHub Actions workflow lives at `.github/workflows/android-test.yml`.

It supports two manual build modes:

- `debug`: builds a debug APK for device testing
- `release`: builds a release APK/AAB and expects signing secrets to be configured

Release builds require these GitHub Actions secrets:

- `ANDROID_KEYSTORE_BASE64`: base64-encoded `.jks` or `.keystore` file
- `ANDROID_KEYSTORE_PASSWORD`: keystore password
- `ANDROID_KEY_ALIAS`: signing key alias
- `ANDROID_KEY_PASSWORD`: signing key password

You can prepare the base64 secret locally with:

```bash
base64 -i release.keystore | pbcopy
```

On Linux, use:

```bash
base64 -w 0 release.keystore
```

In CI, the workflow will:

- decode the keystore into `src-tauri/gen/android/keystores/upload-keystore.jks`
- write `src-tauri/gen/android/keystore.properties`
- patch `src-tauri/gen/android/app/build.gradle.kts` to attach the release signing config

This keeps Android host generation ephemeral while still allowing signed release builds in CI.

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

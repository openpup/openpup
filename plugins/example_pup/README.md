# Example Pup Plugin (Skeleton)

This folder contains a **skeleton** for building a third‑party Pup plugin that can be loaded by openpup at runtime.

> Current status: the main app expects plugins to be built as `cdylib` shared libraries and placed under `~/.openpup/plugins`. This README shows the expected ABI and a minimal example crate layout; you can copy it to your own repo and adjust paths as needed.

## 1. Expected ABI

The runtime loader (`agents/plugins.rs`) looks for dynamic libraries in:

- `~/.openpup/plugins/*.dylib` (macOS)
- `~/.openpup/plugins/*.so` (Linux)
- `~/.openpup/plugins/*.dll` (Windows)

Each plugin **must** export a function with the following signature:

```rust
#[no_mangle]
pub extern "C" fn create_pup() -> *mut dyn SpecialistPup
```

Inside `create_pup`, you construct your Pup type and return it as a raw pointer:

```rust
#[no_mangle]
pub extern "C" fn create_pup() -> *mut dyn SpecialistPup {
    let pup = ExamplePup;
    let boxed: Box<dyn SpecialistPup> = Box::new(pup);
    Box::into_raw(boxed)
}
```

The loader will take ownership of this pointer and wrap it in `Arc<dyn SpecialistPup>`.

## 2. Example crate layout

You would typically create a **separate crate** (can live outside this repo) with:

```text
example_pup/
  Cargo.toml
  src/
    lib.rs
```

### `Cargo.toml`

```toml
[package]
name = "example_pup"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
anyhow = "1"
serde = { version = "1", features = ["derive"] }

# Important:
# You need a shared crate that defines the SpecialistPup trait,
# compiled with the same version as openpup. For now this is
# a conceptual example; in a real plugin you would depend on a
# shared "openpup-core" crate exposing the trait.
```

### `src/lib.rs`

```rust
use anyhow::Result;
use async_trait::async_trait;

// In a real plugin this trait would come from a shared crate, e.g. `openpup_core`.
// Here we duplicate the definition as documentation only.

#[derive(Debug, Clone)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub intent: String,
    pub context: Vec<Message>,
    pub assigned_pup: Option<String>,
    pub status: TaskStatus,
}

#[derive(Debug, Clone)]
pub struct TaskResult {
    pub task_id: String,
    pub status: TaskStatus,
    pub output: String,
}

#[async_trait]
pub trait SpecialistPup: Send + Sync {
    async fn execute(&self, task: Task) -> Result<TaskResult>;
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> Vec<String>;
}

pub struct ExamplePup;

#[async_trait]
impl SpecialistPup for ExamplePup {
    async fn execute(&self, task: Task) -> Result<TaskResult> {
        Ok(TaskResult {
            task_id: task.id,
            status: TaskStatus::Completed,
            output: format!(
                "[example_pup] received intent: {} (context messages: {})",
                task.intent,
                task.context.len()
            ),
        })
    }

    fn name(&self) -> &'static str {
        "example_pup"
    }

    fn capabilities(&self) -> Vec<String> {
        vec!["demo".to_string(), "logging".to_string()]
    }
}

/// Plugin entrypoint expected by openpup.
#[no_mangle]
pub extern "C" fn create_pup() -> *mut dyn SpecialistPup {
    let pup = ExamplePup;
    let boxed: Box<dyn SpecialistPup> = Box::new(pup);
    Box::into_raw(boxed)
}
```

## 3. How to build and install a plugin

1. Copy this example crate to a separate folder (or your own repo).
2. Replace the in‑file duplicated `SpecialistPup` / `Task` / `TaskResult` with imports from a future shared `openpup` core crate when it exists.
3. Build the cdylib:

```bash
cargo build --release
```

4. Copy the resulting shared library to your plugins directory, for example on macOS:

```bash
cp target/release/libexample_pup.dylib ~/.openpup/plugins/
```

5. 启动 openpup：启动时会扫描 `~/.openpup/plugins` 并尝试加载每个插件 Pup。

> 注意：当前仓库中的这个示例文件仅作为文档与参考实现，不直接参与 openpup 的构建流程。


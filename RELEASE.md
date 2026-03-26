## Release 指南

本文件描述如何为 openpup 准备和发布一个新版本。

---

### 1. 准备版本号

1. 选择一个新的语义化版本号，例如 `v0.1.11`。
2. 在以下位置统一更新版本号（保持一致）：
   - `src-tauri/Cargo.toml`
   - `core/Cargo.toml`
   - `cli/Cargo.toml`
   - `daemon/Cargo.toml`
   - `package.json`
   - `package-lock.json`
   - `src-tauri/tauri.conf.json`
   - `CHANGELOG.md` 中的发布区间与版本条目
3. 为该版本补齐 `CHANGELOG.md` 发布说明，并确认最近提交已覆盖目标版本的范围。

---

### 2. 本地检查与打包

在发布前，建议先在本地完成一次完整检查：

```bash
cargo fmt --all
cargo clippy -- -D warnings
cargo test
```

如果你希望在本地先构建 release 二进制，可以执行：

```bash
cargo build --release
```

> 注：当前仓库以 Tauri App 为主，完整的桌面应用打包可以通过前端脚本（例如 `npm run tauri build`）执行，视你的前端工具链而定。

---

### 3. 使用 CI/CD 工作流发布

仓库中已包含两个 GitHub Actions 工作流：

- `.github/workflows/ci.yml`：在 `push` / `pull_request` 时自动运行 `cargo fmt / build / test / clippy`。
- `.github/workflows/release.yml`：在推送以 `v*` 命名的 tag 时构建并创建 GitHub Release。

#### 3.1 创建 Tag 并推送

```bash
git tag v0.1.11
git push origin v0.1.11
```

当 tag 被推送到远程后：

1. `Release` 工作流会在 Linux / macOS 上构建 release 二进制。
2. 工作流会将打包好的 tarball 作为 artifact 上传，并在 GitHub 上创建对应的 Release，附带这些二进制文件。

> 注意：`release.yml` 中使用的是 `softprops/action-gh-release`，并依赖 `secrets.PERSONAL_ACCESS_TOKEN`。在使用前，你需要在 GitHub 仓库的 `Settings → Secrets and variables → Actions` 中配置这个 token，确保它具备 `repo` 权限。

---

### 4. 手动发布（可选）

如果你不希望通过 CI/CD 自动发布，也可以手动：

1. 在本地构建 release 二进制：

   ```bash
   cargo build --release --target x86_64-unknown-linux-gnu   # 例如 Linux
   ```

2. 将构建产物打包：

   ```bash
   tar czvf openpup-0.1.11-linux-x86_64.tar.gz -C target/x86_64-unknown-linux-gnu/release openpup
   ```

3. 在 GitHub 页面上新建一个 Release，上传打包文件，并填写版本说明。

---

### 5. 发布后检查

发布完成后，建议：

- 在至少一个平台上下载 Release 产物并运行，确认启动流程正常。
- 检查 `README.md` 中的描述与实际功能是否一致。
- 在必要时更新后续 Roadmap 或 Issue 列表，标记已完成的工作。


<div align="center">

<h1>
  <img src="openpup-wordmark.png" width="320" alt="OpenPup">
</h1>

**一只记得你是谁的小狗狗 · The local Pup that remembers who you are**

> Not another ChatGPT wrapper.
> Its memory files are yours to edit. Its skills run on your machine.
> The longer you use it, the better it knows you.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE-MIT)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)
[![Tauri](https://img.shields.io/badge/tauri-2.0-blue.svg)](https://tauri.app)

---

**[English →](README.en.md)** · **[中文 →](README.zh.md)** · **[Architecture →](docs/architecture.md)**

<img src="openpup.png" width="860" alt="OpenPup interface">

<p><strong>Ming Dynasty</strong></p>
<img src="openpup-ming.png" width="860" alt="Pack Channel collaboration interface">
<p><a href="./openpup-backup-Ming.zip">openpup-backup-Ming.zip</a></p>

<p><strong>AI Trading</strong></p>
<img src="openpup-trading.png" width="860" alt="Pack Channel collaboration interface">
<p><a href="./openpup-backup-Trading.zip">openpup-backup-Trading.zip</a></p>

</div>

---

## Quick Start

```bash
git clone https://github.com/openpup/openpup
cd openpup
npm install
npm run tauri dev          # desktop app (dev mode)
# or
cargo build --release -p openpup   # CLI only
```

Configure `~/.openpup/config.toml` (auto-created on first launch) with your API key and preferred model.

## What's Inside

A team of specialized pups handles your requests — **Alpha** orchestrates, **Dev / Writer / Ops / Research / Life Admin** execute. Messages route automatically based on intent; use `@dev` or `@writer` to address a pup directly.

All data lives in `~/.openpup/` — plain files, SQLite, no cloud.

## Docs

| Document | Contents |
|----------|----------|
| [README.en.md](README.en.md) | Full English guide — install, config, CLI, skills, MCP |
| [README.zh.md](README.zh.md) | 完整中文说明 |
| [docs/roadmap2.0.md](docs/roadmap2.0.md) | Configurable Organization OS |
| [docs/roadmap2.1.md](docs/roadmap2.1.md) | Agent collaboration spaces |
| [docs/architecture.md](docs/architecture.md) | Technical design — agent routing, memory system, IPC, data flow |

## Mascot & Merch

OpenPup is also growing a small visual world around the product, from mascot explorations to community merchandise.

<div align="center">
<table>
  <tr>
    <td width="70%" align="center" valign="top">
      <img src="mascot.png" height="320" alt="OpenPup mascot" style="border-radius: 16px; border: 1px solid rgba(128,128,128,0.2); box-shadow: 0 8px 24px rgba(0,0,0,0.08);">
    </td>
    <td width="30%" align="center" valign="top">
      <img src="merchandise.png" height="320" alt="OpenPup merchandise and community visuals" style="border-radius: 16px; border: 1px solid rgba(128,128,128,0.2); box-shadow: 0 8px 24px rgba(0,0,0,0.08);">
    </td>
  </tr>
</table>
</div>

## License

[MIT](LICENSE-MIT) OR [Apache 2.0](LICENSE) — © 2026 OpenPup Contributors

//! 统一的 HTTP 客户端入口：支持通过环境变量为整个项目配置代理。
//!
//! 环境变量：
//! - `OPENPUP_PROXY`：HTTP/HTTPS 代理地址，例如 `http://127.0.0.1:7890`。
//!   若未设置，则不使用代理，直接直连。

use anyhow::Result;

/// 创建一个带可选代理的 async 客户端。
pub fn async_client() -> Result<reqwest::Client> {
    let proxy = std::env::var("OPENPUP_PROXY").ok();
    let mut builder = reqwest::Client::builder();
    if let Some(p) = proxy {
        builder = builder.proxy(reqwest::Proxy::all(p)?);
    }
    Ok(builder.build()?)
}

/// 在任意上下文中安全地同步等待一个 async 操作：
/// - 若当前在线程中已存在 Tokio runtime，则使用 block_in_place + Handle::block_on；
/// - 否则创建一个新的 runtime，仅用于本次操作。
pub fn block_on_async<F, T>(fut: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    // 有现成 runtime：在其 blocking 池中同步等待。
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return tokio::task::block_in_place(|| handle.block_on(fut));
    }

    // 无现成 runtime：创建一个临时 runtime。
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(fut)
}

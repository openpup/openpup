//! Node（Worker）相关抽象与传输层。
//!
//! - Trait `NodeTransport` 定义在 `crate::tools`。
//! - 默认 HTTP 实现在 `crate::core::kernel::node_transport::HttpNodeTransport`，通过 HTTP POST `{host}/tool` 调用远端 Worker。

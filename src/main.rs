use anyhow::Result;
use clap::Parser;
use openpup::cli::OpenpupCli;

fn main() -> Result<()> {
    let cli = OpenpupCli::parse();

    // 所有 CLI 调用统一进入审计流水（人类层面的操作）
    openpup::audit::record_invocation(&cli)?;

    openpup::cli::commands::dispatch(&cli)
}



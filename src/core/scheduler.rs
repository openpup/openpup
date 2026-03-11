//! 轻量调度：按配置时间执行 work_morning / invest_morning / invest_close，写 runtime_audit。
//! 归属 core 层，通过 core::runtime::handle_event 驱动 Loop。

use anyhow::Result;
use std::collections::HashSet;
use std::time::Duration;

use crate::config::{self, ScheduleJob};
use crate::core::runtime::RuntimeEvent;

/// 调度循环：每分钟检查一次，到点执行对应 Loop。时间使用 UTC。
pub async fn run_scheduler_loop() -> Result<()> {
    let cfg = config::load_or_init()?;
    let jobs: Vec<ScheduleJob> = cfg
        .schedule
        .as_ref()
        .map(|s| s.jobs.clone())
        .unwrap_or_else(config::default_schedule_jobs);

    let mut last_tick: HashSet<(u8, u8, String)> = HashSet::new();
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    interval.tick().await;

    loop {
        interval.tick().await;
        let now = time::OffsetDateTime::now_utc();
        let hour = now.hour();
        let minute = now.minute();

        for job in &jobs {
                let key = (job.hour, job.minute, job.loop_id.clone());
                if job.hour == hour && job.minute == minute && !last_tick.contains(&key) {
                    last_tick.insert(key);
                    run_loop_by_id(&job.loop_id).await?;
            }
        }
        // 每分钟清掉已执行过的 (hour, minute, id)，避免下一小时同分钟重复
        last_tick.retain(|(h, m, _)| *h == hour && *m == minute);
    }
}

async fn run_loop_by_id(loop_id: &str) -> Result<()> {
    let ev = RuntimeEvent::time(loop_id);
    super::runtime::handle_event(&ev).await
}

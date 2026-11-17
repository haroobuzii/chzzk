//! 상태 감시기: MediaMTX의 "지금 영상이 들어오는 경로" 목록과 DB의 방송 상태를 맞춘다.
//!
//! 서버가 켜지면 바로 한 번, 그 뒤로 2초마다 맞춘다.
//! 한 번 맞추는 동작(`sync_once`)은 지금 시각을 받으므로, 테스트가 시간 흐름을 흉내 낼 수 있다.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::{mediamtx::MediaMtx, store};

/// 맞추는 간격.
pub const INTERVAL: Duration = Duration::from_secs(2);

/// 준비로 이만큼 지나도록 영상이 없으면 버린다.
pub const READY_TIMEOUT: chrono::TimeDelta = chrono::TimeDelta::seconds(30);

/// 한 번 맞춘다.
///
/// - 준비인 방송의 경로에 영상이 들어오면 방송 중으로 바꾸고 시작 시각을 `now`로 남긴다.
/// - 방송 중인 방송의 경로가 사라지면 종료로 바꾸고 종료 시각을 `now`로 남긴다.
/// - 준비로 30초가 지나도록 영상이 없으면 버린다.
///
/// 모두 조건부 전환이라 종료·버려진 방송은 경로가 다시 나타나도 되돌아가지 않는다.
///
/// MediaMTX에 닿지 못하면 아무것도 바꾸지 않고 에러를 돌려준다.
/// 경로 목록을 모르는 채로 상태를 바꾸면 멀쩡한 방송을 잘못 끝낼 수 있기 때문이다.
pub async fn sync_once(
    pool: &PgPool,
    mediamtx: &MediaMtx,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let online = mediamtx.online_paths().await?;
    let online_ids: Vec<i64> = online.iter().filter_map(|p| mediamtx.id_of(p)).collect();
    store::mark_live(pool, &online_ids, now).await?;
    store::end_missing(pool, &online_ids, now).await?;
    store::abandon_stale(pool, now - READY_TIMEOUT, now).await?;
    Ok(())
}

/// 상태 감시기를 띄운다. 첫 맞추기는 바로 하고, 그 뒤로 2초마다 한다.
pub fn spawn(pool: PgPool, mediamtx: MediaMtx) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(INTERVAL);
        // 맞추기가 늦어져도 밀린 횟수를 몰아서 돌지 않는다.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            if let Err(e) = sync_once(&pool, &mediamtx, Utc::now()).await {
                dioxus::logger::tracing::warn!("상태 감시 실패(다음 차례에 다시 시도): {e:#}");
            }
        }
    });
}

//! 만료 정리: 결제 대기로 30분이 지난 주문서를 만료로 바꾼다(ADR 0007).
//!
//! 서버가 켜지면 바로 한 번, 그 뒤로 1분마다 정리한다.
//! 한 번 정리하는 동작(`expire_once`)은 지금 시각을 받으므로, 테스트가 시간 흐름을 흉내 낼 수 있다.

use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use sqlx::PgPool;

use super::store::{self, OrderStatus};

/// 정리 간격.
pub const INTERVAL: Duration = Duration::from_secs(60);

/// 결제 대기로 이만큼 지나면 만료다. 토스 결제창이 열려 있을 수 있는 시간(30분)과 같다.
pub const PENDING_TIMEOUT: TimeDelta = TimeDelta::minutes(30);

/// `now` 기준으로 만료가 되는 기준 시각. 이보다 먼저 만든 결제 대기 주문서가 만료다.
pub fn cutoff(now: DateTime<Utc>) -> DateTime<Utc> {
    now - PENDING_TIMEOUT
}

/// 한 번 정리한다. 결제 대기로 30분이 지난 주문서만 만료로 바꾸고, 바꾼 개수를 돌려준다.
/// 조건부 전환이라 확정·실패·취소된 주문서는 바뀌지 않는다.
pub async fn expire_once(pool: &PgPool, now: DateTime<Utc>) -> sqlx::Result<u64> {
    let mut expired = 0;
    for order_id in store::pending_created_before(pool, cutoff(now)).await? {
        let changed = store::mark_ended(
            pool,
            &order_id,
            OrderStatus::Expired,
            store::EXPIRED_CODE,
            store::EXPIRED_MESSAGE,
        )
        .await?;
        expired += u64::from(changed);
    }
    Ok(expired)
}

/// 만료 정리를 띄운다. 첫 정리는 바로 하고, 그 뒤로 1분마다 한다.
pub fn spawn(pool: PgPool) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(INTERVAL);
        // 정리가 늦어져도 밀린 횟수를 몰아서 돌지 않는다.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            match expire_once(&pool, Utc::now()).await {
                Ok(0) => {}
                Ok(count) => log::info!("결제 대기 주문서 {count}개를 만료로 바꿨습니다."),
                Err(e) => log::warn!("만료 정리 실패(다음 차례에 다시 시도): {e}"),
            }
        }
    });
}

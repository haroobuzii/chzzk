//! 여러 테스트가 함께 쓰는 도우미.

use chrono::{DateTime, TimeDelta, TimeZone, Utc};

/// 테스트 기준 시각(2026-11-15 12:00:00 UTC)에서 `second`초 뒤.
pub fn at(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, 0).unwrap() + TimeDelta::seconds(second.into())
}

/// 녹화 방송 한 줄을 정한 등록 시각으로 DB에 직접 넣고 방송 번호를 돌려준다.
#[cfg(feature = "server")]
pub async fn insert_recorded(pool: &sqlx::PgPool, title: &str, created_at: DateTime<Utc>) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO broadcasts (title, video_key, created_at) VALUES ($1, 'videos/x.mp4', $2) RETURNING id",
    )
    .bind(title)
    .bind(created_at)
    .fetch_one(pool)
    .await
    .unwrap()
}

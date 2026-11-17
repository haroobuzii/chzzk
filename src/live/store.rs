//! 라이브 방송 저장소: 시작, 소유자 확인에 쓸 정보 조회, 방송 상태 바꾸기.
//!
//! 방송 상태는 "지금 상태가 X일 때만 Y로" 바꾸는 조건부 UPDATE 하나로 바꾼다.
//! 상태와 그 시각은 같은 UPDATE에서 함께 정한다(DB의 CHECK 제약이 그렇게 요구한다).

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::LiveStatus;

/// 소유자를 확인하고 송출을 받을지 정하는 데 필요한 정보.
#[derive(Debug, sqlx::FromRow)]
pub struct LiveAccess {
    pub status: LiveStatus,
    /// 소유자의 사용자 번호. 2단계에 만든 방송은 소유자가 없다.
    pub owner_id: Option<i64>,
}

/// `owner_id`(사용자 번호)가 소유자인 라이브 방송을 준비 상태로 만들고 방송 번호를 돌려준다.
pub async fn create(pool: &PgPool, owner_id: i64, title: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO broadcasts (title, kind, status, owner_id)
         VALUES ($1, 'live', 'ready', $2) RETURNING id",
    )
    .bind(title)
    .bind(owner_id)
    .fetch_one(pool)
    .await
}

/// 방송 번호로 라이브 방송의 상태와 소유자를 찾는다. 없거나 녹화 방송이면 `None`.
pub async fn find_access(pool: &PgPool, id: i64) -> Result<Option<LiveAccess>, sqlx::Error> {
    sqlx::query_as("SELECT status, owner_id FROM broadcasts WHERE id = $1 AND kind = 'live'")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// 준비 → 방송 중: 영상이 들어온 방송들의 상태를 바꾸고 시작 시각을 `now`로 남긴다.
///
/// 준비가 아닌 방송(이미 방송 중, 종료, 버려짐)은 건드리지 않는다. 바뀐 방송 수를 돌려준다.
pub async fn mark_live(pool: &PgPool, ids: &[i64], now: DateTime<Utc>) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE broadcasts SET status = 'live', started_at = $2
         WHERE id = ANY($1) AND kind = 'live' AND status = 'ready'",
    )
    .bind(ids)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// 방송 중 → 종료: 영상이 끊긴(경로가 사라진) 방송들의 상태를 바꾸고 종료 시각을 `now`로 남긴다.
///
/// `online_ids`에 없는 방송 중인 방송이 모두 대상이다. 바뀐 방송 수를 돌려준다.
pub async fn end_missing(
    pool: &PgPool,
    online_ids: &[i64],
    now: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE broadcasts SET status = 'ended', ended_at = $2
         WHERE kind = 'live' AND status = 'live' AND NOT (id = ANY($1))",
    )
    .bind(online_ids)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// 준비 → 버려짐: `ready_since` 이전부터 준비였던(그 뒤로 영상이 들어오지 않은) 방송들을 버리고
/// 종료 시각을 `now`로 남긴다. 준비 시작 시각은 방송을 만든 시각이다. 바뀐 방송 수를 돌려준다.
pub async fn abandon_stale(
    pool: &PgPool,
    ready_since: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE broadcasts SET status = 'abandoned', ended_at = $2
         WHERE kind = 'live' AND status = 'ready' AND created_at <= $1",
    )
    .bind(ready_since)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// 소유자의 종료 요청: 방송 중 → 종료, 준비 → 버려짐으로 바꾸고 종료 시각을 `now`로 남긴다.
///
/// 이미 종료됐거나 버려진 방송은 그대로 둔다.
pub async fn end(pool: &PgPool, id: i64, now: DateTime<Utc>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE broadcasts
         SET status = CASE status WHEN 'live' THEN 'ended'::live_status ELSE 'abandoned'::live_status END,
             ended_at = $2
         WHERE id = $1 AND kind = 'live' AND status IN ('live', 'ready')",
    )
    .bind(id)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

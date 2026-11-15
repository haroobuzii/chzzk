//! 라이브 방송 저장소: 시작, 송출 키 확인에 쓸 정보 조회, 방송 상태 바꾸기.
//!
//! 방송 상태는 "지금 상태가 X일 때만 Y로" 바꾸는 조건부 UPDATE 하나로 바꾼다.
//! 상태와 그 시각은 같은 UPDATE에서 함께 정한다(DB의 CHECK 제약이 그렇게 요구한다).

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::LiveStatus;

/// 송출 키를 확인하고 송출을 받을지 정하는 데 필요한 정보.
#[derive(Debug, sqlx::FromRow)]
pub struct LiveAccess {
    pub status: LiveStatus,
    pub stream_key_hash: Vec<u8>,
}

/// 라이브 방송을 준비 상태로 만들고 방송 번호를 돌려준다.
pub async fn create(pool: &PgPool, title: &str, key_hash: &[u8]) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO broadcasts (title, kind, status, stream_key_hash)
         VALUES ($1, 'live', 'ready', $2) RETURNING id",
    )
    .bind(title)
    .bind(key_hash)
    .fetch_one(pool)
    .await
}

/// 방송 번호로 라이브 방송의 상태와 송출 키 해시를 찾는다. 없거나 녹화 방송이면 `None`.
pub async fn find_access(pool: &PgPool, id: i64) -> Result<Option<LiveAccess>, sqlx::Error> {
    sqlx::query_as("SELECT status, stream_key_hash FROM broadcasts WHERE id = $1 AND kind = 'live'")
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

//! 방송 관리 저장소: 권한 확인에 쓸 정보 조회, 제목 바꾸기, 방송 지우기.

use sqlx::PgPool;

use crate::{db::VISIBLE, LiveStatus};

/// 방송을 관리할 때 필요한 정보.
#[derive(Debug, sqlx::FromRow)]
pub struct Managed {
    /// 소유자의 사용자 번호. 1·2단계에서 만든 방송은 없다.
    pub owner_id: Option<i64>,
    /// 방송 상태. 라이브 방송만 가진다.
    pub status: Option<LiveStatus>,
}

/// 화면에 보이는 방송 하나의 관리 정보. 없거나 준비·버려짐이면 `None`.
pub async fn find(pool: &PgPool, id: i64) -> Result<Option<Managed>, sqlx::Error> {
    let sql = format!("SELECT owner_id, status FROM broadcasts WHERE id = $1 AND {VISIBLE}");
    sqlx::query_as(&sql).bind(id).fetch_optional(pool).await
}

/// 화면에 보이는 방송의 제목을 바꾼다. 바꿀 방송이 없었으면 `false`.
pub async fn rename(pool: &PgPool, id: i64, title: &str) -> Result<bool, sqlx::Error> {
    let sql = format!("UPDATE broadcasts SET title = $2 WHERE id = $1 AND {VISIBLE}");
    let result = sqlx::query(&sql).bind(id).bind(title).execute(pool).await?;
    Ok(result.rows_affected() == 1)
}

/// 방송 행을 지우고 영상 키(녹화 방송만 있다)를 돌려준다. 지울 방송이 없었으면 `None`.
/// 댓글은 DB가 연쇄로 지운다.
pub async fn delete(pool: &PgPool, id: i64) -> Result<Option<Option<String>>, sqlx::Error> {
    sqlx::query_scalar("DELETE FROM broadcasts WHERE id = $1 RETURNING video_key")
        .bind(id)
        .fetch_optional(pool)
        .await
}

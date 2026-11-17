//! 댓글을 DB에 쓰고 읽는다.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::db::VISIBLE;

/// DB에서 읽은 댓글 한 줄.
#[derive(sqlx::FromRow)]
pub struct Row {
    pub id: i64,
    pub author_id: i64,
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

/// 화면에 보이는 방송인지 본다. 없거나 준비·버려짐 라이브 방송이면 `false`.
pub async fn broadcast_exists(pool: &PgPool, broadcast_id: i64) -> sqlx::Result<bool> {
    let sql = format!("SELECT EXISTS (SELECT 1 FROM broadcasts WHERE id = $1 AND {VISIBLE})");
    sqlx::query_scalar(&sql)
        .bind(broadcast_id)
        .fetch_one(pool)
        .await
}

/// 방송의 댓글을 오래된 순으로 읽는다. 같은 시각이면 먼저 쓴(번호가 작은) 댓글이 앞이다.
pub async fn list(pool: &PgPool, broadcast_id: i64) -> sqlx::Result<Vec<Row>> {
    sqlx::query_as(
        "SELECT c.id, c.author_id, u.username AS author, c.body, c.created_at
         FROM comments c JOIN users u ON u.id = c.author_id
         WHERE c.broadcast_id = $1
         ORDER BY c.created_at, c.id",
    )
    .bind(broadcast_id)
    .fetch_all(pool)
    .await
}

/// 댓글을 쓴다. 방송이 없거나 보이지 않으면 `None`.
pub async fn insert(
    pool: &PgPool,
    broadcast_id: i64,
    author_id: i64,
    body: &str,
) -> sqlx::Result<Option<Row>> {
    let sql = format!(
        "WITH inserted AS (
             INSERT INTO comments (broadcast_id, author_id, body)
             SELECT id, $2, $3 FROM broadcasts WHERE id = $1 AND {VISIBLE}
             RETURNING id, author_id, body, created_at
         )
         SELECT i.id, i.author_id, u.username AS author, i.body, i.created_at
         FROM inserted i JOIN users u ON u.id = i.author_id"
    );
    sqlx::query_as(&sql)
        .bind(broadcast_id)
        .bind(author_id)
        .bind(body)
        .fetch_optional(pool)
        .await
}

/// 댓글의 작성자 번호. 없으면 `None`.
pub async fn author_of(pool: &PgPool, comment_id: i64) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar("SELECT author_id FROM comments WHERE id = $1")
        .bind(comment_id)
        .fetch_optional(pool)
        .await
}

/// 댓글을 지운다.
pub async fn delete(pool: &PgPool, comment_id: i64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM comments WHERE id = $1")
        .bind(comment_id)
        .execute(pool)
        .await?;
    Ok(())
}

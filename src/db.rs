//! 방송 정보를 DB에 저장하고 꺼내는 코드(방송 저장소). 서버에서만 쓴다.
//!
//! 1단계에는 인증이 없으므로 수정·삭제 함수는 일부러 두지 않는다.

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};

use crate::Broadcast;

/// DB에 접속하고, 아직 적용하지 않은 마이그레이션(표 만들기 등)을 적용한다.
pub async fn connect(
    database_url: &str,
) -> Result<PgPool, Box<dyn std::error::Error + Send + Sync>> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

/// 등록된 방송 목록. 최근에 등록한 방송이 먼저 온다.
pub async fn list_broadcasts(pool: &PgPool) -> Result<Vec<Broadcast>, sqlx::Error> {
    // 같은 시각에 등록된 방송은 번호가 큰(나중에 등록한) 쪽을 먼저 둔다.
    sqlx::query_as("SELECT id, title, created_at FROM broadcasts ORDER BY created_at DESC, id DESC")
        .fetch_all(pool)
        .await
}

/// 방송 정보를 넣고 새 방송 번호를 받는다. 영상 키는 번호로 지으므로 같은 트랜잭션에서 이어서 정한다.
///
/// 등록 처리가 트랜잭션 안에서 부른다. 영상 파일을 옮기기 전에 실패하면 트랜잭션이 되돌려진다.
pub async fn insert_broadcast(conn: &mut PgConnection, title: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("INSERT INTO broadcasts (title, video_key) VALUES ($1, '') RETURNING id")
        .bind(title)
        .fetch_one(conn)
        .await
}

/// 방금 넣은 방송의 영상 키를 정한다.
pub async fn set_video_key(conn: &mut PgConnection, id: i64, key: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE broadcasts SET video_key = $1 WHERE id = $2")
        .bind(key)
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}

/// 방송 번호로 방송 하나를 찾는다. 없으면 `None`.
pub async fn find_broadcast(pool: &PgPool, id: i64) -> Result<Option<Broadcast>, sqlx::Error> {
    sqlx::query_as("SELECT id, title, created_at FROM broadcasts WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// 방송 번호로 영상 키(보관 폴더 기준 상대 위치)를 찾는다. 없으면 `None`.
pub async fn find_video_key(pool: &PgPool, id: i64) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT video_key FROM broadcasts WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// 이 테스트들은 실제 PostgreSQL이 필요하다.
/// `docker compose up -d`로 DB를 띄우고 `DATABASE_URL`을 설정한 뒤 실행한다.
/// `#[sqlx::test]`는 테스트마다 빈 임시 DB를 만들고 마이그레이션을 적용한 뒤, 끝나면 지운다.
#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test]
    async fn 방송이_없으면_빈_목록이다(pool: PgPool) {
        assert_eq!(list_broadcasts(&pool).await.unwrap(), vec![]);
    }
}

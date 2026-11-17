//! 방송 정보를 DB에 저장하고 꺼내는 코드(방송 저장소). 서버에서만 쓴다.
//!
//! 제목 수정과 삭제는 소유자 확인이 필요하므로 방송 관리 모듈(`manage`)에 둔다.

use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::ConnectOptions;

use crate::Broadcast;

/// DB에 접속하고, 아직 적용하지 않은 마이그레이션(표 만들기 등)을 적용한다.
///
/// 실행한 SQL 문장은 trace로만 남긴다(개발 서버 로그는 debug까지 보인다). 상태 감시기가 2초마다 묻는 문장이 서버 로그를 덮지 않게 하기 위해서다.
/// 느린 문장 경고와 실패는 그대로 보인다.
pub async fn connect(
    database_url: &str,
) -> Result<PgPool, Box<dyn std::error::Error + Send + Sync>> {
    let options = PgConnectOptions::from_str(database_url)?.log_statements(log::LevelFilter::Trace);
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect_with(options)
        .await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}

/// 조회 결과에 담는 칸. 소유자는 사용자 번호 대신 아이디로 담는다.
const COLUMNS: &str = "b.id, b.title, b.created_at, b.kind, b.status, b.started_at, b.ended_at, \
                       owner.username AS owner";

/// 조회할 표: 방송과, 소유자가 있으면 그 사용자.
const FROM: &str = "broadcasts b LEFT JOIN users owner ON owner.id = b.owner_id";

/// 화면에 보이는 방송만 고르는 조건. 준비·버려짐 라이브 방송은 없는 방송으로 취급한다.
/// 댓글 저장소도 `broadcasts` 표만 놓고 쓰므로 표 별칭 없이 쓴다. 사용자 표에는 `status` 칸이 없어 함께 조회해도 헷갈리지 않는다.
pub(crate) const VISIBLE: &str = "(status IS NULL OR status IN ('live', 'ended'))";

/// 화면에 보이는 방송 목록. 최신 방송이 먼저 온다.
/// 녹화 방송은 등록 시각, 라이브 방송은 시작 시각을 기준으로 한다.
pub async fn list_broadcasts(pool: &PgPool) -> Result<Vec<Broadcast>, sqlx::Error> {
    // 같은 시각의 방송은 번호가 큰(나중에 만든) 쪽을 먼저 둔다.
    let sql = format!(
        "SELECT {COLUMNS} FROM {FROM} WHERE {VISIBLE} \
         ORDER BY COALESCE(b.started_at, b.created_at) DESC, b.id DESC"
    );
    sqlx::query_as(&sql).fetch_all(pool).await
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

/// 방금 넣은 방송의 소유자(사용자 번호)를 정한다. 등록 처리가 같은 트랜잭션에서 부른다.
pub async fn set_owner(conn: &mut PgConnection, id: i64, owner_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE broadcasts SET owner_id = $1 WHERE id = $2")
        .bind(owner_id)
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
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

/// 방송 번호로 화면에 보이는 방송 하나를 찾는다. 없거나 준비·버려짐이면 `None`.
pub async fn find_broadcast(pool: &PgPool, id: i64) -> Result<Option<Broadcast>, sqlx::Error> {
    let sql = format!("SELECT {COLUMNS} FROM {FROM} WHERE b.id = $1 AND {VISIBLE}");
    sqlx::query_as(&sql).bind(id).fetch_optional(pool).await
}

/// 방송 번호로 녹화 방송의 영상 키(보관 폴더 기준 상대 위치)를 찾는다.
/// 없거나 라이브 방송이면 `None`.
pub async fn find_video_key(pool: &PgPool, id: i64) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT video_key FROM broadcasts WHERE id = $1 AND video_key IS NOT NULL")
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

    use crate::test_support::{at, insert_recorded};
    use crate::{BroadcastKind, LiveStatus};
    use chrono::{DateTime, Utc};

    #[sqlx::test]
    async fn 방송이_없으면_빈_목록이다(pool: PgPool) {
        assert_eq!(list_broadcasts(&pool).await.unwrap(), vec![]);
    }

    /// 라이브 방송 한 줄을 DB에 직접 넣는다. 상태와 시각을 마음대로 정하려고(제약 조건 검사 포함)
    /// 라이브 방송 저장소(`live::store`)를 거치지 않는다.
    async fn insert_live(
        pool: &PgPool,
        status: &str,
        started_at: Option<DateTime<Utc>>,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar(
            "INSERT INTO broadcasts (title, kind, status, started_at, ended_at)
             VALUES ('라이브', 'live', $1::live_status, $2, $3) RETURNING id",
        )
        .bind(status)
        .bind(started_at)
        .bind(ended_at)
        .fetch_one(pool)
        .await
    }

    #[sqlx::test]
    async fn 기존_방식으로_넣은_방송은_녹화_방송이다(pool: PgPool) {
        let mut conn = pool.acquire().await.unwrap();
        let id = insert_broadcast(&mut conn, "옛 방송").await.unwrap();
        set_video_key(&mut conn, id, "videos/1.mp4").await.unwrap();

        let found = find_broadcast(&pool, id).await.unwrap().unwrap();
        assert_eq!(found.kind, BroadcastKind::Recorded);
        assert_eq!(found.status, None);
        assert_eq!(found.started_at, None);
        assert_eq!(found.ended_at, None);
    }

    #[sqlx::test]
    async fn 라이브_방송은_상태와_시작_종료_시각을_함께_돌려준다(
        pool: PgPool,
    ) {
        let live = insert_live(&pool, "live", Some(at(10)), None)
            .await
            .unwrap();
        let ended = insert_live(&pool, "ended", Some(at(1)), Some(at(5)))
            .await
            .unwrap();

        let found = find_broadcast(&pool, live).await.unwrap().unwrap();
        assert_eq!(found.kind, BroadcastKind::Live);
        assert_eq!(found.status, Some(LiveStatus::Live));
        assert_eq!(found.started_at, Some(at(10)));
        assert_eq!(found.ended_at, None);

        let found = find_broadcast(&pool, ended).await.unwrap().unwrap();
        assert_eq!(found.status, Some(LiveStatus::Ended));
        assert_eq!(found.started_at, Some(at(1)));
        assert_eq!(found.ended_at, Some(at(5)));
    }

    #[sqlx::test]
    async fn 준비와_버려짐_방송은_목록에_없고_단건_조회는_없음이다(
        pool: PgPool,
    ) {
        let ready = insert_live(&pool, "ready", None, None).await.unwrap();
        let abandoned = insert_live(&pool, "abandoned", None, None).await.unwrap();

        assert_eq!(list_broadcasts(&pool).await.unwrap(), vec![]);
        assert_eq!(find_broadcast(&pool, ready).await.unwrap(), None);
        assert_eq!(find_broadcast(&pool, abandoned).await.unwrap(), None);
    }

    #[sqlx::test]
    async fn 목록은_녹화는_등록_시각_라이브는_시작_시각으로_최신순이다(
        pool: PgPool,
    ) {
        let old_recorded = insert_recorded(&pool, "옛 녹화", at(0)).await;
        let ended = insert_live(&pool, "ended", Some(at(2)), Some(at(3)))
            .await
            .unwrap();
        let new_recorded = insert_recorded(&pool, "새 녹화", at(4)).await;
        let live = insert_live(&pool, "live", Some(at(6)), None).await.unwrap();

        let ids: Vec<i64> = list_broadcasts(&pool)
            .await
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect();
        assert_eq!(ids, vec![live, new_recorded, ended, old_recorded]);
    }

    #[sqlx::test]
    async fn 라이브_방송에_영상_키가_있으면_DB가_거절한다(pool: PgPool) {
        let result = sqlx::query(
            "INSERT INTO broadcasts (title, kind, status, video_key)
             VALUES ('라이브', 'live', 'ready', 'videos/1.mp4')",
        )
        .execute(&pool)
        .await;
        assert!(
            result.is_err(),
            "영상 키가 있는 라이브 방송은 거절돼야 한다"
        );
    }

    #[sqlx::test]
    async fn 녹화_방송에_방송_상태가_있으면_DB가_거절한다(pool: PgPool) {
        let result = sqlx::query(
            "INSERT INTO broadcasts (title, video_key, status) VALUES ('녹화', 'videos/1.mp4', 'live')",
        )
        .execute(&pool)
        .await;
        assert!(
            result.is_err(),
            "방송 상태가 있는 녹화 방송은 거절돼야 한다"
        );
    }

    #[sqlx::test]
    async fn 라이브_방송에_방송_상태가_없으면_DB가_거절한다(pool: PgPool) {
        let no_status =
            sqlx::query("INSERT INTO broadcasts (title, kind) VALUES ('라이브', 'live')")
                .execute(&pool)
                .await;
        assert!(
            no_status.is_err(),
            "방송 상태가 없는 라이브 방송은 거절돼야 한다"
        );
    }

    /// 2단계까지의 마이그레이션(0001, 0002)만 적용한 DB에 그때 방식으로 방송을 넣고,
    /// 나머지 마이그레이션을 적용한 뒤 조회한다. 운영 DB가 3단계로 올라가는 순서 그대로다.
    #[sqlx::test(migrations = false)]
    async fn 마이그레이션_뒤에도_1_2단계_방송은_그대로이고_소유자가_없다(
        pool: PgPool,
    ) {
        let mut stage2 = sqlx::migrate!();
        stage2.migrations = stage2
            .migrations
            .iter()
            .filter(|m| m.version <= 2)
            .cloned()
            .collect();
        stage2.run(&pool).await.unwrap();
        let recorded: i64 = sqlx::query_scalar(
            "INSERT INTO broadcasts (title, video_key) VALUES ('옛 녹화', 'videos/1.mp4') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let live: i64 = sqlx::query_scalar(
            "INSERT INTO broadcasts (title, kind, status, stream_key_hash, started_at, ended_at)
             VALUES ('옛 라이브', 'live', 'ended', sha256('키'::bytea), $1, $2) RETURNING id",
        )
        .bind(at(1))
        .bind(at(2))
        .fetch_one(&pool)
        .await
        .unwrap();

        sqlx::migrate!().run(&pool).await.unwrap();

        let list = list_broadcasts(&pool).await.unwrap();
        let titles: Vec<(i64, &str, Option<&str>)> = list
            .iter()
            .map(|b| (b.id, b.title.as_str(), b.owner.as_deref()))
            .collect();
        assert_eq!(
            titles,
            vec![(recorded, "옛 녹화", None), (live, "옛 라이브", None)]
        );
        let found = find_broadcast(&pool, live).await.unwrap().unwrap();
        assert_eq!(found.owner, None);
        assert_eq!(found.status, Some(LiveStatus::Ended));
    }

    #[sqlx::test]
    async fn 방송_중인데_시작_시각이_없으면_DB가_거절한다(pool: PgPool) {
        assert!(insert_live(&pool, "live", None, None).await.is_err());
        assert!(insert_live(&pool, "ended", Some(at(1)), None)
            .await
            .is_err());
    }
}

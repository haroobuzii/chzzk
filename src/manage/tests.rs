//! 방송 관리 라우터(제목 수정, 삭제) 테스트.
//!
//! 실제 PostgreSQL(`#[sqlx::test]` 임시 DB)과 가짜 MediaMTX를 쓴다. 서버처럼 등록·라이브·댓글·관리 라우터를
//! 한데 붙이고, 요청을 넣은 뒤 응답과 그 뒤의 조회 결과로 판단한다.

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    Router,
};
use sqlx::PgPool;
use tempfile::TempDir;
use tower::ServiceExt;

use super::{api, ManageState};
use crate::auth::personas::Persona;
use crate::live::fake_mediamtx::{FakeMediaMtx, FAKE_SESSION_ID};
use crate::live::LiveState;
use crate::routes::tests::{multipart_body, register_request_as, tiny_mp4};
use crate::routes::AppState;
use crate::test_support::{at, insert_recorded};
use crate::videos::VideoStore;

/// 테스트용 서버: 등록·라이브·댓글·관리 라우터와 DB, 보관 폴더, 가짜 MediaMTX.
struct ManageApp {
    router: Router,
    pool: PgPool,
    data_dir: TempDir,
    fake: FakeMediaMtx,
}

impl ManageApp {
    async fn new(pool: PgPool) -> Self {
        // 소유자 없는 방송 테스트가 쓰는 테스트 사용자([`Persona::tester`])의 세션을 만들어 둔다.
        Persona::ensure_tester(&pool).await;
        let data_dir = tempfile::tempdir().unwrap();
        let fake = FakeMediaMtx::start().await;
        let videos = VideoStore::new(data_dir.path());
        let register = AppState {
            pool: pool.clone(),
            videos: videos.clone(),
            max_upload_bytes: 1024 * 1024,
        };
        let live = LiveState {
            pool: pool.clone(),
            mediamtx: fake.client(),
        };
        let manage = ManageState {
            pool: pool.clone(),
            videos,
            mediamtx: fake.client(),
        };
        let router = crate::routes::api(register)
            .merge(crate::live::api(live))
            .merge(crate::comments::api(pool.clone()))
            .merge(api(manage));
        Self {
            router,
            pool,
            data_dir,
            fake,
        }
    }

    async fn send(&self, request: Request<Body>) -> (StatusCode, serde_json::Value) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    /// `owner`가 녹화 방송을 등록하고 번호를 돌려준다.
    async fn register(&self, owner: &Persona, title: &str) -> i64 {
        let body = multipart_body(Some(title), Some(("clip.mp4", &tiny_mp4())));
        let (status, json) = self.send(register_request_as(owner, body)).await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        json["id"].as_i64().unwrap()
    }

    /// `owner`가 라이브 방송을 시작하고, 영상이 들어온 것처럼 만들어 방송 중으로 바꾼다.
    async fn go_live(&self, owner: &Persona, title: &str) -> i64 {
        let request = Request::post("/api/live")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({ "title": title }).to_string(),
            ))
            .unwrap();
        let (status, json) = self.send(owner.sign(request)).await;
        assert_eq!(status, StatusCode::CREATED, "{json}");
        let id = json["id"].as_i64().unwrap();
        self.fake.set_online(&[&format!("live/{id}")]);
        crate::live::status_sync::sync_once(&self.pool, &self.fake.client(), at(10))
            .await
            .unwrap();
        id
    }

    async fn rename(
        &self,
        persona: &Persona,
        id: &str,
        title: &str,
    ) -> (StatusCode, serde_json::Value) {
        let request = Request::patch(format!("/api/broadcasts/{id}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({ "title": title }).to_string(),
            ))
            .unwrap();
        self.send(persona.sign(request)).await
    }

    async fn delete(&self, persona: &Persona, id: &str) -> (StatusCode, serde_json::Value) {
        let request = Request::delete(format!("/api/broadcasts/{id}"))
            .body(Body::empty())
            .unwrap();
        self.send(persona.sign(request)).await
    }

    /// 상세 조회로 본 제목. 없거나 보이지 않으면 `None`.
    async fn title_of(&self, id: i64) -> Option<String> {
        crate::db::find_broadcast(&self.pool, id)
            .await
            .unwrap()
            .map(|b| b.title)
    }

    /// 목록에 보이는 `id` 방송의 제목.
    async fn listed_title(&self, id: i64) -> Option<String> {
        crate::db::list_broadcasts(&self.pool)
            .await
            .unwrap()
            .into_iter()
            .find(|b| b.id == id)
            .map(|b| b.title)
    }

    async fn video_status(&self, id: i64) -> StatusCode {
        let request = Request::get(format!("/api/broadcasts/{id}/video"))
            .body(Body::empty())
            .unwrap();
        self.send(request).await.0
    }

    /// 보관 폴더의 영상 파일(임시 폴더 제외) 수.
    fn video_files(&self) -> usize {
        std::fs::read_dir(self.data_dir.path().join("videos"))
            .map(|entries| entries.count())
            .unwrap_or(0)
    }
}

// ── 제목 수정 ──

#[sqlx::test]
async fn 소유자가_제목을_고치면_200이고_목록과_상세에_새_제목이_보인다(
    pool: PgPool,
) {
    let app = ManageApp::new(pool.clone()).await;
    let owner = Persona::user_a(&pool).await;
    let id = app.register(&owner, "옛 제목").await;

    let (status, json) = app.rename(&owner, &id.to_string(), "  새 제목  ").await;

    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(app.title_of(id).await.as_deref(), Some("새 제목"));
    assert_eq!(app.listed_title(id).await.as_deref(), Some("새 제목"));
}

#[sqlx::test]
async fn 제목_규칙을_어기면_400이고_제목이_그대로다(pool: PgPool) {
    let app = ManageApp::new(pool.clone()).await;
    let owner = Persona::user_a(&pool).await;
    let id = app.register(&owner, "그대로").await;

    for (title, code) in [("", "title_empty"), ("   ", "title_empty")] {
        let (status, json) = app.rename(&owner, &id.to_string(), title).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
        assert_eq!(json["code"], code);
    }
    let long = "가".repeat(101);
    let (status, json) = app.rename(&owner, &id.to_string(), &long).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["code"], "title_too_long");

    assert_eq!(app.title_of(id).await.as_deref(), Some("그대로"));
}

#[sqlx::test]
async fn 비로그인은_401_다른_사용자는_403이고_제목이_그대로다(pool: PgPool) {
    let app = ManageApp::new(pool.clone()).await;
    let owner = Persona::user_a(&pool).await;
    let other = Persona::user_b(&pool).await;
    let id = app.register(&owner, "내 방송").await;

    let (status, json) = app
        .rename(&Persona::anonymous(), &id.to_string(), "익명이 고침")
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["code"], "login_required");

    let (status, json) = app.rename(&other, &id.to_string(), "남이 고침").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["code"], "forbidden");

    assert_eq!(app.title_of(id).await.as_deref(), Some("내 방송"));
}

#[sqlx::test]
async fn 소유자_없는_방송의_제목은_누구도_고칠_수_없다(pool: PgPool) {
    let app = ManageApp::new(pool.clone()).await;
    // 1·2단계의 소유자 없는 방송은 지금 API로 만들 수 없어 DB에 직접 넣는다.
    let id = insert_recorded(&pool, "예전 방송", at(0)).await;

    for persona in [Persona::user_a(&pool).await, Persona::tester()] {
        let (status, _) = app.rename(&persona, &id.to_string(), "고침").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    assert_eq!(app.title_of(id).await.as_deref(), Some("예전 방송"));
}

#[sqlx::test]
async fn 없거나_번호가_아닌_방송의_제목_수정은_404다(pool: PgPool) {
    let app = ManageApp::new(pool.clone()).await;
    let owner = Persona::user_a(&pool).await;

    for segment in ["999", "0", "abc"] {
        let (status, _) = app.rename(&owner, segment, "고침").await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{segment}");
    }
}

#[sqlx::test]
async fn 방송_중인_라이브의_제목도_고칠_수_있다(pool: PgPool) {
    let app = ManageApp::new(pool.clone()).await;
    let owner = Persona::user_a(&pool).await;
    let id = app.go_live(&owner, "라이브 옛 제목").await;

    let (status, json) = app.rename(&owner, &id.to_string(), "라이브 새 제목").await;

    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(app.title_of(id).await.as_deref(), Some("라이브 새 제목"));
}

// ── 삭제 ──

#[sqlx::test]
async fn 소유자가_녹화_방송을_지우면_204이고_방송과_영상이_사라진다(
    pool: PgPool,
) {
    let app = ManageApp::new(pool.clone()).await;
    let owner = Persona::user_a(&pool).await;
    let id = app.register(&owner, "지울 방송").await;
    assert_eq!(app.video_files(), 1);

    let (status, json) = app.delete(&owner, &id.to_string()).await;

    assert_eq!(status, StatusCode::NO_CONTENT, "{json}");
    assert_eq!(app.title_of(id).await, None);
    assert_eq!(app.listed_title(id).await, None);
    assert_eq!(app.video_status(id).await, StatusCode::NOT_FOUND);
    assert_eq!(app.video_files(), 0, "영상 파일도 지워져야 한다");
    // 이미 지운 방송을 다시 지우거나 고치면 없는 방송이다.
    assert_eq!(
        app.delete(&owner, &id.to_string()).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.rename(&owner, &id.to_string(), "고침").await.0,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn 거절된_삭제는_방송과_영상을_그대로_둔다(pool: PgPool) {
    let app = ManageApp::new(pool.clone()).await;
    let owner = Persona::user_a(&pool).await;
    let other = Persona::user_b(&pool).await;
    let id = app.register(&owner, "남길 방송").await;

    let (status, json) = app.delete(&Persona::anonymous(), &id.to_string()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["code"], "login_required");
    let (status, json) = app.delete(&other, &id.to_string()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(json["code"], "forbidden");

    assert_eq!(app.title_of(id).await.as_deref(), Some("남길 방송"));
    assert_eq!(app.video_status(id).await, StatusCode::OK);
    assert_eq!(app.video_files(), 1);
}

#[sqlx::test]
async fn 소유자_없는_방송은_누구도_지울_수_없다(pool: PgPool) {
    let app = ManageApp::new(pool.clone()).await;
    // 1·2단계의 소유자 없는 방송은 지금 API로 만들 수 없어 DB에 직접 넣는다.
    let id = insert_recorded(&pool, "예전 방송", at(0)).await;

    for persona in [Persona::user_a(&pool).await, Persona::tester()] {
        let (status, _) = app.delete(&persona, &id.to_string()).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    assert_eq!(app.title_of(id).await.as_deref(), Some("예전 방송"));
}

#[sqlx::test]
async fn 방송_중인_라이브를_지우면_송출을_끊고_방송이_사라진다(
    pool: PgPool,
) {
    let app = ManageApp::new(pool.clone()).await;
    let owner = Persona::user_a(&pool).await;
    let id = app.go_live(&owner, "지울 라이브").await;

    let (status, json) = app.delete(&owner, &id.to_string()).await;

    assert_eq!(status, StatusCode::NO_CONTENT, "{json}");
    let kicked = app.fake.requests().into_iter().any(|r| {
        r.method == Method::POST && r.path == format!("/v3/webrtc/sessions/kick/{FAKE_SESSION_ID}")
    });
    assert!(kicked, "송출 세션 강제 종료 요청이 가야 한다");
    assert_eq!(app.title_of(id).await, None);
    assert_eq!(app.listed_title(id).await, None);
}

#[sqlx::test]
async fn MediaMTX에_닿지_못해도_라이브_방송은_지워진다(pool: PgPool) {
    let app = ManageApp::new(pool.clone()).await;
    let owner = Persona::user_a(&pool).await;
    let id = app.go_live(&owner, "지울 라이브").await;
    app.fake.set_unreachable(true);

    let (status, json) = app.delete(&owner, &id.to_string()).await;

    assert_eq!(status, StatusCode::NO_CONTENT, "{json}");
    assert_eq!(app.title_of(id).await, None);
}

#[sqlx::test]
async fn 댓글이_달린_방송도_지워지고_댓글_목록은_404다(pool: PgPool) {
    let app = ManageApp::new(pool.clone()).await;
    let owner = Persona::user_a(&pool).await;
    let other = Persona::user_b(&pool).await;
    let id = app.register(&owner, "댓글 방송").await;
    let request = Request::post(format!("/api/broadcasts/{id}/comments"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({ "body": "안녕하세요" }).to_string(),
        ))
        .unwrap();
    assert_eq!(app.send(other.sign(request)).await.0, StatusCode::CREATED);

    let (status, json) = app.delete(&owner, &id.to_string()).await;

    assert_eq!(status, StatusCode::NO_CONTENT, "{json}");
    let request = Request::get(format!("/api/broadcasts/{id}/comments"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.send(request).await.0, StatusCode::NOT_FOUND);
}

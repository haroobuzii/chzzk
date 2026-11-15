//! 라이브 방송 라우터와 상태 감시기 테스트.
//!
//! 실제 PostgreSQL(`#[sqlx::test]` 임시 DB)과 가짜 MediaMTX를 쓴다.
//! 서버를 띄우지 않고 라우터에 요청을 직접 넣어 응답과 그 뒤의 조회 결과를 본다.
//! 03·04·05 티켓의 테스트도 이 파일의 `LiveApp`과 요청 도우미를 쓴다.
#![allow(non_snake_case)]

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    response::Response,
    Router,
};
use chrono::{DateTime, TimeZone, Utc};
use sqlx::PgPool;
use tower::ServiceExt;

use super::fake_mediamtx::{FakeMediaMtx, FAKE_ANSWER, FAKE_SECRET};
use super::{api, LiveState};
use crate::{BroadcastKind, LiveStatus};

/// 테스트용 라이브 서버: 라우터와 DB, 가짜 MediaMTX.
pub struct LiveApp {
    pub router: Router,
    pub pool: PgPool,
    pub fake: FakeMediaMtx,
}

impl LiveApp {
    pub async fn new(pool: PgPool) -> Self {
        let fake = FakeMediaMtx::start().await;
        let state = LiveState {
            pool: pool.clone(),
            mediamtx: fake.client(),
        };
        Self {
            router: api(state),
            pool,
            fake,
        }
    }

    pub async fn request(&self, request: Request<Body>) -> Response {
        self.router.clone().oneshot(request).await.unwrap()
    }

    /// 라이브 방송을 시작하고 `(번호, 송출 키)`를 돌려준다.
    pub async fn start_live(&self, title: &str) -> (i64, String) {
        let response = self.request(start_request(title)).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let json = body_json(response).await;
        (
            json["id"].as_i64().expect("응답에 방송 번호가 있어야 한다"),
            json["key"]
                .as_str()
                .expect("응답에 송출 키가 있어야 한다")
                .to_string(),
        )
    }

    /// 상태 감시기를 한 번 돌린다.
    pub async fn sync(&self, now: DateTime<Utc>) {
        super::watcher::sync_once(&self.pool, &self.fake.client(), now)
            .await
            .unwrap();
    }

    /// 방송을 시작하고 영상이 들어온 것처럼 만들어 방송 중으로 바꾼다.
    pub async fn go_live(&self, title: &str, now: DateTime<Utc>) -> (i64, String) {
        let (id, key) = self.start_live(title).await;
        self.fake.set_online(&[&format!("live/{id}")]);
        self.sync(now).await;
        (id, key)
    }
}

pub fn start_request(title: &str) -> Request<Body> {
    Request::post("/api/live")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({ "title": title }).to_string(),
        ))
        .unwrap()
}

/// 송출 연결 요청. `key`가 `None`이면 Authorization 헤더를 넣지 않는다.
pub fn whip_request(id: i64, key: Option<&str>) -> Request<Body> {
    let mut builder = Request::post(format!("/api/live/{id}/whip"))
        .header(header::CONTENT_TYPE, "application/sdp");
    if let Some(key) = key {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    builder.body(Body::from(OFFER)).unwrap()
}

/// 송출 세션 요청(PATCH·DELETE).
pub fn session_request(method: Method, path: &str, key: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method(method.clone()).uri(path);
    if method == Method::PATCH {
        builder = builder.header(header::CONTENT_TYPE, "application/trickle-ice-sdpfrag");
    }
    if let Some(key) = key {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    let body = if method == Method::PATCH {
        "a=candidate:1 1 udp 1 192.0.2.1 5000 typ host\r\n"
    } else {
        ""
    };
    builder.body(Body::from(body)).unwrap()
}

pub const OFFER: &str = "v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\ns=test-offer\r\n";

pub async fn body_text(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

pub async fn body_json(response: Response) -> serde_json::Value {
    serde_json::from_str(&body_text(response).await).unwrap_or(serde_json::Value::Null)
}

pub fn at(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 15, 12, 0, second).unwrap()
}

// ── 라이브 시작 ──

#[sqlx::test]
async fn 라이브를_시작하면_201과_번호_송출_키가_오고_아직_목록과_조회에_없다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;

    let (id, key) = app.start_live("첫 라이브").await;

    assert!(!key.is_empty());
    assert_eq!(crate::db::list_broadcasts(&app.pool).await.unwrap(), vec![]);
    assert_eq!(
        crate::db::find_broadcast(&app.pool, id).await.unwrap(),
        None
    );
}

#[sqlx::test]
async fn DB에는_송출_키_원문이_남지_않는다(pool: PgPool) {
    let app = LiveApp::new(pool).await;

    let (id, key) = app.start_live("키 확인").await;

    // 방송 행의 모든 칸을 글자로 바꿔도 원문이 보이지 않아야 한다.
    let row: String = sqlx::query_scalar("SELECT b::text FROM broadcasts b WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(!row.contains(&key), "DB에 송출 키 원문이 있다: {row}");
}

#[sqlx::test]
async fn 송출_키는_방송마다_다르다(pool: PgPool) {
    let app = LiveApp::new(pool).await;

    let (_, a) = app.start_live("하나").await;
    let (_, b) = app.start_live("둘").await;

    assert_ne!(a, b);
}

#[sqlx::test]
async fn 라이브_제목_규칙을_어기면_400이다(pool: PgPool) {
    let app = LiveApp::new(pool).await;

    for (title, code) in [
        ("", "title_empty"),
        ("   ", "title_empty"),
        (&"가".repeat(101), "title_too_long"),
    ] {
        let response = app.request(start_request(title)).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{title:?}");
        assert_eq!(body_json(response).await["code"], code);
    }

    // 제목 칸이 없거나 JSON이 아니어도 400이다.
    let no_title = Request::post("/api/live")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        app.request(no_title).await.status(),
        StatusCode::BAD_REQUEST
    );
    let broken = Request::post("/api/live")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("제목"))
        .unwrap();
    let response = app.request(broken).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(body_json(response).await["message"].is_string());
}

// ── 송출 연결 ──

#[sqlx::test]
async fn 송출_키가_없거나_틀리면_401이고_MediaMTX에_넘기지_않는다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let (id, _key) = app.start_live("키 없음").await;

    let response = app.request(whip_request(id, None)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(body_json(response).await["message"].is_string());

    let response = app.request(whip_request(id, Some("틀린키"))).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // 다른 방송의 키도 틀린 키다.
    let (_, other_key) = app.start_live("다른 방송").await;
    let response = app.request(whip_request(id, Some(&other_key))).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    assert!(app.fake.requests().is_empty());
}

#[sqlx::test]
async fn 맞는_키면_offer가_넘어가고_answer와_우리_서버의_세션_주소가_온다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let (id, key) = app.start_live("송출").await;

    let response = app.request(whip_request(id, Some(&key))).await;

    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/sdp");
    assert_eq!(
        response.headers()[header::LOCATION],
        format!("/api/live/{id}/whip/{FAKE_SECRET}").as_str()
    );
    assert_eq!(body_text(response).await, FAKE_ANSWER);

    let sent = app.fake.webrtc_requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, Method::POST);
    assert_eq!(sent[0].path, format!("/live/{id}/whip"));
    assert_eq!(sent[0].headers[header::CONTENT_TYPE], "application/sdp");
    assert_eq!(sent[0].body, OFFER);
    // 송출 키는 MediaMTX에 넘기지 않는다.
    assert!(sent[0].headers.get(header::AUTHORIZATION).is_none());
}

#[sqlx::test]
async fn 없는_번호로_송출하면_404다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let (_, key) = app.start_live("있는 방송").await;

    assert_eq!(
        app.request(whip_request(9999, Some(&key))).await.status(),
        StatusCode::NOT_FOUND
    );
    let not_number = Request::post("/api/live/abc/whip")
        .header(header::AUTHORIZATION, format!("Bearer {key}"))
        .body(Body::from(OFFER))
        .unwrap();
    assert_eq!(
        app.request(not_number).await.status(),
        StatusCode::NOT_FOUND
    );
    assert!(app.fake.requests().is_empty());
}

#[sqlx::test]
async fn 녹화_방송_번호로_송출하면_404다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let (_, key) = app.start_live("라이브").await;
    let recorded: i64 = sqlx::query_scalar(
        "INSERT INTO broadcasts (title, video_key) VALUES ('녹화', 'videos/x.mp4') RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();

    assert_eq!(
        app.request(whip_request(recorded, Some(&key)))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn 준비가_아닌_방송에_송출하면_409다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let (id, key) = app.go_live("이미 방송 중", at(0)).await;
    let before = app.fake.webrtc_requests().len();

    let response = app.request(whip_request(id, Some(&key))).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(body_json(response).await["message"].is_string());
    assert_eq!(app.fake.webrtc_requests().len(), before);
}

#[sqlx::test]
async fn 세션_PATCH와_DELETE도_송출_키를_확인한다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let (id, key) = app.start_live("세션").await;
    let session = format!("/api/live/{id}/whip/{FAKE_SECRET}");

    for method in [Method::PATCH, Method::DELETE] {
        for wrong in [None, Some("틀린키")] {
            let response = app
                .request(session_request(method.clone(), &session, wrong))
                .await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{method}");
        }
    }
    assert!(app.fake.requests().is_empty());

    let response = app
        .request(session_request(Method::PATCH, &session, Some(&key)))
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = app
        .request(session_request(Method::DELETE, &session, Some(&key)))
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let sent = app.fake.webrtc_requests();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].method, Method::PATCH);
    assert_eq!(sent[0].path, format!("/live/{id}/whip/{FAKE_SECRET}"));
    assert_eq!(
        sent[0].headers[header::CONTENT_TYPE],
        "application/trickle-ice-sdpfrag"
    );
    assert!(sent[0].body.contains("candidate"));
    assert_eq!(sent[1].method, Method::DELETE);
    assert_eq!(sent[1].path, format!("/live/{id}/whip/{FAKE_SECRET}"));
}

#[sqlx::test]
async fn 세션_주소가_이상하면_넘기지_않고_404다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let (id, key) = app.start_live("세션").await;

    let path = format!("/api/live/{id}/whip/..%2F..%2Fv3%2Fconfig");
    let response = app
        .request(session_request(Method::DELETE, &path, Some(&key)))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(app.fake.requests().is_empty());
}

#[sqlx::test]
async fn MediaMTX가_실패하면_그_상태를_알리고_방송은_준비로_남는다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let (id, key) = app.start_live("실패").await;
    app.fake.set_unreachable(true);

    let response = app.request(whip_request(id, Some(&key))).await;

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(body_json(response).await["message"].is_string());
    assert_eq!(
        crate::db::find_broadcast(&app.pool, id).await.unwrap(),
        None
    );
}

// ── 상태 감시기 ──

#[sqlx::test]
async fn 경로에_영상이_들어오면_방송_중이_되고_시작_시각이_생기며_목록에_나타난다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let (id, _) = app.start_live("방송 중으로").await;
    app.fake.set_online(&[&format!("live/{id}")]);

    app.sync(at(7)).await;

    let found = crate::db::find_broadcast(&app.pool, id)
        .await
        .unwrap()
        .expect("방송 중이면 조회된다");
    assert_eq!(found.kind, BroadcastKind::Live);
    assert_eq!(found.status, Some(LiveStatus::Live));
    assert_eq!(found.started_at, Some(at(7)));
    assert_eq!(found.ended_at, None);
    let list = crate::db::list_broadcasts(&app.pool).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
}

#[sqlx::test]
async fn 영상이_들어오지_않은_방송은_준비로_남는다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let (waiting, _) = app.start_live("기다림").await;
    let (online, _) = app.start_live("들어옴").await;
    app.fake.set_online(&[&format!("live/{online}")]);

    app.sync(at(3)).await;

    assert_eq!(
        crate::db::find_broadcast(&app.pool, waiting).await.unwrap(),
        None
    );
    assert!(crate::db::find_broadcast(&app.pool, online)
        .await
        .unwrap()
        .is_some());
}

#[sqlx::test]
async fn 이미_방송_중이면_다시_맞춰도_시작_시각이_바뀌지_않는다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let (id, _) = app.go_live("그대로", at(1)).await;

    app.sync(at(9)).await;

    let found = crate::db::find_broadcast(&app.pool, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.started_at, Some(at(1)));
}

#[sqlx::test]
async fn MediaMTX에_닿지_못하면_맞추기는_실패하고_아무것도_바뀌지_않는다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let (id, _) = app.start_live("그대로").await;
    app.fake.set_online(&[&format!("live/{id}")]);
    app.fake.set_unreachable(true);

    let result = super::watcher::sync_once(&app.pool, &app.fake.client(), at(5)).await;

    assert!(result.is_err());
    assert_eq!(
        crate::db::find_broadcast(&app.pool, id).await.unwrap(),
        None
    );
}

//! 라이브 방송 라우터와 상태 감시기 테스트.
//!
//! 실제 PostgreSQL(`#[sqlx::test]` 임시 DB)과 가짜 MediaMTX를 쓴다.
//! 서버를 띄우지 않고 라우터에 요청을 직접 넣어 응답과 그 뒤의 조회 결과를 본다.
//! 03·04·05 티켓의 테스트도 이 파일의 `LiveApp`과 요청 도우미를 쓴다.

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    response::Response,
    Router,
};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tower::ServiceExt;

use super::fake_mediamtx::{FakeMediaMtx, FAKE_ANSWER, FAKE_SECRET, FAKE_SESSION_ID};
use super::{api, LiveState};
pub use crate::auth::personas::Persona;
pub use crate::test_support::at;
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
        // `request`가 붙이는 테스트 사용자의 세션을 DB에 만들어 둔다.
        Persona::ensure_tester(&pool).await;
        Self {
            router: api(state),
            pool,
            fake,
        }
    }

    /// 요청을 보낸다. 쿠키가 없는 요청에는 로그인한 테스트 사용자([`Persona::tester`])의 쿠키를 붙인다.
    pub async fn request(&self, request: Request<Body>) -> Response {
        if request.headers().contains_key(header::COOKIE) {
            self.send(request).await
        } else {
            self.send(Persona::tester().sign(request)).await
        }
    }

    /// `persona`로 요청을 보낸다. 비로그인이면 쿠키 없이 보낸다.
    pub async fn request_as(&self, persona: &Persona, request: Request<Body>) -> Response {
        self.send(persona.sign(request)).await
    }

    async fn send(&self, request: Request<Body>) -> Response {
        self.router.clone().oneshot(request).await.unwrap()
    }

    /// 테스트 사용자로 라이브 방송을 시작하고 방송 번호를 돌려준다. 그 사용자가 소유자다.
    pub async fn start_live(&self, title: &str) -> i64 {
        let response = self.request(start_request(title)).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        body_json(response).await["id"]
            .as_i64()
            .expect("응답에 방송 번호가 있어야 한다")
    }

    /// 상태 감시기를 한 번 돌린다.
    pub async fn sync(&self, now: DateTime<Utc>) {
        super::status_sync::sync_once(&self.pool, &self.fake.client(), now)
            .await
            .unwrap();
    }

    /// 방송을 시작하고 영상이 들어온 것처럼 만들어 방송 중으로 바꾼다.
    pub async fn go_live(&self, title: &str, now: DateTime<Utc>) -> i64 {
        let id = self.start_live(title).await;
        self.fake.set_online(&[&format!("live/{id}")]);
        self.sync(now).await;
        id
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

/// 송출 연결 요청. 누가 보내는지는 [`LiveApp::request`]·[`LiveApp::request_as`]가 정한다.
pub fn whip_request(id: i64) -> Request<Body> {
    Request::post(format!("/api/live/{id}/whip"))
        .header(header::CONTENT_TYPE, "application/sdp")
        .body(Body::from(OFFER))
        .unwrap()
}

/// 송출 세션 요청(PATCH·DELETE).
pub fn session_request(method: Method, path: &str) -> Request<Body> {
    let mut builder = Request::builder().method(method.clone()).uri(path);
    if method == Method::PATCH {
        builder = builder.header(header::CONTENT_TYPE, "application/trickle-ice-sdpfrag");
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

// ── 라이브 시작 ──

#[sqlx::test]
async fn 라이브를_시작하면_201과_번호가_오고_아직_목록과_조회에_없다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;

    let id = app.start_live("첫 라이브").await;

    assert_eq!(crate::db::list_broadcasts(&app.pool).await.unwrap(), vec![]);
    assert_eq!(
        crate::db::find_broadcast(&app.pool, id).await.unwrap(),
        None
    );
}

#[sqlx::test]
async fn 라이브_시작_응답과_DB에_송출_키가_없다(pool: PgPool) {
    let app = LiveApp::new(pool).await;

    let response = app.request(start_request("키 없음")).await;

    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_json(response).await;
    let fields: Vec<&String> = json.as_object().unwrap().keys().collect();
    assert_eq!(fields, ["id"]);
    // 송출 키 칸이 없어졌는지는 공개 API로 볼 수 없으므로 DB의 칸 목록을 직접 본다(ADR 0005).
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns WHERE table_name = 'broadcasts'",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert!(
        !columns
            .iter()
            .any(|c| c.contains("key") && c != "video_key"),
        "송출 키 칸이 남아 있다: {columns:?}"
    );
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

// ── 로그인과 소유자(3단계) ──

#[sqlx::test]
async fn 로그인하지_않고_라이브를_시작하면_401이고_방송이_남지_않는다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;

    let response = app
        .request_as(&Persona::anonymous(), start_request("익명 라이브"))
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(response).await["code"], "login_required");
    // 준비 상태는 목록·조회에 보이지 않으므로 표를 직접 센다.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM broadcasts")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test]
async fn 로그인한_사용자가_시작한_라이브_방송의_소유자는_그_사용자다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool.clone()).await;
    let owner = Persona::user_a(&pool).await;

    let response = app.request_as(&owner, start_request("내 라이브")).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let id = body_json(response).await["id"].as_i64().unwrap();
    app.fake.set_online(&[&format!("live/{id}")]);
    app.sync(at(1)).await;

    let list = crate::db::list_broadcasts(&app.pool).await.unwrap();
    assert_eq!(list[0].owner.as_deref(), Some("user_a"));
    let found = crate::db::find_broadcast(&app.pool, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.owner.as_deref(), Some("user_a"));
}

// ── 송출 연결 ──

#[sqlx::test]
async fn 비로그인은_401_다른_사용자는_403으로_송출하지_못하고_MediaMTX에_넘기지_않는다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool.clone()).await;
    let id = app.start_live("남의 방송").await;
    let other = Persona::user_b(&pool).await;

    let response = app
        .request_as(&Persona::anonymous(), whip_request(id))
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(response).await["code"], "login_required");

    let response = app.request_as(&other, whip_request(id)).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(body_json(response).await["code"], "forbidden");

    assert!(app.fake.requests().is_empty());
    assert_eq!(status_of(&app, id).await, LiveStatus::Ready);
}

#[sqlx::test]
async fn 소유자면_offer가_넘어가고_answer와_우리_서버의_세션_주소가_온다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let id = app.start_live("송출").await;

    let response = app.request(whip_request(id)).await;

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
    // 로그인 쿠키는 MediaMTX에 넘기지 않는다.
    assert!(sent[0].headers.get(header::COOKIE).is_none());
}

#[sqlx::test]
async fn 없는_번호로_송출하면_404다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    app.start_live("있는 방송").await;

    assert_eq!(
        app.request(whip_request(9999)).await.status(),
        StatusCode::NOT_FOUND
    );
    let not_number = Request::post("/api/live/abc/whip")
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
    app.start_live("라이브").await;
    let recorded: i64 = sqlx::query_scalar(
        "INSERT INTO broadcasts (title, video_key) VALUES ('녹화', 'videos/x.mp4') RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();

    assert_eq!(
        app.request(whip_request(recorded)).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn 준비가_아닌_방송에_송출하면_409다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let id = app.go_live("이미 방송 중", at(0)).await;
    let before = app.fake.webrtc_requests().len();

    let response = app.request(whip_request(id)).await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(body_json(response).await["message"].is_string());
    assert_eq!(app.fake.webrtc_requests().len(), before);
}

#[sqlx::test]
async fn 세션_PATCH와_DELETE도_소유자만_된다(pool: PgPool) {
    let app = LiveApp::new(pool.clone()).await;
    let id = app.start_live("세션").await;
    let session = format!("/api/live/{id}/whip/{FAKE_SECRET}");
    let other = Persona::user_b(&pool).await;

    for method in [Method::PATCH, Method::DELETE] {
        let response = app
            .request_as(
                &Persona::anonymous(),
                session_request(method.clone(), &session),
            )
            .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{method}");
        let response = app
            .request_as(&other, session_request(method.clone(), &session))
            .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method}");
    }
    assert!(app.fake.requests().is_empty());

    let response = app.request(session_request(Method::PATCH, &session)).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = app.request(session_request(Method::DELETE, &session)).await;
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
    let id = app.start_live("세션").await;

    let path = format!("/api/live/{id}/whip/..%2F..%2Fv3%2Fconfig");
    let response = app.request(session_request(Method::DELETE, &path)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(app.fake.requests().is_empty());
}

#[sqlx::test]
async fn MediaMTX가_실패하면_그_상태를_알리고_방송은_준비로_남는다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let id = app.start_live("실패").await;
    app.fake.set_unreachable(true);

    let response = app.request(whip_request(id)).await;

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
    let id = app.start_live("방송 중으로").await;
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
    let waiting = app.start_live("기다림").await;
    let online = app.start_live("들어옴").await;
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
    let id = app.go_live("그대로", at(1)).await;

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
    let id = app.start_live("그대로").await;
    app.fake.set_online(&[&format!("live/{id}")]);
    app.fake.set_unreachable(true);

    let result = super::status_sync::sync_once(&app.pool, &app.fake.client(), at(5)).await;

    assert!(result.is_err());
    assert_eq!(
        crate::db::find_broadcast(&app.pool, id).await.unwrap(),
        None
    );
}

// ── 방송 종료 ──

/// 종료 요청. 누가 보내는지는 [`LiveApp::request`]·[`LiveApp::request_as`]가 정한다.
pub fn end_request(id: i64) -> Request<Body> {
    Request::post(format!("/api/live/{id}/end"))
        .body(Body::empty())
        .unwrap()
}

/// 저장소에서 본 방송 상태. 준비·버려짐은 조회 함수에 나오지 않으므로 이것으로 본다.
async fn status_of(app: &LiveApp, id: i64) -> LiveStatus {
    super::store::find_access(&app.pool, id)
        .await
        .unwrap()
        .expect("라이브 방송이 있어야 한다")
        .status
}

#[sqlx::test]
async fn 소유자가_종료하면_204이고_종료가_되며_종료_시각이_생긴다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let id = app.go_live("끝낼 방송", at(1)).await;
    let before = Utc::now();

    let response = app.request(end_request(id)).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let found = crate::db::find_broadcast(&app.pool, id)
        .await
        .unwrap()
        .expect("종료된 방송은 조회된다");
    assert_eq!(found.status, Some(LiveStatus::Ended));
    assert_eq!(found.started_at, Some(at(1)));
    let ended_at = found.ended_at.expect("종료 시각이 있어야 한다");
    assert!(ended_at >= before - chrono::Duration::seconds(1));
}

#[sqlx::test]
async fn 비로그인은_401_다른_사용자는_403으로_종료하지_못하고_상태는_그대로다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool.clone()).await;
    let id = app.go_live("그대로", at(1)).await;
    let other = Persona::user_b(&pool).await;
    let before = app.fake.requests().len();

    let response = app.request_as(&Persona::anonymous(), end_request(id)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(body_json(response).await["message"].is_string());
    let response = app.request_as(&other, end_request(id)).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(body_json(response).await["message"].is_string());

    assert_eq!(status_of(&app, id).await, LiveStatus::Live);
    // 송출 세션 끊기 요청도 가지 않는다.
    assert_eq!(app.fake.requests().len(), before);
}

#[sqlx::test]
async fn 준비_중에_종료하면_버려지고_조회되지_않는다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let id = app.start_live("영상 없이 끝").await;

    let response = app.request(end_request(id)).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(status_of(&app, id).await, LiveStatus::Abandoned);
    assert_eq!(
        crate::db::find_broadcast(&app.pool, id).await.unwrap(),
        None
    );
}

#[sqlx::test]
async fn 두_번_종료해도_204이고_종료_시각은_처음_그대로다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let id = app.go_live("두 번", at(1)).await;
    app.request(end_request(id)).await;
    let first = crate::db::find_broadcast(&app.pool, id)
        .await
        .unwrap()
        .unwrap()
        .ended_at;

    let response = app.request(end_request(id)).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let found = crate::db::find_broadcast(&app.pool, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.status, Some(LiveStatus::Ended));
    assert_eq!(found.ended_at, first);
}

#[sqlx::test]
async fn 없는_번호를_종료하면_404다(pool: PgPool) {
    let app = LiveApp::new(pool).await;

    let response = app.request(end_request(424242)).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn 종료하면_MediaMTX의_송출_세션을_끊는다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let id = app.go_live("끊기", at(1)).await;

    app.request(end_request(id)).await;

    let kicked = app.fake.requests().into_iter().any(|r| {
        r.method == Method::POST && r.path == format!("/v3/webrtc/sessions/kick/{FAKE_SESSION_ID}")
    });
    assert!(kicked, "송출 세션 강제 종료 요청이 가야 한다");
}

#[sqlx::test]
async fn MediaMTX가_고장이어도_종료는_204이고_종료가_된다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let id = app.go_live("고장", at(1)).await;
    app.fake.set_unreachable(true);

    let response = app.request(end_request(id)).await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(status_of(&app, id).await, LiveStatus::Ended);
}

// ── 상태 감시기: 종료와 버려짐 ──

#[sqlx::test]
async fn 경로가_사라지면_방송_중이_종료가_되고_종료_시각이_남는다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let gone = app.go_live("사라짐", at(1)).await;
    let still = app.start_live("계속").await;
    app.fake
        .set_online(&[&format!("live/{gone}"), &format!("live/{still}")]);
    app.sync(at(2)).await;

    app.fake.set_online(&[&format!("live/{still}")]);
    app.sync(at(8)).await;

    let found = crate::db::find_broadcast(&app.pool, gone)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.status, Some(LiveStatus::Ended));
    assert_eq!(found.started_at, Some(at(1)));
    assert_eq!(found.ended_at, Some(at(8)));
    assert_eq!(status_of(&app, still).await, LiveStatus::Live);
}

/// 준비 시각은 DB가 정하므로(시작할 때의 지금), 30초 경계는 실제 시각을 기준으로 넉넉히 떨어뜨려 본다.
#[sqlx::test]
async fn 준비로_30초가_지나도록_영상이_없으면_버려지고_그_전에는_그대로다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let started = Utc::now();
    let id = app.start_live("안 옴").await;

    app.sync(started + chrono::Duration::seconds(20)).await;
    assert_eq!(status_of(&app, id).await, LiveStatus::Ready);

    app.sync(started + chrono::Duration::seconds(40)).await;
    assert_eq!(status_of(&app, id).await, LiveStatus::Abandoned);
}

#[sqlx::test]
async fn 종료된_방송은_경로가_다시_나타나도_방송_중이_되지_않는다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let ended = app.go_live("끝남", at(1)).await;
    app.request(end_request(ended)).await;
    let abandoned = app.start_live("버려짐").await;
    app.request(end_request(abandoned)).await;

    app.fake
        .set_online(&[&format!("live/{ended}"), &format!("live/{abandoned}")]);
    app.sync(at(30)).await;

    assert_eq!(status_of(&app, ended).await, LiveStatus::Ended);
    assert_eq!(status_of(&app, abandoned).await, LiveStatus::Abandoned);
}

#[sqlx::test]
async fn 같은_DB에_새로_연결해도_라이브_방송의_상태와_시각이_그대로_조회된다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let live = app.go_live("방송 중", at(1)).await;
    let ended = app.start_live("끝난 방송").await;
    app.fake
        .set_online(&[&format!("live/{live}"), &format!("live/{ended}")]);
    app.sync(at(2)).await;
    app.fake.set_online(&[&format!("live/{live}")]);
    app.sync(at(9)).await;

    let reconnected = PgPool::connect_with(app.pool.connect_options().as_ref().clone())
        .await
        .unwrap();

    let found_live = crate::db::find_broadcast(&reconnected, live)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found_live.status, Some(LiveStatus::Live));
    assert_eq!(found_live.started_at, Some(at(1)));
    assert_eq!(found_live.ended_at, None);
    let found_ended = crate::db::find_broadcast(&reconnected, ended)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found_ended.status, Some(LiveStatus::Ended));
    assert_eq!(found_ended.started_at, Some(at(2)));
    assert_eq!(found_ended.ended_at, Some(at(9)));
}

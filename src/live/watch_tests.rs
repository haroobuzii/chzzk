//! 시청 연결(WHEP 중계) 테스트. `tests.rs`의 `LiveApp`과 요청 도우미를 쓴다.
#![allow(non_snake_case)]

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
};
use sqlx::PgPool;

use super::fake_mediamtx::{FAKE_ANSWER, FAKE_SECRET};
use super::tests::{at, body_json, body_text, session_request, LiveApp, OFFER};

/// 시청 연결 요청. 키가 필요 없다.
fn whep_request(id: impl std::fmt::Display) -> Request<Body> {
    Request::post(format!("/api/live/{id}/whep"))
        .header(header::CONTENT_TYPE, "application/sdp")
        .body(Body::from(OFFER))
        .unwrap()
}

#[sqlx::test]
async fn 방송_중이면_키_없이_offer가_넘어가고_answer와_우리_서버의_세션_주소가_온다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let (id, _) = app.go_live("시청", at(0)).await;

    let response = app.request(whep_request(id)).await;

    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/sdp");
    assert_eq!(
        response.headers()[header::LOCATION],
        format!("/api/live/{id}/whep/{FAKE_SECRET}").as_str()
    );
    assert_eq!(body_text(response).await, FAKE_ANSWER);

    let sent = app.fake.webrtc_requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, Method::POST);
    assert_eq!(sent[0].path, format!("/live/{id}/whep"));
    assert_eq!(sent[0].headers[header::CONTENT_TYPE], "application/sdp");
    assert_eq!(sent[0].body, OFFER);
}

/// 방송을 종료 상태로 만든다. 종료 API는 04 티켓이 만들므로 DB를 직접 바꾼다.
async fn end_in_db(app: &LiveApp, id: i64, now: chrono::DateTime<chrono::Utc>) {
    sqlx::query("UPDATE broadcasts SET status = 'ended', ended_at = $2 WHERE id = $1")
        .bind(id)
        .bind(now)
        .execute(&app.pool)
        .await
        .unwrap();
}

#[sqlx::test]
async fn 준비나_종료된_방송을_시청하면_409이고_MediaMTX에_넘기지_않는다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let (ready, _) = app.start_live("준비").await;
    let (ended, _) = app.go_live("종료", at(0)).await;
    end_in_db(&app, ended, at(30)).await;

    for id in [ready, ended] {
        let response = app.request(whep_request(id)).await;
        assert_eq!(response.status(), StatusCode::CONFLICT, "{id}");
        assert!(body_json(response).await["message"].is_string());
    }
    assert!(app.fake.webrtc_requests().is_empty());
}

#[sqlx::test]
async fn 없는_번호나_녹화_방송을_시청하면_404다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let recorded: i64 = sqlx::query_scalar(
        "INSERT INTO broadcasts (title, video_key) VALUES ('녹화', 'videos/x.mp4') RETURNING id",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();

    for id in [recorded.to_string(), "9999".into(), "abc".into()] {
        let response = app.request(whep_request(&id)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{id}");
    }
    assert!(app.fake.requests().is_empty());
}

#[sqlx::test]
async fn 시청_세션의_PATCH와_DELETE는_키_없이_넘어간다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let (id, _) = app.go_live("세션", at(0)).await;
    let session = format!("/api/live/{id}/whep/{FAKE_SECRET}");

    let response = app
        .request(session_request(Method::PATCH, &session, None))
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    // 방송이 끝난 뒤에도 연결을 끊을 수 있다.
    end_in_db(&app, id, at(30)).await;
    let response = app
        .request(session_request(Method::DELETE, &session, None))
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let sent = app.fake.webrtc_requests();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].method, Method::PATCH);
    assert_eq!(sent[0].path, format!("/live/{id}/whep/{FAKE_SECRET}"));
    assert_eq!(
        sent[0].headers[header::CONTENT_TYPE],
        "application/trickle-ice-sdpfrag"
    );
    assert!(sent[0].body.contains("candidate"));
    assert_eq!(sent[1].method, Method::DELETE);
    assert_eq!(sent[1].path, format!("/live/{id}/whep/{FAKE_SECRET}"));
}

#[sqlx::test]
async fn 시청_세션_주소가_이상하거나_없는_방송이면_넘기지_않고_404다(
    pool: PgPool,
) {
    let app = LiveApp::new(pool).await;
    let (id, _) = app.go_live("세션", at(0)).await;
    let before = app.fake.requests().len();

    for path in [
        format!("/api/live/{id}/whep/..%2F..%2Fv3%2Fconfig"),
        format!("/api/live/9999/whep/{FAKE_SECRET}"),
    ] {
        let response = app
            .request(session_request(Method::DELETE, &path, None))
            .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    assert_eq!(app.fake.requests().len(), before);
}

// ── 목록 ──

/// 녹화 방송 한 줄을 정한 등록 시각으로 넣는다.
async fn insert_recorded(app: &LiveApp, title: &str, created_at: chrono::DateTime<chrono::Utc>) {
    sqlx::query(
        "INSERT INTO broadcasts (title, video_key, created_at) VALUES ($1, 'videos/x.mp4', $2)",
    )
    .bind(title)
    .bind(created_at)
    .execute(&app.pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn 녹화_방송_중_종료가_섞인_목록이_최신순으로_표시와_시각을_갖는다(
    pool: PgPool,
) {
    use crate::display::Mark;

    let app = LiveApp::new(pool).await;
    insert_recorded(&app, "옛 녹화", at(1)).await;
    let (ended, _) = app.go_live("끝난 라이브", at(10)).await;
    end_in_db(&app, ended, at(20)).await;
    insert_recorded(&app, "새 녹화", at(30)).await;
    let (_live, _) = app.go_live("지금 라이브", at(40)).await;
    // 준비 상태는 목록에 나오지 않는다.
    app.start_live("준비").await;

    let list = crate::db::list_broadcasts(&app.pool).await.unwrap();
    let shown: Vec<(&str, Mark, chrono::DateTime<chrono::Utc>)> = list
        .iter()
        .map(|b| (b.title.as_str(), b.mark(), b.shown_time()))
        .collect();
    assert_eq!(
        shown,
        vec![
            ("지금 라이브", Mark::Live, at(40)),
            ("새 녹화", Mark::Recorded, at(30)),
            ("끝난 라이브", Mark::Ended, at(10)),
            ("옛 녹화", Mark::Recorded, at(1)),
        ]
    );
}

#[sqlx::test]
async fn MediaMTX가_시청을_거절하면_502다(pool: PgPool) {
    let app = LiveApp::new(pool).await;
    let (id, _) = app.go_live("거절", at(0)).await;
    app.fake.set_unreachable(true);

    let response = app.request(whep_request(id)).await;

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(body_json(response).await["message"].is_string());
}

//! 댓글 라우터 테스트.
//!
//! 실제 PostgreSQL(`#[sqlx::test]` 임시 DB)을 쓴다. 서버를 띄우지 않고 라우터에 요청을 직접 넣어
//! 응답과 그 뒤의 목록 조회 결과로 판단한다. 방송은 DB에 직접 넣는다.

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    Router,
};
use sqlx::PgPool;
use tower::ServiceExt;

use crate::auth::personas::Persona;
use crate::test_support::{at, insert_recorded};

/// 실제 서버처럼 `Origin` 확인을 씌운 댓글 라우터.
fn router(pool: &PgPool) -> Router {
    crate::auth::guard_origin(super::api(pool.clone()))
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, serde_json::Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// 이 손님으로 `broadcast` 방송에 댓글 `body`를 쓴다.
async fn write(
    router: &Router,
    persona: &Persona,
    broadcast: i64,
    body: &str,
) -> (StatusCode, serde_json::Value) {
    let request = Request::post(format!("/api/broadcasts/{broadcast}/comments"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::json!({ "body": body }).to_string()))
        .unwrap();
    send(router, persona.sign(request)).await
}

/// 이 손님으로 `broadcast` 방송의 댓글 목록을 본다.
async fn list(router: &Router, persona: &Persona, broadcast: i64) -> Vec<serde_json::Value> {
    let request = Request::get(format!("/api/broadcasts/{broadcast}/comments"))
        .body(Body::empty())
        .unwrap();
    let (status, json) = send(router, persona.sign(request)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    json.as_array().expect("목록은 배열이다").clone()
}

/// 이 손님으로 댓글 `comment`를 지운다.
async fn delete(router: &Router, persona: &Persona, comment: i64) -> StatusCode {
    let request = Request::delete(format!("/api/comments/{comment}"))
        .body(Body::empty())
        .unwrap();
    send(router, persona.sign(request)).await.0
}

/// 목록의 본문들.
fn bodies(comments: &[serde_json::Value]) -> Vec<&str> {
    comments
        .iter()
        .map(|c| c["body"].as_str().unwrap())
        .collect()
}

#[sqlx::test]
async fn 비로그인_댓글_쓰기는_401이고_목록에_남지_않는다(pool: PgPool) {
    let router = router(&pool);
    let broadcast = insert_recorded(&pool, "방송", at(0)).await;

    let (status, json) = write(&router, &Persona::anonymous(), broadcast, "안녕하세요").await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["code"], "login_required");
    assert!(list(&router, &Persona::anonymous(), broadcast)
        .await
        .is_empty());
}

#[sqlx::test]
async fn 로그인_쓰기는_201이고_목록_끝에_작성자와_함께_나온다(pool: PgPool) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    let b = Persona::user_b(&pool).await;
    let broadcast = insert_recorded(&pool, "방송", at(0)).await;
    write(&router, &b, broadcast, "먼저 왔어요").await;

    let (status, json) = write(&router, &a, broadcast, "  반가워요 🎮  ").await;

    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["author"], "user_a");
    assert_eq!(json["body"], "반가워요 🎮", "앞뒤 공백은 지운다");
    assert_eq!(json["mine"], true);
    let comments = list(&router, &Persona::anonymous(), broadcast).await;
    assert_eq!(bodies(&comments), ["먼저 왔어요", "반가워요 🎮"]);
    let last = comments.last().unwrap();
    assert_eq!(last["id"], json["id"]);
    assert_eq!(last["author"], "user_a");
    assert!(last["created_at"].is_string());
}

#[sqlx::test]
async fn 빈_댓글과_공백뿐인_댓글과_301자_댓글은_400이다(pool: PgPool) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    let broadcast = insert_recorded(&pool, "방송", at(0)).await;
    let too_long = "가".repeat(301);

    for (body, code) in [
        ("", "comment_empty"),
        ("  \n\t ", "comment_empty"),
        (too_long.as_str(), "comment_too_long"),
    ] {
        let (status, json) = write(&router, &a, broadcast, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
        assert_eq!(json["code"], code, "{body:?}");
        assert!(json["message"].is_string());
    }
    let (status, _) = write(&router, &a, broadcast, &"가".repeat(300)).await;
    assert_eq!(status, StatusCode::CREATED, "300자는 받는다");
    assert_eq!(list(&router, &a, broadcast).await.len(), 1);
}

#[sqlx::test]
async fn 없는_방송에_쓰거나_목록을_보면_404다(pool: PgPool) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;

    let (status, _) = write(&router, &a, 999, "여보세요").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = write(&router, &a, 0, "여보세요").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let request = Request::get("/api/broadcasts/999/comments")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&router, request).await.0, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn 목록은_누구나_오래된_순으로_보고_mine은_작성자에게만_참이다(
    pool: PgPool,
) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    let b = Persona::user_b(&pool).await;
    let broadcast = insert_recorded(&pool, "방송", at(0)).await;
    write(&router, &a, broadcast, "하나").await;
    write(&router, &b, broadcast, "둘").await;
    write(&router, &a, broadcast, "셋").await;

    let mine = |comments: &[serde_json::Value]| -> Vec<bool> {
        comments
            .iter()
            .map(|c| c["mine"].as_bool().unwrap())
            .collect()
    };
    let seen_by_anonymous = list(&router, &Persona::anonymous(), broadcast).await;
    assert_eq!(bodies(&seen_by_anonymous), ["하나", "둘", "셋"]);
    assert_eq!(mine(&seen_by_anonymous), [false, false, false]);
    let authors: Vec<&str> = seen_by_anonymous
        .iter()
        .map(|c| c["author"].as_str().unwrap())
        .collect();
    assert_eq!(authors, ["user_a", "user_b", "user_a"]);

    assert_eq!(
        mine(&list(&router, &a, broadcast).await),
        [true, false, true]
    );
    assert_eq!(
        mine(&list(&router, &b, broadcast).await),
        [false, true, false]
    );
}

#[sqlx::test]
async fn 종료된_라이브_방송에도_댓글을_쓴다(pool: PgPool) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    // 이 라우터에는 라이브 API(시작·송출·종료)가 없고, 종료까지 가려면 가짜 MediaMTX도 띄워야 한다.
    // 여기서 보려는 것은 댓글 쪽뿐이므로 끝난 라이브 방송 행을 DB에 바로 넣는다.
    let ended: i64 = sqlx::query_scalar(
        "INSERT INTO broadcasts (title, kind, status, started_at, ended_at)
         VALUES ('끝난 라이브', 'live', 'ended', now(), now()) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let (status, json) = write(&router, &a, ended, "다시 보고 싶어요").await;

    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(
        bodies(&list(&router, &a, ended).await),
        ["다시 보고 싶어요"]
    );
}

#[sqlx::test]
async fn 댓글_삭제는_작성자만_204이고_다른_사용자는_403_비로그인은_401_없는_댓글은_404다(
    pool: PgPool,
) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    let b = Persona::user_b(&pool).await;
    let broadcast = insert_recorded(&pool, "방송", at(0)).await;
    let (_, json) = write(&router, &a, broadcast, "A의 댓글").await;
    let comment = json["id"].as_i64().unwrap();

    assert_eq!(delete(&router, &b, comment).await, StatusCode::FORBIDDEN);
    assert_eq!(
        delete(&router, &Persona::anonymous(), comment).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        bodies(&list(&router, &a, broadcast).await),
        ["A의 댓글"],
        "거절된 뒤에는 댓글이 그대로다"
    );

    assert_eq!(delete(&router, &a, comment).await, StatusCode::NO_CONTENT);
    assert!(list(&router, &a, broadcast).await.is_empty());
    assert_eq!(delete(&router, &a, comment).await, StatusCode::NOT_FOUND);
    assert_eq!(delete(&router, &a, 999).await, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn 방송_행을_지우면_그_방송의_댓글도_사라진다(pool: PgPool) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    let gone = insert_recorded(&pool, "지울 방송", at(0)).await;
    let kept = insert_recorded(&pool, "남을 방송", at(1)).await;
    let (_, json) = write(&router, &a, gone, "사라질 댓글").await;
    let comment = json["id"].as_i64().unwrap();
    write(&router, &a, kept, "남을 댓글").await;

    // 보려는 것은 DB의 연쇄 삭제(ON DELETE CASCADE)다. 공개 API(DELETE /api/broadcasts/{번호})는
    // 소유자가 필요하고 영상 파일까지 지우므로, 방송 행만 DB에서 바로 지운다.
    sqlx::query("DELETE FROM broadcasts WHERE id = $1")
        .bind(gone)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(delete(&router, &a, comment).await, StatusCode::NOT_FOUND);
    assert_eq!(bodies(&list(&router, &a, kept).await), ["남을 댓글"]);
}

#[sqlx::test]
async fn 다른_출처의_댓글_쓰기는_쿠키가_있어도_403이다(pool: PgPool) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    let broadcast = insert_recorded(&pool, "방송", at(0)).await;
    let request = Request::post(format!("/api/broadcasts/{broadcast}/comments"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, "https://evil.example")
        .header(header::HOST, "localhost:8080")
        .body(Body::from(r#"{"body":"몰래"}"#))
        .unwrap();

    assert_eq!(
        send(&router, a.sign(request)).await.0,
        StatusCode::FORBIDDEN
    );
    assert!(list(&router, &a, broadcast).await.is_empty());
}

#[sqlx::test]
async fn DB에_새로_연결해도_댓글과_작성자가_그대로다(
    pool_options: sqlx::postgres::PgPoolOptions,
    options: sqlx::postgres::PgConnectOptions,
) {
    let pool = pool_options.connect_with(options.clone()).await.unwrap();
    let a = Persona::user_a(&pool).await;
    let broadcast = insert_recorded(&pool, "방송", at(0)).await;
    write(&router(&pool), &a, broadcast, "재시작 뒤에도").await;

    // 서버를 다시 켠 것처럼 같은 DB에 새 연결 풀을 열고 라우터를 다시 만든다.
    pool.close().await;
    let reopened = PgPool::connect_with(options).await.unwrap();
    let comments = list(&router(&reopened), &a, broadcast).await;

    assert_eq!(bodies(&comments), ["재시작 뒤에도"]);
    assert_eq!(comments[0]["author"], "user_a");
    assert_eq!(comments[0]["mine"], true, "쿠키 로그인도 그대로다");
}

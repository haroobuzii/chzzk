//! 후원 라우터 테스트.
//!
//! 실제 PostgreSQL(`#[sqlx::test]` 임시 DB)을 쓴다. 서버를 띄우지 않고 라우터에 요청을 직접 넣어
//! 응답과 그 뒤의 합계·내역 조회 결과로 판단한다. 방송은 DB에 직접 넣는다.

use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    Router,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

use super::toss::TossConfig;
use super::DonationsState;
use crate::auth::personas::Persona;
use crate::test_support::{at, insert_recorded};

/// 가짜 테스트 키를 넣은 토스 설정. 진짜 키가 아니다.
fn fake_toss() -> TossConfig {
    TossConfig::from_values(
        Some("test_ck_fake".into()),
        Some("test_sk_fake".into()),
        Some("http://127.0.0.1:9".into()),
        None,
    )
    .unwrap()
    .unwrap()
}

/// 실제 서버처럼 `Origin` 확인을 씌운 후원 라우터. 토스 키가 있다.
fn router(pool: &PgPool) -> Router {
    router_with(pool, Some(fake_toss()))
}

pub(super) fn router_with(pool: &PgPool, toss: Option<TossConfig>) -> Router {
    crate::auth::guard_origin(super::api(DonationsState {
        pool: pool.clone(),
        toss,
    }))
}

pub(super) async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// 이 손님으로 `broadcast` 방송에 후원 주문서를 만든다.
pub(super) async fn order(
    router: &Router,
    persona: &Persona,
    broadcast: i64,
    body: Value,
) -> (StatusCode, Value) {
    let request = Request::post(format!("/api/broadcasts/{broadcast}/donations"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    send(router, persona.sign(request)).await
}

/// 비로그인으로 `broadcast` 방송의 후원 합계·내역을 본다.
pub(super) async fn donations(router: &Router, broadcast: i64) -> (StatusCode, Value) {
    let request = Request::get(format!("/api/broadcasts/{broadcast}/donations"))
        .body(Body::empty())
        .unwrap();
    send(router, request).await
}

pub(super) async fn user_id(pool: &PgPool, persona: &Persona) -> i64 {
    sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(persona.username.as_deref().unwrap())
        .fetch_one(pool)
        .await
        .unwrap()
}

/// `owner`가 소유한 녹화 방송을 DB에 넣는다. 방송 등록 API는 영상 파일이 필요해 여기서는 직접 넣는다.
pub(super) async fn owned_broadcast(pool: &PgPool, owner: &Persona) -> i64 {
    let id = insert_recorded(pool, "방송", at(0)).await;
    sqlx::query("UPDATE broadcasts SET owner_id = $1 WHERE id = $2")
        .bind(user_id(pool, owner).await)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    id
}

/// 확정된 후원 한 줄을 DB에 직접 넣는다. 확정(승인 확인)은 02에서 만들므로, 그 전에는 합계·내역을 이렇게 채운다.
async fn insert_confirmed(
    pool: &PgPool,
    broadcast: i64,
    donor: &Persona,
    amount: i64,
    message: Option<&str>,
    confirmed_at: DateTime<Utc>,
) {
    let order_id = super::store::new_order_id();
    sqlx::query(
        "INSERT INTO donation_orders
             (order_id, broadcast_id, donor_id, recipient_id, amount, message, status, payment_key, confirmed_at)
         SELECT $1, b.id, $3, b.owner_id, $4, $5, 'confirmed', $6, $7 FROM broadcasts b WHERE b.id = $2",
    )
    .bind(&order_id)
    .bind(broadcast)
    .bind(user_id(pool, donor).await)
    .bind(amount)
    .bind(message)
    .bind(format!("pay_{order_id}"))
    .bind(confirmed_at)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn 비로그인_주문서_만들기는_401이다(pool: PgPool) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    let broadcast = owned_broadcast(&pool, &a).await;

    let (status, json) = order(
        &router,
        &Persona::anonymous(),
        broadcast,
        json!({ "amount": 5000 }),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json["code"], "login_required");
}

#[sqlx::test]
async fn 주문서를_만들면_201이고_결제_대기라_합계와_내역에_없다(
    pool: PgPool,
) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    let b = Persona::user_b(&pool).await;
    let broadcast = owned_broadcast(&pool, &a).await;

    let (status, json) = order(
        &router,
        &b,
        broadcast,
        json!({ "amount": 5000, "message": "  응원해요  " }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{json}");
    assert_eq!(json["amount"], 5000);
    assert_eq!(json["client_key"], "test_ck_fake");
    let order_name = json["order_name"].as_str().unwrap();
    assert!(
        order_name.contains("user_a"),
        "받는 사람이 보인다: {order_name}"
    );
    assert!(
        order_name.contains("5,000원"),
        "금액이 보인다: {order_name}"
    );
    let order_id = json["order_id"].as_str().unwrap();
    assert!(super::store::is_valid_order_id(order_id), "{order_id}");
    assert!(json.get("secret_key").is_none());
    assert!(
        !json.to_string().contains("test_sk_fake"),
        "시크릿 키는 브라우저로 가지 않는다"
    );

    let (status, list) = donations(&router, broadcast).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["total_amount"], 0);
    assert_eq!(list["count"], 0);
    assert_eq!(list["items"], json!([]));
}

#[sqlx::test]
async fn 주문서의_받는_사람은_방송_소유자이고_금액과_메시지는_서버가_받은_값이다(
    pool: PgPool,
) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    let b = Persona::user_b(&pool).await;
    let broadcast = owned_broadcast(&pool, &a).await;

    let (_, json) = order(
        &router,
        &b,
        broadcast,
        // 받는 사람을 바꾸려는 값은 무시된다.
        json!({ "amount": 12300, "message": " 고마워요 ", "recipient_id": 999 }),
    )
    .await;

    // 결제 대기 주문서는 공개 API로 볼 수 없어 DB에서 직접 읽는다.
    let row: (Option<i64>, i64, i64, i64, Option<String>, String) = sqlx::query_as(
        "SELECT broadcast_id, donor_id, recipient_id, amount, message, status
         FROM donation_orders WHERE order_id = $1",
    )
    .bind(json["order_id"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            Some(broadcast),
            user_id(&pool, &b).await,
            user_id(&pool, &a).await,
            12300,
            Some("고마워요".to_string()),
            "pending".to_string(),
        )
    );
}

#[sqlx::test]
async fn 없는_방송은_404다(pool: PgPool) {
    let router = router(&pool);
    let b = Persona::user_b(&pool).await;

    let (status, json) = order(&router, &b, 987_654, json!({ "amount": 5000 })).await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    assert_eq!(donations(&router, 987_654).await.0, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn 소유자_없는_방송과_내_방송은_400이다(pool: PgPool) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    let ownerless = insert_recorded(&pool, "예전 방송", at(0)).await;
    let mine = owned_broadcast(&pool, &a).await;

    let (status, json) = order(&router, &a, ownerless, json!({ "amount": 5000 })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["code"], "no_owner");

    let (status, json) = order(&router, &a, mine, json!({ "amount": 5000 })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(json["code"], "own_broadcast");
}

#[sqlx::test]
async fn 금액_위반과_101자_메시지는_400이다(pool: PgPool) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    let b = Persona::user_b(&pool).await;
    let broadcast = owned_broadcast(&pool, &a).await;

    for body in [
        json!({ "amount": 999 }),
        json!({ "amount": 500_100 }),
        json!({ "amount": 1_050 }),
        json!({ "amount": 5000, "message": "가".repeat(101) }),
        json!({ "amount": "5000" }),
        json!({}),
    ] {
        let (status, json) = order(&router, &b, broadcast, body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body} → {json}");
        assert!(json["message"].is_string(), "{json}");
    }
    let (_, list) = donations(&router, broadcast).await;
    assert_eq!(list["count"], 0);
}

#[sqlx::test]
async fn 토스_키가_없으면_주문서_만들기는_503이고_내역은_볼_수_있다(
    pool: PgPool,
) {
    let router = router_with(&pool, None);
    let a = Persona::user_a(&pool).await;
    let b = Persona::user_b(&pool).await;
    let broadcast = owned_broadcast(&pool, &a).await;

    let (status, json) = order(&router, &b, broadcast, json!({ "amount": 5000 })).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json["code"], "donations_disabled");
    let (status, list) = donations(&router, broadcast).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        list["enabled"], false,
        "후원 칸이 키 안내를 보여 줄 수 있다"
    );
    assert_eq!(
        donations(&router_with(&pool, Some(fake_toss())), broadcast)
            .await
            .1["enabled"],
        true
    );
}

#[sqlx::test]
async fn 내역은_누구나_보고_확정된_후원만_최신순으로_합계와_함께_나온다(
    pool: PgPool,
) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    let b = Persona::user_b(&pool).await;
    let broadcast = owned_broadcast(&pool, &a).await;
    let other = owned_broadcast(&pool, &a).await;
    insert_confirmed(&pool, broadcast, &b, 1_000, None, at(10)).await;
    insert_confirmed(&pool, broadcast, &b, 50_000, Some("최고예요"), at(30)).await;
    insert_confirmed(&pool, broadcast, &b, 5_000, Some("첫 후원"), at(20)).await;
    insert_confirmed(&pool, other, &b, 10_000, None, at(40)).await;
    // 결제 대기 주문서는 합계·내역에 넣지 않는다.
    order(&router, &b, broadcast, json!({ "amount": 100_000 })).await;

    let (status, list) = donations(&router, broadcast).await;

    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["total_amount"], 56_000);
    assert_eq!(list["count"], 3);
    let items = list["items"].as_array().unwrap();
    let amounts: Vec<i64> = items
        .iter()
        .map(|i| i["amount"].as_i64().unwrap())
        .collect();
    assert_eq!(amounts, [50_000, 5_000, 1_000], "최신순");
    assert_eq!(items[0]["donor"], "user_b");
    assert_eq!(items[0]["message"], "최고예요");
    assert_eq!(items[2]["message"], Value::Null);
    let confirmed_at: DateTime<Utc> =
        serde_json::from_value(items[0]["confirmed_at"].clone()).unwrap();
    assert_eq!(confirmed_at, at(30));
}

#[sqlx::test]
async fn 방송을_지워도_주문서는_남고_그_방송의_내역은_404다(pool: PgPool) {
    let router = router(&pool);
    let a = Persona::user_a(&pool).await;
    let b = Persona::user_b(&pool).await;
    let broadcast = owned_broadcast(&pool, &a).await;
    insert_confirmed(&pool, broadcast, &b, 5_000, None, at(10)).await;
    order(&router, &b, broadcast, json!({ "amount": 1000 })).await;

    // 방송 삭제 API는 영상 보관소와 MediaMTX가 필요해, 여기서는 DB의 외래 키 동작만 본다.
    sqlx::query("DELETE FROM broadcasts WHERE id = $1")
        .bind(broadcast)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(donations(&router, broadcast).await.0, StatusCode::NOT_FOUND);
    // 지워진 방송의 주문서는 공개 API로 볼 수 없어 DB에서 직접 읽는다.
    let remaining: Vec<(Option<i64>, String)> =
        sqlx::query_as("SELECT broadcast_id, status FROM donation_orders ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        remaining,
        [
            (None, "confirmed".to_string()),
            (None, "pending".to_string())
        ]
    );
}

//! 실패·취소·만료 테스트: `POST /api/donations/fail`, 만료 정리, 만료된 주문서의 승인 확인.
//!
//! 실제 PostgreSQL(`#[sqlx::test]` 임시 DB)과 가짜 토스 서버([`FakeToss`])를 쓴다.
//! 주문서의 상태는 승인 확인 응답으로 본다. 결제 대기가 아닌 주문서는 토스를 부르지 않고 지금 상태를 돌려준다.

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    Router,
};
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;

use super::expiry;
use super::fake_toss::FakeToss;
use super::tests::{donations, order, owned_broadcast, router_with, send};
use crate::auth::personas::Persona;

/// 가짜 토스를 쓰는 후원 라우터, A의 방송, 그 방송에 후원하는 B.
struct Setup {
    router: Router,
    toss: FakeToss,
    broadcast: i64,
    owner: Persona,
    donor: Persona,
}

impl Setup {
    async fn new(pool: &PgPool) -> Self {
        let toss = FakeToss::start().await;
        let router = router_with(pool, Some(toss.config()));
        let owner = Persona::user_a(pool).await;
        let donor = Persona::user_b(pool).await;
        let broadcast = owned_broadcast(pool, &owner).await;
        Self {
            router,
            toss,
            broadcast,
            owner,
            donor,
        }
    }

    /// B가 주문서 API로 5,000원 결제 대기 주문서를 만들고 주문 번호를 돌려준다.
    async fn order(&self) -> String {
        let (status, created) = order(
            &self.router,
            &self.donor,
            self.broadcast,
            json!({ "amount": 5000 }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        created["order_id"].as_str().unwrap().to_string()
    }

    /// `persona`로 실패 API를 부른다.
    async fn fail(&self, persona: &Persona, body: Value) -> (StatusCode, Value) {
        let request = Request::post("/api/donations/fail")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        send(&self.router, persona.sign(request)).await
    }

    /// 결과 화면처럼 승인 확인을 부르고 응답을 돌려준다.
    async fn confirm(&self, order_id: &str) -> Value {
        let request = Request::post("/api/donations/confirm")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "payment_key": "pay_1", "order_id": order_id, "amount": 5000 }).to_string(),
            ))
            .unwrap();
        let (status, json) = send(&self.router, request).await;
        assert_eq!(status, StatusCode::OK, "{json}");
        json
    }

    async fn summary(&self) -> Value {
        donations(&self.router, self.broadcast).await.1
    }
}

#[sqlx::test]
async fn 후원자가_취소_코드로_부르면_취소가_되고_합계는_그대로다(
    pool: PgPool,
) {
    let s = Setup::new(&pool).await;
    let order_id = s.order().await;

    let (status, json) = s
        .fail(
            &s.donor,
            json!({ "order_id": order_id, "code": "PAY_PROCESS_CANCELED", "message": "사용자에 의해 결제가 취소되었습니다." }),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "canceled");
    assert_eq!(json["broadcast_id"], s.broadcast);
    let after = s.confirm(&order_id).await;
    assert_eq!(after["status"], "canceled");
    assert_eq!(after["code"], "PAY_PROCESS_CANCELED");
    assert!(
        s.toss.requests().is_empty(),
        "취소된 주문서는 토스에 묻지 않는다"
    );
    assert_eq!(s.summary().await["count"], 0);
}

/// 지금부터 `minutes`분 뒤.
fn minutes_later(minutes: i64) -> DateTime<Utc> {
    Utc::now() + TimeDelta::minutes(minutes)
}

#[sqlx::test]
async fn 정리하면_30분이_지난_결제_대기는_만료되고_토스에_묻지_않는다(
    pool: PgPool,
) {
    let s = Setup::new(&pool).await;
    let order_id = s.order().await;

    expiry::expire_once(&pool, minutes_later(31)).await.unwrap();

    let after = s.confirm(&order_id).await;
    assert_eq!(after["status"], "expired");
    assert_eq!(after["message"], "시간이 지나 결제가 취소되었어요.");
    assert!(s.toss.requests().is_empty());
    assert_eq!(s.summary().await["count"], 0);
}

#[sqlx::test]
async fn 정리해도_30분_전의_결제_대기는_그대로다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order().await;

    expiry::expire_once(&pool, minutes_later(29)).await.unwrap();

    assert_eq!(s.confirm(&order_id).await["status"], "confirmed");
}

#[sqlx::test]
async fn 정리는_확정이나_취소된_주문서를_바꾸지_않는다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let confirmed = s.order().await;
    s.confirm(&confirmed).await;
    let canceled = s.order().await;
    s.fail(
        &s.donor,
        json!({ "order_id": canceled, "code": "USER_CANCEL" }),
    )
    .await;

    expiry::expire_once(&pool, minutes_later(60)).await.unwrap();

    assert_eq!(s.confirm(&confirmed).await["status"], "confirmed");
    assert_eq!(s.confirm(&canceled).await["status"], "canceled");
    assert_eq!(s.summary().await["total_amount"], 5000);
}

/// 주문서를 `minutes`분 전에 만든 것으로 되돌린다. 승인 확인은 진짜 지금 시각으로 판단하므로,
/// 시간이 흐른 상황은 만든 시각을 옮겨서 꾸민다.
async fn backdate(pool: &PgPool, order_id: &str, minutes: i64) {
    sqlx::query("UPDATE donation_orders SET created_at = $2 WHERE order_id = $1")
        .bind(order_id)
        .bind(Utc::now() - TimeDelta::minutes(minutes))
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test]
async fn 만료된_주문서로_확인하면_확정되지_않고_토스에_승인을_요청하지_않는다(
    pool: PgPool,
) {
    let s = Setup::new(&pool).await;
    let order_id = s.order().await;
    backdate(&pool, &order_id, 31).await;

    // 정리가 아직 돌지 않았어도 승인 확인이 직접 만료로 바꾼다.
    let json = s.confirm(&order_id).await;

    assert_eq!(json["status"], "expired");
    assert_eq!(json["message"], "시간이 지나 결제가 취소되었어요.");
    assert_eq!(json["broadcast_id"], s.broadcast);
    assert!(s.toss.confirm_requests().is_empty());
    assert_eq!(s.summary().await["count"], 0);
}

#[sqlx::test]
async fn 만들고_29분이면_아직_확정할_수_있다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order().await;
    backdate(&pool, &order_id, 29).await;

    assert_eq!(s.confirm(&order_id).await["status"], "confirmed");
}

#[sqlx::test]
async fn 만료된_주문서는_실패_보고로_바뀌지_않는다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order().await;
    expiry::expire_once(&pool, minutes_later(31)).await.unwrap();

    let (_, json) = s
        .fail(
            &s.donor,
            json!({ "order_id": order_id, "code": "USER_CANCEL" }),
        )
        .await;

    assert_eq!(json["status"], "expired");
}

#[sqlx::test]
async fn 후원자가_다른_코드로_부르면_실패가_되고_이유가_남는다(
    pool: PgPool,
) {
    let s = Setup::new(&pool).await;
    let order_id = s.order().await;

    let (status, json) = s
        .fail(
            &s.donor,
            json!({ "order_id": order_id, "code": "REJECT_CARD_COMPANY", "message": "카드사에서 거절했습니다." }),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "failed");
    assert_eq!(json["message"], "카드사에서 거절했습니다.");
    let after = s.confirm(&order_id).await;
    assert_eq!(after["status"], "failed");
    assert_eq!(after["code"], "REJECT_CARD_COMPANY");
    assert_eq!(s.summary().await["count"], 0);
}

#[sqlx::test]
async fn sdk의_user_cancel도_취소다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order().await;

    let (_, json) = s
        .fail(
            &s.donor,
            json!({ "order_id": order_id, "code": "USER_CANCEL" }),
        )
        .await;

    assert_eq!(json["status"], "canceled");
    assert_eq!(json["message"], "결제를 취소했습니다.");
}

#[sqlx::test]
async fn 다른_사용자가_부르면_204이고_주문서는_그대로다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order().await;

    let (status, _) = s
        .fail(
            &s.owner,
            json!({ "order_id": order_id, "code": "PAY_PROCESS_CANCELED", "message": "취소" }),
        )
        .await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    let after = s.confirm(&order_id).await;
    assert_eq!(
        after["status"], "confirmed",
        "결제 대기로 남아 있어 확정할 수 있다"
    );
}

#[sqlx::test]
async fn 비로그인이_부르면_401이고_주문서는_그대로다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order().await;

    let (status, json) = s
        .fail(
            &Persona::anonymous(),
            json!({ "order_id": order_id, "code": "REJECT_CARD_COMPANY", "message": "거절" }),
        )
        .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED, "{json}");
    assert_eq!(s.confirm(&order_id).await["status"], "confirmed");
}

#[sqlx::test]
async fn 주문_번호가_없거나_없는_주문서면_204다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order().await;

    let (missing, _) = s
        .fail(
            &s.donor,
            json!({ "code": "PAY_PROCESS_CANCELED", "message": "취소" }),
        )
        .await;
    let (unknown, _) = s
        .fail(
            &s.donor,
            json!({ "order_id": "don_nope", "code": "PAY_PROCESS_CANCELED", "message": "취소" }),
        )
        .await;

    assert_eq!(missing, StatusCode::NO_CONTENT);
    assert_eq!(unknown, StatusCode::NO_CONTENT);
    assert_eq!(s.confirm(&order_id).await["status"], "confirmed");
}

#[sqlx::test]
async fn 확정된_주문서는_실패_보고로_바뀌지_않는다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order().await;
    assert_eq!(s.confirm(&order_id).await["status"], "confirmed");

    let (status, json) = s
        .fail(
            &s.donor,
            json!({ "order_id": order_id, "code": "PAY_PROCESS_CANCELED", "message": "취소" }),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "confirmed");
    assert_eq!(s.summary().await["total_amount"], 5000);
}

#[sqlx::test]
async fn 취소된_주문서는_다시_실패로_바뀌지_않는다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order().await;
    s.fail(
        &s.donor,
        json!({ "order_id": order_id, "code": "USER_CANCEL" }),
    )
    .await;

    let (_, json) = s
        .fail(
            &s.donor,
            json!({ "order_id": order_id, "code": "REJECT_CARD_COMPANY", "message": "거절" }),
        )
        .await;

    assert_eq!(json["status"], "canceled");
    assert_eq!(json["code"], "USER_CANCEL");
}

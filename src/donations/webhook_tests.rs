//! 승인 통지 테스트: `POST /api/payments/toss-webhook`과 중복 방지.
//!
//! 실제 PostgreSQL(`#[sqlx::test]` 임시 DB)과 가짜 토스 서버([`FakeToss`])를 쓴다.
//! 통지는 토스처럼 쿠키·`Origin` 없이 보낸다. 결과는 응답, 합계·내역, 확인 API가 돌려주는
//! 주문서 상태, 가짜 토스가 받은 요청으로 판단한다.

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;

use super::confirm_tests::{confirm, Setup};
use super::fake_toss::Reply;
use super::tests::send;

/// 토스가 보내는 모양의 승인 통지 본문. `data`는 토스 Payment 객체다.
fn notice(payment_key: &str, order_id: &str, status: &str, amount: i64) -> Value {
    json!({
        "eventType": "PAYMENT_STATUS_CHANGED",
        "createdAt": "2026-11-22T10:00:00.000000",
        "data": {
            "paymentKey": payment_key,
            "orderId": order_id,
            "status": status,
            "totalAmount": amount,
            "method": "카드",
        }
    })
}

/// 토스처럼 쿠키·`Origin` 없이 통지를 보낸다.
async fn webhook(router: &Router, body: &str) -> (StatusCode, Value) {
    let request = Request::post("/api/payments/toss-webhook")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    send(router, request).await
}

#[sqlx::test]
async fn 같은_승인_통지를_두_번_보내면_한_번만_확정된다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, Some("응원해요")).await;
    s.toss
        .add_payment(Reply::payment("DONE", "pay_1", &order_id, 5000));
    let body = notice("pay_1", &order_id, "DONE", 5000).to_string();

    let first = webhook(&s.router, &body).await;
    let second = webhook(&s.router, &body).await;

    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(second.0, StatusCode::OK);
    let summary = s.summary().await;
    assert_eq!(summary["count"], 1);
    assert_eq!(summary["total_amount"], 5000);
    assert_eq!(summary["items"][0]["message"], "응원해요");
    let lookups = s.toss.lookup_requests();
    assert_eq!(lookups[0].path, "/v1/payments/pay_1", "결제 키로 조회한다");
    assert_eq!(
        lookups[0].header("authorization"),
        Some("Basic dGVzdF9za19mYWtlOg==")
    );
    assert!(
        s.toss.confirm_requests().is_empty(),
        "통지는 승인하지 않는다"
    );
}

#[sqlx::test]
async fn 통지가_먼저_오고_확인이_나중에_와도_후원은_하나다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    s.toss
        .add_payment(Reply::payment("DONE", "pay_1", &order_id, 5000));

    webhook(
        &s.router,
        &notice("pay_1", &order_id, "DONE", 5000).to_string(),
    )
    .await;
    let (status, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;

    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "confirmed");
    assert!(
        s.toss.confirm_requests().is_empty(),
        "이미 확정이면 결과 화면은 승인을 다시 요청하지 않는다"
    );
    let summary = s.summary().await;
    assert_eq!(summary["count"], 1);
    assert_eq!(summary["total_amount"], 5000);
}

#[sqlx::test]
async fn 확인이_먼저_오고_통지가_나중에_와도_후원은_하나다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    s.toss
        .add_payment(Reply::payment("DONE", "pay_1", &order_id, 5000));

    let (_, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;
    let (status, _) = webhook(
        &s.router,
        &notice("pay_1", &order_id, "DONE", 5000).to_string(),
    )
    .await;

    assert_eq!(json["status"], "confirmed");
    assert_eq!(status, StatusCode::OK);
    let summary = s.summary().await;
    assert_eq!(summary["count"], 1);
    assert_eq!(summary["total_amount"], 5000);
}

#[sqlx::test]
async fn 통지와_확인이_동시에_와도_후원은_하나다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    s.toss
        .add_payment(Reply::payment("DONE", "pay_1", &order_id, 5000));
    let body = notice("pay_1", &order_id, "DONE", 5000).to_string();

    let (hook, (status, json)) = tokio::join!(
        webhook(&s.router, &body),
        confirm(&s.router, "pay_1", &order_id, 5000),
    );

    assert_eq!(hook.0, StatusCode::OK);
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "confirmed");
    let summary = s.summary().await;
    assert_eq!(summary["count"], 1);
    assert_eq!(summary["total_amount"], 5000);
}

#[sqlx::test]
async fn 같은_통지가_동시에_여러_번_와도_후원은_하나다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    s.toss
        .add_payment(Reply::payment("DONE", "pay_1", &order_id, 5000));
    let body = notice("pay_1", &order_id, "DONE", 5000).to_string();

    let (first, second, third) = tokio::join!(
        webhook(&s.router, &body),
        webhook(&s.router, &body),
        webhook(&s.router, &body),
    );

    for (status, _) in [first, second, third] {
        assert_eq!(status, StatusCode::OK);
    }
    let summary = s.summary().await;
    assert_eq!(summary["count"], 1);
    assert_eq!(summary["total_amount"], 5000);
}

#[sqlx::test]
async fn 본문의_금액을_부풀려도_조회한_금액으로만_판단한다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    s.toss
        .add_payment(Reply::payment("DONE", "pay_1", &order_id, 5000));

    let inflated = notice("pay_1", &order_id, "DONE", 1_000_000).to_string();
    let (status, _) = webhook(&s.router, &inflated).await;

    assert_eq!(status, StatusCode::OK);
    let summary = s.summary().await;
    assert_eq!(summary["count"], 1);
    assert_eq!(summary["total_amount"], 5000);
}

#[sqlx::test]
async fn 조회한_금액이_주문서와_다르면_확정되지_않는다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    // 본문은 주문서 금액이지만, 토스에 물어보면 다른 금액이다.
    s.toss
        .add_payment(Reply::payment("DONE", "pay_1", &order_id, 1000));

    let (status, _) = webhook(
        &s.router,
        &notice("pay_1", &order_id, "DONE", 5000).to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(s.summary().await["count"], 0);
    let (_, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;
    assert_eq!(json["status"], "failed");
    assert_eq!(json["code"], "payment_mismatch");
}

#[sqlx::test]
async fn 모르는_결제_키는_200이고_아무것도_생기지_않는다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;

    // 가짜 토스에 없는 결제 키. 본문은 그럴듯하게 꾸몄다.
    let (status, _) = webhook(
        &s.router,
        &notice("pay_made_up", &order_id, "DONE", 5000).to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(s.summary().await["count"], 0);
    assert_eq!(s.toss.lookup_requests().len(), 1);
    // 주문서는 결제 대기 그대로라 결과 화면이 나중에 확정할 수 있다.
    let (_, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;
    assert_eq!(json["status"], "confirmed");
}

#[sqlx::test]
async fn 우리_주문서가_아닌_결제는_200이고_아무것도_생기지_않는다(
    pool: PgPool,
) {
    let s = Setup::new(&pool).await;
    s.toss.add_payment(Reply::payment(
        "DONE",
        "pay_other",
        "someone_else_order",
        5000,
    ));

    let (status, _) = webhook(
        &s.router,
        &notice("pay_other", "someone_else_order", "DONE", 5000).to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(s.summary().await["count"], 0);
}

#[sqlx::test]
async fn 형식이_틀린_본문은_200이고_토스에_묻지도_않는다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    s.order(5000, None).await;

    for body in [
        "",
        "not json",
        "{}",
        r#"{"eventType":"PAYMENT_STATUS_CHANGED","data":{}}"#,
        r#"{"data":{"paymentKey":5000}}"#,
        r#"{"data":{"paymentKey":"../confirm"}}"#,
    ] {
        let (status, _) = webhook(&s.router, body).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    assert!(s.toss.requests().is_empty(), "{:?}", s.toss.requests());
    assert_eq!(s.summary().await["count"], 0);
}

#[sqlx::test]
async fn 조회_결과가_취소_중단_만료면_주문서가_그에_맞게_바뀐다(
    pool: PgPool,
) {
    let s = Setup::new(&pool).await;
    for (toss_status, expected) in [
        ("CANCELED", "canceled"),
        ("ABORTED", "failed"),
        ("EXPIRED", "expired"),
    ] {
        let order_id = s.order(5000, None).await;
        let payment_key = format!("pay_{toss_status}");
        s.toss
            .add_payment(Reply::payment(toss_status, &payment_key, &order_id, 5000));

        // 본문은 DONE이라고 꾸며도 조회 결과를 따른다.
        let (status, _) = webhook(
            &s.router,
            &notice(&payment_key, &order_id, "DONE", 5000).to_string(),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let (_, json) = confirm(&s.router, &payment_key, &order_id, 5000).await;
        assert_eq!(json["status"], expected, "{toss_status}: {json}");
        assert_eq!(json["code"], toss_status);
    }
    assert!(s.toss.confirm_requests().is_empty());
    assert_eq!(s.summary().await["count"], 0);
}

#[sqlx::test]
async fn 아직_끝나지_않은_결제면_결제_대기로_둔다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    s.toss
        .add_payment(Reply::payment("IN_PROGRESS", "pay_1", &order_id, 5000));

    let (status, _) = webhook(
        &s.router,
        &notice("pay_1", &order_id, "DONE", 5000).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(s.summary().await["count"], 0);

    // 결제가 끝나고 통지가 다시 오면 확정된다.
    s.toss
        .add_payment(Reply::payment("DONE", "pay_1", &order_id, 5000));
    webhook(
        &s.router,
        &notice("pay_1", &order_id, "DONE", 5000).to_string(),
    )
    .await;
    assert_eq!(s.summary().await["count"], 1);
}

#[sqlx::test]
async fn 토스_키가_없어도_통지는_200이다(pool: PgPool) {
    let router = super::tests::router_with(&pool, None);

    let (status, _) = webhook(&router, &notice("pay_1", "don_x", "DONE", 5000).to_string()).await;

    assert_eq!(status, StatusCode::OK);
}

#[sqlx::test]
async fn origin_없는_통지는_csrf_확인에_막히지_않는다(pool: PgPool) {
    // `Setup`의 라우터는 서버처럼 `Origin` 확인을 씌웠다.
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    s.toss
        .add_payment(Reply::payment("DONE", "pay_1", &order_id, 5000));
    let request = Request::post("/api/payments/toss-webhook")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            notice("pay_1", &order_id, "DONE", 5000).to_string(),
        ))
        .unwrap();
    assert!(request.headers().get(header::ORIGIN).is_none());
    assert!(request.headers().get(header::COOKIE).is_none());

    let (status, _) = send(&s.router, request).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(s.summary().await["count"], 1);
}

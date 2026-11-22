//! 승인 확인 테스트: `POST /api/donations/confirm`과 그 뒤의 합계·내역.
//!
//! 실제 PostgreSQL(`#[sqlx::test]` 임시 DB)과 가짜 토스 서버([`FakeToss`])를 쓴다.
//! 주문서는 주문서 API로 만들고, 결과는 확인 응답·합계·내역·가짜 토스가 받은 요청으로 판단한다.

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;

use super::fake_toss::{FakeToss, Reply};
use super::tests::{donations, order, owned_broadcast, router_with, send};
use crate::auth::personas::Persona;

/// 가짜 토스를 쓰는 후원 라우터, A의 방송, 그 방송에 후원하는 B.
pub(super) struct Setup {
    pub(super) router: Router,
    pub(super) toss: FakeToss,
    pub(super) broadcast: i64,
    donor: Persona,
}

impl Setup {
    pub(super) async fn new(pool: &PgPool) -> Self {
        Self::with_fail_code(pool, None).await
    }

    /// `TOSS_TEST_FAIL_CODE`를 넣은 서버.
    async fn with_fail_code(pool: &PgPool, fail_code: Option<&str>) -> Self {
        let toss = FakeToss::start().await;
        let router = router_with(pool, Some(toss.config_with_fail_code(fail_code)));
        let a = Persona::user_a(pool).await;
        let donor = Persona::user_b(pool).await;
        let broadcast = owned_broadcast(pool, &a).await;
        Self {
            router,
            toss,
            broadcast,
            donor,
        }
    }

    /// 주문서 API로 결제 대기 주문서를 만들고 주문 번호를 돌려준다.
    pub(super) async fn order(&self, amount: i64, message: Option<&str>) -> String {
        let (status, created) = order(
            &self.router,
            &self.donor,
            self.broadcast,
            json!({ "amount": amount, "message": message }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        created["order_id"].as_str().unwrap().to_string()
    }

    pub(super) async fn summary(&self) -> Value {
        donations(&self.router, self.broadcast).await.1
    }
}

/// 결과 화면처럼 비로그인으로 확인 API를 부른다.
pub(super) async fn confirm(
    router: &Router,
    payment_key: &str,
    order_id: &str,
    amount: i64,
) -> (StatusCode, Value) {
    let request = Request::post("/api/donations/confirm")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "payment_key": payment_key, "order_id": order_id, "amount": amount })
                .to_string(),
        ))
        .unwrap();
    send(router, request).await
}

#[sqlx::test]
async fn 토스가_done으로_답하면_확정되고_합계와_내역에_나온다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, Some("응원해요")).await;

    let (status, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;

    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "confirmed");
    assert_eq!(json["broadcast_id"], s.broadcast);
    assert_eq!(json["amount"], 5000);
    let summary = s.summary().await;
    assert_eq!(summary["total_amount"], 5000);
    assert_eq!(summary["count"], 1);
    assert_eq!(summary["items"][0]["donor"], "user_b");
    assert_eq!(summary["items"][0]["amount"], 5000);
    assert_eq!(summary["items"][0]["message"], "응원해요");
    assert_eq!(s.toss.confirm_requests().len(), 1);
}

#[sqlx::test]
async fn 돌아온_금액이_다르면_토스를_부르지_않고_실패가_된다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(100_000, None).await;

    let (status, json) = confirm(&s.router, "pay_1", &order_id, 1000).await;

    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "failed");
    assert_eq!(json["code"], "amount_mismatch");
    assert_eq!(json["amount"], 100_000, "주문서 금액을 알려 준다");
    assert!(
        s.toss.requests().is_empty(),
        "토스에 아무 요청도 가지 않는다"
    );
    let summary = s.summary().await;
    assert_eq!(summary["count"], 0);

    // 실패는 되돌아가지 않는다. 맞는 금액으로 다시 보내도 실패 그대로다.
    let (_, again) = confirm(&s.router, "pay_1", &order_id, 100_000).await;
    assert_eq!(again, json);
    assert!(s.toss.requests().is_empty());
}

#[sqlx::test]
async fn 토스_응답_금액이나_주문_번호가_다르면_확정되지_않는다(
    pool: PgPool,
) {
    let s = Setup::new(&pool).await;
    for (reply_amount, reply_order) in [(1000, None), (5000, Some("don_someone_else"))] {
        let order_id = s.order(5000, None).await;
        let reply_order = reply_order.unwrap_or(&order_id);
        s.toss
            .on_confirm(Reply::payment("DONE", "pay_1", reply_order, reply_amount));

        let (status, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;

        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["status"], "failed", "{json}");
        assert_eq!(json["code"], "payment_mismatch");
        let summary = s.summary().await;
        assert_eq!(summary["count"], 0);
    }
}

#[sqlx::test]
async fn 토스_응답이_done이_아니면_확정되지_않는다(pool: PgPool) {
    // 아직 끝나지 않은 상태: 주문서는 결제 대기로 남고, 다시 확인할 수 있다.
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    s.toss
        .on_confirm(Reply::payment("IN_PROGRESS", "pay_1", &order_id, 5000));
    let (status, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{json}");
    assert_eq!(json["code"], "payment_unknown");
    let summary = s.summary().await;
    assert_eq!(summary["count"], 0);

    // 끝난 상태(중단)면 실패가 된다.
    let order_id = s.order(5000, None).await;
    s.toss
        .on_confirm(Reply::payment("ABORTED", "pay_2", &order_id, 5000));
    let (status, json) = confirm(&s.router, "pay_2", &order_id, 5000).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "failed");
    let summary = s.summary().await;
    assert_eq!(summary["count"], 0);
}

#[sqlx::test]
async fn 거절_오류는_주문_번호로_다시_조회한_뒤_실패가_된다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    s.toss.on_confirm(Reply::error(
        403,
        "REJECT_CARD_COMPANY",
        "카드사에서 거절했어요.",
    ));

    let (status, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;

    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "failed");
    assert_eq!(json["code"], "REJECT_CARD_COMPANY");
    assert_eq!(json["message"], "카드사에서 거절했어요.");
    let lookups = s.toss.lookup_requests();
    assert_eq!(lookups.len(), 1, "승인 오류 뒤 한 번 다시 조회한다");
    assert_eq!(lookups[0].path, format!("/v1/payments/orders/{order_id}"));
    assert_eq!(s.summary().await["count"], 0);
}

#[sqlx::test]
async fn 이미_처리됨_오류에_조회_결과가_done이면_확정된다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    s.toss.on_confirm(Reply::error(
        400,
        "ALREADY_PROCESSED_PAYMENT",
        "이미 처리된 결제 입니다.",
    ));
    s.toss
        .on_lookup(Reply::payment("DONE", "pay_1", &order_id, 5000));

    let (status, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;

    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "confirmed");
    assert_eq!(s.summary().await["total_amount"], 5000);
}

#[sqlx::test]
async fn 토스_내부_오류면_결제_대기로_남고_다시_확인할_수_있다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    s.toss.on_confirm(Reply::error(
        500,
        "FAILED_INTERNAL_SYSTEM_PROCESSING",
        "내부 오류",
    ));

    let (status, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{json}");
    assert_eq!(json["code"], "payment_unknown");

    // 토스가 돌아오면 같은 요청으로 확정된다.
    s.toss
        .on_confirm(Reply::payment("DONE", "pay_1", &order_id, 5000));
    let (status, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "confirmed");
}

#[sqlx::test]
async fn 승인_요청에_basic_인증과_멱등키가_붙는다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;

    confirm(&s.router, "pay_1", &order_id, 5000).await;

    let requests = s.toss.confirm_requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    // base64("test_sk_fake:")
    assert_eq!(
        request.header("authorization"),
        Some("Basic dGVzdF9za19mYWtlOg==")
    );
    assert_eq!(request.header("idempotency-key"), Some(order_id.as_str()));
    assert_eq!(request.header("tosspayments-test-code"), None);
    assert_eq!(
        request.body,
        json!({ "paymentKey": "pay_1", "orderId": order_id, "amount": 5000 })
    );
}

#[sqlx::test]
async fn 실패_코드를_정하면_승인_요청에_테스트_헤더가_붙는다(pool: PgPool) {
    let s = Setup::with_fail_code(&pool, Some("REJECT_CARD_PAYMENT")).await;
    let order_id = s.order(5000, None).await;

    confirm(&s.router, "pay_1", &order_id, 5000).await;

    let requests = s.toss.confirm_requests();
    assert_eq!(
        requests[0].header("tosspayments-test-code"),
        Some("REJECT_CARD_PAYMENT")
    );
}

#[sqlx::test]
async fn 같은_확인을_두_번_보내면_한_번만_확정되고_같은_결과다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;

    let first = confirm(&s.router, "pay_1", &order_id, 5000).await;
    let second = confirm(&s.router, "pay_1", &order_id, 5000).await;

    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(first, second);
    assert_eq!(
        s.toss.confirm_requests().len(),
        1,
        "이미 확정이면 토스를 다시 부르지 않는다"
    );
    let summary = s.summary().await;
    assert_eq!(summary["count"], 1);
    assert_eq!(summary["total_amount"], 5000);
}

#[sqlx::test]
async fn 같은_확인을_동시에_보내도_한_번만_확정된다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;

    let results = futures_join(
        (0..5)
            .map(|_| {
                let router = s.router.clone();
                let order_id = order_id.clone();
                async move { confirm(&router, "pay_1", &order_id, 5000).await }
            })
            .collect(),
    )
    .await;

    for result in &results {
        assert_eq!(result, &results[0]);
    }
    assert_eq!(results[0].1["status"], "confirmed");
    let summary = s.summary().await;
    assert_eq!(summary["count"], 1);
    assert_eq!(summary["total_amount"], 5000);
}

#[sqlx::test]
async fn 같은_db에_새로_연결해도_확정된_후원이_그대로다(pool: PgPool) {
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, Some("다시 봐요")).await;
    confirm(&s.router, "pay_1", &order_id, 5000).await;

    let reconnected = PgPool::connect_with(pool.connect_options().as_ref().clone())
        .await
        .unwrap();
    let router = router_with(&reconnected, Some(s.toss.config()));
    let (_, summary) = donations(&router, s.broadcast).await;

    assert_eq!(summary["count"], 1);
    assert_eq!(summary["items"][0]["message"], "다시 봐요");
    let (_, again) = confirm(&router, "pay_1", &order_id, 5000).await;
    assert_eq!(again["status"], "confirmed");
    assert_eq!(s.toss.confirm_requests().len(), 1);
}

#[sqlx::test]
async fn 없는_주문서는_404이고_토스를_부르지_않는다(pool: PgPool) {
    let s = Setup::new(&pool).await;

    let (status, json) = confirm(&s.router, "pay_1", "don_nothing_here", 5000).await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    assert!(s.toss.requests().is_empty());
}

/// 여러 요청을 한꺼번에 띄워 모두 끝나길 기다린다.
async fn futures_join<F: std::future::Future<Output = (StatusCode, Value)> + Send + 'static>(
    futures: Vec<F>,
) -> Vec<(StatusCode, Value)> {
    let handles: Vec<_> = futures.into_iter().map(tokio::spawn).collect();
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.unwrap());
    }
    results
}

#[sqlx::test]
async fn 이미_처리됨_오류에_조회가_실패하면_결제_대기로_남는다(
    pool: PgPool,
) {
    // 토스가 "이미 처리됨"이라고 했는데 조회가 내부 오류면, 돈이 나갔을 수 있다. 실패로 단정하지 않는다.
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    s.toss.on_confirm(Reply::error(
        400,
        "ALREADY_PROCESSED_PAYMENT",
        "이미 처리된 결제 입니다.",
    ));
    s.toss.on_lookup(Reply::error(
        500,
        "FAILED_INTERNAL_SYSTEM_PROCESSING",
        "내부 오류",
    ));

    let (status, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;

    assert_eq!(status, StatusCode::BAD_GATEWAY, "{json}");
    assert_eq!(json["code"], "payment_unknown");
    assert_eq!(s.summary().await["count"], 0);

    // 토스가 돌아와 DONE을 알려 주면 확정된다.
    s.toss
        .on_lookup(Reply::payment("DONE", "pay_1", &order_id, 5000));
    let (status, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "confirmed");
    assert_eq!(s.summary().await["count"], 1);
}

#[sqlx::test]
async fn 처리_중_오류에_조회도_실패하면_결제_대기로_남는다(pool: PgPool) {
    // 409 IDEMPOTENT_REQUEST_PROCESSING: 앞선 승인 요청이 아직 처리 중이다.
    let s = Setup::new(&pool).await;
    let order_id = s.order(5000, None).await;
    s.toss.on_confirm(Reply::error(
        409,
        "IDEMPOTENT_REQUEST_PROCESSING",
        "이전 요청이 처리 중입니다.",
    ));
    for lookup in [
        Reply::error(503, "FAILED_INTERNAL_SYSTEM_PROCESSING", "내부 오류"),
        Reply::error(429, "TOO_MANY_REQUESTS", "요청이 많아요"),
        Reply::error(404, "NOT_FOUND_PAYMENT", "존재하지 않는 결제 정보 입니다."),
    ] {
        s.toss.on_lookup(lookup);
        let (status, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{json}");
        assert_eq!(json["code"], "payment_unknown");
    }
    assert_eq!(s.summary().await["count"], 0);

    // 처리가 끝나 DONE이 되면 다시 확인할 때 확정된다.
    s.toss
        .on_confirm(Reply::payment("DONE", "pay_1", &order_id, 5000));
    let (status, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "confirmed");
}

#[sqlx::test]
async fn 조회한_결제가_done이_아니면_승인_오류의_코드가_실패_이유다(
    pool: PgPool,
) {
    // TOSS_TEST_FAIL_CODE로 승인을 거절시키고, 조회하면 결제는 중단(ABORTED)이다.
    let s = Setup::with_fail_code(&pool, Some("REJECT_CARD_PAYMENT")).await;
    let order_id = s.order(5000, None).await;
    s.toss
        .add_payment(Reply::payment("ABORTED", "pay_1", &order_id, 5000));

    let (status, json) = confirm(&s.router, "pay_1", &order_id, 5000).await;

    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["status"], "failed");
    assert_eq!(json["code"], "REJECT_CARD_PAYMENT");
    assert_eq!(s.toss.lookup_requests().len(), 1);
}

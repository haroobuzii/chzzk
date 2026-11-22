//! 승인 통지(웹훅): `POST /api/payments/toss-webhook`.
//!
//! 토스가 결제 상태가 바뀔 때 보내는 통지 `{eventType, createdAt, data: Payment}`를 받는다.
//! 본문은 누구나 꾸며 보낼 수 있으므로 결제 연결([`toss::read_notice`])로 결제 키만 읽고, 토스에 다시
//! 조회한 결제를 승인 확인([`confirm::settle`])에 넘긴다. 주문서가 결제 대기가 아니어도 넘긴다:
//! 승인을 마친 결제면 만료·실패·취소된 주문서도 확정되고, 확정된 주문서는 그대로다.
//! 그래서 결과 화면과 통지가 몇 번, 어떤 순서로 와도 한 번만 확정된다.
//!
//! 토스는 10초 안에 200을 받지 못하면 다시 보낸다(최대 7번). 그래서 언제나 200을 준다.
//! 모르는 결제 키, 꾸며낸 통지, 틀린 본문도 200이고 아무것도 만들지 않는다.

use std::time::Duration;

use axum::{body::Bytes, extract::State, http::StatusCode};
use sqlx::PgPool;

use super::confirm::{self, ConfirmError};
use super::store;
use super::toss::{self, TossConfig};
use super::DonationsState;

/// 이 시간 안에 끝나지 않으면 처리는 뒤에서 이어 가고 먼저 200을 준다. 토스는 10초를 기다린다.
const ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// `POST /api/payments/toss-webhook`: 토스가 부른다. 쿠키·`Origin`이 없는 서버 간 요청이다.
/// 결과와 상관없이 언제나 200이다.
pub(super) async fn toss_webhook(State(state): State<DonationsState>, body: Bytes) -> StatusCode {
    let Some(payment_key) = toss::read_notice(&body) else {
        log::warn!("승인 통지: 결제 키가 없는 본문이라 무시해요");
        return StatusCode::OK;
    };
    let Some(toss) = state.toss else {
        log::warn!("승인 통지: 토스 키가 없어 무시해요");
        return StatusCode::OK;
    };
    let pool = state.pool;
    let work = tokio::spawn(async move { handle(&pool, &toss, &payment_key).await });
    match tokio::time::timeout(ANSWER_WITHIN, work).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log::error!("승인 통지 처리가 멈췄어요: {e}"),
        Err(_) => log::warn!("승인 통지 처리가 오래 걸려 먼저 200을 줘요"),
    }
    StatusCode::OK
}

/// 결제 키로 토스에 조회하고, 그 결제의 주문서와 함께 승인 확인에 넘긴다.
async fn handle(pool: &PgPool, toss: &TossConfig, payment_key: &str) {
    let found = match toss.lookup(payment_key).await {
        Ok(found) => found,
        Err(reason) => {
            log::warn!("승인 통지: 결제를 조회하지 못해 무시해요: {reason}");
            return;
        }
    };
    let order = match store::find_by_order_id(pool, &found.order_id).await {
        Ok(Some(order)) => order,
        Ok(None) => {
            log::warn!("승인 통지: 우리 주문서가 아닌 결제라 무시해요");
            return;
        }
        Err(e) => {
            log::error!("승인 통지: 주문서를 읽지 못했어요: {e}");
            return;
        }
    };
    match confirm::settle(pool, &order, found.approval).await {
        Ok(_) => {}
        Err(ConfirmError::Unknown(reason)) => {
            log::info!("승인 통지: 아직 끝나지 않은 결제라 결제 대기로 둬요: {reason}")
        }
        Err(ConfirmError::NotFound) => {}
        Err(ConfirmError::Db(e)) => log::error!("승인 통지: 주문서를 바꾸지 못했어요: {e}"),
    }
}

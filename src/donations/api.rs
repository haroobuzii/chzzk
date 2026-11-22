//! 후원 HTTP 라우트. 주문서 만들기는 로그인한 사용자, 합계·내역은 누구나.
//! 권한 검사는 [`CurrentUser`]를 거친다.

use axum::{
    extract::{rejection::JsonRejection, FromRef, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use sqlx::PgPool;

use super::rules::{format_won, validate_amount, validate_message, MessageTooLong};
use super::store::{self, NewOrder};
use super::toss::TossConfig;
use super::{CreatedOrder, DonationSummary};
use crate::auth::CurrentUser;
use crate::number::parse_number;
use crate::routes::{self, bad_request, failure, not_found};

/// 후원 라우트가 함께 쓰는 것.
#[derive(Clone)]
pub struct DonationsState {
    pub pool: PgPool,
    /// 토스 설정. 키가 없으면 `None`이고 주문서 만들기는 503이다.
    pub toss: Option<TossConfig>,
}

/// 로그인 확인([`CurrentUser`])이 DB를 찾을 수 있게 한다.
impl FromRef<DonationsState> for PgPool {
    fn from_ref(state: &DonationsState) -> Self {
        state.pool.clone()
    }
}

/// 후원 API 라우터.
pub fn api(state: DonationsState) -> Router {
    Router::new()
        .route("/api/broadcasts/{number}/donations", get(list).post(create))
        .with_state(state)
}

/// 주문서 만들기 요청 본문. 받는 사람은 받지 않는다(서버가 방송 소유자로 정한다).
#[derive(Deserialize)]
struct NewDonation {
    amount: i64,
    message: Option<String>,
}

/// `GET /api/broadcasts/{번호}/donations`: 누구나. 확정된 후원만 최신순
/// `{total_amount, count, items: [{donor, amount, message, confirmed_at}], enabled}`.
/// 없거나 보이지 않는 방송(지워진 방송 포함)은 404다.
async fn list(State(state): State<DonationsState>, Path(segment): Path<String>) -> Response {
    let Some(broadcast) = parse_number(&segment) else {
        return not_found();
    };
    match store::find_target(&state.pool, broadcast).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found(),
        Err(e) => return server_error("후원 내역 조회", e),
    }
    match store::confirmed_for_broadcast(&state.pool, broadcast).await {
        Ok(items) => Json(DonationSummary {
            total_amount: items.iter().map(|item| item.amount).sum(),
            count: items.len() as i64,
            items,
            enabled: state.toss.is_some(),
        })
        .into_response(),
        Err(e) => server_error("후원 내역 조회", e),
    }
}

/// `POST /api/broadcasts/{번호}/donations` `{amount, message?}`: 로그인 필요.
/// 결제 대기 주문서를 만들고 201과 `{order_id, amount, order_name, client_key}`를 준다.
///
/// 거절: 비로그인 401, 토스 키 없음 503, 없는 방송 404, 소유자 없는 방송 400(`no_owner`),
/// 내 방송 400(`own_broadcast`), 금액·메시지 위반 400.
async fn create(
    State(state): State<DonationsState>,
    current: CurrentUser,
    Path(segment): Path<String>,
    body: Result<Json<NewDonation>, JsonRejection>,
) -> Response {
    let user = match current.require() {
        Ok(user) => user,
        Err(response) => return response,
    };
    let Some(toss) = &state.toss else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "donations_disabled",
            "후원 기능을 쓰려면 토스 테스트 키가 필요해요.",
        );
    };
    let Some(broadcast) = parse_number(&segment) else {
        return not_found();
    };
    let Ok(Json(new)) = body else {
        return bad_request("invalid_body", "후원 금액을 숫자로 보내 주세요.");
    };
    let target = match store::find_target(&state.pool, broadcast).await {
        Ok(Some(target)) => target,
        Ok(None) => return not_found(),
        Err(e) => return server_error("주문서 만들기", e),
    };
    let Some((recipient_id, recipient)) = target.owner else {
        return bad_request("no_owner", "소유자가 없는 방송에는 후원할 수 없어요.");
    };
    if recipient_id == user.id {
        return bad_request("own_broadcast", "내 방송에는 후원할 수 없어요.");
    }
    let amount = match validate_amount(new.amount) {
        Ok(amount) => amount,
        Err(e) => return bad_request(e.code(), e.message()),
    };
    let message = match validate_message(new.message.as_deref()) {
        Ok(message) => message,
        Err(MessageTooLong) => return bad_request(MessageTooLong::CODE, MessageTooLong::MESSAGE),
    };
    let order = NewOrder {
        broadcast_id: broadcast,
        donor_id: user.id,
        recipient_id,
        amount,
        message: message.as_deref(),
    };
    match store::create(&state.pool, order).await {
        Ok(order_id) => (
            StatusCode::CREATED,
            Json(CreatedOrder {
                order_id,
                amount,
                order_name: format!("{recipient}님에게 후원 {}", format_won(amount)),
                client_key: toss.client_key.clone(),
            }),
        )
            .into_response(),
        Err(e) => server_error("주문서 만들기", e),
    }
}

fn server_error(what: &str, e: sqlx::Error) -> Response {
    routes::server_error(what, e, "요청을 처리하지 못했어요.")
}

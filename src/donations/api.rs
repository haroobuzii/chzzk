//! 후원 HTTP 라우트. 주문서 만들기와 실패 보고는 로그인한 사용자, 합계·내역과 승인 확인·승인 통지는 누구나.
//! 권한 검사는 [`CurrentUser`]를 거친다.

use axum::{
    extract::{rejection::JsonRejection, FromRef, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use sqlx::PgPool;

use super::confirm::{self, ConfirmError, Returned};
use super::rules::{format_won, validate_amount, validate_message, MessageTooLong};
use super::store::{self, NewOrder, Order, OrderStatus};
use super::toss::{self, TossConfig};
use super::{ConfirmResult, CreatedOrder, DonationSummary};
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
        .route("/api/donations/confirm", post(confirm_returned))
        .route("/api/donations/fail", post(fail_returned))
        .route(
            "/api/payments/toss-webhook",
            post(super::webhook::toss_webhook),
        )
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

/// 승인 확인 요청 본문. 결제창이 돌아온 주소의 `paymentKey`·`orderId`·`amount`다.
#[derive(Deserialize)]
struct ConfirmRequest {
    payment_key: String,
    order_id: String,
    amount: i64,
}

/// `POST /api/donations/confirm` `{payment_key, order_id, amount}`: 결과 화면이 부른다.
/// 세션이 끊겨도 결제는 확정되어야 하므로 로그인은 요구하지 않는다.
///
/// 승인 확인([`confirm::confirm`])이 끝나면 200과 `{status, broadcast_id, amount, code?, message?}`를 준다.
/// 같은 요청을 다시 보내도 같은 결과다.
///
/// 거절: 본문이 틀리면 400, 없는 주문서 404, 토스 키 없음 503,
/// 토스 결과를 아직 알 수 없으면 502(`payment_unknown`, 주문서는 결제 대기로 남는다).
async fn confirm_returned(
    State(state): State<DonationsState>,
    body: Result<Json<ConfirmRequest>, JsonRejection>,
) -> Response {
    let Some(toss) = &state.toss else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "donations_disabled",
            "후원 기능을 쓰려면 토스 테스트 키가 필요해요.",
        );
    };
    let Ok(Json(request)) = body else {
        return bad_request("invalid_body", "결제 결과를 확인할 값이 없어요.");
    };
    let returned = Returned {
        payment_key: &request.payment_key,
        order_id: &request.order_id,
        amount: request.amount,
    };
    match confirm::confirm(&state.pool, toss, returned).await {
        Ok(order) => Json(confirm_result(order)).into_response(),
        Err(ConfirmError::NotFound) => failure(
            StatusCode::NOT_FOUND,
            "not_found",
            "주문서를 찾을 수 없어요.",
        ),
        Err(ConfirmError::Unknown(reason)) => {
            log::warn!("승인 확인 보류: {reason}");
            failure(
                StatusCode::BAD_GATEWAY,
                "payment_unknown",
                "결제 결과를 아직 확인하지 못했어요. 잠시 뒤 새로고침해 주세요.",
            )
        }
        Err(ConfirmError::Db(e)) => server_error("승인 확인", e),
    }
}

/// 실패 보고 본문. 결제창이 `failUrl`로 돌려보낸 `code`·`message`·`orderId`다.
/// 취소면 토스가 주문 번호를 주지 않을 수 있다.
#[derive(Deserialize)]
struct FailRequest {
    order_id: Option<String>,
    code: String,
    message: Option<String>,
}

/// 남겨 둘 실패 코드·메시지의 최대 글자 수. 브라우저가 보낸 값이라 길이를 자른다.
const MAX_CODE_CHARS: usize = 64;
const MAX_MESSAGE_CHARS: usize = 200;

/// `POST /api/donations/fail` `{order_id?, code, message?}`: 실패·취소 결과 화면이 부른다. 로그인 필요.
///
/// 로그인한 후원자 본인의 결제 대기 주문서만 바꾼다. 구매자 취소 코드([`toss::is_buyer_cancel`])면 취소, 그 밖은 실패다.
/// 본인 주문서면 200과 승인 확인과 같은 `{status, broadcast_id, amount, code?, message?}`를 준다.
/// 이미 결제 대기가 아니면 바꾸지 않고 지금 상태를 준다.
/// 주문 번호가 없거나 남의 주문서·없는 주문서면 아무것도 바꾸지 않고 204다.
///
/// 거절: 비로그인 401, 본문이 틀리면 400.
async fn fail_returned(
    State(state): State<DonationsState>,
    current: CurrentUser,
    body: Result<Json<FailRequest>, JsonRejection>,
) -> Response {
    let user = match current.require() {
        Ok(user) => user,
        Err(response) => return response,
    };
    let Ok(Json(request)) = body else {
        return bad_request("invalid_body", "결제 실패 코드가 없어요.");
    };
    let Some(order_id) = request.order_id.filter(|id| !id.is_empty()) else {
        return StatusCode::NO_CONTENT.into_response();
    };
    let code: String = request.code.chars().take(MAX_CODE_CHARS).collect();
    let to = if toss::is_buyer_cancel(&code) {
        OrderStatus::Canceled
    } else {
        OrderStatus::Failed
    };
    let message: String = match request.message.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => text.chars().take(MAX_MESSAGE_CHARS).collect(),
        _ if to == OrderStatus::Canceled => "결제를 취소했습니다.".into(),
        _ => "결제가 승인되지 않았어요.".into(),
    };
    match end_own_order(&state.pool, &order_id, user.id, to, &code, &message).await {
        Ok(Some(order)) => Json(confirm_result(order)).into_response(),
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => server_error("결제 실패 기록", e),
    }
}

/// `donor_id`의 주문서면 결제 대기일 때만 `to`로 바꾸고 끝난 뒤의 주문서를 돌려준다.
/// 없거나 남의 주문서면 아무것도 바꾸지 않고 `None`이다.
async fn end_own_order(
    pool: &PgPool,
    order_id: &str,
    donor_id: i64,
    to: OrderStatus,
    code: &str,
    message: &str,
) -> sqlx::Result<Option<Order>> {
    match store::find_by_order_id(pool, order_id).await? {
        Some(order) if order.donor_id == donor_id => {
            store::mark_ended(pool, order_id, to, code, message).await?;
            store::find_by_order_id(pool, order_id).await
        }
        _ => Ok(None),
    }
}

fn confirm_result(order: Order) -> ConfirmResult {
    let confirmed = order.status == OrderStatus::Confirmed;
    ConfirmResult {
        status: order.status,
        broadcast_id: order.broadcast_id,
        amount: order.amount,
        code: if confirmed { None } else { order.failure_code },
        message: if confirmed {
            None
        } else {
            order.failure_message
        },
    }
}

fn server_error(what: &str, e: sqlx::Error) -> Response {
    routes::server_error(what, e, "요청을 처리하지 못했어요.")
}

//! 승인 확인: 결제가 정말 끝났는지 서버가 결제 회사에 직접 확인하고, 그때만 주문서를 확정한다.
//!
//! 돌아온 결과 화면([`confirm`])과 승인 통지(04)가 함께 쓴다. 통지는 결제 회사에 조회한 결과를
//! 주문서와 함께 [`settle`]로 넘긴다. 결제 회사의 응답 모양은 모르고 [`Approval`]만 다룬다.

use chrono::Utc;
use sqlx::PgPool;

use super::expiry;
use super::store::{self, Order, OrderStatus};
use super::toss::{Approval, Ending, TossConfig};

/// 결제창에서 돌아온 주소에 담긴 값. 브라우저가 보낸 값이라 그대로 믿지 않는다.
pub struct Returned<'a> {
    pub payment_key: &'a str,
    pub order_id: &'a str,
    pub amount: i64,
}

/// 승인 확인이 끝내지 못한 이유.
#[derive(Debug)]
pub enum ConfirmError {
    /// 그런 주문서가 없다.
    NotFound,
    /// 결제 회사의 결과를 아직 알 수 없다. 주문서는 결제 대기로 남고, 다시 확인할 수 있다.
    Unknown(String),
    Db(sqlx::Error),
}

impl From<sqlx::Error> for ConfirmError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

/// 돌아온 금액이 주문서와 다를 때의 실패 코드.
pub const AMOUNT_MISMATCH: &str = "amount_mismatch";
/// 결제 회사가 승인한 금액·주문 번호가 주문서와 다를 때의 실패 코드.
pub const PAYMENT_MISMATCH: &str = "payment_mismatch";

/// 돌아온 결과 화면의 승인 확인. 끝난 뒤의 주문서를 돌려준다.
///
/// 1. 주문서가 이미 끝났으면(확정·실패 등) 결제 회사를 부르지 않고 그대로 돌려준다.
///    같은 요청을 다시 보내도 같은 결과다. 끝난 주문서에 승인을 요청하면 돈이 나갈 수 있기 때문이다.
/// 2. 결제 대기로 30분이 지났으면 결제 회사를 부르지 않고 만료로 바꾼다.
/// 3. 돌아온 금액이 주문서와 다르면 결제 회사를 부르지 않고 실패(`amount_mismatch`)로 바꾼다.
/// 4. 결제 회사에 주문서 금액으로 승인을 요청하고, 그 결과를 [`settle`]로 반영한다.
pub async fn confirm(
    pool: &PgPool,
    toss: &TossConfig,
    returned: Returned<'_>,
) -> Result<Order, ConfirmError> {
    let order = store::find_by_order_id(pool, returned.order_id)
        .await?
        .ok_or(ConfirmError::NotFound)?;
    if is_ended(&order) {
        return Ok(order);
    }
    if order.created_at < expiry::cutoff(Utc::now()) {
        store::mark_expired(pool, &order.order_id).await?;
        return reload(pool, &order.order_id).await;
    }
    if returned.amount != order.amount {
        store::mark_ended(
            pool,
            &order.order_id,
            OrderStatus::Failed,
            AMOUNT_MISMATCH,
            "결제 금액이 주문서와 달라요.",
        )
        .await?;
        return reload(pool, &order.order_id).await;
    }
    let approval = toss
        .approve(returned.payment_key, &order.order_id, order.amount)
        .await;
    settle(pool, &order, approval).await
}

/// 주문서가 이미 끝났는지(결제 대기가 아닌지). 끝난 주문서는 결제 회사에 승인을 요청하지 않고,
/// 승인을 마친 결제가 아니면 바꾸지 않는다.
fn is_ended(order: &Order) -> bool {
    order.status != OrderStatus::Pending
}

/// 결제 회사의 결과를 주문서에 반영하고, 끝난 뒤의 주문서를 돌려준다. 결과 화면과 승인 통지가 함께 쓴다.
///
/// - 승인됐고 금액·주문 번호가 주문서와 같으면 확정한다. 돈이 이미 나갔으므로 그사이 만료·실패·취소된
///   주문서도 확정한다("늦은 확정", 경고 로그를 남긴다). 이미 확정이면 그대로다.
/// - 그 밖의 결과는 결제 대기일 때만 반영한다. 이미 끝난 주문서는 그대로 돌려준다.
///
/// 상태는 조건부로 바꾸므로 동시에 온 확인 중 하나만 이기고, 나머지는 이긴 쪽의 결과를 그대로 돌려준다.
pub async fn settle(
    pool: &PgPool,
    order: &Order,
    approval: Approval,
) -> Result<Order, ConfirmError> {
    match approval {
        Approval::Approved(payment)
            if payment.order_id == order.order_id && payment.amount == order.amount =>
        {
            let before = store::mark_confirmed(pool, &order.order_id, &payment.payment_key).await?;
            if let Some(
                status @ (OrderStatus::Failed | OrderStatus::Canceled | OrderStatus::Expired),
            ) = before
            {
                log::warn!(
                    "늦은 확정: {} 상태였던 주문서 {}를 승인을 마친 결제로 확정했어요",
                    status.as_str(),
                    order.order_id
                );
            }
        }
        _ if is_ended(order) => return Ok(order.clone()),
        Approval::Approved(_) => {
            store::mark_ended(
                pool,
                &order.order_id,
                OrderStatus::Failed,
                PAYMENT_MISMATCH,
                "승인된 결제가 주문서와 달라요.",
            )
            .await?;
        }
        Approval::Declined {
            ending,
            code,
            message,
        } => {
            let status = match ending {
                Ending::Failed => OrderStatus::Failed,
                Ending::Canceled => OrderStatus::Canceled,
                Ending::Expired => OrderStatus::Expired,
            };
            store::mark_ended(pool, &order.order_id, status, &code, &message).await?;
        }
        Approval::Unknown { message } => return Err(ConfirmError::Unknown(message)),
    }
    reload(pool, &order.order_id).await
}

async fn reload(pool: &PgPool, order_id: &str) -> Result<Order, ConfirmError> {
    store::find_by_order_id(pool, order_id)
        .await?
        .ok_or(ConfirmError::NotFound)
}

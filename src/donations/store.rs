//! 주문서 저장소: 주문서를 만들고, 주문 번호로 찾고, 상태를 바꾸고, 방송별 확정된 후원을 읽는다.
//!
//! 상태 바꾸기는 조건부 UPDATE 하나로 한다. 실패·취소·만료는 "결제 대기일 때만" 바뀌므로 동시에 온
//! 확인 중 하나만 이긴다. 확정은 "확정이 아닐 때만" 바뀐다. 결제 회사가 승인을 마친 결제는 돈이 이미
//! 나갔으므로 만료·실패·취소된 주문서도 확정한다(ADR 0007). 확정은 되돌아가지 않는다.

use chrono::{DateTime, Utc};
use rand::RngCore;
use sqlx::PgPool;

use super::Donation;
use crate::db::VISIBLE;

/// 주문 번호 앞부분. 토스 개발자센터의 결제 내역에서 우리 후원을 알아보기 쉽게 한다.
const ORDER_ID_PREFIX: &str = "don_";

/// 추측할 수 없는 새 주문 번호. `don_`과 운영체제 난수 24바이트의 hex(48자), 모두 52자다.
pub fn new_order_id() -> String {
    let mut bytes = [0u8; 24];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    format!("{ORDER_ID_PREFIX}{}", hex::encode(bytes))
}

/// 후원받을 방송. 화면에 보이는 방송만 찾는다.
pub struct Target {
    /// 소유자 번호와 아이디. 소유자 없는 예전 방송이면 `None`.
    pub owner: Option<(i64, String)>,
}

/// 후원받을 방송을 찾는다. 없거나 보이지 않으면 `None`.
pub async fn find_target(pool: &PgPool, broadcast_id: i64) -> sqlx::Result<Option<Target>> {
    let sql = format!(
        "SELECT b.owner_id, u.username
         FROM broadcasts b LEFT JOIN users u ON u.id = b.owner_id
         WHERE b.id = $1 AND {VISIBLE}"
    );
    let row: Option<(Option<i64>, Option<String>)> = sqlx::query_as(&sql)
        .bind(broadcast_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(id, username)| Target {
        owner: id.zip(username),
    }))
}

/// 새 주문서에 적을 값. 모두 서버가 정하거나 검사한 값이다.
pub struct NewOrder<'a> {
    pub broadcast_id: i64,
    pub donor_id: i64,
    pub recipient_id: i64,
    pub amount: i64,
    pub message: Option<&'a str>,
}

/// 결제 대기 주문서를 만들고 주문 번호를 돌려준다.
pub async fn create(pool: &PgPool, order: NewOrder<'_>) -> sqlx::Result<String> {
    let order_id = new_order_id();
    sqlx::query(
        "INSERT INTO donation_orders (order_id, broadcast_id, donor_id, recipient_id, amount, message)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&order_id)
    .bind(order.broadcast_id)
    .bind(order.donor_id)
    .bind(order.recipient_id)
    .bind(order.amount)
    .bind(order.message)
    .execute(pool)
    .await?;
    Ok(order_id)
}

pub use super::OrderStatus;

/// 저장된 주문서 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    pub order_id: String,
    /// 방송 번호. 방송이 지워졌으면 `None`.
    pub broadcast_id: Option<i64>,
    /// 후원자 번호.
    pub donor_id: i64,
    pub amount: i64,
    pub status: OrderStatus,
    /// 실패·취소·만료면 그 코드와 메시지.
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
    /// 주문서를 만든 시각. 결제 대기로 30분이 지났는지 볼 때 쓴다.
    pub created_at: DateTime<Utc>,
}

type OrderRow = (
    String,
    Option<i64>,
    i64,
    i64,
    String,
    Option<String>,
    Option<String>,
    DateTime<Utc>,
);

/// 주문 번호로 주문서를 찾는다. 없으면 `None`.
pub async fn find_by_order_id(pool: &PgPool, order_id: &str) -> sqlx::Result<Option<Order>> {
    let row: Option<OrderRow> = sqlx::query_as(
        "SELECT order_id, broadcast_id, donor_id, amount, status, failure_code, failure_message, created_at
             FROM donation_orders WHERE order_id = $1",
    )
    .bind(order_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(
            order_id,
            broadcast_id,
            donor_id,
            amount,
            status,
            failure_code,
            failure_message,
            created_at,
        )| Order {
            order_id,
            broadcast_id,
            donor_id,
            amount,
            status: OrderStatus::parse(&status),
            failure_code,
            failure_message,
            created_at,
        },
    ))
}

/// 확정되지 않은 주문서(결제 대기·실패·취소·만료)를 확정하고 결제 키를 남긴다.
/// 결제 회사가 승인을 마쳤다고 확인한 결제 키로만 부른다(돈이 이미 나갔다).
///
/// 바꿨으면 바뀌기 전 상태를, 이미 확정이었으면 `None`을 돌려준다. 확정된 주문서는 건드리지 않는다.
pub async fn mark_confirmed(
    pool: &PgPool,
    order_id: &str,
    payment_key: &str,
) -> sqlx::Result<Option<OrderStatus>> {
    let before: Option<String> = sqlx::query_scalar(
        "UPDATE donation_orders o
         SET status = 'confirmed', payment_key = $2, confirmed_at = now(),
             failure_code = NULL, failure_message = NULL
         FROM (SELECT id, status FROM donation_orders WHERE order_id = $1 FOR UPDATE) before
         WHERE o.id = before.id AND before.status <> 'confirmed'
         RETURNING before.status",
    )
    .bind(order_id)
    .bind(payment_key)
    .fetch_optional(pool)
    .await?;
    Ok(before.as_deref().map(OrderStatus::parse))
}

/// 결제 대기일 때만 만료로 바꾼다. 바꿨으면 `true`.
pub async fn mark_expired(pool: &PgPool, order_id: &str) -> sqlx::Result<bool> {
    mark_ended(
        pool,
        order_id,
        OrderStatus::Expired,
        EXPIRED_CODE,
        EXPIRED_MESSAGE,
    )
    .await
}

/// 만료 코드. 만료된 주문서의 실패 코드로 남는다.
pub const EXPIRED_CODE: &str = "expired";
/// 만료 안내. 결과 화면이 그대로 보여 준다.
pub const EXPIRED_MESSAGE: &str = "시간이 지나 결제가 취소되었어요.";

/// `cutoff`보다 먼저 만든 결제 대기 주문서의 주문 번호. 만료 정리가 하나씩 [`mark_expired`]로 바꾼다.
pub async fn pending_created_before(
    pool: &PgPool,
    cutoff: DateTime<Utc>,
) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar(
        "SELECT order_id FROM donation_orders WHERE status = 'pending' AND created_at < $1",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await
}

/// 결제 대기일 때만 실패·취소·만료(`status`)로 바꾸고 코드와 메시지를 남긴다. 바꿨으면 `true`.
pub async fn mark_ended(
    pool: &PgPool,
    order_id: &str,
    status: OrderStatus,
    code: &str,
    message: &str,
) -> sqlx::Result<bool> {
    debug_assert!(matches!(
        status,
        OrderStatus::Failed | OrderStatus::Canceled | OrderStatus::Expired
    ));
    let result = sqlx::query(
        "UPDATE donation_orders SET status = $2, failure_code = $3, failure_message = $4
         WHERE order_id = $1 AND status = 'pending'",
    )
    .bind(order_id)
    .bind(status.as_str())
    .bind(code)
    .bind(message)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 방송의 확정된 후원을 최신순으로 읽는다. 같은 시각이면 나중에 만든 주문서가 앞이다.
pub async fn confirmed_for_broadcast(
    pool: &PgPool,
    broadcast_id: i64,
) -> sqlx::Result<Vec<Donation>> {
    let rows: Vec<(String, i64, Option<String>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT u.username, o.amount, o.message, o.confirmed_at
         FROM donation_orders o JOIN users u ON u.id = o.donor_id
         WHERE o.broadcast_id = $1 AND o.status = 'confirmed'
         ORDER BY o.confirmed_at DESC, o.id DESC",
    )
    .bind(broadcast_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(donor, amount, message, confirmed_at)| Donation {
            donor,
            amount,
            message,
            confirmed_at,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::donations::tests::is_valid_order_id;

    #[test]
    fn 주문_번호는_토스_규칙에_맞는다() {
        let order_id = new_order_id();
        assert!(is_valid_order_id(&order_id), "{order_id}");
        assert!(order_id.starts_with("don_"));
    }

    #[test]
    fn 주문_번호는_매번_다르다() {
        let ids: std::collections::HashSet<String> = (0..1000).map(|_| new_order_id()).collect();
        assert_eq!(ids.len(), 1000);
    }

    #[test]
    fn 주문_번호_규칙의_경계() {
        assert!(is_valid_order_id("abc-_9"));
        assert!(is_valid_order_id(&"a".repeat(64)));
        assert!(!is_valid_order_id("abcde"));
        assert!(!is_valid_order_id(&"a".repeat(65)));
        assert!(!is_valid_order_id("abc=def"), "우리는 =를 쓰지 않는다");
        assert!(!is_valid_order_id("주문번호입니다"));
    }
}

//! 주문서 저장소: 주문서를 만들고, 방송별 확정된 후원을 읽는다.
//!
//! 주문 번호로 찾기와 상태 바꾸기(확정·실패·취소·만료)는 승인 확인(02~04)에서 여기에 더한다.
//! 상태 바꾸기는 "결제 대기일 때만"을 조건으로 하는 UPDATE 하나로 한다.

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

/// 토스 `orderId` 규칙(6~64자, 영문·숫자·`-`·`_`)에 맞는지. DB의 `CHECK`와 같다.
/// 지금은 테스트가 주문 번호 형식을 확인할 때만 쓴다.
#[cfg(test)]
pub fn is_valid_order_id(order_id: &str) -> bool {
    (6..=64).contains(&order_id.len())
        && order_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
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

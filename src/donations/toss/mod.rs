//! 결제 연결(토스페이먼츠). 결제 회사와 주고받는 코드와 토스의 코드 이름은 모두 이 모듈에 둔다(ADR 0007).
//!
//! 서버 쪽 연결(승인·조회·승인 통지 읽기)은 `connection`에 있고, 여기서 다시 내보낸다.
//! 화면과 서버가 함께 쓰는 것은 "구매자가 취소했다"는 코드를 알아보는 [`is_buyer_cancel`] 하나다.

#[cfg(feature = "server")]
mod connection;
#[cfg(feature = "server")]
pub use connection::*;

/// 토스가 "구매자가 취소했다"고 알리는 코드. failUrl의 `PAY_PROCESS_CANCELED`와 SDK의 `USER_CANCEL`.
const BUYER_CANCEL_CODES: [&str; 2] = ["PAY_PROCESS_CANCELED", "USER_CANCEL"];

/// 결제창이 돌려준 코드가 구매자 취소인지. 취소면 주문서는 실패가 아니라 취소다.
pub fn is_buyer_cancel(code: &str) -> bool {
    BUYER_CANCEL_CODES.contains(&code)
}

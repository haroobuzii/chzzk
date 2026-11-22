//! 후원: 로그인한 사용자가 방송 소유자에게 토스페이먼츠 테스트 결제로 돈을 보낸다(ADR 0007).
//!
//! 서버 쪽은 `POST`·`GET /api/broadcasts/{번호}/donations` 라우트(주문서 만들기, 합계·내역)와
//! `POST /api/donations/confirm`(승인 확인), 화면 쪽은 상세 화면의 후원 칸([`DonationPanel`])과
//! 결제 뒤 돌아오는 결과 화면([`DonationSuccess`])이다.
//!
//! - [`rules`]: 금액·메시지 규칙(화면과 서버가 함께 쓴다)
//! - `store`: 주문서 저장소
//! - `confirm`: 승인 확인. 결과 화면과 승인 통지가 함께 쓴다.
//! - [`toss`]: 결제 연결. 토스와 주고받는 코드는 이 모듈에만 둔다.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[cfg(feature = "server")]
mod api;
#[cfg(feature = "server")]
pub mod confirm;
#[cfg(test)]
mod confirm_tests;
#[cfg(test)]
pub mod fake_toss;
mod panel;
mod result;
pub mod rules;
#[cfg(feature = "server")]
pub mod store;
#[cfg(test)]
mod tests;
#[cfg(feature = "server")]
pub mod toss;

#[cfg(feature = "server")]
pub use api::{api, DonationsState};
pub use panel::DonationPanel;
pub use result::DonationSuccess;

/// 확정된 후원 하나. 내역에 보인다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Donation {
    /// 후원자 아이디.
    pub donor: String,
    /// 금액(원).
    pub amount: i64,
    /// 응원 메시지. 없으면 `None`.
    pub message: Option<String>,
    pub confirmed_at: DateTime<Utc>,
}

/// `GET /api/broadcasts/{번호}/donations`의 응답: 확정된 후원의 합계·건수와 최신순 내역.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DonationSummary {
    pub total_amount: i64,
    pub count: i64,
    pub items: Vec<Donation>,
    /// 서버에 토스 테스트 키가 있어 후원할 수 있는지. 거짓이면 후원 칸이 키 안내를 보여 준다.
    pub enabled: bool,
}

/// `POST /api/broadcasts/{번호}/donations`의 성공 응답. 브라우저가 이 값으로 토스 결제창을 연다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreatedOrder {
    /// 주문 번호(토스 `orderId`).
    pub order_id: String,
    pub amount: i64,
    /// 결제창에 보이는 이름. 받는 사람과 금액이 들어간다.
    pub order_name: String,
    /// 토스 클라이언트 키. 원래 공개해도 되는 키다.
    pub client_key: String,
}

/// `POST /api/donations/confirm`의 응답: 승인 확인이 끝난 주문서의 상태.
/// 같은 확인 요청을 다시 보내도 같은 값이다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfirmResult {
    /// `confirmed`(확정), `failed`(실패), `canceled`(취소), `expired`(만료).
    pub status: String,
    /// 후원한 방송. 방송이 지워졌으면 `None`.
    pub broadcast_id: Option<i64>,
    /// 주문서 금액(원).
    pub amount: i64,
    /// 확정이 아니면 실패 코드와 사용자에게 보여 줄 이유.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

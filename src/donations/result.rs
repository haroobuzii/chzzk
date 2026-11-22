//! 결제 뒤 돌아오는 결과 화면. 토스가 `/donations/success?paymentKey&orderId&amount`로 돌려보낸다.
//!
//! 화면은 주소의 값을 믿지 않는다. 그 값으로 승인 확인 API(`POST /api/donations/confirm`)를 부르고
//! 서버 응답만 보여 준다. 새로고침해도 서버가 같은 결과를 돌려주므로 후원은 한 번만 쌓인다.

use dioxus::prelude::*;
use serde_json::json;

use super::panel::TEST_PAYMENT_NOTICE;
use super::rules::format_won;
use super::ConfirmResult;
use crate::login::call_api;
use crate::Route;

/// 결과 화면의 상태.
#[derive(Clone, PartialEq)]
enum Shown {
    /// 서버가 확인하는 중.
    Checking,
    /// 서버가 확인을 마쳤다(확정이든 아니든).
    Done(ConfirmResult),
    /// 확인 API가 실패를 돌려줬거나 닿지 않았다.
    Error(String),
}

/// `/donations/success`: 주소의 쿼리(`query`)를 그대로 받아 읽는다.
#[component]
pub fn DonationSuccess(query: String) -> Element {
    let mut shown = use_signal(|| Shown::Checking);

    use_effect(use_reactive!(|query| {
        shown.set(Shown::Checking);
        spawn(async move {
            shown.set(check(&query).await);
        });
    }));

    rsx! {
        section { class: "donation-result",
            p { class: "test-payment", "{TEST_PAYMENT_NOTICE}" }
            match shown() {
                Shown::Checking => rsx! {
                    p { "결제를 확인하는 중…" }
                },
                Shown::Done(result) if result.status == "confirmed" => rsx! {
                    h1 { "후원이 완료되었습니다" }
                    p { class: "donation-result-amount", "{format_won(result.amount)}" }
                    if let Some(id) = result.broadcast_id {
                        p { "{id}번 방송에 후원했어요." }
                    }
                    BackLink { broadcast_id: result.broadcast_id }
                },
                Shown::Done(result) => rsx! {
                    h1 { "후원이 완료되지 않았어요" }
                    p { class: "error",
                        {result.message.clone().unwrap_or_else(|| "결제가 승인되지 않았어요.".into())}
                    }
                    BackLink { broadcast_id: result.broadcast_id }
                },
                Shown::Error(message) => rsx! {
                    h1 { "결제를 확인하지 못했어요" }
                    p { class: "error", "{message}" }
                    Link { class: "button", to: Route::BroadcastList {}, "목록으로" }
                },
            }
        }
    }
}

/// `/donations/fail`: 결제창이 `?code&message&orderId`로 돌려보낸 실패·취소 결과 화면.
///
/// 실패 보고 API(`POST /api/donations/fail`)로 내 주문서를 실패·취소로 바꾸고 안내를 보여 준다.
/// 서버가 주문서 상태를 알려 주면(본인 주문서) 그 상태를, 아니면 주소의 코드를 따른다.
#[component]
pub fn DonationFail(query: String) -> Element {
    let mut reported = use_signal(|| None::<Option<ConfirmResult>>);

    use_effect(use_reactive!(|query| {
        reported.set(None);
        spawn(async move {
            reported.set(Some(report_failure(&query).await));
        });
    }));

    let code = query_value(&query, "code");
    let message = query_value(&query, "message");
    rsx! {
        section { class: "donation-result",
            p { class: "test-payment", "{TEST_PAYMENT_NOTICE}" }
            match reported() {
                None => rsx! {
                    p { "결제 결과를 정리하는 중…" }
                },
                Some(result) => {
                    let notice = fail_notice(code.as_deref(), message.as_deref(), result.as_ref());
                    rsx! {
                        h1 { "{notice.title}" }
                        if let Some(detail) = notice.detail {
                            p { class: "error", "{detail}" }
                        }
                        BackLink { broadcast_id: result.and_then(|r| r.broadcast_id) }
                    }
                }
            }
        }
    }
}

/// 실패 보고 API를 부른다. 본인 주문서면 그 주문서의 상태를, 아니면(주문 번호 없음·남의 주문서·
/// 비로그인·서버 오류) `None`을 돌려준다. 안내는 어느 쪽이든 보여 준다.
async fn report_failure(query: &str) -> Option<ConfirmResult> {
    let body = json!({
        "order_id": query_value(query, "orderId"),
        "code": query_value(query, "code").unwrap_or_else(|| "UNKNOWN".into()),
        "message": query_value(query, "message"),
    });
    match call_api("POST", "/api/donations/fail", Some(body)).await {
        Ok((200, body)) => serde_json::from_value(body).ok(),
        _ => None,
    }
}

/// 토스가 "구매자가 취소했다"고 알리는 코드.
const CANCEL_CODES: [&str; 2] = ["PAY_PROCESS_CANCELED", "USER_CANCEL"];

/// 실패 화면의 안내 문구.
#[derive(Debug, PartialEq)]
struct FailNotice {
    title: &'static str,
    detail: Option<String>,
}

/// 실패 화면에 보여 줄 안내. 서버가 알려 준 주문서 상태(`result`)가 있으면 그것을, 없으면 주소의 코드를 따른다.
fn fail_notice(
    code: Option<&str>,
    message: Option<&str>,
    result: Option<&ConfirmResult>,
) -> FailNotice {
    const CANCELED: &str = "결제를 취소했습니다";
    const FAILED: &str = "결제에 실패했어요";
    const DEFAULT_REASON: &str = "결제가 승인되지 않았어요.";
    let failed = |reason: Option<&str>| FailNotice {
        title: FAILED,
        detail: Some(reason.unwrap_or(DEFAULT_REASON).to_string()),
    };
    let bare = |title| FailNotice {
        title,
        detail: None,
    };
    match result.map(|r| r.status.as_str()) {
        Some("canceled") => bare(CANCELED),
        Some("expired") => bare("시간이 지나 결제가 취소되었어요"),
        Some("confirmed") => bare("이미 후원이 완료되었어요"),
        Some(_) => failed(result.and_then(|r| r.message.as_deref())),
        None if code.is_some_and(|c| CANCEL_CODES.contains(&c)) => bare(CANCELED),
        None => failed(message),
    }
}

/// 방송으로 돌아가는 링크. 방송이 지워졌으면 목록으로 간다.
#[component]
fn BackLink(broadcast_id: Option<i64>) -> Element {
    match broadcast_id {
        Some(id) => rsx! {
            Link {
                class: "button",
                to: Route::BroadcastDetail { segment: id.to_string() },
                "방송으로 돌아가기"
            }
        },
        None => rsx! {
            Link { class: "button", to: Route::BroadcastList {}, "목록으로" }
        },
    }
}

/// 주소의 값으로 승인 확인 API를 부른다.
async fn check(query: &str) -> Shown {
    let (Some(payment_key), Some(order_id), Some(amount)) = (
        query_value(query, "paymentKey"),
        query_value(query, "orderId"),
        query_value(query, "amount").and_then(|a| a.parse::<i64>().ok()),
    ) else {
        return Shown::Error("결제 결과 주소가 올바르지 않아요.".into());
    };
    let body = json!({ "payment_key": payment_key, "order_id": order_id, "amount": amount });
    match call_api("POST", "/api/donations/confirm", Some(body)).await {
        Ok((200, body)) => match serde_json::from_value::<ConfirmResult>(body) {
            Ok(result) => Shown::Done(result),
            Err(_) => Shown::Error(UNKNOWN_ERROR.into()),
        },
        Ok((_, body)) => Shown::Error(
            body["message"]
                .as_str()
                .unwrap_or(UNKNOWN_ERROR)
                .to_string(),
        ),
        Err(message) => Shown::Error(message),
    }
}

const UNKNOWN_ERROR: &str = "요청을 처리하지 못했어요. 잠시 뒤 다시 시도해 주세요.";

/// 쿼리 문자열(`a=1&b=2`, 앞의 `?`는 있어도 된다)에서 `name`의 값을 꺼내 퍼센트 인코딩을 푼다.
fn query_value(query: &str, name: &str) -> Option<String> {
    query
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| percent_decode(value))
        .filter(|value| !value.is_empty())
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| Some(hex(bytes[i + 1])? * 16 + hex(bytes[i + 2])?))
            .flatten();
        match (bytes[i], escaped) {
            (_, Some(byte)) => {
                out.push(byte);
                i += 2;
            }
            (b'+', None) => out.push(b' '),
            (byte, None) => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(digit: u8) -> Option<u8> {
    (digit as char).to_digit(16).map(|d| d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 돌아온_주소에서_값을_읽는다() {
        let query = "paymentKey=tgen_2024%2Babc&orderId=don_ab12&amount=5000&paymentType=NORMAL";
        assert_eq!(
            query_value(query, "paymentKey").as_deref(),
            Some("tgen_2024+abc")
        );
        assert_eq!(query_value(query, "orderId").as_deref(), Some("don_ab12"));
        assert_eq!(
            query_value(&format!("?{query}"), "amount").as_deref(),
            Some("5000")
        );
        assert_eq!(query_value(query, "code"), None);
        assert_eq!(query_value("orderId=", "orderId"), None);
        assert_eq!(query_value("a=%zz%4", "a").as_deref(), Some("%zz%4"));
    }

    fn result(status: &str, message: Option<&str>) -> ConfirmResult {
        ConfirmResult {
            status: status.into(),
            broadcast_id: Some(1),
            amount: 5000,
            code: Some("X".into()),
            message: message.map(Into::into),
        }
    }

    #[test]
    fn 취소_코드면_취소_안내다() {
        for code in ["PAY_PROCESS_CANCELED", "USER_CANCEL"] {
            let notice = fail_notice(Some(code), Some("사용자가 취소"), None);
            assert_eq!(notice.title, "결제를 취소했습니다");
            assert_eq!(notice.detail, None);
        }
    }

    #[test]
    fn 다른_코드면_실패_이유를_보여_준다() {
        let notice = fail_notice(Some("REJECT_CARD_COMPANY"), Some("카드사 거절"), None);
        assert_eq!(notice.title, "결제에 실패했어요");
        assert_eq!(notice.detail.as_deref(), Some("카드사 거절"));
        let bare = fail_notice(None, None, None);
        assert_eq!(bare.title, "결제에 실패했어요");
        assert_eq!(bare.detail.as_deref(), Some("결제가 승인되지 않았어요."));
    }

    #[test]
    fn 서버가_알려_준_상태를_주소보다_믿는다() {
        let expired = result("expired", Some("시간이 지나 결제가 취소되었어요."));
        let notice = fail_notice(Some("PAY_PROCESS_CANCELED"), None, Some(&expired));
        assert_eq!(notice.title, "시간이 지나 결제가 취소되었어요");
        assert_eq!(notice.detail, None);

        let failed = result("failed", Some("거절"));
        let notice = fail_notice(Some("USER_CANCEL"), None, Some(&failed));
        assert_eq!(notice.title, "결제에 실패했어요");
        assert_eq!(notice.detail.as_deref(), Some("거절"));

        let confirmed = result("confirmed", None);
        let notice = fail_notice(Some("USER_CANCEL"), None, Some(&confirmed));
        assert_eq!(notice.title, "이미 후원이 완료되었어요");
    }
}

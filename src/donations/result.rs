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
}

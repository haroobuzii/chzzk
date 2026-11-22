//! 상세 화면의 후원 칸: 후원 합계와 내역, 금액·메시지를 골라 "후원하기".
//!
//! "후원하기"를 누르면 서버가 주문서를 먼저 만들고, 브라우저가 그 주문서의 값으로 토스 v2 SDK 카드 결제창을 연다.
//! 결제 결과는 `/donations/success`·`/donations/fail`로 돌아온다. 화면은 결과를 믿지 않고 서버가 확인한다(02).
//! 합계·내역은 서버가 그리지 않고 화면이 뜬 뒤에 불러온다. 댓글 칸과 같다.

use dioxus::prelude::*;
use serde::Deserialize;
use serde_json::json;

use super::rules::{format_won, validate_amount, validate_message, MessageTooLong};
use super::{CreatedOrder, DonationSummary};
use crate::login::{call_api, use_login_state, LoginState};
use crate::refresh::{sleep_ms, REFRESH_MS};
use crate::{format_kst, Route};

/// 금액 버튼.
const PRESET_AMOUNTS: [i64; 4] = [1_000, 5_000, 10_000, 50_000];

/// 테스트 결제 표시. 후원 칸과 결과 화면에 언제나 보인다.
pub const TEST_PAYMENT_NOTICE: &str = "테스트 결제 · 실제로 돈이 나가지 않아요";

/// 합계·내역 상태. `None`이면 아직 불러오지 못했다.
type Loaded = Option<Result<DonationSummary, String>>;

/// 후원 칸. `owner`는 방송 소유자의 아이디(없으면 `None`)다.
/// `live`가 참(방송 중인 라이브 방송)인 동안 5초마다 합계·내역을 다시 불러온다.
#[component]
pub fn DonationPanel(
    broadcast_id: ReadSignal<i64>,
    owner: ReadSignal<Option<String>>,
    live: ReadSignal<bool>,
) -> Element {
    let mut login = use_login_state();
    let route = use_route::<Route>();
    let summary = use_signal(|| None as Loaded);
    // 고른 금액(원). 버튼을 누르거나 직접 입력하면 바뀐다.
    let mut amount = use_signal(|| PRESET_AMOUNTS[1].to_string());
    let mut message = use_signal(String::new);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    use_effect(move || {
        spawn(load(broadcast_id(), summary));
    });

    // 방송 중인 동안 5초마다 다시 불러온다. 댓글 칸과 같은 방식이다.
    let mut polling = use_signal(|| false);
    use_effect(move || {
        if live() && !*polling.peek() {
            polling.set(true);
            spawn(async move {
                while *live.peek() {
                    sleep_ms(REFRESH_MS).await;
                    if *live.peek() {
                        load(*broadcast_id.peek(), summary).await;
                    }
                }
                polling.set(false);
            });
        }
    });

    let submit = move |event: FormEvent| {
        event.prevent_default();
        if pending() {
            return;
        }
        error.set(None);
        // 결제창을 열기 전에 규칙을 먼저 본다. 서버도 같은 규칙으로 다시 검사한다.
        let Ok(value) = amount().trim().replace(',', "").parse::<i64>() else {
            error.set(Some("후원 금액을 숫자로 적어 주세요.".into()));
            return;
        };
        if let Err(e) = validate_amount(value) {
            error.set(Some(e.message().into()));
            return;
        }
        if let Err(MessageTooLong) = validate_message(Some(&message())) {
            error.set(Some(MessageTooLong::MESSAGE.into()));
            return;
        }
        pending.set(true);
        spawn(async move {
            let id = *broadcast_id.peek();
            let url = format!("/api/broadcasts/{id}/donations");
            let body = json!({ "amount": value, "message": message() });
            let result = match call_api("POST", &url, Some(body)).await {
                Ok((201, body)) => match serde_json::from_value::<CreatedOrder>(body) {
                    Ok(order) => open_payment_window(&order).await,
                    Err(_) => Err(UNKNOWN_ERROR.into()),
                },
                Ok((401, body)) => {
                    // 다른 창에서 로그아웃했거나 로그인이 만료됐다. 로그인 안내로 바꾼다.
                    login.set(LoginState::Anonymous);
                    Err(message_of(&body))
                }
                Ok((_, body)) => Err(message_of(&body)),
                Err(message) => Err(message),
            };
            // 결제창으로 넘어가면 이 화면을 떠난다. 남아 있으면 창을 닫았거나 열지 못한 것이다.
            if let Err(message) = result {
                error.set(Some(message));
            }
            pending.set(false);
        });
    };

    let enabled = match summary() {
        Some(Ok(summary)) => Some(summary.enabled),
        _ => None,
    };
    let mine = matches!((&login(), &owner()), (LoginState::User(me), Some(owner)) if me == owner);

    let history = match summary() {
        None => rsx! {
            p { class: "donation-empty", "후원 내역을 불러오는 중…" }
        },
        Some(Err(message)) => rsx! {
            p { class: "error", role: "alert", "{message}" }
        },
        Some(Ok(summary)) => rsx! {
            p { class: "donation-total",
                "총 {format_won(summary.total_amount)} · {summary.count}건"
            }
            if summary.items.is_empty() {
                p { class: "donation-empty", "아직 후원이 없습니다" }
            } else {
                ul { class: "donation-list",
                    for (index, item) in summary.items.iter().enumerate() {
                        li { key: "{index}-{item.confirmed_at}", class: "donation",
                            div { class: "donation-meta",
                                span { class: "donation-donor", "{item.donor}" }
                                span { class: "donation-amount", "{format_won(item.amount)}" }
                                span { class: "card-time", "{format_kst(&item.confirmed_at)}" }
                            }
                            if let Some(text) = &item.message {
                                p { class: "donation-message", "{text}" }
                            }
                        }
                    }
                }
            }
        },
    };

    let compose = if owner().is_none() || mine {
        // 소유자 없는 방송과 내 방송에는 후원 입력이 없다.
        rsx! {}
    } else if enabled == Some(false) {
        rsx! {
            p { class: "donation-disabled", role: "status",
                "후원 기능을 쓰려면 토스 테스트 키가 필요해요"
            }
        }
    } else {
        match login() {
            // 서버가 그린 첫 화면에서는 로그인 여부를 아직 모른다.
            LoginState::Unknown => rsx! {},
            LoginState::Anonymous => rsx! {
                Link {
                    class: "button",
                    to: Route::Login { next: route.to_string() },
                    "로그인하고 후원하기"
                }
            },
            LoginState::User(_) => rsx! {
                // 브라우저 기본 검사 대신 우리 규칙의 안내 문구를 보여 준다.
                form { class: "donation-form", novalidate: true, onsubmit: submit,
                    div { class: "donation-presets", role: "group", aria_label: "후원 금액",
                        for preset in PRESET_AMOUNTS {
                            button {
                                key: "{preset}",
                                r#type: "button",
                                class: if amount() == preset.to_string() { "preset selected" } else { "preset" },
                                disabled: pending(),
                                onclick: move |_| amount.set(preset.to_string()),
                                "{format_won(preset)}"
                            }
                        }
                    }
                    label { class: "donation-amount-input",
                        "직접 입력"
                        input {
                            r#type: "number",
                            name: "amount",
                            inputmode: "numeric",
                            min: 1000,
                            max: 500000,
                            step: 100,
                            value: "{amount}",
                            disabled: pending(),
                            oninput: move |e| amount.set(e.value()),
                        }
                        "원"
                    }
                    p { class: "donation-hint", "1,000원부터 500,000원까지, 100원 단위" }
                    textarea {
                        name: "message",
                        rows: 2,
                        maxlength: 100,
                        placeholder: "응원 메시지 (100자까지, 쓰지 않아도 돼요)",
                        value: "{message}",
                        disabled: pending(),
                        oninput: move |e| message.set(e.value()),
                    }
                    button { class: "button", r#type: "submit", disabled: pending(),
                        if pending() { "결제창을 여는 중…" } else { "후원하기" }
                    }
                }
            },
        }
    };

    rsx! {
        section { class: "donations",
            h2 { class: "section-title", "후원" }
            p { class: "test-payment", "{TEST_PAYMENT_NOTICE}" }
            {history}
            {compose}
            if let Some(message) = error() {
                p { class: "error", role: "alert", "{message}" }
            }
        }
    }
}

/// 방송의 후원 합계·내역을 불러와 `summary`에 넣는다.
/// 이미 불러온 값이 있으면 실패해도 그대로 두고, 다음에 다시 묻는다.
async fn load(id: i64, mut summary: Signal<Loaded>) {
    let failed = match call_api("GET", &format!("/api/broadcasts/{id}/donations"), None).await {
        Ok((200, body)) => match serde_json::from_value::<DonationSummary>(body) {
            Ok(latest) => {
                if summary.peek().as_ref() != Some(&Ok(latest.clone())) {
                    summary.set(Some(Ok(latest)));
                }
                return;
            }
            Err(_) => "후원 내역을 불러오지 못했어요. 잠시 뒤 다시 시도해 주세요.".to_string(),
        },
        Ok((_, body)) => message_of(&body),
        Err(message) => message,
    };
    if !matches!(*summary.peek(), Some(Ok(_))) {
        summary.set(Some(Err(failed)));
    }
}

/// 토스 v2 SDK를 불러와 카드 결제창을 연다. 결제창으로 넘어가면 돌아오지 않는다.
/// 창을 닫거나(`USER_CANCEL`) 열지 못하면 그 이유를 돌려준다.
async fn open_payment_window(order: &CreatedOrder) -> Result<(), String> {
    let mut script = document::eval(PAYMENT_SCRIPT);
    script.send(order).map_err(|_| UNKNOWN_ERROR.to_string())?;
    let reply: PaymentReply = script.recv().await.map_err(|_| UNKNOWN_ERROR.to_string())?;
    match reply.code.as_deref() {
        None => Ok(()),
        Some("USER_CANCEL") | Some("PAY_PROCESS_CANCELED") => Err("결제를 취소했습니다.".into()),
        Some(_) => Err(reply
            .message
            .unwrap_or_else(|| "결제창을 열지 못했어요.".into())),
    }
}

#[derive(Deserialize)]
struct PaymentReply {
    /// 실패면 토스 오류 코드(없으면 `"UNKNOWN"`). 성공이면 `None`.
    code: Option<String>,
    message: Option<String>,
}

/// Rust 쪽에서 주문서(`{order_id, amount, order_name, client_key}`)를 받아 토스 카드 결제창을 연다.
/// 돌아올 주소는 지금 출처(`window.location.origin`)에 붙여 만든다.
const PAYMENT_SCRIPT: &str = r#"
const order = await dioxus.recv();
try {
  if (!window.TossPayments) {
    await new Promise((resolve, reject) => {
      const script = document.createElement("script");
      script.src = "https://js.tosspayments.com/v2/standard";
      script.onload = resolve;
      script.onerror = () => reject({ code: "SDK_LOAD_FAILED", message: "토스 결제 모듈을 불러오지 못했어요." });
      document.head.appendChild(script);
    });
  }
  const payment = TossPayments(order.client_key).payment({ customerKey: TossPayments.ANONYMOUS });
  await payment.requestPayment({
    method: "CARD",
    amount: { currency: "KRW", value: order.amount },
    orderId: order.order_id,
    orderName: order.order_name,
    successUrl: window.location.origin + "/donations/success",
    failUrl: window.location.origin + "/donations/fail",
    card: { flowMode: "DEFAULT", useEscrow: false, useCardPoint: false, useAppCardOnly: false },
  });
  dioxus.send({ code: null, message: null });
} catch (e) {
  dioxus.send({ code: (e && e.code) || "UNKNOWN", message: (e && e.message) || null });
}
"#;

const UNKNOWN_ERROR: &str = "요청을 처리하지 못했어요. 잠시 뒤 다시 시도해 주세요.";

/// 실패 응답 `{code, message}`에서 사용자에게 보여 줄 문구를 꺼낸다.
fn message_of(body: &serde_json::Value) -> String {
    body["message"]
        .as_str()
        .unwrap_or(UNKNOWN_ERROR)
        .to_string()
}

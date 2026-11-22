//! 결제 연결(토스페이먼츠). 결제 회사와 주고받는 코드는 모두 이 모듈에 둔다(ADR 0007).
//!
//! 나머지 모듈은 토스의 응답 모양을 모른다. [`TossConfig::approve`]와 [`TossConfig::lookup`]이
//! 돌려주는 [`Approval`]("이 주문 번호·금액으로 승인됐다/아니다/아직 모른다")만 다룬다.
//!
//! 토스 응답에서는 상태, 금액, 주문 번호, 결제 키만 읽는다.

use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

/// 토스 API 기본 주소.
pub const DEFAULT_API_URL: &str = "https://api.tosspayments.com";

/// 토스 테스트 키와 API 설정. 키가 둘 다 있을 때만 만들어진다.
#[derive(Clone, PartialEq, Eq)]
pub struct TossConfig {
    /// 클라이언트 키(`test_ck_…`). 브라우저에 주는 공개 키다.
    pub client_key: String,
    /// 시크릿 키(`test_sk_…`). 서버만 쓰고 브라우저로 보내지 않는다.
    pub secret_key: String,
    /// 토스 API 주소. 기본 [`DEFAULT_API_URL`], 테스트는 가짜 토스 서버 주소를 넣는다.
    pub api_url: String,
    /// 승인 요청에 붙일 `TossPayments-Test-Code` 값. 없으면 붙이지 않는다.
    pub test_fail_code: Option<String>,
}

// 시크릿 키가 로그에 찍히지 않도록 `Debug`를 직접 쓴다.
impl fmt::Debug for TossConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TossConfig")
            .field("client_key", &self.client_key)
            .field("secret_key", &"(숨김)")
            .field("api_url", &self.api_url)
            .field("test_fail_code", &self.test_fail_code)
            .finish()
    }
}

/// 테스트 키 앞부분. 이 앞부분이 아닌 키(라이브 키 등)로는 서버가 켜지지 않는다.
const TEST_KEY_PREFIX: &str = "test_";

impl TossConfig {
    /// 환경 변수 값(`TOSS_CLIENT_KEY`, `TOSS_SECRET_KEY`, `TOSS_API_URL`, `TOSS_TEST_FAIL_CODE`)으로 설정을 만든다.
    /// 값은 이미 앞뒤 공백을 지웠고, 비었으면 `None`이다.
    ///
    /// - 키가 둘 다 없으면 `Ok(None)`: 후원 기능을 끈 채로 켠다.
    /// - 하나만 있거나 `test_`로 시작하지 않으면 `Err`: 서버를 켜지 않는다.
    pub fn from_values(
        client_key: Option<String>,
        secret_key: Option<String>,
        api_url: Option<String>,
        test_fail_code: Option<String>,
    ) -> Result<Option<Self>, String> {
        let (client_key, secret_key) = match (client_key, secret_key) {
            (None, None) => return Ok(None),
            (Some(client), Some(secret)) => (client, secret),
            (Some(_), None) => {
                return Err(
                    "TOSS_SECRET_KEY가 없습니다. 토스 키는 TOSS_CLIENT_KEY와 함께 둘 다 넣거나 둘 다 비워 주세요"
                        .into(),
                )
            }
            (None, Some(_)) => {
                return Err(
                    "TOSS_CLIENT_KEY가 없습니다. 토스 키는 TOSS_SECRET_KEY와 함께 둘 다 넣거나 둘 다 비워 주세요"
                        .into(),
                )
            }
        };
        for (name, key) in [
            ("TOSS_CLIENT_KEY", &client_key),
            ("TOSS_SECRET_KEY", &secret_key),
        ] {
            if !key.starts_with(TEST_KEY_PREFIX) {
                // 키 값은 오류 메시지에 넣지 않는다.
                return Err(format!(
                    "{name}는 토스 테스트 키(test_로 시작)여야 합니다. 이 서버는 실제 결제를 하지 않습니다"
                ));
            }
        }
        Ok(Some(Self {
            client_key,
            secret_key,
            api_url: api_url
                .unwrap_or_else(|| DEFAULT_API_URL.into())
                .trim_end_matches('/')
                .to_string(),
            test_fail_code,
        }))
    }
}

/// 결제 회사가 알려 준 승인 결과. 결제 회사와 상관없는 모양이다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Approval {
    /// 결제 회사가 승인을 마쳤다. 금액·주문 번호가 주문서와 같은지는 받는 쪽이 본다.
    Approved(ApprovedPayment),
    /// 승인되지 않았다(거절, 취소, 만료 등). 주문서를 `ending`으로 끝내도 된다.
    Declined {
        ending: Ending,
        code: String,
        message: String,
    },
    /// 지금은 알 수 없다(결제 회사에 닿지 않음, 결제 회사 내부 오류, 아직 진행 중).
    /// 주문서는 결제 대기로 둔다.
    Unknown { message: String },
}

/// 승인되지 않은 결제가 끝난 모양. 주문서의 실패·취소·만료에 맞춘다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// 거절·중단 등.
    Failed,
    /// 결제가 취소됐다.
    Canceled,
    /// 시간이 지나 결제가 만료됐다.
    Expired,
}

/// 결제 키로 조회한 결제. 어느 주문서의 결제인지와 그 승인 결과다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub order_id: String,
    pub approval: Approval,
}

/// 승인을 마친 결제 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovedPayment {
    /// 결제 회사의 결제 키(`paymentKey`).
    pub payment_key: String,
    pub order_id: String,
    pub amount: i64,
}

/// 토스 Payment 객체에서 읽는 칸.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Payment {
    payment_key: String,
    order_id: String,
    status: String,
    total_amount: i64,
}

/// 토스 오류 응답 `{code, message}`.
#[derive(Deserialize)]
struct TossError {
    code: String,
    message: String,
}

/// 토스 API 한 번의 결과.
enum Answer {
    Payment(Payment),
    Error {
        status: u16,
        error: TossError,
    },
    /// 닿지 않았거나 응답을 읽지 못했다.
    Unreachable(String),
}

/// 토스 요청이 이보다 오래 걸리면 포기하고 "아직 모름"으로 둔다.
const TIMEOUT: Duration = Duration::from_secs(15);

/// 토스 요청에 쓰는 HTTP 클라이언트. 연결을 다시 쓰도록 하나만 만든다.
fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .expect("HTTP 클라이언트를 만들 수 있다")
    })
}

impl TossConfig {
    /// 결제를 승인하고 그 결과를 알려 준다.
    ///
    /// 1. 승인 API(`POST /v1/payments/confirm`)를 부른다. Basic 인증(`시크릿키:`)을 쓰고,
    ///    `Idempotency-Key`는 주문 번호다. 같은 주문서를 두 번 승인해도 토스는 첫 응답을 다시 준다.
    ///    `TOSS_TEST_FAIL_CODE`가 있으면 `TossPayments-Test-Code` 헤더를 붙인다.
    /// 2. 승인이 오류를 주면 실패라고 단정하지 않는다. 이미 승인됐을 수 있으므로 주문 번호로
    ///    다시 조회해 실제 상태로 판단한다(조회도 오류면 승인 오류를 그대로 알려 준다).
    pub async fn approve(&self, payment_key: &str, order_id: &str, amount: i64) -> Approval {
        let mut request = http()
            .post(format!("{}/v1/payments/confirm", self.api_url))
            .basic_auth(&self.secret_key, None::<&str>)
            .header("Idempotency-Key", order_id)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(
                json!({ "paymentKey": payment_key, "orderId": order_id, "amount": amount })
                    .to_string(),
            );
        if let Some(code) = &self.test_fail_code {
            request = request.header("TossPayments-Test-Code", code);
        }
        match send(request).await {
            Answer::Payment(payment) => judge(payment),
            Answer::Unreachable(message) => Approval::Unknown { message },
            Answer::Error { status, error } => match self.lookup_by_order_id(order_id).await {
                Answer::Payment(payment) => judge(payment),
                // 다시 조회해도 결제가 없으면 승인 오류가 결과다. 토스 내부 오류(5xx)는 다시 해 볼 수 있다.
                _ if status >= 500 => Approval::Unknown {
                    message: error.message,
                },
                _ => Approval::Declined {
                    ending: Ending::Failed,
                    code: error.code,
                    message: error.message,
                },
            },
        }
    }

    /// 결제 키로 결제를 조회한다(`GET /v1/payments/{결제 키}`). 승인은 하지 않는다.
    ///
    /// 승인 통지처럼 믿을 수 없는 곳에서 온 결제 키를 받는다. 그래서 결제 키 모양부터 보고,
    /// 토스가 돌려준 결제의 결제 키가 같은지도 본다. 결제가 없거나 조회하지 못했으면 그 이유가 `Err`다.
    pub async fn lookup(&self, payment_key: &str) -> Result<Found, String> {
        if !is_payment_key(payment_key) {
            return Err("결제 키 모양이 아니에요.".into());
        }
        let request = http()
            .get(format!("{}/v1/payments/{payment_key}", self.api_url))
            .basic_auth(&self.secret_key, None::<&str>);
        match send(request).await {
            Answer::Payment(payment) if payment.payment_key == payment_key => Ok(Found {
                order_id: payment.order_id.clone(),
                approval: judge(payment),
            }),
            Answer::Payment(_) => Err("조회한 결제의 결제 키가 달라요.".into()),
            Answer::Error { status, error } => Err(format!(
                "토스가 {status}({})로 답했어요: {}",
                error.code, error.message
            )),
            Answer::Unreachable(message) => Err(message),
        }
    }

    /// 주문 번호로 결제를 조회한다(`GET /v1/payments/orders/{주문 번호}`).
    async fn lookup_by_order_id(&self, order_id: &str) -> Answer {
        let request = http()
            .get(format!("{}/v1/payments/orders/{order_id}", self.api_url))
            .basic_auth(&self.secret_key, None::<&str>);
        send(request).await
    }
}

async fn send(request: reqwest::RequestBuilder) -> Answer {
    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => return Answer::Unreachable(format!("토스에 연결하지 못했어요: {e}")),
    };
    let status = response.status();
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => return Answer::Unreachable(format!("토스 응답을 읽지 못했어요: {e}")),
    };
    if status.is_success() {
        match serde_json::from_slice(&bytes) {
            Ok(payment) => Answer::Payment(payment),
            Err(e) => Answer::Unreachable(format!("토스 응답을 읽지 못했어요: {e}")),
        }
    } else {
        let error = serde_json::from_slice(&bytes).unwrap_or_else(|_| TossError {
            code: "UNKNOWN".into(),
            message: format!("토스가 {status}로 답했어요."),
        });
        Answer::Error {
            status: status.as_u16(),
            error,
        }
    }
}

/// 토스 결제 키 모양인지(1~200자, 영문·숫자·`-`·`_`). 조회 주소 경로에 그대로 넣으므로
/// `/`·`.` 같은 글자로 다른 주소를 부르지 못하게 한다.
fn is_payment_key(key: &str) -> bool {
    (1..=200).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// 토스 결제 상태를 승인 결과로 바꾼다. `DONE`만 승인이다.
fn judge(payment: Payment) -> Approval {
    match payment.status.as_str() {
        "DONE" => Approval::Approved(ApprovedPayment {
            payment_key: payment.payment_key,
            order_id: payment.order_id,
            amount: payment.total_amount,
        }),
        "CANCELED" | "PARTIAL_CANCELED" | "ABORTED" | "EXPIRED" => Approval::Declined {
            ending: match payment.status.as_str() {
                "CANCELED" | "PARTIAL_CANCELED" => Ending::Canceled,
                "EXPIRED" => Ending::Expired,
                _ => Ending::Failed,
            },
            code: payment.status.clone(),
            message: format!("결제가 승인되지 않았어요({}).", payment.status),
        },
        // READY·IN_PROGRESS·WAITING_FOR_DEPOSIT: 아직 끝나지 않았다.
        _ => Approval::Unknown {
            message: format!("결제가 아직 끝나지 않았어요({}).", payment.status),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    #[test]
    fn 키가_둘_다_없으면_후원을_끈다() {
        assert_eq!(TossConfig::from_values(None, None, None, None), Ok(None));
        assert_eq!(
            TossConfig::from_values(None, None, some("http://localhost:1"), some("X")),
            Ok(None)
        );
    }

    #[test]
    fn 테스트_키가_둘_다_있으면_켜고_기본_주소를_쓴다() {
        let config =
            TossConfig::from_values(some("test_ck_fake"), some("test_sk_fake"), None, None)
                .unwrap()
                .expect("켜진다");
        assert_eq!(config.client_key, "test_ck_fake");
        assert_eq!(config.secret_key, "test_sk_fake");
        assert_eq!(config.api_url, "https://api.tosspayments.com");
        assert_eq!(config.test_fail_code, None);
    }

    #[test]
    fn api_주소와_실패_코드를_읽는다() {
        let config = TossConfig::from_values(
            some("test_ck_fake"),
            some("test_sk_fake"),
            some("http://127.0.0.1:9999/"),
            some("REJECT_CARD_PAYMENT"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(config.api_url, "http://127.0.0.1:9999");
        assert_eq!(
            config.test_fail_code.as_deref(),
            Some("REJECT_CARD_PAYMENT")
        );
    }

    #[test]
    fn 한쪽만_있는_키는_거절한다() {
        let error = TossConfig::from_values(some("test_ck_fake"), None, None, None).unwrap_err();
        assert!(error.contains("TOSS_SECRET_KEY"), "{error}");
        let error = TossConfig::from_values(None, some("test_sk_fake"), None, None).unwrap_err();
        assert!(error.contains("TOSS_CLIENT_KEY"), "{error}");
    }

    #[test]
    fn 테스트_키가_아니면_거절하고_키_값은_알리지_않는다() {
        let error = TossConfig::from_values(some("live_ck_fake"), some("test_sk_fake"), None, None)
            .unwrap_err();
        assert!(error.contains("TOSS_CLIENT_KEY"), "{error}");
        assert!(!error.contains("live_ck_fake"), "{error}");
        let error = TossConfig::from_values(some("test_ck_fake"), some("live_sk_fake"), None, None)
            .unwrap_err();
        assert!(error.contains("TOSS_SECRET_KEY"), "{error}");
        assert!(!error.contains("live_sk_fake"), "{error}");
    }

    #[test]
    fn 디버그_출력에_시크릿_키가_없다() {
        let config =
            TossConfig::from_values(some("test_ck_fake"), some("test_sk_fake"), None, None)
                .unwrap()
                .unwrap();
        assert!(!format!("{config:?}").contains("test_sk_fake"));
    }
}

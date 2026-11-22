//! 테스트용 가짜 토스 서버. 임의 포트에 띄운 작은 axum 서버다. `src/live/fake_mediamtx.rs`와 같은 방식이다.
//!
//! 하는 일은 둘이다.
//! - 받은 승인·조회 요청을 헤더까지 모두 기록한다.
//! - 테스트가 정한 대답을 돌려준다. 정하지 않으면 승인은 받은 값 그대로 `DONE`이다.
//!   조회는 [`FakeToss::add_payment`]로 넣어 둔 결제를 결제 키·주문 번호로 찾아 주고, 없으면 404(`NOT_FOUND_PAYMENT`)다.
//!
//! 쓰는 법:
//! ```ignore
//! let toss = FakeToss::start().await;
//! let config = toss.config();                                   // 우리 서버가 쓸 토스 설정(가짜 키)
//! toss.on_confirm(Reply::error(403, "REJECT_CARD_COMPANY", "거절"));
//! toss.on_lookup(Reply::payment("DONE", "pay_1", "don_…", 5000)); // 모든 조회에 같은 대답
//! toss.add_payment(Reply::payment("DONE", "pay_1", "don_…", 5000)); // 이 결제 키·주문 번호로만 찾힌다
//! assert!(toss.confirm_requests().is_empty());                  // 승인 요청이 가지 않았는지
//! ```

use std::sync::{Arc, Mutex};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    Json, Router,
};
use serde_json::{json, Value};

use super::toss::TossConfig;

/// 가짜 클라이언트 키. 진짜 키가 아니다.
pub const FAKE_CLIENT_KEY: &str = "test_ck_fake";
/// 가짜 시크릿 키. 진짜 키가 아니다.
pub const FAKE_SECRET_KEY: &str = "test_sk_fake";

/// 승인 API 경로.
pub const CONFIRM_PATH: &str = "/v1/payments/confirm";

/// 가짜가 받은 요청 하나.
#[derive(Clone, Debug)]
pub struct CapturedRequest {
    pub method: Method,
    /// 쿼리를 뺀 경로. 예: `/v1/payments/confirm`
    pub path: String,
    pub headers: HeaderMap,
    /// JSON 본문. 본문이 없거나 JSON이 아니면 `Null`.
    pub body: Value,
}

impl CapturedRequest {
    /// 헤더 값. 없으면 `None`.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }
}

/// 가짜가 돌려줄 대답.
#[derive(Clone, Debug)]
pub struct Reply {
    pub status: StatusCode,
    pub body: Value,
}

impl Reply {
    /// 토스 Payment 객체(읽는 칸만)로 200을 준다.
    pub fn payment(status: &str, payment_key: &str, order_id: &str, amount: i64) -> Self {
        Self {
            status: StatusCode::OK,
            body: json!({
                "paymentKey": payment_key,
                "orderId": order_id,
                "status": status,
                "totalAmount": amount,
                "method": "카드",
                "version": "2022-11-16",
            }),
        }
    }

    /// 토스 오류 응답 `{code, message}`.
    pub fn error(status: u16, code: &str, message: &str) -> Self {
        Self {
            status: StatusCode::from_u16(status).unwrap(),
            body: json!({ "code": code, "message": message }),
        }
    }
}

#[derive(Default)]
struct Inner {
    requests: Vec<CapturedRequest>,
    confirm: Option<Reply>,
    lookup: Option<Reply>,
    /// 조회로 찾을 수 있는 결제들(토스 Payment 객체).
    payments: Vec<Value>,
}

#[derive(Clone)]
pub struct FakeToss {
    /// 가짜 서버 주소. 예: `http://127.0.0.1:54321`
    pub url: String,
    inner: Arc<Mutex<Inner>>,
}

impl FakeToss {
    /// 임의 포트에 가짜를 띄운다. 테스트가 끝나면 런타임과 함께 꺼진다.
    pub async fn start() -> Self {
        let inner = Arc::new(Mutex::new(Inner::default()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().fallback(handle).with_state(inner.clone());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, inner }
    }

    /// 이 가짜를 가리키는 토스 설정. 키는 가짜 테스트 키다.
    pub fn config(&self) -> TossConfig {
        self.config_with_fail_code(None)
    }

    /// `TOSS_TEST_FAIL_CODE`를 넣은 토스 설정.
    pub fn config_with_fail_code(&self, fail_code: Option<&str>) -> TossConfig {
        TossConfig::from_values(
            Some(FAKE_CLIENT_KEY.into()),
            Some(FAKE_SECRET_KEY.into()),
            Some(self.url.clone()),
            fail_code.map(str::to_string),
        )
        .unwrap()
        .unwrap()
    }

    /// 승인 요청에 줄 대답을 정한다.
    pub fn on_confirm(&self, reply: Reply) {
        self.inner.lock().unwrap().confirm = Some(reply);
    }

    /// 조회 요청(결제 키·주문 번호 둘 다)에 줄 대답을 정한다.
    pub fn on_lookup(&self, reply: Reply) {
        self.inner.lock().unwrap().lookup = Some(reply);
    }

    /// 조회로 찾을 수 있는 결제를 넣는다. 같은 결제 키가 이미 있으면 바꾼다.
    /// `on_lookup`으로 정한 대답이 있으면 그쪽이 먼저다.
    pub fn add_payment(&self, payment: Reply) {
        let mut inner = self.inner.lock().unwrap();
        let key = payment.body["paymentKey"].clone();
        inner.payments.retain(|p| p["paymentKey"] != key);
        inner.payments.push(payment.body);
    }

    /// 지금까지 받은 요청 전부.
    pub fn requests(&self) -> Vec<CapturedRequest> {
        self.inner.lock().unwrap().requests.clone()
    }

    /// 승인 요청만.
    pub fn confirm_requests(&self) -> Vec<CapturedRequest> {
        self.requests()
            .into_iter()
            .filter(|r| r.path == CONFIRM_PATH)
            .collect()
    }

    /// 조회 요청만.
    pub fn lookup_requests(&self) -> Vec<CapturedRequest> {
        self.requests()
            .into_iter()
            .filter(|r| r.method == Method::GET)
            .collect()
    }
}

async fn handle(
    State(inner): State<Arc<Mutex<Inner>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = uri.path().to_string();
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let mut inner = inner.lock().unwrap();
    inner.requests.push(CapturedRequest {
        method: method.clone(),
        path: path.clone(),
        headers,
        body: body.clone(),
    });

    let reply = if method == Method::POST && path == CONFIRM_PATH {
        inner.confirm.clone().unwrap_or_else(|| {
            // 정하지 않으면 받은 값 그대로 승인한다.
            Reply::payment(
                "DONE",
                body["paymentKey"].as_str().unwrap_or_default(),
                body["orderId"].as_str().unwrap_or_default(),
                body["amount"].as_i64().unwrap_or_default(),
            )
        })
    } else if method == Method::GET && path.starts_with("/v1/payments/") {
        inner.lookup.clone().unwrap_or_else(|| {
            // `/v1/payments/orders/{주문 번호}` 또는 `/v1/payments/{결제 키}`
            let rest = &path["/v1/payments/".len()..];
            let found = match rest.strip_prefix("orders/") {
                Some(order_id) => inner.payments.iter().find(|p| p["orderId"] == order_id),
                None => inner.payments.iter().find(|p| p["paymentKey"] == rest),
            };
            match found {
                Some(payment) => Reply {
                    status: StatusCode::OK,
                    body: payment.clone(),
                },
                None => Reply::error(404, "NOT_FOUND_PAYMENT", "존재하지 않는 결제 정보 입니다."),
            }
        })
    } else {
        Reply::error(404, "NOT_FOUND", "가짜 토스에 없는 주소")
    };
    (reply.status, Json(reply.body)).into_response()
}

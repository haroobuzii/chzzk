//! 결제 연결(토스페이먼츠). 결제 회사와 주고받는 코드는 모두 이 모듈에 둔다(ADR 0007).
//!
//! 지금은 키 설정만 있다. 승인·조회 API 호출은 승인 확인(02)에서 여기에 더한다.

use std::fmt;

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

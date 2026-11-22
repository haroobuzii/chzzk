//! 후원 규칙: 금액과 메시지. 화면과 서버가 함께 쓰는 순수 함수다.

/// 가장 적은 후원 금액(원). DB의 `CHECK`와 같다.
pub const MIN_AMOUNT: i64 = 1_000;
/// 가장 큰 후원 금액(원). DB의 `CHECK`와 같다.
pub const MAX_AMOUNT: i64 = 500_000;
/// 금액 단위(원). DB의 `CHECK`와 같다.
pub const AMOUNT_STEP: i64 = 100;
/// 메시지 최대 길이(문자 수). DB의 `CHECK`와 같다.
pub const MAX_MESSAGE_CHARS: usize = 100;

#[derive(Debug, PartialEq, Eq)]
pub enum InvalidAmount {
    /// 1,000원보다 적거나 500,000원보다 많다.
    OutOfRange,
    /// 100원 단위가 아니다.
    NotStep,
}

impl InvalidAmount {
    /// 응답에 넣는 원인 코드.
    #[cfg(feature = "server")]
    pub fn code(&self) -> &'static str {
        match self {
            Self::OutOfRange => "amount_out_of_range",
            Self::NotStep => "amount_not_step",
        }
    }

    /// 사용자에게 보여 줄 이유.
    pub fn message(&self) -> &'static str {
        match self {
            Self::OutOfRange => "후원 금액은 1,000원부터 500,000원까지예요.",
            Self::NotStep => "후원 금액은 100원 단위로 정해 주세요.",
        }
    }
}

/// 후원 금액을 검사한다.
pub fn validate_amount(amount: i64) -> Result<i64, InvalidAmount> {
    if !(MIN_AMOUNT..=MAX_AMOUNT).contains(&amount) {
        return Err(InvalidAmount::OutOfRange);
    }
    if amount % AMOUNT_STEP != 0 {
        return Err(InvalidAmount::NotStep);
    }
    Ok(amount)
}

#[derive(Debug, PartialEq, Eq)]
pub struct MessageTooLong;

impl MessageTooLong {
    /// 응답에 넣는 원인 코드.
    #[cfg(feature = "server")]
    pub const CODE: &'static str = "message_too_long";
    pub const MESSAGE: &'static str = "후원 메시지는 100자까지 쓸 수 있어요.";
}

/// 후원 메시지를 검사한다. 앞뒤 공백을 지우고, 비었으면 `None`(메시지 없음)이다.
pub fn validate_message(input: Option<&str>) -> Result<Option<String>, MessageTooLong> {
    let message = input.unwrap_or("").trim();
    if message.is_empty() {
        return Ok(None);
    }
    // 바이트가 아니라 문자 수로 센다. 한글 한 글자도, 이모지 하나도 한 자다.
    if message.chars().count() > MAX_MESSAGE_CHARS {
        return Err(MessageTooLong);
    }
    Ok(Some(message.to_string()))
}

/// 금액을 "35,000원"처럼 쓴다.
pub fn format_won(amount: i64) -> String {
    let digits = amount.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    let sign = if amount < 0 { "-" } else { "" };
    format!("{sign}{grouped}원")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 금액은_천_원부터_오십만_원까지_받는다() {
        assert_eq!(validate_amount(1_000), Ok(1_000));
        assert_eq!(validate_amount(500_000), Ok(500_000));
        assert_eq!(validate_amount(999), Err(InvalidAmount::OutOfRange));
        assert_eq!(validate_amount(900), Err(InvalidAmount::OutOfRange));
        assert_eq!(validate_amount(500_100), Err(InvalidAmount::OutOfRange));
        assert_eq!(validate_amount(0), Err(InvalidAmount::OutOfRange));
        assert_eq!(validate_amount(-5_000), Err(InvalidAmount::OutOfRange));
    }

    #[test]
    fn 금액은_백_원_단위만_받는다() {
        assert_eq!(validate_amount(1_100), Ok(1_100));
        assert_eq!(validate_amount(1_050), Err(InvalidAmount::NotStep));
        assert_eq!(validate_amount(499_999), Err(InvalidAmount::NotStep));
    }

    #[test]
    fn 메시지는_없어도_되고_공백뿐이면_없는_것으로_친다() {
        assert_eq!(validate_message(None), Ok(None));
        assert_eq!(validate_message(Some("")), Ok(None));
        assert_eq!(validate_message(Some("  \n ")), Ok(None));
    }

    #[test]
    fn 메시지는_앞뒤_공백을_지우고_백_자까지_받는다() {
        assert_eq!(
            validate_message(Some("  응원해요 🎮 ")),
            Ok(Some("응원해요 🎮".to_string()))
        );
        assert!(validate_message(Some(&"가".repeat(100))).is_ok());
        assert_eq!(
            validate_message(Some(&"가".repeat(101))),
            Err(MessageTooLong)
        );
        assert!(validate_message(Some(&format!("  {}  ", "🎮".repeat(100)))).is_ok());
    }

    #[test]
    fn 금액은_세_자리마다_쉼표를_찍는다() {
        assert_eq!(format_won(0), "0원");
        assert_eq!(format_won(1_000), "1,000원");
        assert_eq!(format_won(35_000), "35,000원");
        assert_eq!(format_won(500_000), "500,000원");
        assert_eq!(format_won(1_234_567), "1,234,567원");
    }
}

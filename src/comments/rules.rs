//! 댓글 본문 규칙. 화면과 무관한 순수 함수다.

/// 댓글 최대 길이(문자 수). DB의 `CHECK`와 같다.
pub const MAX_BODY_CHARS: usize = 300;

#[derive(Debug, PartialEq, Eq)]
pub enum InvalidBody {
    /// 비었거나 공백뿐이다.
    Empty,
    /// 앞뒤 공백을 지운 뒤 300자를 넘는다.
    TooLong,
}

impl InvalidBody {
    /// 응답에 넣는 원인 코드.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Empty => "comment_empty",
            Self::TooLong => "comment_too_long",
        }
    }

    /// 사용자에게 보여 줄 이유.
    pub fn message(&self) -> &'static str {
        match self {
            Self::Empty => "댓글을 입력해 주세요.",
            Self::TooLong => "댓글은 300자까지 쓸 수 있어요.",
        }
    }
}

/// 댓글 본문을 검사하고, 앞뒤 공백을 지운 본문을 돌려준다.
pub fn validate_body(input: &str) -> Result<String, InvalidBody> {
    let body = input.trim();
    if body.is_empty() {
        return Err(InvalidBody::Empty);
    }
    // 바이트가 아니라 문자 수로 센다. 한글 한 글자도, 이모지 하나도 한 자다.
    if body.chars().count() > MAX_BODY_CHARS {
        return Err(InvalidBody::TooLong);
    }
    Ok(body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 앞뒤_공백은_지우고_가운데_줄바꿈은_둔다() {
        assert_eq!(
            validate_body("  좋은 방송\n고마워요 \n"),
            Ok("좋은 방송\n고마워요".to_string())
        );
    }

    #[test]
    fn 빈_댓글과_공백뿐인_댓글은_거절한다() {
        assert_eq!(validate_body(""), Err(InvalidBody::Empty));
        assert_eq!(validate_body(" \t\n "), Err(InvalidBody::Empty));
    }

    #[test]
    fn 한_자와_삼백_자는_받고_삼백한_자는_거절한다() {
        assert_eq!(validate_body("a"), Ok("a".to_string()));
        assert!(validate_body(&"가".repeat(300)).is_ok());
        assert_eq!(validate_body(&"가".repeat(301)), Err(InvalidBody::TooLong));
    }

    #[test]
    fn 이모지는_한_글자로_센다() {
        assert!(validate_body(&"🎮".repeat(300)).is_ok());
        assert_eq!(validate_body(&"🎮".repeat(301)), Err(InvalidBody::TooLong));
    }

    #[test]
    fn 공백을_지운_뒤의_길이로_센다() {
        let body = format!("   {}   ", "a".repeat(300));
        assert_eq!(validate_body(&body), Ok("a".repeat(300)));
    }
}

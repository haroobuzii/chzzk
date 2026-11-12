//! 방송 제목 규칙. 화면과 무관한 순수 함수다.

/// 제목 최대 길이(문자 수). DB의 `CHECK`와 같다.
pub const MAX_TITLE_CHARS: usize = 100;

#[derive(Debug, PartialEq, Eq)]
pub enum InvalidTitle {
    /// 비었거나 공백뿐이다.
    Empty,
    /// 앞뒤 공백을 지운 뒤 100자를 넘는다.
    TooLong,
}

impl InvalidTitle {
    /// 응답에 넣는 원인 코드.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Empty => "title_empty",
            Self::TooLong => "title_too_long",
        }
    }

    /// 사용자에게 보여 줄 이유.
    pub fn message(&self) -> &'static str {
        match self {
            Self::Empty => "제목을 입력해 주세요.",
            Self::TooLong => "제목은 100자까지 쓸 수 있어요.",
        }
    }
}

/// 제목을 검사하고, 앞뒤 공백을 지운 제목을 돌려준다. 같은 제목은 허용한다.
pub fn validate_title(input: &str) -> Result<String, InvalidTitle> {
    let title = input.trim();
    if title.is_empty() {
        return Err(InvalidTitle::Empty);
    }
    // 바이트가 아니라 문자 수로 센다. 한글 한 글자도, 이모지 하나도 한 자다.
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(InvalidTitle::TooLong);
    }
    Ok(title.to_string())
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn 앞뒤_공백은_지운다() {
        assert_eq!(validate_title("  첫 방송 \n"), Ok("첫 방송".to_string()));
    }

    #[test]
    fn 가운데_공백은_그대로_둔다() {
        assert_eq!(validate_title("첫  방송"), Ok("첫  방송".to_string()));
    }

    #[test]
    fn 빈_제목과_공백뿐인_제목은_거절한다() {
        assert_eq!(validate_title(""), Err(InvalidTitle::Empty));
        assert_eq!(validate_title("   \t\n"), Err(InvalidTitle::Empty));
    }

    #[test]
    fn 백_자까지는_받고_백한_자는_거절한다() {
        assert!(validate_title(&"가".repeat(100)).is_ok());
        assert_eq!(validate_title(&"가".repeat(101)), Err(InvalidTitle::TooLong));
    }

    #[test]
    fn 이모지와_한글은_한_글자로_센다() {
        assert!(validate_title(&"🎮".repeat(100)).is_ok());
        assert_eq!(validate_title(&"🎮".repeat(101)), Err(InvalidTitle::TooLong));
    }

    #[test]
    fn 공백을_지운_뒤의_길이로_센다() {
        let title = format!("  {}  ", "a".repeat(100));
        assert_eq!(validate_title(&title), Ok("a".repeat(100)));
    }
}

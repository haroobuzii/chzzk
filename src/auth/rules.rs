//! 아이디·비밀번호 규칙.

/// 아이디 글자 수 범위.
pub const USERNAME_MIN_CHARS: usize = 2;
pub const USERNAME_MAX_CHARS: usize = 20;
/// 비밀번호 글자 수 범위. 최대가 있는 까닭은 아주 긴 비밀번호의 해시 계산에 서버가 묶이지 않게 하기 위해서다.
pub const PASSWORD_MIN_CHARS: usize = 8;
pub const PASSWORD_MAX_CHARS: usize = 128;

/// 규칙에 어긋난 입력. 실패 응답의 원인 코드와 한국어 메시지를 준다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidCredentials {
    /// 아이디가 2~20자, 한글·영문·숫자·밑줄이 아니다.
    Username,
    /// 비밀번호가 8자보다 짧다.
    PasswordTooShort,
    /// 비밀번호가 128자보다 길다.
    PasswordTooLong,
}

impl InvalidCredentials {
    pub fn code(self) -> &'static str {
        match self {
            Self::Username => "invalid_username",
            Self::PasswordTooShort => "password_too_short",
            Self::PasswordTooLong => "password_too_long",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Username => "아이디는 2~20자의 한글, 영문, 숫자, 밑줄(_)로 지어 주세요.",
            Self::PasswordTooShort => "비밀번호는 8자 이상으로 지어 주세요.",
            Self::PasswordTooLong => "비밀번호는 128자 이하로 지어 주세요.",
        }
    }
}

/// 아이디가 규칙에 맞는지 본다. 앞뒤 공백도 규칙 위반으로 친다.
pub fn validate_username(username: &str) -> Result<(), InvalidCredentials> {
    let length = username.chars().count();
    let allowed = username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || is_hangul_syllable(c));
    if (USERNAME_MIN_CHARS..=USERNAME_MAX_CHARS).contains(&length) && allowed {
        Ok(())
    } else {
        Err(InvalidCredentials::Username)
    }
}

/// 완성된 한글 글자(가~힣)인지 본다. 자음·모음만 있는 글자(ㅎ, ㅏ)는 받지 않는다.
fn is_hangul_syllable(c: char) -> bool {
    ('가'..='힣').contains(&c)
}

/// 비밀번호가 규칙에 맞는지 본다. 글자 수는 바이트가 아니라 글자로 센다.
pub fn validate_password(password: &str) -> Result<(), InvalidCredentials> {
    let length = password.chars().count();
    if length < PASSWORD_MIN_CHARS {
        Err(InvalidCredentials::PasswordTooShort)
    } else if length > PASSWORD_MAX_CHARS {
        Err(InvalidCredentials::PasswordTooLong)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 아이디는_2자부터_20자까지다() {
        assert_eq!(validate_username(""), Err(InvalidCredentials::Username));
        assert_eq!(validate_username("a"), Err(InvalidCredentials::Username));
        assert_eq!(validate_username("ab"), Ok(()));
        assert_eq!(validate_username(&"a".repeat(20)), Ok(()));
        assert_eq!(
            validate_username(&"a".repeat(21)),
            Err(InvalidCredentials::Username)
        );
    }

    #[test]
    fn 한글_아이디는_바이트가_아니라_글자로_센다() {
        assert_eq!(validate_username("가"), Err(InvalidCredentials::Username));
        assert_eq!(validate_username("하루"), Ok(()));
        assert_eq!(validate_username(&"가".repeat(20)), Ok(()));
        assert_eq!(
            validate_username(&"가".repeat(21)),
            Err(InvalidCredentials::Username)
        );
    }

    #[test]
    fn 아이디에는_한글_영문_숫자_밑줄만_쓴다() {
        assert_eq!(validate_username("Haroo_123하루"), Ok(()));
        assert_eq!(validate_username("__"), Ok(()));
        for bad in [
            "ha roo",
            "haroo!",
            "ha-roo",
            "하루🙂",
            "🙂🙂",
            " haroo",
            "haroo ",
            "ㅎㅎ",
            "éé",
        ] {
            assert_eq!(
                validate_username(bad),
                Err(InvalidCredentials::Username),
                "{bad:?}는 거절되어야 한다"
            );
        }
    }

    #[test]
    fn 비밀번호는_8자_이상이다() {
        assert_eq!(
            validate_password(""),
            Err(InvalidCredentials::PasswordTooShort)
        );
        assert_eq!(
            validate_password("1234567"),
            Err(InvalidCredentials::PasswordTooShort)
        );
        assert_eq!(validate_password("12345678"), Ok(()));
    }

    #[test]
    fn 비밀번호_글자_수는_한글과_이모지도_한_글자로_센다() {
        // 한글 7자는 21바이트지만 7자라서 거절된다.
        assert_eq!(
            validate_password("가나다라마바사"),
            Err(InvalidCredentials::PasswordTooShort)
        );
        assert_eq!(validate_password("가나다라마바사아"), Ok(()));
        assert_eq!(
            validate_password(&"🙂".repeat(7)),
            Err(InvalidCredentials::PasswordTooShort)
        );
        assert_eq!(validate_password(&"🙂".repeat(8)), Ok(()));
    }
}

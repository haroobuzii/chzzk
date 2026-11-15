//! 주소에 들어온 방송 번호 읽기. 화면과 서버가 함께 쓴다.

/// 주소 조각을 방송 번호로 읽는다. 번호가 아니면 `None`.
///
/// ASCII 숫자만 받고, 앞에 0이 붙으면 안 되며, 1 이상이고 i64 범위 안이어야 한다.
/// 어긋나는 주소는 DB를 보지 않고 "방송을 찾을 수 없습니다"로 처리한다.
pub fn parse_number(segment: &str) -> Option<i64> {
    let is_digits = !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit());
    if !is_digits || segment.starts_with('0') {
        return None;
    }
    segment.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 평범한_숫자는_번호다() {
        assert_eq!(parse_number("1"), Some(1));
        assert_eq!(parse_number("42"), Some(42));
        assert_eq!(parse_number("9223372036854775807"), Some(i64::MAX));
    }

    #[test]
    fn 영과_앞에_붙은_영은_번호가_아니다() {
        assert_eq!(parse_number("0"), None);
        assert_eq!(parse_number("01"), None);
    }

    #[test]
    fn 숫자가_아닌_글자가_섞이면_번호가_아니다() {
        for segment in ["", "abc", "1a", "-1", "+1", " 1", "１", "1.0"] {
            assert_eq!(parse_number(segment), None, "{segment:?}");
        }
    }

    #[test]
    fn 너무_큰_수는_번호가_아니다() {
        assert_eq!(parse_number("9223372036854775808"), None);
    }
}

//! 송출 키: 라이브 방송을 시작한 화면만 받는 무작위 값(ADR 0005).
//!
//! DB에는 SHA-256 해시만 남기고, 원문은 시작 응답에만 담는다.

use rand::RngCore;
use sha2::{Digest, Sha256};

/// 송출 키의 무작위 바이트 수. 256비트라 짐작으로 맞힐 수 없다.
const KEY_BYTES: usize = 32;

/// 새 송출 키 원문을 만든다. 16진수 글자 64개다.
pub fn generate() -> String {
    let mut bytes = [0u8; KEY_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// 송출 키 원문의 해시. DB에는 이것만 저장한다.
pub fn hash(key: &str) -> Vec<u8> {
    Sha256::digest(key.as_bytes()).to_vec()
}

/// 받은 송출 키가 저장된 해시와 맞는지 본다.
///
/// 해시끼리 비교하므로 원문 길이나 앞부분이 맞는지는 시간 차이로 드러나지 않는다.
pub fn matches(key: &str, stored_hash: &[u8]) -> bool {
    let candidate = hash(key);
    candidate.len() == stored_hash.len()
        && candidate
            .iter()
            .zip(stored_hash)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 만들_때마다_다른_키가_나온다() {
        let a = generate();
        let b = generate();
        assert_ne!(a, b);
        assert!(a.len() >= 32, "키가 충분히 길어야 한다: {a}");
    }

    #[test]
    fn 맞는_키만_통과한다() {
        let key = generate();
        let stored = hash(&key);

        assert!(matches(&key, &stored));
        assert!(!matches(&generate(), &stored));
        assert!(!matches("", &stored));
    }

    #[test]
    fn 해시에는_원문이_드러나지_않는다() {
        let key = generate();
        let stored = hash(&key);
        assert_eq!(stored.len(), 32);
        assert_ne!(stored, key.as_bytes());
    }
}

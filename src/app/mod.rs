//! 데스크톱 앱 "녹화방송". 배포 서버의 방송 목록을 보고 녹화 방송을 재생하는 보기 전용 앱이다(ADR 0009).
//!
//! 서버 연결 모듈(`deploy_server`)은 앱 빌드가 아니어도 서버 테스트에서 함께 컴파일해 테스트한다.
//! 그래서 테스트 개수를 세는 `cargo test --features server` 한 번에 앱 테스트도 들어간다.

pub mod deploy_server;
#[cfg(test)]
mod deploy_server_tests;

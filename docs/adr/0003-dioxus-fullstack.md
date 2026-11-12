# Rust Dioxus fullstack 크레이트 하나로 화면과 서버를 만든다

강사가 치지직 과제도 Dioxus로 하라고 확인했다. 그래서 단축 URL 과제와 같은 Dioxus 0.7 fullstack 크레이트 하나로 화면과 서버를 함께 만들고, 모노레포는 쓰지 않는다. 단축 URL에서 쓴 구조(서버 함수, axum 라우트 추가, sqlx와 PostgreSQL)를 그대로 다시 쓸 수 있다.

과제에서 어려운 부분(2단계 휴대폰 카메라 송출, 4단계 결제창)은 브라우저 JavaScript API와 JavaScript SDK 쪽에 있다. 이 부분은 Dioxus의 `document::eval`로 작은 JavaScript를 불러 처리한다. 단축 URL의 복사 버튼과 같은 방식이다.

## Considered Options

- **TypeScript + Next.js**: 브라우저 API와 결제 SDK를 바로 쓸 수 있고 자료가 많다. 처음에는 이것을 골랐으나, 강사가 Dioxus를 쓰라고 해서 바꿨다.

-- 3단계: 사용자와 로그인 세션(ADR 0006).

-- 사용자: 아이디 하나와 비밀번호 하나를 가진다.
CREATE TABLE users (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- 아이디 규칙(2~20자, 한글·영문·숫자·밑줄)은 서버가 먼저 확인한다. DB는 길이만 한 번 더 막는다.
    username      TEXT        NOT NULL CHECK (char_length(username) BETWEEN 2 AND 20),
    -- 비밀번호의 Argon2id 해시(PHC 문자열). 원문은 저장하지 않는다.
    password_hash TEXT        NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 영문 대소문자만 다른 아이디(Haroo와 haroo)는 같은 아이디로 친다.
CREATE UNIQUE INDEX users_username_lower ON users (lower(username));

-- 로그인 세션: 쿠키에 담긴 세션 값의 SHA-256 해시로 찾는다. 원문은 저장하지 않는다.
CREATE TABLE sessions (
    token_hash BYTEA       PRIMARY KEY CHECK (octet_length(token_hash) = 32),
    user_id    BIGINT      NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);

-- 3단계: 댓글. 로그인한 사용자가 방송 하나에 남기는 1~300자 글.

CREATE TABLE comments (
    id           BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- 방송을 지우면 그 방송의 댓글도 함께 지워진다.
    broadcast_id BIGINT      NOT NULL REFERENCES broadcasts (id) ON DELETE CASCADE,
    -- 작성자.
    author_id    BIGINT      NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 앞뒤 공백을 지운 본문. 서버가 먼저 확인하고, DB는 길이를 한 번 더 막는다.
    body         TEXT        NOT NULL CHECK (char_length(body) BETWEEN 1 AND 300),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 방송 하나의 댓글을 오래된 순으로 읽는다.
CREATE INDEX comments_broadcast_created ON comments (broadcast_id, created_at, id);

-- 방송 하나 = 방송 번호 하나 + 제목 하나 + 영상 하나.
-- 방송 번호는 DB가 매기고(GENERATED ALWAYS), 직접 지정할 수 없다.
-- 동시에 여러 사람이 등록해도 번호가 겹치지 않고, 한 번 나간 번호는 다시 나오지 않는다.
CREATE TABLE broadcasts (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- 제목은 앞뒤 공백을 지운 뒤 1~100자(문자 수 기준)다. 서버 코드가 먼저 검사하고 DB가 한 번 더 막는다.
    title      TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 100),
    -- 영상 보관 폴더 기준 상대 위치. 예: videos/12.mp4
    video_key  TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

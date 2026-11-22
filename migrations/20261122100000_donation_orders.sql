-- 4단계: 후원 주문서(ADR 0007). "후원하기"를 누를 때 서버가 먼저 만들고, 승인 확인을 통과하면 확정된다.

CREATE TABLE donation_orders (
    id             BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    -- 주문 번호(토스 orderId). 서버가 만든 추측할 수 없는 무작위 값이다.
    order_id       TEXT        NOT NULL UNIQUE CHECK (order_id ~ '^[A-Za-z0-9_-]{6,64}$'),
    -- 방송이 지워져도 주문서는 남긴다(돈과 관련된 기록이다). 방송 번호만 비운다.
    broadcast_id   BIGINT      REFERENCES broadcasts (id) ON DELETE SET NULL,
    -- 후원자와 받는 사람(만들 때의 방송 소유자). 사용자는 지우지 않는다.
    donor_id       BIGINT      NOT NULL REFERENCES users (id),
    recipient_id   BIGINT      NOT NULL REFERENCES users (id),
    -- 금액(원). 서버가 먼저 확인하고, DB가 한 번 더 막는다.
    amount         BIGINT      NOT NULL CHECK (amount BETWEEN 1000 AND 500000 AND amount % 100 = 0),
    -- 앞뒤 공백을 지운 응원 메시지. 없으면 NULL.
    message        TEXT        CHECK (char_length(message) BETWEEN 1 AND 100),
    -- 결제 대기·확정·실패·취소·만료. 결제 대기에서만 다른 상태로 바뀐다.
    status         TEXT        NOT NULL DEFAULT 'pending'
                               CHECK (status IN ('pending', 'confirmed', 'failed', 'canceled', 'expired')),
    -- 토스 결제 키(paymentKey). 결제 한 건은 후원 한 건이다.
    payment_key    TEXT        UNIQUE,
    failure_code   TEXT,
    failure_message TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    confirmed_at   TIMESTAMPTZ,
    CHECK (status <> 'confirmed' OR (payment_key IS NOT NULL AND confirmed_at IS NOT NULL))
);

-- 방송 하나의 확정된 후원을 최신순으로 읽는다.
CREATE INDEX donation_orders_confirmed ON donation_orders (broadcast_id, confirmed_at DESC, id DESC)
    WHERE status = 'confirmed';

-- 오래된 결제 대기 주문서를 만료로 정리할 때 쓴다.
CREATE INDEX donation_orders_pending ON donation_orders (created_at) WHERE status = 'pending';

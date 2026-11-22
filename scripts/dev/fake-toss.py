#!/usr/bin/env python3
"""개발용 가짜 토스 API. 진짜 키 없이 브라우저·curl로 후원 흐름을 확인할 때 쓴다.

쓰는 법:
    python3 scripts/dev/fake-toss.py 9045
    # .env(작업 폴더)에 아래를 넣고 앱을 켠다. 키 값은 가짜면 된다(test_로만 시작하면 된다).
    #   TOSS_CLIENT_KEY=test_ck_fake
    #   TOSS_SECRET_KEY=test_sk_fake
    #   TOSS_API_URL=http://127.0.0.1:9045

흉내 내는 것(docs/toss-payments.md "실제로 확인한 동작"과 같다):
    POST /v1/payments/confirm           받은 값 그대로 DONE.
                                        TossPayments-Test-Code 헤더가 있으면 400 거절이고,
                                        그 결제는 IN_PROGRESS로 남는다(진짜 토스와 같다).
    GET  /v1/payments/{paymentKey}      저장된 결제, 없으면 404 NOT_FOUND_PAYMENT
    GET  /v1/payments/orders/{orderId}  위와 같음
    POST /_seed {paymentKey, orderId, totalAmount, status?}
                                        토스 쪽에서 이미 결제가 끝난 것처럼 결제를 넣는다
                                        (승인 통지 중복을 curl로 확인할 때 쓴다). status 기본값 DONE.

진짜 결제창(카드 입력)은 흉내 내지 않는다. 결과 화면은 /donations/success?paymentKey=…&orderId=…&amount=… 를 직접 연다.
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

payments = {}


def payment(key, order, amount, status="DONE"):
    return {"paymentKey": key, "orderId": order, "status": status, "totalAmount": amount}


class Handler(BaseHTTPRequestHandler):
    def send(self, code, obj):
        body = json.dumps(obj, ensure_ascii=False).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def body(self):
        n = int(self.headers.get("Content-Length") or 0)
        return json.loads(self.rfile.read(n) or b"{}")

    def do_POST(self):
        b = self.body()
        test_code = self.headers.get("TossPayments-Test-Code")
        print("POST", self.path, "test-code:", test_code, flush=True)
        if self.path == "/_seed":
            p = payment(b["paymentKey"], b["orderId"], b["totalAmount"], b.get("status", "DONE"))
            payments[b["paymentKey"]] = p
            return self.send(200, p)
        if self.path == "/v1/payments/confirm":
            if test_code:
                payments.setdefault(
                    b["paymentKey"], payment(b["paymentKey"], b["orderId"], b["amount"], "IN_PROGRESS")
                )
                return self.send(400, {"code": test_code, "message": "한도초과 혹은 잔액부족으로 결제에 실패했습니다."})
            p = payment(b["paymentKey"], b["orderId"], b["amount"])
            payments[b["paymentKey"]] = p
            return self.send(200, p)
        self.send(404, {"code": "NOT_FOUND", "message": "가짜 토스에 없는 주소"})

    def do_GET(self):
        print("GET", self.path, flush=True)
        orders = "/v1/payments/orders/"
        if self.path.startswith(orders):
            order_id = self.path[len(orders):]
            for p in payments.values():
                if p["orderId"] == order_id:
                    return self.send(200, p)
        elif self.path.startswith("/v1/payments/"):
            p = payments.get(self.path[len("/v1/payments/"):])
            if p:
                return self.send(200, p)
        self.send(404, {"code": "NOT_FOUND_PAYMENT", "message": "존재하지 않는 결제 정보 입니다."})

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9045
    print(f"가짜 토스: http://127.0.0.1:{port}", flush=True)
    HTTPServer(("127.0.0.1", port), Handler).serve_forever()

<!--
이 문서는 4단계를 만들 때 토스페이먼츠 공식 문서·API 레퍼런스·공식 샘플에서 확인한 사실 메모다(2026-11).
- 키 값은 적지 않는다. 예전 문서용 공개 키는 지금 쓸 수 없고(폐기됨), 각자 개발자센터에서 받은 테스트 키를 .env에만 둔다.
- "해석"/"unverified"/"inference" 표시가 붙은 항목은 추론이다.
- 토스 API나 SDK를 바꿀 때는 이 문서의 사실부터 다시 확인한다.
-->

# Toss Payments: test-mode donation research notes

Researched 2026-11-20 from docs.tosspayments.com (raw markdown: append `.md` to any docs URL) and
github.com/tosspayments/tosspayments-sample (HEAD 39bc448, 2026-11-17).
Versions: **JS SDK v2** (`https://js.tosspayments.com/v2/standard`, npm `@tosspayments/tosspayments-sdk` 2.10.0);
**REST API path `/v1/*`** with response schema version **2024-06-01** (latest in release notes).
The SDK version and the API version are independent: v1 and v2 SDKs both call `api.tosspayments.com/v1/*`.
The API response version is chosen per key in the developer center, not by a request header. New test stores start on the latest version.
Sources: https://docs.tosspayments.com/guides/v2/get-started/llms-quick-reference (§3), https://docs.tosspayments.com/reference/versioning, https://docs.tosspayments.com/resources/release-note

> **Renamed products (2026):** "결제위젯" (payment widget) is now **주문서형 결제** (checkout-page type) plus **결제창형 결제**
> (payment-window type). "결제창 / API 개별 연동" is now **결제창(구버전)** (legacy payment window).
> Search the docs by the Korean names. Source: llms-quick-reference §4.

## 1. Which product + minimal browser code

| Option | SDK call | Keys | Notes |
|---|---|---|---|
| 주문서형 (Toss default) | `tossPayments.widgets({customerKey})` → `setAmount` → `renderPaymentMethods` + `renderAgreement` → `requestPayment` | `test_gck_` / `test_gsk_` | Needs container divs. successUrl also carries `paymentType` |
| 결제창형 | `widgets()` → `setAmount` → `renderPaymentWindow` → `paymentRequest` event → `requestPayment` | `test_gck_` / `test_gsk_` | Popup |
| 결제창(구버전) / API 개별 연동 | `tossPayments.payment({customerKey}).requestPayment({method:"CARD",...})` | `test_ck_` / `test_sk_` | One call, no UI to render. Docs say "do not pick it for a new integration" |

Source: https://docs.tosspayments.com/guides/v2/get-started/llms-quick-reference (§4, §5)

**Key gotchas that matter for us:**
- `GET /v1/payments/orders/{orderId}` **cannot be called with `test_gsk`/`live_gsk`** (widget) secret keys. Only `test_sk`/`live_sk` work (https://docs.tosspayments.com/reference.md, "orderId로 결제 조회"). I confirmed this live: a `gsk` key got `NOT_FOUND_MERCHANT` on the orderId lookup.
- The webhook-signature **보안 키 (security key) is issued only for API 개별 연동 keys** (https://docs.tosspayments.com/reference/using-api/api-keys, 재발급 대상 table).
- So the legacy `payment()` window with `test_ck`/`test_sk` is the simplest fit for one-off donations. It allows lookup by orderId and webhook signature checks, despite Toss's "legacy" label. The widget is the documented default.

Minimal card payment for the v2 legacy window (https://docs.tosspayments.com/guides/v2/payment-window/integration, https://docs.tosspayments.com/sdk/v2/js/payment):
```html
<script src="https://js.tosspayments.com/v2/standard"></script>   <!-- unversioned, served by Toss (cache max-age=10) -->
<script>
  const tossPayments = TossPayments("{TEST_CLIENT_KEY}");          // test_ck_...
  const payment = tossPayments.payment({ customerKey });           // or { customerKey: TossPayments.ANONYMOUS }
  async function pay() {
    // Save orderId + amount on the server BEFORE this call (anti-tamper)
    await payment.requestPayment({
      method: "CARD",
      amount: { currency: "KRW", value: 5000 },                    // v2 SDK: object; server API: integer
      orderId, orderName: "후원 5,000원",                            // orderName max 100 chars
      successUrl: window.location.origin + "/donate/success",       // must be absolute (origin included)
      failUrl:    window.location.origin + "/donate/fail",
      card: { flowMode: "DEFAULT", useEscrow: false, useCardPoint: false, useAppCardOnly: false },
    });
  }
</script>
```
- Redirect mode (with successUrl/failUrl) resolves to `void`. Promise mode returns the result instead but **does not work on mobile**. `windowTarget`: `iframe` on PC by default; `self` on mobile, where iframe is not allowed. Source: https://docs.tosspayments.com/sdk/v2/js/payment
- `customerKey`: 22–255 chars of `A-Za-z0-9-_=.@`, **at least one special char**, UUID recommended. Never use an email, phone number or member ID. Use `ANONYMOUS` for guests. Source: llms-quick-reference §5.
- Widget variant (sample `express-javascript/public/widget/checkout.html`): `widgets.setAmount({currency:"KRW",value})` must be called before the render calls and `requestPayment`. Then `renderPaymentMethods({selector:"#payment-method"})`, `renderAgreement({selector:"#agreement"})`, `widgets.requestPayment({orderId, orderName, successUrl, failUrl})`.
- To avoid popup-blocking errors, send `Cross-Origin-Opener-Policy: same-origin-allow-popups` (https://docs.tosspayments.com/reference/using-api/authorization).

## 2. Test keys

- Key prefixes: `test_`/`live_` combined with `ck`/`sk` (API 개별 연동) or `gck`/`gsk` (주문서형/결제창형). Toss says these existing prefixes are stable and safe to branch on (llms-quick-reference §5; https://docs.tosspayments.com/sdk/v2/js/environment).
- **Docs keys no longer usable as of Nov 2026.** The docs now say "문서에는 시크릿 키를 보여주지 않아요", meaning they show only `{SECRET_KEY}` placeholders (https://docs.tosspayments.com/reference/using-api/authorization). On 2026-11-17 the sample repo removed its hard-coded keys (commit 39bc448): `test_gck_…(키 값 생략)`, `test_gsk_…(키 값 생략)`, `test_ck_…(키 값 생략)`, `test_sk_…(키 값 생략)`.
  Probed on 2026-11-20: `test_sk_…(키 값 생략)...` → `UNAUTHORIZED_KEY` (revoked). `test_gsk_…(키 값 생략)...` still authenticates (`NOT_FOUND_PAYMENT`) but is no longer published. **Do not rely on either.**
- Getting your own keys: sign up at https://app.tosspayments.com/signup (email only; no business registration or contract needed). You get a "개발 연동 체험 상점" with test keys at https://developers.tosspayments.com/my/api-keys, plus test payment history, API logs and webhooks. Sources: https://docs.tosspayments.com/reference/using-api/api-keys, https://docs.tosspayments.com/blog/how-to-test-toss-payments
- **Test mode charges nothing**: "테스트 환경에서는 결제가 승인되어도 실제 결제수단에서 돈이 출금되지 않아요", even when real card details are entered (https://docs.tosspayments.com/guides/v2/get-started/environment).

## 3. Redirect query params / cancel

- successUrl: `?paymentKey=&orderId=&amount=` (the widget adds `paymentType=NORMAL`). Source: https://docs.tosspayments.com/sdk/v2/js/payment, https://docs.tosspayments.com/guides/v2/payment-widget/integration
- failUrl: `?code=&message=&orderId=`. **Do not call confirm** after a failUrl redirect (llms-quick-reference §5).
- failUrl codes (https://docs.tosspayments.com/sdk/v2/error-codes, "failUrl로 전달되는 에러"):
  - `PAY_PROCESS_CANCELED`: the buyer canceled. **The `orderId` is NOT included**, so the fail handler must accept a missing orderId (https://docs.tosspayments.com/guides/v2/payment-window/integration).
  - `PAY_PROCESS_ABORTED`: approval failed during the process.
  - `REJECT_CARD_COMPANY`: the card was rejected.
- `USER_CANCEL` ("취소되었습니다.") is an **SDK error**, separate from failUrl codes (same error-codes page, SDK tables). Expect it as a rejected `requestPayment` promise, e.g. when the PC iframe window is closed. My reading, not explicitly stated: handle both.
- The blog says the easiest way to reproduce a failUrl redirect is to "request the payment in a mobile environment and close the payment window". Conditions vary by method and environment (https://docs.tosspayments.com/blog/how-to-test-toss-payments).
- Closing the window does not change the payment status, so **no webhook is sent** (https://docs.tosspayments.com/reference/using-api/webhook-events).

## 4. Confirm API

`POST https://api.tosspayments.com/v1/payments/confirm` (https://docs.tosspayments.com/reference.md "결제 승인"; llms-quick-reference §5)
```
Authorization: Basic base64("{SECRET_KEY}:")   # trailing colon required; a UTF-8 BOM yields a "77u/" prefix, re-encode without it
Content-Type: application/json
Idempotency-Key: <uuid v4>                      # optional, recommended
{"paymentKey":"...","orderId":"...","amount":5000}   # amount is an integer here
```
- Before calling, **compare the query `amount` with the server-stored order amount**. A user can change the amount in the browser console *before* authentication, and confirming the authenticated amount would then succeed. Send the stored amount (llms-quick-reference §2.2).
- `paymentKey` is opaque, max 200 chars. Store it as received in a case-sensitive column of at least 200 chars, and never parse it or check its prefix (llms-quick-reference §5).
- Success returns a Payment object. Check `status == "DONE"`, `orderId`, `totalAmount` (fixed at the original amount even after a cancel), `method` (e.g. `"카드"`, localized; `Accept-Language: en-US` returns English) and `version` (`"2024-06-01"`). Also: `balanceAmount`, `approvedAt`, `card{...}`, `receipt.url` (no real receipt in test). Source: https://docs.tosspayments.com/reference.md (Payment 객체)
- Statuses: `READY`, `IN_PROGRESS`, `WAITING_FOR_DEPOSIT`, `DONE`, `CANCELED`, `PARTIAL_CANCELED`, `ABORTED`, `EXPIRED`.
- **Time limit: confirm within 10 minutes of authentication**, or the payment expires: `NOT_FOUND_PAYMENT_SESSION` and status `EXPIRED`. The window itself is valid for 30 minutes (https://docs.tosspayments.com/reference.md; https://docs.tosspayments.com/reference/using-api/webhook-events).
- Errors (https://docs.tosspayments.com/reference/error-codes):
  - 400 `ALREADY_PROCESSED_PAYMENT`: already processed.
  - 400 `INVALID_REQUEST`.
  - 400 `INVALID_API_KEY`.
  - 400 `DUPLICATED_ORDER_ID`: this orderId was already approved or canceled.
  - 400 `INVALID_ORDER_ID`.
  - 400 `BELOW_MINIMUM_AMOUNT`.
  - 400 `EXCEED_MAX_AMOUNT`.
  - 401 `UNAUTHORIZED_KEY`.
  - 403 `REJECT_CARD_COMPANY`.
  - 404 `NOT_FOUND_PAYMENT`.
  - 404 `NOT_FOUND_PAYMENT_SESSION`: the payment session expired.
  - 500 `FAILED_INTERNAL_SYSTEM_PROCESSING`: retry later.
  - 500 `FAILED_PAYMENT_INTERNAL_SYSTEM_PROCESSING`: an error at the card company or bank.
  - `REJECT_CARD_PAYMENT` (limit exceeded or insufficient balance) appears only as the test-header example (https://docs.tosspayments.com/guides/v2/get-started/environment).
- **A confirm failure does not prove the payment failed.** It may already be approved, so query the payment before branching (llms-quick-reference §8).

## 5. Idempotency

Source: https://docs.tosspayments.com/reference/using-api/authorization ("멱등키 헤더").
- `Idempotency-Key: <value>` works on **all POST APIs**. It is ignored on GET.
- Max 300 chars, UUID recommended. Valid for **15 days** from the first request.
- The key is scoped by idempotency key + API key + URL + HTTP method. A repeat returns the **first response** without re-executing.
- Errors:
  - 400 `INVALID_IDEMPOTENCY_KEY`: longer than 300 chars.
  - 409 `IDEMPOTENT_REQUEST_PROCESSING`: the first request is still in flight; retry to get the result.
- Do not retry a failed idempotent request by just changing the key. Find the cause first.
- Design idea (mine): derive the key deterministically per order, e.g. a UUIDv5 of the orderId or a UUID stored with the order. A duplicate successUrl hit or a double click then replays the same response. Separately, keep our DB transition `PENDING→DONE` conditional, e.g. `UPDATE ... WHERE status='PENDING'`.

## 6. Lookup APIs

Source: https://docs.tosspayments.com/reference.md
- `GET /v1/payments/{paymentKey}` returns a Payment object. Works with `sk` and `gsk` keys.
- `GET /v1/payments/orders/{orderId}` returns a Payment object. **`sk` keys only, not `gsk`.**
- Both use the same Basic auth. Errors: `NOT_FOUND_PAYMENT` (404) and others.

## 7. Webhooks

Sources: https://docs.tosspayments.com/guides/v2/webhook, https://docs.tosspayments.com/reference/using-api/webhook-events
- Register in the developer center at https://developers.tosspayments.com/my/webhooks → "웹훅 등록하기": name, URL, events. Webhooks are configured **per MID**, so test and live are separate stores and keys.
- **The URL must be publicly reachable**; a localhost:port URL cannot be registered, so use ngrok for local tests.
- **Webhooks do fire for test payments**: "테스트 결제를 해보세요. 등록한 URL로 웹훅이 발송됩니다". The test store includes webhooks.
- The developer center shows delivery logs (searchable by orderId/paymentKey, last 30 days) and a "다시 시도" (retry) button.
- Payment status event: **`PAYMENT_STATUS_CHANGED`** for card, transfer, mobile and gift certificates. It is sent on transitions to `DONE`, `ABORTED`, `EXPIRED`, `CANCELED` and `PARTIAL_CANCELED` (not `READY`/`IN_PROGRESS`).
- Related events: `CANCEL_STATUS_CHANGED`, and `DEPOSIT_CALLBACK` for virtual accounts.
- Payload: `POST` with a JSON body `{"eventType":"PAYMENT_STATUS_CHANGED","createdAt":"2022-01-01T00:00:00.000000","data":{<full Payment object: paymentKey, orderId, status, version, ...>}}`.
  **This event has no `eventId`**, so dedupe on (paymentKey, status/lastTransactionKey) yourself (my inference).
- Headers:
  - `tosspayments-webhook-transmission-time`
  - `tosspayments-webhook-transmission-retried-count`
  - `tosspayments-webhook-transmission-id`: new on every send, so don't dedupe on it.
  - `tosspayments-webhook-signature: v1:<b64>,v1:<b64>`: allow more than 1KB of header length.
- Signature check: compute HMAC-SHA256 of `"{raw body}:{transmission-time}"` with the **보안 키** and match **any** `v1:` value after base64-decoding it. The security key exists only for API 개별 연동 keys.
- The llms page says *every* webhook carries the signature, *and* that for payment webhooks you should **re-query** `GET /v1/payments/{paymentKey}` to verify status (llms-quick-reference §7.2).
- Safest approach: treat the body as a hint and re-query with the secret key.
- Delivery and retries: return HTTP 200 **within 10 s**. Otherwise Toss retries up to **7 times** at 1, 4, 16, 64, 256, 1024 and 4096 minutes (about 3 days 19 hours in total). A failure email is sent. Retries and manual resends mean **duplicate deliveries are expected**, so the handler must be idempotent.
- Not sent for billing or key-in payments, nor when the window is closed.

## 8. Simulating outcomes in test mode

Sources: https://docs.tosspayments.com/guides/v2/get-started/environment, https://docs.tosspayments.com/blog/how-to-test-toss-payments
- **Success**: there are **no test card numbers**. Enter any real card details; approval is simulated and nothing is charged.
- **Cancel or fail at authentication**: close the payment window. On mobile this gives a failUrl redirect with `PAY_PROCESS_CANCELED`; on PC the SDK promise rejects with `USER_CANCEL`. Insufficient-balance or expired cards still *authenticate* in test mode, so they do not reproduce failures.
- **Server-side errors**: add the header `TossPayments-Test-Code: <ERROR_CODE>` (e.g. `REJECT_CARD_PAYMENT`, `NOT_FOUND_PAYMENT_SESSION`, `ALREADY_PROCESSED_PAYMENT`). It works **with a test secret key only** and is ignored with live keys. Toss then returns that error object exactly as in live.
- **Cancel or refund after DONE**: `POST /v1/payments/{paymentKey}/cancel {"cancelReason":"..."}`, plus `cancelAmount` for a partial cancel. You can also cancel from the developer center's 테스트 결제내역 page (https://developers.tosspayments.com/my/payment-logs).
- **Expire**: wait more than 10 minutes after authentication before confirming. The result is `NOT_FOUND_PAYMENT_SESSION` and status `EXPIRED`, plus a webhook.
- A no-code **sandbox** at https://developers.tosspayments.com/sandbox can issue a test paymentKey/orderId and press confirm.
- Limits:
  - **100 requests per minute per API** in test.
  - Receipt URLs exist but hold no data.
  - KakaoPay needs a contracted MID test key; Payco is live-key only; Naver Pay testing should stick to card.
  - Virtual-account numbers are prefixed `X`; trigger the deposit from 테스트 결제내역.
  - TLS 1.2+ and HTTPS only.

## 9. orderId and amount rules

- `orderId`: **6–64 chars, `[A-Za-z0-9_=-]` only**, unique per payment, random enough to be hard to guess. Violations give `INVALID_ORDER_ID`. Reuse after approval or cancel gives `DUPLICATED_ORDER_ID`. Sources: https://docs.tosspayments.com/reference.md (결제 승인 orderId), https://docs.tosspayments.com/reference/error-codes
- `orderName`: max 100 chars. `metadata`: at most 5 pairs (key ≤ 40 chars without `[`/`]`, value ≤ 2000 chars) (https://docs.tosspayments.com/sdk/v2/js/payment).
- `amount`: KRW integer, no decimals. In the v2 SDK it is an object `{value, currency:"KRW"}`; in the server API it is a plain integer.
  **Minimum: card ≥ 100 KRW, account ≥ 200 KRW** (`BELOW_MINIMUM_AMOUNT`). The maximum depends on contract and card (`EXCEED_MAX_AMOUNT`).
  Sources: llms-quick-reference §5; https://docs.tosspayments.com/reference/error-codes

## 결제 수단 여러 개 (추가 조사)

2026-11-20 조사. 출처는 docs.tosspayments.com의 `.md` 원문이다. "(추론)"은 문서에 직접 쓰여 있지 않은 내 해석이다.

### A. 주문서형(결제위젯) v2: 금액 변경과 결제수단 고르기

- 흐름: `tossPayments.widgets({customerKey})` → `widgets.setAmount({currency:"KRW", value})` → `renderPaymentMethods({selector, variantKey?})` + `renderAgreement({selector, variantKey?})` → 버튼을 누르면 `widgets.requestPayment({orderId, orderName, successUrl, failUrl})`. `setAmount()`는 `renderPaymentMethods()`보다 **먼저** 불러야 한다. 금액이 바뀌면 `setAmount()`를 다시 부른다. 후원 금액 입력칸의 change 이벤트에 걸면 된다. https://docs.tosspayments.com/sdk/v2/js/payment-widget (widgets.setAmount, widgets.renderPaymentMethods)
- `variantKey`는 생략하면 `"DEFAULT"`다. 이 값은 결제 어드민에서 만든 "결제 UI"를 가리킨다. 어떤 결제수단을 보여줄지는 **SDK 인자로 정하지 않는다**. 어드민의 **결제 UI 설정 > 기능 > 결제수단**에서 노코드로 정한다. https://docs.tosspayments.com/guides/v2/payment-widget/admin
- **어드민은 계약을 마친 뒤에만 쓸 수 있다.** 계약 전에는 체험만 할 수 있고, 체험 화면은 테스트 계정과 연결되지 않는다. https://docs.tosspayments.com/guides/v2/payment-widget ("어드민은 토스페이먼츠와 계약을 완료한 뒤에 사용할 수 있어요")
- 계약 전 테스트에서 보이는 수단은 FAQ 기준으로 **카드, 네이버페이, 카카오페이, 토스페이, 페이코, 퀵계좌이체, 휴대폰**이다. 가상계좌는 계약 전에 테스트할 수 없다. https://docs.tosspayments.com/resources/faq ("결제에 사용하고 싶은 결제수단이 보이지 않아요")
  - 다른 문서는 카카오페이는 계약 후 MID 테스트 키로만, 페이코는 라이브 키로만 된다고 한다. 따라서 실제로 결제가 끝까지 되는 것은 카드·토스페이·네이버페이·퀵계좌이체·휴대폰으로 보는 게 맞다. https://docs.tosspayments.com/guides/v2/get-started/environment
- **가상계좌 빼기**: 계약 후 어드민에서 결제수단 목록에서 끄면 된다. 계약 전에는 원래 보이지 않는다.
  - 코드로 막는 방법(추론): 결제 직전에 `paymentMethodWidget.getSelectedPaymentMethod()`의 `method`를 확인해 `VIRTUAL_ACCOUNT`면 요청을 막는다.
  - 서버에서 막는 방법(추론): confirm 응답이 `WAITING_FOR_DEPOSIT`이면 바로 취소한다. 입금 전 취소는 일반 취소와 같다. https://docs.tosspayments.com/guides/v2/cancel-payment (Case 1)
- **키 발급 조건**: 주문서형·결제창형 연동 키(`gck`/`gsk`)는 **전자결제를 신청한 뒤에만** 개발자센터에 나온다. 신청 전에는 "같은 메뉴에 있는 테스트 키" 또는 "문서에 있는 테스트 키"로 해 보라고 안내한다. https://docs.tosspayments.com/guides/v2/payment-widget/integration, https://docs.tosspayments.com/reference/using-api/api-keys
  - 앞 절에서 확인했듯 문서용 키는 이제 공개되지 않는다. 그러니 **사업자 신청 없이 우리 계정만으로 위젯 키를 받을 수 있는지는 불확실하다.** 우리 개발자센터 화면에서 직접 확인해야 한다.

### B. 위젯 시크릿 키(`gsk`)로 되는 것과 안 되는 것

- 되는 것: 결제 승인(confirm), 결제 취소, **paymentKey로 결제 조회**, 거래 조회, 정산 조회. API 버전은 `2024-06-01`로 고정이다. https://docs.tosspayments.com/reference/using-api/api-keys ("주문서형, 결제창형 연동 키 > 시크릿 키")
- 안 되는 것: **`GET /v1/payments/orders/{orderId}`**. 문서에 "`test_gsk`, `live_gsk`로는 호출할 수 없습니다"라고 쓰여 있다(앞 절에서 실제로 확인함). https://docs.tosspayments.com/reference (orderId로 결제 조회)
- 웹훅: `PAYMENT_STATUS_CHANGED`는 카드·계좌이체·휴대폰·상품권 결제에 대해 보내고, 결제 제품 종류로 구분하지 않는다. 그래서 위젯 결제에도 온다고 본다(추론). https://docs.tosspayments.com/reference/using-api/webhook-events
  - 다만 **웹훅 서명용 보안 키는 API 개별 연동 키에만 발급된다**. 위젯만 쓰면 서명을 검증할 수 없다. https://docs.tosspayments.com/reference/using-api/api-keys (재발급 표)
  - 우리는 원래 웹훅을 믿지 않고 paymentKey로 다시 조회하므로 영향은 작다. LLM 문서도 결제 웹훅은 paymentKey로 다시 조회해 확인하라고 한다. https://docs.tosspayments.com/guides/v2/get-started/llms-quick-reference §7.2

### C. 결제창(구버전, API 개별 연동) v2의 `requestPayment`

출처: https://docs.tosspayments.com/sdk/v2/js/payment, https://docs.tosspayments.com/guides/v2/payment-window

- `method`는 `CARD`, `VIRTUAL_ACCOUNT`, `TRANSFER`(퀵계좌이체), `MOBILE_PHONE`, `CULTURE_GIFT_CERTIFICATE`, `FOREIGN_EASY_PAY`(PayPal, USD) 중 하나다. **호출 한 번에 결제수단 하나**만 연다. 그래서 수단 고르는 버튼은 우리가 직접 만들어야 한다.
- `method:"CARD"`에 `card.flowMode:"DEFAULT"`(기본값)를 주면 **카드/간편결제 통합결제창**이 열린다. 이 창 안에서 카드와 간편결제(토스페이·네이버페이·카카오페이 등)를 모두 고를 수 있다. 간편결제를 쓰려고 따로 옵션을 줄 필요는 없다.
  - `DEFAULT`일 때는 `easyPay` 값을 넣어도 무시되고, 기본 통합결제창이 열린다.
  - `flowMode:"DIRECT"`에 `easyPay:"TOSSPAY"` 같은 값을 주면 해당 간편결제 앱(자체창)이 바로 열린다.
  - `cardCompany`는 `DEFAULT`에서는 표시할 카드사를 거르는 필터(`BC|삼성`처럼)로, `DIRECT`에서는 열 카드사 앱으로 쓰인다.
- 결제 결과의 Payment `method` 값은 `카드`·`가상계좌`·`간편결제`·`휴대폰`·`계좌이체`·`문화상품권`·`도서문화상품권`·`게임문화상품권` 중 하나다(한국어로 옴). https://docs.tosspayments.com/reference (Payment 객체)
- 테스트 모드(체험 상점, 계약 전)에서 되는 것. https://docs.tosspayments.com/guides/v2/get-started/environment, https://docs.tosspayments.com/resources/faq
  - 카드: 실제 카드 정보를 넣지만 돈은 나가지 않는다.
  - 토스페이·네이버페이(카드 위주로 테스트할 것)·애플페이·삼성페이 등: 체험 상점 키로 연동할 수 있다.
  - 카카오페이: 계약 후 받은 MID 테스트 키에서만 된다.
  - 페이코: 라이브 키에서만 된다.
  - 계좌이체: 계좌 정보는 가상 값으로 넣어도 되지만 **공동인증서 인증은 실제로 해야 한다**.
  - 상품권: 가상 PIN을 넣으면 되고, 잔액은 항상 10,000원으로 나온다.
  - 휴대폰: 위젯 FAQ에는 계약 전 테스트 목록에 들어 있다. 결제창(구버전)의 `test_ck`에서도 되는지는 **문서로 확인하지 못했다**. 직접 해 봐야 한다.
  - 가상계좌: 계좌번호 앞에 `X`가 붙고, 입금은 개발자센터의 테스트 결제내역에서 처리한다. 체험 상점에서 가능하다는 블로그 글과 계약 전에는 불가하다는 FAQ가 서로 맞지 않는다. https://docs.tosspayments.com/blog/how-to-test-toss-payments

### D. 승인(confirm) 직후 DONE이 아닌 경우

- `WAITING_FOR_DEPOSIT`은 **가상계좌에만 있는 상태**다. 계좌이체·휴대폰·간편결제·카드는 confirm이 성공하면 바로 `DONE`이 된다(상태 정의로 판단). https://docs.tosspayments.com/reference (Payment.status)
- 가상계좌의 흐름은 이렇다. https://docs.tosspayments.com/guides/v2/get-started/llms-quick-reference §5.C, https://docs.tosspayments.com/reference/using-api/webhook-events (DEPOSIT_CALLBACK)
  1. confirm 응답이 `WAITING_FOR_DEPOSIT`이다.
  2. 구매자가 입금한다.
  3. `DEPOSIT_CALLBACK` 웹훅이 온다.
  4. 서버가 다시 조회해서 `DONE`인지 확인한다.
- `DEPOSIT_CALLBACK` 웹훅에 대해 알아둘 것.
  - 본문은 `{createdAt, secret, status, transactionKey, orderId}`이고 **paymentKey가 없다**.
  - `secret`이 confirm 응답의 `secret`과 같으면 진짜 요청이다.
  - 입금 기한이 지나도 웹훅은 오지 않는다. 상태는 `WAITING_FOR_DEPOSIT`으로 남는다.
  - 입금 오류가 나면 `DONE`에서 `WAITING_FOR_DEPOSIT`으로 되돌아간다.
  - `PAYMENT_STATUS_CHANGED`도 함께 등록해 두면 같은 변화에 웹훅이 두 번 온다.
- 가상계좌 환불은 `refundReceiveAccount`(환불받을 계좌)가 있어야 하고, 처리는 취소일 + 2일이 걸린다. 위젯의 환불 계좌 정보는 30분 동안만 조회할 수 있다. 다른 수단의 취소 기한은 계좌이체 180일, 휴대폰은 **결제한 달 안**이다. https://docs.tosspayments.com/guides/v2/cancel-payment
- 후원 흐름에 주는 영향(추론).
  - 가상계좌를 **빼는 것이 가장 간단하다**. 즉시 끝나는 수단(카드/간편결제/계좌이체/휴대폰)만 쓰면, 지금의 "confirm → 조회 → DONE이면 후원 완료" 흐름을 그대로 쓸 수 있다. 바꿀 것은 결과의 `method` 값을 저장하는 정도다.
  - 가상계좌를 지원하려면 다음이 필요하다.
    - 후원에 `WAITING_FOR_DEPOSIT` 상태를 추가한다.
    - 입금 안내 화면을 만든다.
    - `DEPOSIT_CALLBACK`을 처리한다. 이 웹훅은 orderId만 주므로, orderId로 우리 DB에서 paymentKey를 찾아 다시 조회한다(`gsk`는 orderId로 조회할 수 없으니 필수).
    - 입금 기한이 지난 건을 정리하는 작업을 만든다.
    - 알림(방송 화면에 띄우는 후원 표시)은 입금이 끝난 뒤에 보낸다.

### E. 실제 서비스로 배포할 때(라이브 키)

- 라이브 키와 MID는 **토스페이먼츠 전자결제를 신청하고 계약한 뒤**에 발급된다. 토스페이먼츠 심사는 1~2일, 카드사 심사는 최대 14일이 걸린다. https://docs.tosspayments.com/resources/faq ("테스트를 완료했는데 라이브 키 및 상점아이디(MID)는 어떻게 발급받나요?")
- 카드는 주요 카드사 9곳의 심사를 모두 통과해야 한다. 휴대폰은 이동통신사 심사, 상품권은 상품권 종류별 심사가 따로 있다. 카드사 심사 때 실제로 판매하는 사이트와 테스트 키 연동 상태를 본다. https://docs.tosspayments.com/guides/v2/deploy-checklist, https://docs.tosspayments.com/resources/faq
- 개인이나 학생: 토스페이먼츠 공식 블로그는 "대부분의 PG사가 사업자등록번호 입력을 요청"하므로 신청 전에 사업자등록을 먼저 하라고 안내한다. https://www.tosspayments.com/blog/articles/semo-60
  - 사업자 없이 직접 계약할 수 있다는 공식 안내는 찾지 못했다. 정확한 답은 고객센터(1544-7772)에 물어봐야 한다.
  - 후원(기부) 같은 업종이 심사를 통과하는지도 문서에 없다. 계약할 때 물어봐야 한다.
- 배포한 뒤에도 테스트 모드로 둘 수 있나(추론): 테스트 키에 도메인 제한이 있다는 문서는 찾지 못했다. 다만 다음 제약이 있다.
  - 실제 돈이 오가지 않는다.
  - API당 분당 100건으로 제한된다.
  - 영수증에는 데이터가 없다.
  - 웹훅 URL은 공개된 HTTPS 주소여야 한다(이건 장점이 될 수도 있다).
  - 따라서 **데모나 포트폴리오로 배포할 때는 테스트 키로 운영할 수 있다**. 사용자에게 "테스트 결제이며 실제로 돈이 나가지 않는다"고 알려야 한다. 배포 체크리스트는 `live_` 키인지 확인하라고 하지만, 테스트 키를 쓰는 걸 금지하는 문장은 없다.

### F. 우리 프로젝트에 권하는 방식

- **추천: 지금처럼 결제창(구버전) + `test_ck`/`test_sk`를 유지한다.** 수단 고르는 버튼을 우리 화면에 만들어 `CARD`(통합결제창: 카드와 토스페이·네이버페이 등 간편결제를 한 번에), `TRANSFER`, `MOBILE_PHONE`을 지원하고, `VIRTUAL_ACCOUNT`는 넣지 않는다.
  - 근거 1: orderId로 조회할 수 있다. 웹훅 서명 보안 키도 있다.
  - 근거 2: 계약하지 않은 체험 상점 키로 바로 테스트할 수 있다. 위젯 키는 전자결제를 신청해야 나온다.
  - 근거 3: 가상계좌가 처음부터 빠진다. 지금 서버 흐름(confirm → 조회 → DONE)을 거의 바꾸지 않아도 된다.
- 위젯(주문서형)으로 바꾸면 얻는 것과 잃는 것.
  - 얻는 것: Toss가 만든 결제수단 UI, 금액 바꾸기(`setAmount`), 어드민에서 노코드로 수단 관리, Toss가 권하는 기본 제품.
  - 잃는 것: `gsk`로는 orderId로 조회할 수 없다(웹훅 재조회와 비정상 상태 복구 때 paymentKey를 반드시 저장해 둬야 한다). 웹훅 서명용 보안 키가 없다. 수단을 줄이거나 빼는 어드민은 계약 후에만 쓸 수 있다. 계약 후 가상계좌가 켜지면 서버에서 따로 막아야 한다.
  - 위젯은 실제로 계약해서 라이브 MID가 생긴 뒤에 다시 검토할 만하다. 그때도 두 제품의 키와 MID가 달라서 바꾸는 것은 코드 변경이다.

## 실제로 확인한 동작 (2026-11-22, 진짜 테스트 키)

4단계를 사용자의 토스 테스트 키(`test_ck_`/`test_sk_`, 결제창 v2)로 사람이 직접 확인하며 알게 된 사실이다. 가짜 토스(`src/donations/fake_toss.rs`, `scripts/dev/fake-toss.py`)는 이 동작을 흉내 낸다.

- **카드 거절 뒤 조회는 `IN_PROGRESS`다.** 승인 요청에 `TossPayments-Test-Code: REJECT_CARD_PAYMENT`를 붙이면 승인은 거절 오류(4xx)가 오고, 그 뒤 `GET /v1/payments/orders/{orderId}`는 200과 `status: IN_PROGRESS`(승인 전)를 돌려준다. 그래서 "승인 오류 뒤 조회"에서 `IN_PROGRESS`를 "아직 모름"으로 두면 결제 대기로 남는다. 확실한 거절(4xx, 409·429 제외) 뒤의 `READY`·`IN_PROGRESS`는 실패로 본다.
- **거절 메시지**: 결과 화면에 "한도초과 혹은 잔액부족으로 결제에 실패했습니다."가 왔다. 테스트 모드가 카드 거절을 흉내 낼 때의 문구이고, 사용자의 실제 카드와는 관계없다.
- **취소**: PC 결제창을 닫으면 failUrl 대신 SDK가 `USER_CANCEL`로 거절했고, 후원 칸이 실패 API를 불러 주문서가 취소가 됐다.
- **성공 뒤 조회**: 결제 키로 조회하면 `DONE`과 주문서와 같은 금액이 온다. 같은 통지를 여러 번 보내도 매번 이 조회로 판단해 후원은 하나다.
- **개발자센터**: API 키 화면에서 시크릿 키가 "복호화에 실패했다"며 보이지 않을 수 있다. 새로고침·다시 로그인·다른 브라우저로 안 되면 시크릿 키를 재발급한다(테스트 키라 문제없다). 재발급하면 예전 키는 바로 무효다.

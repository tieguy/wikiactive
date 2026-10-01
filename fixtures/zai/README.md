# z.ai contract fixtures (B.0 spike, 2026-09-29; refreshed 2026-09-30)

Captured live by `scripts/zai-spike.sh` (run with `ZAI_API_KEY` in the
environment; the key appears nowhere in these files — only response
headers/bodies are saved, with `set-cookie` redacted).

## Endpoint decision (the B.0 question)

The operator's key is a **Coding-Plan key**:

- `https://api.z.ai/api/coding/paas/v4/chat/completions` → **200**, real
  completions (`assess.response.json`, `propose.response.json`).
- `https://api.z.ai/api/paas/v4/chat/completions` (standard) → **429**
  `{"error":{"code":"1113","message":"Insufficient balance or no resource
  package. Please recharge."}}`
  (`standard-endpoint.response.json`) — authenticated but no package:
  this 429 is **terminal**, not Retry-After-retryable. The client must
  distinguish it from a real rate-limit 429 by the `error.code` body.

Consequence for `src/driver/model.rs`: default base is the standard
`https://api.z.ai/api/paas/v4` (a standard key works out of the box);
this operator exports `ZAI_BASE_URL=https://api.z.ai/api/coding/paas/v4`
(the env switch is exactly the plan's design).

## Wire facts (pinned by `tests/integrations.rs` in B.1)

- OpenAI-compatible chat completions; non-streaming; bearer auth.
- Model `glm-5.3`, echoed in the response `"model"` field.
- `choices[0].message` has **three keys**: `role`, `content`,
  `reasoning_content` (the chain-of-thought arrives in its own field —
  never in `content`; parse `content` only).
- `content` is either raw JSON or ```json-fenced JSON (both observed) —
  the parser strips optional fences.
- `finish_reason: "stop"` on both captures; `usage` includes
  `completion_tokens_details.reasoning_tokens` and
  `prompt_tokens_details.cached_tokens`.
- **No** `Retry-After` / `x-ratelimit` headers on the 200s or on the
  1113-429. True rate-limit 429s were not observed (backoff policy:
  honor `Retry-After` when present, else exponential; never retry 1113).
- The identifying User-Agent (`src/lib.rs::USER_AGENT`) is accepted.

## Files

- `assess.request.json` / `propose.request.json` — the two judgment
  points' request shapes (temperature 0.2; system+user messages). The
  assess pair was refreshed 2026-09-30 for the loop-mechanization rename
  (findings → assessments, `AS<n>` ids); the capture shows the model
  following the AS schema.
- `assess.response.json` — the assessment step's output, `AS<n>` ids.
- `propose.response.json` — raw-JSON proposal output, `reasoning_content`
  populated.
- `standard-endpoint.response.json` — the 1113 rejection.
- `*.response.headers` — response headers (set-cookie redacted).
- `endpoint.txt` — the base URL the successful capture used.

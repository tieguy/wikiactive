#!/usr/bin/env bash
# B.0 — z.ai contract spike (plan-003).
#
# Captures request/response fixtures for the two model judgment points
# (findings authoring, proposal drafting) into fixtures/zai/, plus response
# headers (rate-limit fields) and HTTP status. Run with the key in the
# environment only — it is never written anywhere by this script
# (only RESPONSE headers are saved; the Authorization header is request-side).
#
#   export ZAI_API_KEY=...
#   ./scripts/zai-spike.sh
#
# Environment overrides:
#   ZAI_BASE_URL  default https://api.z.ai/api/paas/v4  (the Coding-Plan
#                 endpoint switch: set this if the key is a Coding-Plan key)
#   ZAI_MODEL     default glm-5.3
set -euo pipefail

: "${ZAI_API_KEY:?export ZAI_API_KEY first}"
BASE="${ZAI_BASE_URL:-https://api.z.ai/api/paas/v4}"
MODEL="${ZAI_MODEL:-glm-5.3}"
OUT="fixtures/zai"
UA="wikiactive/0.1 (en.wikipedia User:LuisVilla; luis@lu.is)"
mkdir -p "$OUT"

echo "endpoint: $BASE  model: $MODEL" | tee "$OUT/endpoint.txt"

# --- findings-authoring-shaped request -------------------------------------
cat > "$OUT/findings.request.json" <<'JSON'
{
  "model": "MODEL_PLACEHOLDER",
  "messages": [
    {"role": "system", "content": "You author Wikipedia improvement findings. Given article context and fetched source text, output a JSON array of findings: {id, wikitext_anchor, rules, evidence, factual_note, proposed_fix, loop}. Every factual note must quote-anchor to the provided source text verbatim. Output JSON only."},
    {"role": "user", "content": "Article: Sarah Kidder (en.wikipedia). Base wikitext (Personal life section): \"Born Sarah A. Clark in Ohio{{Citation needed|date=April 2019}}, Kidder married [[civil engineer]] [[John Flint Kidder]] in 1870.\"\n\nSource S2 (San Francisco Call obituary, 1901-04-11, operator-captured text): \"Mr. Ki3u«sr was married In ; 1874 ' to Miss S. lA; Clark, a native of Ohio.\"\n\nEntry loop: 2 (mine existing sources). Author findings for this section."}
  ],
  "temperature": 0.2
}
JSON
sed -i "s/MODEL_PLACEHOLDER/$MODEL/" "$OUT/findings.request.json"

# --- proposal-drafting-shaped request ---------------------------------------
cat > "$OUT/propose.request.json" <<'JSON'
{
  "model": "MODEL_PLACEHOLDER",
  "messages": [
    {"role": "system", "content": "You draft one scoped Wikipedia edit from an approved finding. Output JSON: {proposed_wikitext_block, edit_summary}. Change only what the finding scopes; reuse the article's existing sentence structure and citation style. Output JSON only."},
    {"role": "user", "content": "Finding F3 (loop 2): restore the marriage year to 1874 cited to the existing sfcall ref; remove the Ohio {{Citation needed}} (same source); add an {{efn}} recording True West's 1870 with a {{notelist}}. Base wikitext line: \"Born Sarah A. Clark in Ohio{{Citation needed|date=April 2019}}, Kidder married [[civil engineer]] [[John Flint Kidder]] in 1870.<ref name=\\\"truewestmagazine\\\" />\". Available named refs: sfcall, truewestmagazine."}
  ],
  "temperature": 0.2
}
JSON
sed -i "s/MODEL_PLACEHOLDER/$MODEL/" "$OUT/propose.request.json"

for name in findings propose; do
  echo "--- $name ---"
  curl -sS -m 120 -A "$UA" \
    -D "$OUT/$name.response.headers" \
    -o "$OUT/$name.response.json" \
    -w "status %{http_code}\n" \
    -H "Authorization: Bearer $ZAI_API_KEY" \
    -H "Content-Type: application/json" \
    "$BASE/chat/completions" \
    --data-binary "@$OUT/$name.request.json" || echo "curl failed for $name (recorded whatever landed)"
done

echo
echo "captured into $OUT/:"
ls -la "$OUT"
echo
echo "rate-limit headers of interest:"
grep -iE "ratelimit|retry-after|x-rate|x-request-id" "$OUT"/*.response.headers || echo "(none present)"

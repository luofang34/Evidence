#!/usr/bin/env bash
# Refuse lowering `cert/floors.toml` floors without explicit
# justification. The contract (PR #48 / LLR-037):
#
#   rigor only goes up. Raising a floor is a PR that edits the
#   TOML. Lowering a floor must be accompanied by a line in the PR
#   body (or direct-to-main commit message) matching:
#
#     Lower-Floor: <dimension> <free-text reason>
#
# Without the line, this script emits FLOORS_LOWERED_WITHOUT_
# JUSTIFICATION and exits 1.
#
# Usage:
#   scripts/floors-lower-lint.sh [base-ref]   # default: origin/main
#
# Environment:
#   PR_BODY          body text to check for `Lower-Floor:` lines.
#                    CI sets from github.event.pull_request.body.
#                    Fallback: last commit message body.
#   FLOORS_BASE_CONTENT / FLOORS_HEAD_CONTENT
#                    override the git read (integration-test hook).
#                    If either is set, both must be.
set -euo pipefail

BASE_REF="${1:-origin/main}"

# Work from the repo root so `cert/floors.toml` resolves predictably.
cd "$(git rev-parse --show-toplevel)"

# Fetch base and HEAD content. Test hook: env-var overrides skip git.
if [[ -n "${FLOORS_BASE_CONTENT:-}" && -n "${FLOORS_HEAD_CONTENT:-}" ]]; then
    base_text="$FLOORS_BASE_CONTENT"
    head_text="$FLOORS_HEAD_CONTENT"
elif [[ -n "${FLOORS_BASE_CONTENT:-}" || -n "${FLOORS_HEAD_CONTENT:-}" ]]; then
    echo "::error::both FLOORS_BASE_CONTENT and FLOORS_HEAD_CONTENT must be set together" >&2
    exit 2
else
    # Base ref may not exist on a first-commit repo; default to empty.
    base_text=$(git show "${BASE_REF}:cert/floors.toml" 2>/dev/null || true)
    head_text=$(cat cert/floors.toml 2>/dev/null || true)
fi

# Parse `name = value` pairs inside the [floors] section only.
# Stops at the next `[section]` header. Assumes ASCII names and
# decimal integer values (matches cert/floors.toml shape).
extract_floors() {
    # Portable awk (works on BSD/mac + gawk): filter lines inside
    # [floors] of shape `name = number`, print `name value`.
    awk '
        /^\[floors\][[:space:]]*$/ { in_floors = 1; next }
        /^\[/ { in_floors = 0 }
        in_floors && /^[[:space:]]*[a-z_][a-z_0-9]*[[:space:]]*=[[:space:]]*[0-9]+/ {
            # Split on `=`, trim whitespace on both sides.
            split($0, parts, "=")
            name = parts[1]
            val = parts[2]
            gsub(/[[:space:]]/, "", name)
            # Trim leading whitespace on val, then truncate at first
            # non-digit (whitespace, comment, end-of-line).
            sub(/^[[:space:]]+/, "", val)
            sub(/[^0-9].*$/, "", val)
            print name, val
        }
    ' <<<"$1"
}

base_floors=$(extract_floors "$base_text")
head_floors=$(extract_floors "$head_text")

# Build associative map of HEAD floors.
declare -A head_map=()
while IFS=' ' read -r name val; do
    [[ -z "$name" ]] && continue
    head_map["$name"]="$val"
done <<<"$head_floors"

# Find decreases (base > head).
decreases=""
while IFS=' ' read -r name base_val; do
    [[ -z "$name" ]] && continue
    head_val="${head_map[$name]:-}"
    # Missing in HEAD = floor was deleted; treat as a decrease.
    if [[ -z "$head_val" ]]; then
        decreases+="$name $base_val DELETED"$'\n'
        continue
    fi
    if (( head_val < base_val )); then
        decreases+="$name $base_val $head_val"$'\n'
    fi
done <<<"$base_floors"

if [[ -z "$decreases" ]]; then
    echo "no floor decreases detected."
    exit 0
fi

# Decreases exist — require a Lower-Floor: justification line per
# dimension in the PR body / commit message.
body="${PR_BODY:-$(git log -1 --pretty=%B)}"

missing=""
while IFS=' ' read -r name base_val head_val; do
    [[ -z "$name" ]] && continue
    if ! grep -qE "^Lower-Floor: ${name}[[:space:]]+" <<<"$body"; then
        missing+="  ${name}: ${base_val} -> ${head_val} (no 'Lower-Floor: ${name} <reason>' line in body)"$'\n'
    fi
done <<<"$decreases"

if [[ -n "$missing" ]]; then
    echo "::error::FLOORS_LOWERED_WITHOUT_JUSTIFICATION: unjustified floor decrease(s):"
    printf '%s' "$missing"
    exit 1
fi

echo "all floor decreases have a matching 'Lower-Floor:' line."
exit 0

#!/usr/bin/env bash
# Fetch the cross-time determinism baseline artifact for a PR's exact
# base commit, failing closed if it is missing, expired, or malformed.
# A required cross-time check that soft-skipped on a missing baseline
# would pass without ever comparing — the exact hole this closes.
#
# Inputs (environment):
#   BASE_SHA           the PR base commit SHA (the current `main` tip
#                      under strict up-to-date checks)              [req]
#   GITHUB_REPOSITORY  owner/repo                                    [req]
#   GH_TOKEN           token for `gh` (unused by a test's fake gh)   [ci]
#   GITHUB_OUTPUT      file to append `prior_missing=0` to on success [opt]
#   GH                 override for the `gh` invocation, e.g.
#                      "bash /path/fake-gh" (tests)                  [opt]
# Args:
#   $1                 destination directory for the artifact        [req]
#
# Exit status: 0 (and `prior_missing=0`) when a well-formed baseline was
# fetched; non-zero (fail closed) on no successful base run, an
# unavailable/expired artifact, a missing manifest, or malformed JSON.
set -euo pipefail

dest="${1:?destination directory required}"
: "${BASE_SHA:?BASE_SHA required}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY required}"
mkdir -p "$dest"

# The `gh` command, overridable so the fail-closed paths are testable
# without a real gh and without depending on a fake's shebang (a pure
# Nix sandbox lacks /usr/bin/env, which exit-126s a `#!/usr/bin/env`
# fake). Default: the real `gh`.
read -r -a gh_cmd <<< "${GH:-gh}"

fail_closed() {
  echo "::error::$1"
  echo "Base SHA: ${BASE_SHA}"
  echo "Fix: bring this PR up to date with \`main\` (its tip publishes the baseline), or re-run CI on the base commit to republish its xhost-Linux artifact."
  exit 1
}

# Bind to the base SHA — the run for the PR's exact base commit, not the
# newest successful run (which could be a stale historical tip).
last_run=$("${gh_cmd[@]}" api \
    "repos/${GITHUB_REPOSITORY}/actions/workflows/ci.yml/runs?head_sha=${BASE_SHA}&status=success&per_page=1" \
    --jq '.workflow_runs[0].id // empty')
if [ -z "$last_run" ]; then
  fail_closed "No successful ci.yml run for base SHA ${BASE_SHA}; the cross-time baseline is not published for this base commit."
fi

if ! "${gh_cmd[@]}" run download "$last_run" --name xhost-Linux --dir "$dest" 2>/dev/null; then
  fail_closed "Baseline artifact xhost-Linux for base SHA ${BASE_SHA} (run ${last_run}) is unavailable (expired or missing)."
fi

manifest="$dest/deterministic-manifest.json"
if [ ! -f "$manifest" ]; then
  fail_closed "Baseline artifact for base SHA ${BASE_SHA} is missing deterministic-manifest.json (indeterminate)."
fi

# Validate the manifest is well-formed JSON before trusting it as a
# baseline; a truncated or corrupt artifact is fail-closed, not a pass.
if ! jq empty "$manifest" 2>/dev/null; then
  fail_closed "Baseline artifact for base SHA ${BASE_SHA} has a malformed deterministic-manifest.json."
fi

echo "prior toolchain projection:"
jq '{rustc, cargo, llvm_version, cargo_lock_hash, rustflags}' "$manifest"

# Only now — after the manifest is present AND parses — record that a
# usable baseline was fetched.
if [ -n "${GITHUB_OUTPUT:-}" ]; then
  echo "prior_missing=0" >> "$GITHUB_OUTPUT"
fi

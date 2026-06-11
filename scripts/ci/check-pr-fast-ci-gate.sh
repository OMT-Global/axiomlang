#!/usr/bin/env bash
set -euo pipefail

failed=0
is_fork_pr="${IS_FORK_PR:-false}"
branch_validation_jobs=" fast-checks validate-pr-description validate-secrets "

for entry in $RESULTS; do
  job="${entry%%=*}"
  status="${entry##*=}"

  if [[ "$is_fork_pr" == "true" && "$branch_validation_jobs" == *" $job "* && "$status" == "skipped" ]]; then
    echo "FAIL $job => $status (fork PR branch validation must not be skipped)"
    failed=1
    continue
  fi

  if [[ "$status" == "success" || "$status" == "skipped" || "$status" == "cancelled" ]]; then
    echo "OK   $job => $status"
  else
    echo "FAIL $job => $status"
    failed=1
  fi
done

exit "$failed"

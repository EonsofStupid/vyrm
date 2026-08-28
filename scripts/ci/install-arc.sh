#!/usr/bin/env bash
set -euo pipefail

readonly arc_chart_version="0.14.2"
readonly controller_namespace="arc-systems"
readonly runner_namespace="arc-runners"
readonly github_secret="rrflow-arc-github"

repository="${GITHUB_REPOSITORY:-EonsofStupid/rrflow}"
github_config_url="${GITHUB_CONFIG_URL:-https://github.com/${repository}}"
repository_root="$(git rev-parse --show-toplevel)"
values_root="${repository_root}/deploy/ci/github-actions/arc"

for required_tool in gh helm kubectl; do
  if ! command -v "$required_tool" >/dev/null 2>&1; then
    printf 'required tool is missing: %s\n' "$required_tool" >&2
    exit 1
  fi
done

repository_visibility="$(gh repo view "$repository" --json visibility --jq .visibility)"
if [[ "$repository_visibility" != "PRIVATE" ]]; then
  printf 'refusing to register self-hosted runners: %s is %s, not PRIVATE\n' \
    "$repository" "$repository_visibility" >&2
  exit 1
fi

kubectl create namespace "$controller_namespace" \
  --dry-run=client --output=yaml | kubectl apply --filename=-
kubectl create namespace "$runner_namespace" \
  --dry-run=client --output=yaml | kubectl apply --filename=-

# The token never appears in argv, a values file, or terminal output.
gh auth token | tr -d '\n' | kubectl --namespace "$runner_namespace" create secret generic "$github_secret" \
  --from-file=github_token=/dev/stdin \
  --dry-run=client --output=yaml | kubectl apply --filename=-

helm upgrade --install arc \
  --namespace "$controller_namespace" \
  --version "$arc_chart_version" \
  --values "$values_root/controller-values.yaml" \
  oci://ghcr.io/actions/actions-runner-controller-charts/gha-runner-scale-set-controller

kubectl --namespace "$controller_namespace" wait \
  --for=condition=Available deployment --all --timeout=180s

for runner_class in standard heavy; do
  helm upgrade --install "rrflow-${runner_class}" \
    --namespace "$runner_namespace" \
    --version "$arc_chart_version" \
    --set-string "githubConfigUrl=${github_config_url}" \
    --values "$values_root/rrflow-${runner_class}.values.yaml" \
    oci://ghcr.io/actions/actions-runner-controller-charts/gha-runner-scale-set
done

kubectl --namespace "$runner_namespace" get autoscalingrunnersets.actions.github.com
kubectl --namespace "$runner_namespace" get pods

gh variable set CI_LINUX_STANDARD_RUNNER \
  --repo "$repository" --body '["rrflow-standard"]'
gh variable set CI_LINUX_HEAVY_RUNNER \
  --repo "$repository" --body '["rrflow-heavy"]'
gh variable set CI_SELF_HOSTED_ENABLED \
  --repo "$repository" --body 'true'

printf 'ARC is installed. Dispatch a trusted proof run with:\n'
printf '  gh workflow run ci.yml --repo %s --ref <branch>\n' "$repository"

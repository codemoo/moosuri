#!/usr/bin/env bash
# Run in the installation administrator's authenticated Google Cloud Shell.
set -euo pipefail
export LC_ALL=C
project=${1:-}
parent=${2:-}
if [[ $# -gt 2 || ! $project =~ ^[a-z][a-z0-9-]{4,28}[a-z0-9]$ ]] ||
   [[ -n $parent && ! $parent =~ ^--(folder|organization)=[0-9]+$ ]]; then
  echo 'Usage: bash deployment/kiri/prepare-google-project.sh PROJECT_ID [--folder=NUMBER | --organization=NUMBER]' >&2
  exit 2
fi
services=(gmail.googleapis.com drive.googleapis.com docs.googleapis.com slides.googleapis.com sheets.googleapis.com)
describe() {
  gcloud projects describe "$project" --format='value(lifecycleState)'
}
if ! state=$(describe 2>/dev/null); then
  echo "Creating Google Cloud project $project"
  create_args=("$project" "--name=Moosuri")
  if [[ -n $parent ]]; then create_args+=("$parent"); fi
  if ! gcloud projects create "${create_args[@]}" --quiet; then
    # A concurrent run may already have created this exact project.
    if ! state=$(describe); then
      echo 'Google could not create or open this project. Check the project ID, project-creation permission and organization policy, then retry with the same ID.' >&2
      exit 1
    fi
  else
    state=$(describe)
  fi
fi
if [[ $state != ACTIVE ]]; then
  echo "The project $project is not active. Check its state in Google Cloud before retrying." >&2
  exit 1
fi
echo "Enabling Google Workspace APIs in $project"
gcloud services enable "${services[@]}" --project="$project" --quiet
enabled=$(gcloud services list --enabled --project="$project" --format='value(config.name)')
for service in "${services[@]}"; do
  if ! grep -Fxq -- "$service" <<<"$enabled"; then
    echo "$service is not yet listed as enabled. Retry with the same project ID." >&2
    exit 1
  fi
done
echo "Ready: $project. Return to Moosuri to complete Google OAuth app registration."

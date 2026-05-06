#!/usr/bin/env bash

set -euo pipefail

TAG="${1:-}"

if [[ -z "${TAG}" ]]; then
  echo "usage: $0 <tag>" >&2
  exit 1
fi

if ! git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  echo "tag not found: ${TAG}" >&2
  exit 1
fi

PREVIOUS_TAG="$(
  git tag --list 'v*' --sort=-version:refname \
    | awk -v current="${TAG}" '$0 != current { print; exit }'
)"

if [[ -n "${PREVIOUS_TAG}" ]]; then
  RANGE="${PREVIOUS_TAG}..${TAG}"
  RANGE_LABEL="${PREVIOUS_TAG} -> ${TAG}"
else
  RANGE="${TAG}"
  RANGE_LABEL="initial release -> ${TAG}"
fi

{
  printf '## Changelog\n\n'
  printf 'Range: `%s`\n\n' "${RANGE_LABEL}"

  CHANGES="$(git log --no-merges --pretty=format:'- %s (%h)' "${RANGE}")"
  if [[ -n "${CHANGES}" ]]; then
    printf '%s\n' "${CHANGES}"
  else
    printf -- '- No non-merge commits found in this range.\n'
  fi

  printf '\n## Install Note\n\n'
  printf 'This is an unsigned macOS test build.\n\n'
  printf 'If macOS blocks the app, go to `System Settings -> Privacy & Security` and click `Open Anyway`.\n\n'
  printf 'Advanced users can also run:\n\n'
  printf '```bash\n'
  printf 'xattr -dr com.apple.quarantine "/Applications/Zest Wallpaper.app"\n'
  printf '```\n'
} 

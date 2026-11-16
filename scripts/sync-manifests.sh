#!/usr/bin/env bash
# Refreshes the vendored RedStone manifests from the monorepo's main branch (feedwatch-platform ADR 0004).
set -euo pipefail
cd "$(dirname "$0")/.."
REPO=redstone-finance/redstone-oracles-monorepo
DIR=packages/relayer-remote-config/fallback/relayer-manifests-multi-feed
SHA=$(gh api "repos/$REPO/commits/main" --jq .sha)
for f in ethereumMultiFeed arbitrumOneMultiFeed baseMultiFeed; do
  gh api "repos/$REPO/contents/$DIR/$f.json?ref=$SHA" --jq .content | base64 -d > "manifests/$f.json"
  echo "$f: $(jq '.priceFeeds | length' "manifests/$f.json") feeds"
done
echo "Fetched at $SHA. Update manifests/SOURCE.md and the counts in the src/manifest.rs tests."

# Manifest source

Vendored from [`redstone-finance/redstone-oracles-monorepo`](https://github.com/redstone-finance/redstone-oracles-monorepo), commit `c75bac13a143f8168731999aa29c99837fa6843f` (2026-11-16), path `packages/relayer-remote-config/fallback/relayer-manifests-multi-feed/`.

| File | Chain | Feeds |
|---|---|---|
| `ethereumMultiFeed.json` | Ethereum (1) | 95 |
| `arbitrumOneMultiFeed.json` | Arbitrum One (42161) | 21 |
| `baseMultiFeed.json` | Base (8453) | 30 |

Refresh with `scripts/sync-manifests.sh` in its own pull request, then update this file and the counts in the `src/manifest.rs` tests.

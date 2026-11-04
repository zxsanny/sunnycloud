# Infrastructure Topology

## Push targets (VCS redundancy)

| Remote | URL | Role | Host kind |
|--------|-----|------|-----------|
| origin | https://github.com/zxsanny/sunnycloud.git | primary | github |
| (not configured) | git.azaion.com | fallback, Forgejo; CI on ci.azaion.com (Woodpecker) | forgejo |

## Host roles

| Role | Host | Notes |
|------|------|-------|
| dev | (local workstation) | |

## Platform inventory

| Component | Host | Type / URL | Status | Notes |
|-----------|------|------------|--------|-------|
| git | github.com | GitHub | present | origin |
| builder | GitHub Actions | CI | TBD | fallback: ci.azaion.com Woodpecker |
| registry | | | TBD | |
| deploy | | | TBD | |

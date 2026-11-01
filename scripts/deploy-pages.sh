#!/usr/bin/env bash
# Builds web/ and force-pushes web/dist to the gh-pages branch (GitHub Pages serves it).
set -euo pipefail
cd "$(dirname "$0")/../web" && npm run build && cd dist && touch .nojekyll
rm -rf .git && git init -q -b gh-pages && git add -A && git -c user.name="Mbanya Dev" -c user.email=mbanyadev@users.noreply.github.com commit -qm "Deploy web app" \
  && git push -qf "$(git -C ../.. remote get-url origin)" gh-pages && rm -rf .git

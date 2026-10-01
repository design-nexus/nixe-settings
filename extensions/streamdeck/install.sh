#!/bin/bash
# Fetches galleon-deck (https://github.com/NLMP-DDHS/galleon-deck) at a known
# commit into vendor/. Its packages, installer and device access are run later
# from the Stream Deck page, one button each.
set -euo pipefail
cd "$(dirname "$0")"

REPO=https://github.com/NLMP-DDHS/galleon-deck.git
COMMIT=bbd622c5ea57f04a9e4c2dcc85e085dc69bb79a5

rm -rf vendor/.galleon-deck.tmp
mkdir -p vendor/.galleon-deck.tmp
git -C vendor/.galleon-deck.tmp init -q
git -C vendor/.galleon-deck.tmp fetch -q --depth 1 "$REPO" "$COMMIT"
git -C vendor/.galleon-deck.tmp checkout -q FETCH_HEAD
rm -rf vendor/galleon-deck
mv vendor/.galleon-deck.tmp vendor/galleon-deck

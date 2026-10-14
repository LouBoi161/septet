#!/usr/bin/env bash
# The automatic upstream sync (twice a week at night: scripts/systemd/septet-sync.timer). In a worktree of
# its own, so the working copy is never touched, it brings every app that is behind to its newest ArtCraft
# release with sync-upstream.sh, on a branch upstream/<date> from origin/main.
#
# While a sync merge request (from an upstream/ branch) is open, nothing happens: one sync at a time.
# All passed: the branch goes to GitLab with a draft merge request to review (nothing goes to main).
# A conflict or a failure: nothing is pushed; a desktop notification says where the logs are, and the
# worktree stays for resolving it (see sync-upstream.sh). Every app up to date: nothing happens.
#
# Settings (environment): SYNC_CACHE (default ~/.cache/septet-sync: worktree, build folder, logs),
# SYNC_JOBS (cargo jobs, default 8), SYNC_PUSH=0 (sync and test, but push nothing).
set -euo pipefail
repo=$(git -C "$(dirname "$(readlink -f "$0")")" rev-parse --show-toplevel)
cache=${SYNC_CACHE:-$HOME/.cache/septet-sync}
today=$(date +%F)
branch=upstream/$today
work=$cache/worktree
logs=$cache/logs/$today
mkdir -p "$logs"
exec >>"$logs/nightly.log" 2>&1
echo "== $(date '+%F %T') nightly sync"

cd "$repo" # glab finds the project from the repository's remote
notify() { notify-send -a Septet -i software-update-available "$1" "$2" 2>/dev/null || true; }

# One sync at a time: while an earlier one waits for review, its merge request comes first.
waiting=$(glab api 'projects/:id/merge_requests?state=opened&per_page=100' 2>/dev/null |
    python3 -c 'import json, sys; print(" ".join(m["web_url"] for m in json.load(sys.stdin) if m["source_branch"].startswith("upstream/")))' || true)
if [ -n "$waiting" ]; then
    echo "a sync is waiting for review: $waiting"
    exit 0
fi
git -C "$repo" fetch -q origin main
# A worktree left by a run that stopped at a conflict is the user's to finish: keep it.
if [ -d "$work" ]; then
    if [ -e "$(git -C "$work" rev-parse --git-path MERGE_HEAD)" ]; then
        echo "the worktree $work still has an unfinished merge; not starting another sync"
        notify "Septet: upstream sync waiting" "An earlier sync stopped at a conflict in $work."
        exit 0
    fi
    git -C "$repo" worktree remove --force "$work"
fi
git -C "$repo" worktree prune
git -C "$repo" worktree add -q --detach "$work" origin/main
cd "$work"

behind=$(scripts/sync-upstream.sh --check all | grep behind || true)
if [ -z "$behind" ]; then
    echo "every app is at its newest release"
    cd "$repo" && git worktree remove --force "$work"
    exit 0
fi
echo "$behind"
if git ls-remote --exit-code --heads origin "$branch" >/dev/null 2>&1; then
    echo "origin already has $branch; not syncing again today"
    exit 0
fi
git switch -q -C "$branch" origin/main

status=0
SYNC_OUT=$logs SYNC_TARGET=$cache/target CARGO_TARGET_DIR=$cache/target SYNC_JOBS=${SYNC_JOBS:-8} \
    scripts/sync-upstream.sh --branch "$branch" all >"$logs/sync.log" 2>&1 || status=$?
cat "$logs/sync.log"
apps=$(sed -n 's/^== \([a-z]*\) \(.*\)/\1 \2/p' "$logs/sync.log" | paste -sd, - | sed 's/,/, /g')

if [ $status != 0 ]; then
    what=$([ $status = 2 ] && echo "stopped at a merge conflict" || echo "failed")
    echo "sync $what (exit $status); nothing pushed"
    notify "Septet: upstream sync $what" "$apps. Logs: $logs; worktree: $work"
    exit $status
fi
if [ "${SYNC_PUSH:-1}" = 0 ]; then
    notify "Septet: upstream sync passed" "$apps, on $branch in $work (not pushed)"
    exit 0
fi
git push -q -u origin "$branch"
description=$(
    echo "Automatic upstream sync ($(basename "$0"), $today): $apps."
    echo
    echo "Built, tested (septet, each app's tests) and autotested on Xvfb. Logs and screenshots on the machine that ran it: \`$logs\`."
    echo
    echo '```'
    grep -E '^(==|  )' "$logs/sync.log"
    echo '```'
    echo
    echo "🤖 Generated with [Claude Code](https://claude.com/claude-code)"
)
url=$(glab mr create --draft --source-branch "$branch" --target-branch main --title "Upstream sync $today: $apps" \
    --description "$description" --yes 2>&1 | grep -o 'https://[^ ]*merge_requests/[0-9]*' | tail -n 1 || true)
echo "draft merge request: ${url:-not created}"
notify "Septet: upstream sync ready to review" "$apps: ${url:-branch $branch pushed, merge request not created}"
cd "$repo" && git worktree remove --force "$work"

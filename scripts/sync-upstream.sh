#!/usr/bin/env bash
# Bring apps up to their newest ArtCraft release: fetch the release tag, `git subtree merge` it into the
# app's folder on a branch `upstream/<date>`, then build, run the tests and the autotests.
#
# Usage: scripts/sync-upstream.sh [options] <app>|all [tag]
#   <app>        photocraft, vectorcraft, lightcraft, designcraft, pdfcraft, filmcraft or effectcraft
#   all          every app with a newer release (designcraft … photocraft, smallest change first); stops at the first
#                conflict or failure
#   tag          a release tag (one app only); default: the app's latest GitHub release
# Options:
#   --check          only list which apps are behind their latest release, change nothing
#   --branch NAME    branch to merge on (default upstream/<today>; created from main if missing)
#   --no-tests       skip the cargo tests and the autotests (merge and build only)
#   --no-autotests   skip the autotests
#   --no-cargo-tests skip the cargo tests, still build and run the autotests
#   --all-tests      test the app's whole workspace, not only the crates the merge changed
#
# Conflicts: the merge stays in progress and the conflicting files are listed (exit 2). Resolve them so our
# Septet changes survive, `git commit`, then run the same command again: an app already at the tag is not
# merged again, and naming it explicitly verifies it. `all` skips apps already at their release.
#
# Autotests run on their own Xvfb (never a nested Xwayland, see HANDOFF.md) with a throwaway HOME, one at a
# time. They write logs and screenshots to $SYNC_OUT (default /tmp/septet-sync/<date>); the script fails on
# a non-zero exit, a panic or a failed screenshot, the screenshots themselves are for a person to look at.
# CRAFT_FONTS_DIR is passed through to the builds when set.
# Builds and tests run at the lowest CPU and IO priority, so the desktop stays usable; SYNC_JOBS limits
# cargo's parallel jobs (default: all cores). Debug info is off (CARGO_PROFILE_DEV_DEBUG=0: a test build
# of an app is ~5 GB instead of 100+ GB, and the disk writes no longer stall the desktop). The app's tests
# build in one folder shared by all apps, SYNC_TARGET (default ~/.cache/septet-sync/target), not in the
# app's own target/ (20-100 GB each). After a merge only the crates it changed are tested, with every
# crate of the app that depends on them; a change to the workspace itself (Cargo.toml, Cargo.lock,
# vendor/, …) tests all of it, as do --all-tests and verifying an app that is already merged.
# Exit status: 0 everything passed, 1 a build, test or autotest failed, 2 a merge conflict.
set -euo pipefail
renice -n 19 -p $$ >/dev/null 2>&1 || true
ionice -c 3 -p $$ 2>/dev/null || true
[ -z "${SYNC_JOBS:-}" ] || export CARGO_BUILD_JOBS=$SYNC_JOBS
export CARGO_PROFILE_DEV_DEBUG=${CARGO_PROFILE_DEV_DEBUG:-0}
app_target=${SYNC_TARGET:-$HOME/.cache/septet-sync/target}

apps=(designcraft effectcraft pdfcraft filmcraft vectorcraft lightcraft photocraft)
root=$(git rev-parse --show-toplevel)
cd "$root"
today=$(date +%F)
branch=upstream/$today
check=0 tests=1 autotests=1 cargo_tests=1 all_tests=0
while [ $# -gt 0 ]; do
    case $1 in
        --check) check=1 ;;
        --branch) branch=$2; shift ;;
        --no-tests) tests=0 autotests=0 ;;
        --no-autotests) autotests=0 ;;
        --no-cargo-tests) cargo_tests=0 ;;
        --all-tests) all_tests=1 ;;
        -h|--help) sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 0 ;;
        -*) echo "unknown option $1" >&2; exit 64 ;;
        *) break ;;
    esac
    shift
done
[ $# -ge 1 ] || { echo "usage: $0 [options] <app>|all [tag]" >&2; exit 64; }
target=$1 want_tag=${2:-}
if [ "$target" = all ]; then
    [ -z "$want_tag" ] || { echo "a tag only goes with a single app" >&2; exit 64; }
    selected=("${apps[@]}")
else
    [[ " ${apps[*]} " == *" $target "* ]] || { echo "unknown app $target (one of: ${apps[*]})" >&2; exit 64; }
    selected=("$target")
fi
out=${SYNC_OUT:-/tmp/septet-sync/$today}
mkdir -p "$out"

latest_tag() {
    if command -v gh >/dev/null && gh api "repos/storytold/$1/releases/latest" --jq .tag_name 2>/dev/null; then
        return
    fi
    curl -fsSL "https://api.github.com/repos/storytold/$1/releases/latest" 2>/dev/null |
        sed -n 's/^ *"tag_name": *"\(.*\)",*$/\1/p' | grep . && return
    # No release API: the highest version tag.
    git ls-remote --tags --refs "https://github.com/storytold/$1" | sed 's#.*refs/tags/##' | sort -V | tail -1
}

# The release commit, kept under refs/upstream/<app>/<tag> (not pushed, not a tag of ours).
fetch_tag() {
    git fetch -q --no-tags "https://github.com/storytold/$1" "+refs/tags/$2:refs/upstream/$1/$2"
    git rev-parse "refs/upstream/$1/$2^{commit}"
}

merge_in_progress() { [ -e "$(git rev-parse --git-path MERGE_HEAD)" ]; }

if [ $check = 1 ]; then
    for app in "${selected[@]}"; do
        tag=${want_tag:-$(latest_tag "$app")}
        commit=$(fetch_tag "$app" "$tag")
        if git merge-base --is-ancestor "$commit" HEAD; then state="up to date"; else
            state="behind ($(git rev-list --count "$commit" --not HEAD) upstream commits)"
        fi
        printf '%-12s %-8s %s\n' "$app" "$tag" "$state"
    done
    exit 0
fi

if merge_in_progress; then
    echo "A merge is in progress. Resolve it and commit, then run this again:" >&2
    git diff --name-only --diff-filter=U >&2
    exit 2
fi
if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "The working tree has uncommitted changes; commit or stash them first." >&2
    exit 1
fi
if [ "$(git branch --show-current)" != "$branch" ]; then
    if git show-ref --verify -q "refs/heads/$branch"; then git switch -q "$branch"; else git switch -q -c "$branch" main; fi
fi
echo "On branch $branch; logs in $out"

xvfb_pid=""
cleanup() { [ -z "$xvfb_pid" ] || kill "$xvfb_pid" 2>/dev/null || true; }
trap cleanup EXIT

start_xvfb() {
    [ -z "$xvfb_pid" ] || return 0
    display=:97
    while [ -e "/tmp/.X11-unix/X${display#:}" ]; do display=:$(( ${display#:} + 1 )); done
    Xvfb "$display" -screen 0 1600x1000x24 -nolisten tcp >"$out/xvfb.log" 2>&1 &
    xvfb_pid=$!
    for _ in $(seq 50); do [ -e "/tmp/.X11-unix/X${display#:}" ] && return 0; sleep 0.1; done
    echo "Xvfb did not start, see $out/xvfb.log" >&2
    return 1
}

# A sample photo and PDF for the scenarios that open files.
make_samples() {
    [ -s "$out/sample.png" ] && [ -s "$out/sample.pdf" ] && return
    magick -size 800x600 gradient:'#2f6fd6-#e8a33d' -fill white -pointsize 64 -gravity center \
        -annotate 0 'Septet sync' "$out/sample.png"
    magick -size 595x842 xc:white -fill '#2f6fd6' -draw 'rectangle 60,60 535,300' -fill black -pointsize 36 \
        -annotate +80+380 'Septet sync test' "$out/sample.pdf"
}

# One autotest scenario on Xvfb with a throwaway HOME: scenario, label, extra env.
autotest() {
    local scenario=$1 label=$2; shift 2
    local dir=$out/$label home=$out/$label.home
    rm -rf "$dir" "$home"
    mkdir -p "$dir" "$home"
    local status=0
    env -u WAYLAND_DISPLAY DISPLAY="$display" HOME="$home" XDG_CONFIG_HOME="$home/.config" \
        XDG_DATA_HOME="$home/.local/share" XDG_CACHE_HOME="$home/.cache" XDG_STATE_HOME="$home/.local/state" \
        WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1 __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
        SEPTET_NO_VSYNC=1 SEPTET_AUTOTEST="$dir" SEPTET_AUTOTEST_SCENARIO="$scenario" \
        SEPTET_AUTOTEST_FILE="$out/sample.png" SEPTET_AUTOTEST_PDF="$out/sample.pdf" "$@" \
        timeout 600 "${CARGO_TARGET_DIR:-$root/target}/debug/septet" >"$dir/stdout.txt" 2>"$dir/stderr.txt" || status=$?
    rm -rf "$home"
    local why=""
    [ $status = 0 ] || why="exit $status"
    grep -q 'panicked' "$dir/stderr.txt" && why="${why:+$why, }panic"
    grep -q 'FAILED' "$dir/log.txt" 2>/dev/null && why="${why:+$why, }failed screenshot"
    [ -s "$dir/log.txt" ] || why="${why:+$why, }no log"
    if [ -n "$why" ]; then echo "  autotest $label: FAILED ($why), see $dir"; return 1; fi
    echo "  autotest $label: ok ($(ls "$dir"/*.png 2>/dev/null | wc -l) screenshots in $dir)"
}

# Commit lockfiles a build or test run updated after the merge.
commit_lockfiles() {
    local files=(Cargo.lock "$1/Cargo.lock")
    if ! git diff --quiet -- "${files[@]}"; then
        git commit -q -m "$1: Cargo.lock after upstream $2" -- "${files[@]}"
        echo "  committed updated Cargo.lock"
    fi
}

# The `-p` arguments for the app's crates that the merge from $1 changed, and every crate of the app
# that depends on one of them; nothing (test the whole workspace) when the workspace itself changed.
affected_packages() {
    local app=$1 before=$2
    git diff --name-only "$before" HEAD -- "$app" | python3 -c '
import json, os, subprocess, sys
app, root = sys.argv[1], sys.argv[2]
meta = json.loads(subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=os.path.join(root, app),
                                 check=True, capture_output=True, text=True).stdout)
dirs = {os.path.dirname(p["manifest_path"]): p["name"] for p in meta["packages"]}
# Files no test reads: documentation, CI and packaging, licences.
inert = (".github/", "docs/", "packaging/", "contributors/", "plan/")
changed = set()
for f in sys.stdin.read().split():
    rel = f[len(app) + 1:]
    if rel.startswith(inert) or rel.endswith(".md") or rel.split("/")[-1].startswith(("LICENSE", "NOTICE", ".git")):
        continue
    path = os.path.join(root, f)
    owner = max((d for d in dirs if path.startswith(d + os.sep)), key=len, default=None)
    if owner is None or os.path.dirname(owner) == os.path.join(root, app):
        sys.exit(0)  # a file of the workspace itself (or a crate at its root): test all of it
    changed.add(dirs[owner])
users = {p["name"]: {d["name"] for d in p["dependencies"] if d.get("path")} for p in meta["packages"]}
grew = True
while grew:
    more = {name for name, deps in users.items() if deps & changed} - changed
    grew = bool(more)
    changed |= more
print(" ".join("-p " + n for n in sorted(changed)))
' "$app" "$root"
}

step() { # label, log file, command…
    local label=$1 log=$2; shift 2
    if "$@" >"$log" 2>&1; then echo "  $label: ok"; else echo "  $label: FAILED, see $log"; tail -n 30 "$log" | sed 's/^/    /'; return 1; fi
}

verify() {
    local app=$1 tag=$2 before=${3:-}
    step "build septet" "$out/$app-build.log" cargo build -p septet || return 1
    commit_lockfiles "$app" "$tag"
    [ $tests = 1 ] || return 0
    [ $cargo_tests = 1 ] || { verify_autotests; return; }
    step "septet tests" "$out/$app-septet-test.log" cargo test -p septet || return 1
    # Known environment failures (scripts/sync-upstream-skips.txt); only passed when the app has some,
    # because a custom test harness may not take --skip.
    local skips
    skips=$(sed -n "s/^$app \([^ ]*\).*/--skip \1/p" "$root/scripts/sync-upstream-skips.txt" | tr '\n' ' ')
    local which="--workspace"
    if [ $all_tests = 0 ] && [ -n "$before" ]; then
        which=$(affected_packages "$app" "$before") || return 1
        if [ -z "$which" ]; then which="--workspace"; fi
    fi
    echo "  testing ${which/--workspace/the whole workspace}"
    step "$app tests" "$out/$app-test.log" \
        bash -c "cd '$app' && CARGO_TARGET_DIR='$app_target' cargo test $which --no-fail-fast ${skips:+-- $skips}" || return 1
    commit_lockfiles "$app" "$tag"
    verify_autotests
}

verify_autotests() {
    [ $autotests = 1 ] || return 0
    start_xvfb && make_samples || return 1
    local failed=0
    autotest tour "$app-tour" || failed=1
    autotest interop "$app-interop" || failed=1
    autotest content "$app-content" || failed=1
    autotest assistant-apps "$app-assistant-apps" SEPTET_AUTOTEST_APP="$app" || failed=1
    return $failed
}

for app in "${selected[@]}"; do
    tag=${want_tag:-$(latest_tag "$app")}
    commit=$(fetch_tag "$app" "$tag")
    echo "== $app $tag"
    before=""
    if git merge-base --is-ancestor "$commit" HEAD; then
        if [ "$target" = all ]; then echo "  already at $tag"; continue; fi
        echo "  already at $tag, verifying"
        # Merged by an earlier run (say, one that stopped at a conflict): test what that merge changed.
        merge=$(git rev-list --merges --ancestry-path "$commit..HEAD" -- "$app" | tail -n 1)
        [ -z "$merge" ] || before=$(git rev-parse "$merge^1")
    else
        echo "  merging $(git rev-list --count "$commit" --not HEAD) upstream commits"
        before=$(git rev-parse HEAD)
        if ! git subtree merge -q --prefix="$app" "$commit" -m "$app: merge upstream $tag" >"$out/$app-merge.log" 2>&1; then
            if merge_in_progress; then
                if [ -z "$(git diff --name-only --diff-filter=U)" ]; then
                    echo "  CONFLICTS, all resolved again by git rerere: review git diff --cached, git commit, run again"
                else
                    echo "  CONFLICTS (resolve, keep our Septet changes, git commit, run again):"
                    git diff --name-only --diff-filter=U | sed 's/^/    /'
                fi
                exit 2
            fi
            cat "$out/$app-merge.log" >&2
            exit 1
        fi
    fi
    verify "$app" "$tag" "$before" || { echo "$app $tag: verification failed (branch $branch)"; exit 1; }
done
echo "All done on $branch. Review, then push the branch and open a merge request; nothing went to main."

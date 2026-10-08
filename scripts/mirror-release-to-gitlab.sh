#!/bin/sh
# Copy a GitHub release of Septet (built by GitHub Actions) to the GitLab project's release, with the
# files in GitLab's generic package registry. Needs `gh` and `glab`, both logged in.
# Usage: scripts/mirror-release-to-gitlab.sh v0.1.0
set -eu
tag=$1
version=${tag#v}
gh_repo=LouBoi161/septet
gl_project=louiswalder6%2Fseptet
dir=$(mktemp -d)
gh release download "$tag" --repo "$gh_repo" --dir "$dir"
token=$(glab config get token --host gitlab.com 2>/dev/null || true)
[ -n "$token" ] || token=$(glab auth status -t 2>&1 | sed -n 's/.*Token: //p' | head -1)
links=""
for f in "$dir"/*; do
    name=$(basename "$f")
    url="https://gitlab.com/api/v4/projects/$gl_project/packages/generic/septet/$version/$name"
    curl -fsS --header "PRIVATE-TOKEN: $token" --upload-file "$f" "$url" >/dev/null
    echo "uploaded $name"
    links="$links{\"name\":\"$name\",\"url\":\"$url\",\"link_type\":\"package\"},"
done
glab release create "$tag" --repo louiswalder6/septet --name "Septet $tag" --notes-file RELEASE_NOTES.md \
    --assets-links "[${links%,}]"
rm -rf "$dir"

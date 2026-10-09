#!/bin/sh
# Copy a GitHub release of Septet (built by GitHub Actions) to the GitLab project's release, with the
# files in GitLab's generic package registry. Needs `gh` and `glab`, both logged in; uploads go through
# `glab api`, so glab's own login (also a keyring one) is used and no token is read out.
# Usage: scripts/mirror-release-to-gitlab.sh v0.1.0
set -eu
tag=$1
version=${tag#v}
gh_repo=LouBoi161/septet
gl_project=louiswalder6%2Fseptet
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
gh release download "$tag" --repo "$gh_repo" --dir "$dir"
links=""
for f in "$dir"/*; do
    name=$(basename "$f")
    path="projects/$gl_project/packages/generic/septet/$version/$name"
    glab api --hostname gitlab.com --method PUT "$path" --input "$f" >/dev/null
    echo "uploaded $name"
    links="$links{\"name\":\"$name\",\"url\":\"https://gitlab.com/api/v4/$path\",\"link_type\":\"package\"},"
done
glab release create "$tag" --repo louiswalder6/septet --name "Septet $tag" --notes-file RELEASE_NOTES.md \
    --assets-links "[${links%,}]"

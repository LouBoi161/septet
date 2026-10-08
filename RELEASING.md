# Releasing Septet

1. Bump `version` in `Cargo.toml` (workspace package) and update `RELEASE_NOTES.md`.
2. Commit, then push to both remotes:
   `git push origin main && git push github main`
3. Tag and push the tag to both: `git tag v0.2.0 && git push origin v0.2.0 && git push github v0.2.0`
4. GitHub Actions (`.github/workflows/release.yml`) builds Windows, Linux and macOS on native runners
   and publishes the GitHub release with all downloads (about an hour).
5. Mirror the release to GitLab: `scripts/mirror-release-to-gitlab.sh v0.2.0` (downloads the GitHub
   release files, uploads them to GitLab's package registry and creates the GitLab release).

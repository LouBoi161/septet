# Releasing Septet

1. Bump `version` in `Cargo.toml` (workspace package) and update `RELEASE_NOTES.md`.
2. Commit, then push to both remotes:
   `git push origin main && git push github main`
3. Tag and push the tag to both: `git tag v0.2.0 && git push origin v0.2.0 && git push github v0.2.0`
4. GitHub Actions (`.github/workflows/release.yml`) builds Windows, Linux and macOS on native runners
   and publishes the GitHub release with all downloads (about an hour).
5. Mirror the release to GitLab: `scripts/mirror-release-to-gitlab.sh v0.2.0` (downloads the GitHub
   release files, uploads them to GitLab's package registry and creates the GitLab release).

Septet updates itself from the GitHub release (`septet/src/update.rs`, Home › Apps): it reads
`releases/latest`, picks its file by name and checks it against `SHA256SUMS.txt`. So keep the file
names the release workflow gives them (`Septet-<v>-x86_64.AppImage`, `septet-<v>-linux-x86_64.tar.gz`,
`Septet-<v>-windows-x64-setup.exe`, `Septet-<v>-windows-x64-portable.zip`,
`Septet-<v>-macos-<arm64|x86_64>.dmg`) and the checksum file, and publish a release only once all of
them are attached: the running Septets see it within a day.

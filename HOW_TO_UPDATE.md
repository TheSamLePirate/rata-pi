# How to Release an Update for Tau (rata-pi)

This guide documents the end-to-end process for pushing a new release of Tau, generating the cross-platform binaries via GitHub Actions, and publishing the update to the Homebrew tap.

## 1. Local Development & Verification

Before cutting a release, ensure the codebase is clean and passes all checks. 
GitHub Actions will fail if there are any linting errors.

```bash
# Run tests
cargo test

# VERY IMPORTANT: Run clippy and fix any warnings/errors
# (CI will fail on clippy warnings, e.g., collapsible match blocks)
cargo clippy
```

## 2. Bump Version & Commit

Update the application version in the manifest.

1. Open `Cargo.toml` and bump the `version` (e.g., from `1.1.0` to `1.2.0`).
2. Commit your changes:

```bash
git add Cargo.toml
git commit -m "chore: bump version to 1.2.0"
git push origin main
```

## 3. Tag the Release (Triggers CI)

The release GitHub Action (`.github/workflows/release.yml`) is triggered by pushing a tag starting with `v`.

```bash
# Create the tag (matches the Cargo.toml version)
git tag v1.2.0

# Push the tag to GitHub
git push origin v1.2.0
```

## 4. Wait for CI & Publish the GitHub Release

1. Go to the **Actions** tab in the `rata-pi` repository.
2. Monitor the **Release** workflow. It will:
   - Build `tau` for macOS (arm64/intel), Linux (x86_64), and Windows.
   - Archive them (`.tar.gz` and `.zip`).
   - Generate a `SHA256SUMS.txt` file.
   - Create a **Draft Release** on GitHub.
3. Once the workflow is green, go to the **Releases** page.
4. Edit the newly created Draft Release, add any release notes/changelog, and click **Publish release**.

## 5. Update the Homebrew Formula

Once the GitHub release is published, the Homebrew tap needs to be updated with the new version and the new SHA256 hashes of the binaries.

1. Download the `SHA256SUMS.txt` file from the published GitHub release:
   ```bash
   gh release download v1.2.0 -p "SHA256SUMS.txt"
   cat SHA256SUMS.txt
   ```

2. Open `Formula/rata-pi.rb` in the `homebrew-rata-pi` repository.
3. Update the `version`:
   ```ruby
   version "1.2.0"
   ```
4. Replace the `sha256` strings in the `on_macos` and `on_linux` blocks with the exact hashes from `SHA256SUMS.txt`. 
   - *Note: Be careful to match `aarch64-apple-darwin` (Mac ARM), `x86_64-apple-darwin` (Mac Intel), and `x86_64-unknown-linux-gnu` (Linux x86) correctly!*

5. Commit and push the updated formula:
   ```bash
   cd ../homebrew-rata-pi
   git add Formula/rata-pi.rb
   git commit -m "chore: bump version to 1.2.0 and update sha256 sums"
   git push origin main
   ```

## 6. Verify Homebrew Installation

Test the newly published tap update locally:

```bash
brew update
brew upgrade rata-pi

# Or if not installed:
brew install rata-pi

# Verify the app starts correctly
tau --version
```

---

## ⚠️ Troubleshooting & Gotchas

*   **App/Binary Rename (`rata-pi` -> `tau`):** If you ever change the output binary name again, remember to update the `bin` field in `.github/workflows/release.yml` for all targets, otherwise the archive step will fail trying to find the old executable name. You must also update `bin.install "<new_name>"` in the Homebrew formula.
*   **Checksum Mismatches:** If `brew upgrade` fails with `Formula reports different checksum`, it means the `sha256` in the formula does not match the actual downloaded `.tar.gz`. Double-check that you copied the correct hash from `SHA256SUMS.txt` for the specific architecture.
*   **Draft Releases:** The `softprops/action-gh-release` step creates the release as a *draft* by default. If you don't publish the draft manually in the GitHub UI, Homebrew will return a `404 Not Found` when trying to download the tarball.
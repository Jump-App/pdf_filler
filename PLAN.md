# PLAN.md

## Goal

Create a standalone public GitHub repository for `pdf_filler` and distribute prebuilt binaries through GitHub Releases.

The new repo should own:

- Rust source code for `pdf_filler`
- CI for testing and cross-platform builds
- Release automation that uploads binaries to GitHub Releases
- A stable download contract for downstream consumers

This replaces the current "build in repo + upload to GCS" distribution model.

## Why GitHub Releases

Yes, this can be done cleanly with GitHub Releases.

Benefits:

- no GCP bucket or Artifact Registry setup for binary distribution
- no `gcloud` requirement for downstream developers
- public HTTPS download URLs
- native versioned release artifacts
- simple provenance for which binary belongs to which version

Constraints:

- the repo should be public if downloads are meant to be frictionless
- release artifacts should be immutable once published
- downstream code should download by explicit version, not `latest`

## Repo Shape

Recommended standalone repo contents:

- `Cargo.toml`
- `Cargo.lock`
- `src/`
- `tests/`
- `examples/`
- `README.md`
- `LICENSE`
- `.github/workflows/test.yml`
- `.github/workflows/release.yml`

Optional:

- `rust-toolchain.toml`
- `CHANGELOG.md`
- `scripts/`

## Release Contract

Each release should publish binaries for:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `aarch64-apple-darwin`
- `x86_64-apple-darwin`

Recommended asset names:

- `pdf_filler-x86_64-unknown-linux-gnu`
- `pdf_filler-aarch64-unknown-linux-gnu`
- `pdf_filler-aarch64-apple-darwin`
- `pdf_filler-x86_64-apple-darwin`

Recommended tag format:

- `v0.1.0`
- `v0.1.1`
- `v0.2.0`

Downstream consumers should build download URLs from:

- repo owner
- repo name
- version tag
- target triple

Example URL shape:

`https://github.com/<owner>/<repo>/releases/download/v0.1.0/pdf_filler-x86_64-unknown-linux-gnu`

## CI Workflows

### 1. Test Workflow

Trigger on:

- pull requests
- pushes to main

Responsibilities:

- `cargo test`
- optional `cargo fmt --check`
- optional `cargo clippy -- -D warnings`

Suggested matrix:

- `ubuntu-latest`

If native macOS coverage matters, add macOS test jobs later, but start simple.

### 2. Release Workflow

Trigger options:

- preferred: on pushed tags matching `v*`
- optional: manual `workflow_dispatch`

Responsibilities:

- build binaries for all supported targets
- create or update a GitHub Release for the tag
- upload binaries as release assets

Suggested matrix:

- `ubuntu-latest` -> `x86_64-unknown-linux-gnu`
- `ubuntu-24.04-arm` -> `aarch64-unknown-linux-gnu`
- `macos-latest` -> `aarch64-apple-darwin`
- `macos-15-intel` -> `x86_64-apple-darwin`

Recommended release behavior:

- releases only on version tags
- PRs and branch pushes should build/test only
- do not upload release assets from non-tag refs

## Release Workflow Outline

Suggested `release.yml` behavior:

1. Check out code.
2. Install Rust toolchain pinned to the repo version.
3. Build `cargo build --release --target <target>`.
4. Rename/copy the binary to `pdf_filler-<target>`.
5. Create a GitHub Release for the tag if it does not exist.
6. Upload the binary asset to that release.

Recommended GitHub Actions pieces:

- `actions/checkout@v4`
- `dtolnay/rust-toolchain`
- `Swatinem/rust-cache`
- `softprops/action-gh-release` or `gh release upload`

## Versioning Rules

Version should be updated in:

- `Cargo.toml`
- release tag
- downstream consumer version pin

Recommended rule:

- `Cargo.toml` version and Git tag should match
- downstream repo should pin to an explicit version like `v0.1.0`

Do not make downstream consumers fetch `latest`.

## Downstream Integration Changes

The root Jump repo will need to change:

- remove GCS upload workflow usage for distribution
- replace GCS download logic with GitHub Release download logic
- keep the local downloaded binary/version marker ignored in git

Suggested downstream download behavior:

- build GitHub release asset URL from pinned version and target
- download via `curl` or another plain HTTPS client
- mark executable
- save version marker

This is the key simplification:

- no `gcloud`
- no bucket auth
- no Artifact Registry auth

## Security / Trust Model

Recommended minimum:

- release only from protected tags or tags created from `main`
- require PR review before merging release changes
- keep release workflow scoped so only tagged refs publish

Optional hardening:

- attach SHA256 checksums as release assets
- sign artifacts later if needed

Recommended extra release assets:

- `checksums.txt`

## Migration Steps

1. Create the new public `pdf_filler` repository.
2. Move or copy the Rust package into it.
3. Add `README.md` explaining usage, supported targets, and release policy.
4. Add `test.yml`.
5. Add `release.yml`.
6. Create first tag, for example `v0.1.0`.
7. Confirm release assets are downloadable anonymously.
8. Update Jump to download from GitHub Releases instead of GCS.
9. Remove old GCS-based release plumbing once the new path is proven.

## Open Decisions

Decide these before implementation:

- public repo name
- release asset naming convention
- whether to publish checksums
- whether to use tag-push only or also manual release dispatch
- whether the repo should include only Rust source or also templates/fixtures/docs copied from Jump

## Suggested First Cut

Keep the first version intentionally simple:

- public repo
- GitHub Release assets
- tag-driven release workflow
- three binaries only
- no checksums yet unless trivial
- no signing yet

That gets distribution working quickly with minimal infra and minimal maintenance.

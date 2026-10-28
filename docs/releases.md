# Desktop builds and draft releases

`.github/workflows/build-release.yml` builds the Tauri desktop app on every push to `main` and on manual dispatch. It has no path filters, so changes to Rust, the frontend, assets, or Tauri configuration all trigger a full build. Dispatching from a branch other than `main` builds and uploads CI artifacts; the draft-release job runs only for `main`.

## Build matrix

The workflow uses Node.js 24, pnpm 12.3.4, and Rust 1.98.0. It installs the locked frontend dependencies from `desktop/pnpm-lock.yaml` and invokes the Tauri CLI from the `desktop/` project. The CI-only Tauri configuration enables bundling and macOS ad-hoc signing; it does not change the normal development configuration.

Concurrency is keyed by both branch and commit, so a newer merge does not replace a pending build for another commit. Reruns for the same commit may share a concurrency group and reuse that commit's draft release.

| Runner | Target | Installer |
| --- | --- | --- |
| macOS 15 Apple Silicon | `aarch64-apple-darwin` | DMG |
| macOS 15 Intel | `x86_64-apple-darwin` | DMG |
| Windows Server 2022 x64 | `x86_64-pc-windows-msvc` | NSIS `.exe` |
| Ubuntu 22.04 x64 | `x86_64-unknown-linux-gnu` | `.deb` and AppImage |

Ubuntu 22.04 supplies the WebKitGTK 4.1 build dependencies and acts as the Linux compatibility baseline. The macOS Intel runner label is `macos-15-intel`; confirm GitHub's current runner labels when updating this workflow.

## Draft release flow

Each build job has read-only repository permissions and uploads its bundles as workflow artifacts. macOS builds include both a DMG installer and the `.app` bundle; the release staging step accepts the two known `.app` artifacts but excludes them from distribution. The release job has `contents: write` and runs only after every matrix build succeeds. It validates that all five expected installers are present and rejects unknown artifacts, then creates or refreshes a draft release named `omnooth <version> (main <short SHA>)` with the stable tag `main-<full commit SHA>`.

Rerunning a workflow for the same commit reuses the same draft and replaces same-named assets. If that tag is already attached to a published release, the workflow stops without changing it. The workflow does not publish the draft; review and publish it manually on GitHub after checking the assets.

The uploaded names identify their platform and architecture, for example `omnooth-macos-arm64-0.1.0.dmg` and `omnooth-linux-x64-0.1.0.AppImage`. No updater metadata or updater signatures are produced.

## Verification and distribution limits

These jobs prove that the requested installers were built and attached to a draft; they do not run the app on each target OS or verify installation, upgrade, deep-link reception, or removal. Test the draft installers on supported operating systems before publishing.

Windows and Linux bundles are unsigned. The macOS app uses ad-hoc signing and is not notarized. The first distribution workflow does not configure code-signing certificates or notarization credentials.

The workflow uses the official [Tauri Action](https://github.com/tauri-apps/tauri-action) pinned to its `action-v1.0.0` commit, [Tauri's GitHub pipeline guidance](https://v2.tauri.app/distribute/pipelines/github/), and the current [GitHub-hosted runner labels](https://github.com/actions/runner-images#available-images). It uses GitHub's [artifact actions](https://docs.github.com/en/actions/tutorials/store-and-share-data) for artifacts between jobs and `gh release` for draft creation and asset upload.

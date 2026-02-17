set shell := ["sh", "-ce"]
set positional-arguments := true

exe_ext := if os() == "windows" { ".exe" } else { "" }

# GitHub Linux runners replace image's HOME with some nonsense that messes up
# with our container setup.
# Non-container CI environments does not need this workaround.

export HOME := if os() == "linux" { if lowercase(env("GITHUB_ACTIONS", "false")) == "true" { "/root" } else { env("HOME") } } else { env("HOME") }

# Use `mold` linker in devcontainer/CI.

export RUSTFLAGS := if env("DEVCONTAINER_HAS_MOLD", "0") == "1" { env("RUSTFLAGS", "") + " -C linker=clang -C link-arg=-fuse-ld=mold" } else { env("RUSTFLAGS", "") }

# Configuration.

ci_cont_base_src_files := "ci/container/base"
ci_cont_base_image_tag := "ghcr.io/tara-zero/tokio-immediate-ci-base"
ci_cont_base_image_title := "tokio-immediate CI Base Image"
ci_cont_base_image_desc := "Dev Container/Base CI image for tokio-immediate"
ci_cont_image_source := "https://github.com/tara-zero/tokio-immediate"

# Recipes.

[default]
[doc("List available recipes")]
list:
    @just --list

[doc("Run egui example")]
[group("Examples")]
run-egui:
    cargo run --release --package "tokio-immediate-example-egui"

[doc("Run minimal egui example")]
[group("Examples")]
run-egui-minimal:
    cargo run --release --package "tokio-immediate-example-egui-minimal"

[doc("Run minimal imgui example")]
[group("Examples")]
run-imgui-minimal:
    cargo run --release --package "tokio-immediate-example-imgui-minimal"

[doc("Run minimal ratatui example")]
[group("Examples")]
run-ratatui-minimal:
    cargo run --release --package "tokio-immediate-example-ratatui-minimal"

[doc("Run `cargo clean`")]
[group("Development")]
clean:
    cargo clean

[doc("Run `cargo fmt`")]
[group("Development")]
fmt:
    cargo fmt --all

[doc("Run `cargo check`")]
[group("Development")]
check:
    cargo check --workspace --all-targets --all-features

[doc("Run `cargo doc...`")]
[group("Development")]
doc:
    cargo doc --package "tokio-immediate" --no-deps --all-features
    cargo doc --package "tokio-immediate-egui" --no-deps --all-features

[doc("Run `cargo clippy`")]
[group("Development")]
clippy:
    # Features does not play well with `--workspace` so we check each crate separately.
    set -x; \
    for package in \
            "tokio-immediate" \
            "tokio-immediate-egui" \
            "tokio-immediate-tests"; do \
        cargo clippy --package "${package}" --all-targets --no-default-features -- --deny warnings; \
        cargo clippy --package "${package}" --all-targets --no-default-features --features "sync" -- --deny warnings; \
    done
    cargo clippy --package "tokio-immediate-example-egui" --all-targets --all-features -- --deny warnings
    cargo clippy --package "tokio-immediate-example-egui-minimal" --all-targets --all-features -- --deny warnings
    cargo clippy --package "tokio-immediate-example-imgui-minimal" --all-targets --all-features -- --deny warnings
    cargo clippy --package "tokio-immediate-example-ratatui-minimal" --all-targets --all-features -- --deny warnings

[doc("Run `cargo build`")]
[group("Development")]
build:
    # Features does not play well with `--workspace` so we build each crate separately.
    cargo build --package "tokio-immediate-example-egui" --bins
    cargo build --package "tokio-immediate-example-egui-minimal" --bins
    cargo build --package "tokio-immediate-example-imgui-minimal" --bins
    cargo build --package "tokio-immediate-example-ratatui-minimal" --bins
    @echo
    @echo "Examples (debug):"
    @ls -lh \
            target/debug/tokio-immediate-example-egui{{ exe_ext }} \
            target/debug/tokio-immediate-example-egui-minimal{{ exe_ext }} \
            target/debug/tokio-immediate-example-imgui-minimal{{ exe_ext }} \
            target/debug/tokio-immediate-example-ratatui-minimal{{ exe_ext }}

[doc("Run `cargo build --release`")]
[group("Development")]
build-release:
    # Features does not play well with `--workspace` so we build each crate separately.
    cargo build --release --package "tokio-immediate-example-egui" --bins
    cargo build --release --package "tokio-immediate-example-egui-minimal" --bins
    cargo build --release --package "tokio-immediate-example-imgui-minimal" --bins
    cargo build --release --package "tokio-immediate-example-ratatui-minimal" --bins
    @echo
    @echo "Examples (release):"
    @ls -lh \
            target/release/tokio-immediate-example-egui{{ exe_ext }} \
            target/release/tokio-immediate-example-egui-minimal{{ exe_ext }} \
            target/release/tokio-immediate-example-imgui-minimal{{ exe_ext }} \
            target/release/tokio-immediate-example-ratatui-minimal{{ exe_ext }}

[doc("Run `cargo test`")]
[group("Development")]
test *args:
    cargo test --package "tokio-immediate-tests" --all-targets --no-default-features "${@}"
    cargo test --package "tokio-immediate-tests" --all-targets --no-default-features --features "sync" "${@}"

[doc("Run `cargo fmt --check`")]
[group("Continuous integration")]
fmt-check:
    cargo fmt --check --all

# TODO: CI image with caches.

[doc("Create the manifest for the base CI/devcontainer images and push it into registry")]
[group("Continuous integration")]
manifest-base-push: manifest-base-create
    CI_CONT_ARCH="$( uname --machine | sed "s,^aarch64$,arm64,g;s,^x86_64$,amd64,g" )"; \
    CI_CONT_BRANCH="$( git rev-parse --abbrev-ref HEAD )"; \
    CI_CONT_DATE="$( git log -1 --format=%cd --date=format:%Y-%m-%d -- {{ ci_cont_base_src_files }} )"; \
    CI_CONT_MANIFEST="{{ ci_cont_base_image_tag }}:${CI_CONT_BRANCH}-${CI_CONT_DATE}"; \
    buildah manifest push "${CI_CONT_MANIFEST}"

[doc("Create the manifest for the base CI/devcontainer images")]
[group("Continuous integration")]
manifest-base-create:
    CI_CONT_ARCH="$( uname --machine | sed "s,^aarch64$,arm64,g;s,^x86_64$,amd64,g" )"; \
    CI_CONT_BRANCH="$( git rev-parse --abbrev-ref HEAD )"; \
    CI_CONT_DATE="$( git log -1 --format=%cd --date=format:%Y-%m-%d -- {{ ci_cont_base_src_files }} )"; \
    CI_CONT_MANIFEST="{{ ci_cont_base_image_tag }}:${CI_CONT_BRANCH}-${CI_CONT_DATE}"; \
    buildah manifest exists \
            "${CI_CONT_MANIFEST}" \
        || buildah manifest create \
            --annotation "org.opencontainers.image.title={{ ci_cont_base_image_title }}" \
            --annotation "org.opencontainers.image.description={{ ci_cont_base_image_desc }}" \
            --annotation "org.opencontainers.image.source={{ ci_cont_image_source }}" \
            "${CI_CONT_MANIFEST}" \
            "${CI_CONT_MANIFEST}.amd64" \
            "${CI_CONT_MANIFEST}.arm64"

[doc("Build base CI/devcontainer image and push it into registry")]
[group("Continuous integration")]
image-base-push: image-base-build
    CI_CONT_ARCH="$( uname --machine | sed "s,^aarch64$,arm64,g;s,^x86_64$,amd64,g" )"; \
    CI_CONT_BRANCH="$( git rev-parse --abbrev-ref HEAD )"; \
    CI_CONT_DATE="$( git log -1 --format=%cd --date=format:%Y-%m-%d -- {{ ci_cont_base_src_files }} )"; \
    CI_CONT_MANIFEST="{{ ci_cont_base_image_tag }}:${CI_CONT_BRANCH}-${CI_CONT_DATE}"; \
    buildah push \
            --all \
            "${CI_CONT_MANIFEST}.${CI_CONT_ARCH}"

[doc("Build base CI/devcontainer image")]
[group("Continuous integration")]
image-base-build:
    CI_CONT_ARCH="$( uname --machine | sed "s,^aarch64$,arm64,g;s,^x86_64$,amd64,g" )"; \
    CI_CONT_BRANCH="$( git rev-parse --abbrev-ref HEAD )"; \
    CI_CONT_DATE="$( git log -1 --format=%cd --date=format:%Y-%m-%d -- {{ ci_cont_base_src_files }} )"; \
    CI_CONT_MANIFEST="{{ ci_cont_base_image_tag }}:${CI_CONT_BRANCH}-${CI_CONT_DATE}"; \
    buildah inspect \
            --format "{{{{ .ImageCreatedBy }}" \
            "${CI_CONT_MANIFEST}.${CI_CONT_ARCH}" \
        || buildah build \
            --isolation "chroot" \
            --platform "${CI_CONT_ARCH}" \
            --tag "${CI_CONT_MANIFEST}.${CI_CONT_ARCH}" \
            --annotation "org.opencontainers.image.title={{ ci_cont_base_image_title }}" \
            --annotation "org.opencontainers.image.description={{ ci_cont_base_image_desc }}" \
            --annotation "org.opencontainers.image.source={{ ci_cont_image_source }}" \
            "ci/container/base"

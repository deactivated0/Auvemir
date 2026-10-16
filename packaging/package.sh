#!/usr/bin/env bash
set -euo pipefail
auvemir_project_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$auvemir_project_dir"
mkdir -p dist
case "${1:-source}" in
  source)
    if [[ "${2:-}" == "--vendor" ]]; then
      mkdir -p .cargo
      cargo vendor --locked > .cargo/config.toml
    fi
    tar --exclude='./target' --exclude='./dist' --exclude='./.git' \
      -czf dist/auvemir-0.1.0-source.tar.gz .
    ;;
  linux)
    cargo build --locked --release -p auvemir -j "${AUVEMIR_BUILD_JOBS:-2}"
    auvemir_package_dir="$(mktemp -d)"
    trap 'rm -rf "$auvemir_package_dir"' EXIT
    mkdir -p "$auvemir_package_dir/auvemir-0.1.0"
    cp target/release/auvemir LICENSE README.md THIRD_PARTY_NOTICES.md "$auvemir_package_dir/auvemir-0.1.0/"
    cp packaging/auvemir.desktop "$auvemir_package_dir/auvemir-0.1.0/"
    tar -C "$auvemir_package_dir" -czf dist/auvemir-0.1.0-linux-x86_64.tar.gz auvemir-0.1.0
    ;;
  *) echo 'Usage: packaging/package.sh source [--vendor] | linux' >&2; exit 2 ;;
esac

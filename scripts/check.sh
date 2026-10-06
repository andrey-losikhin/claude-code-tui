#!/usr/bin/env bash
set -euo pipefail
project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$project_root"
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
bash -n install.sh run.sh scripts/check.sh
if [[ ${RUN_PTY_TESTS:-1} == 1 ]]; then
    command -v python3 >/dev/null
    [[ -x /usr/bin/nvim ]] || { echo 'PTY tests require /usr/bin/nvim' >&2; exit 1; }
    for smoke in tests/test_*_pty.py; do
        python3 "$smoke"
    done
fi

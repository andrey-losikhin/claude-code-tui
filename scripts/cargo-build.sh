#!/usr/bin/env bash
# Shared by the launchers; fall back only when the selected compiler cannot start.
tui_cargo_build() {
    local compiler_error rustup_binary fallback_compiler
    if compiler_error=$("${RUSTC:-rustc}" -vV 2>&1); then
        cargo "$@"
        return
    fi
    if [[ -z ${RUSTC:-} ]]; then
        rustup_binary=$(command -v rustup || true)
        if [[ -z $rustup_binary && -x ${CARGO_HOME:-${HOME:-}/.cargo}/bin/rustup ]]; then
            rustup_binary="${CARGO_HOME:-${HOME:-}/.cargo}/bin/rustup"
        fi
        if [[ -n $rustup_binary ]] &&
            fallback_compiler=$("$rustup_binary" which --toolchain stable rustc 2>/dev/null) &&
            "$fallback_compiler" -vV >/dev/null 2>&1; then
            echo 'Системный rustc недоступен; сборка через rustup stable.' >&2
            RUSTC="$fallback_compiler" "$rustup_binary" run stable cargo "$@"
            return
        fi
    fi
    printf 'Не удалось запустить компилятор Rust:\n%s\n' "$compiler_error" >&2
    echo 'Установите исправный Rust через rustup: https://rustup.rs' >&2
    echo 'Если RUSTC задан вручную, проверьте указанный компилятор.' >&2
    return 1
}

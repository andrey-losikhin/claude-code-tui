#!/usr/bin/env bash
set -euo pipefail
project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
source "$project_root/scripts/cargo-build.sh"
terminal_kind=auto
if [[ $# -gt 0 ]]; then
    if [[ $# -eq 2 && $1 == --terminal ]]; then
        terminal_kind=$2
    elif [[ $# -eq 1 && ($1 == --help || $1 == -h) ]]; then
        echo 'Использование: ./run.sh [--terminal ghostty|kitty|here]'
        echo 'Отдельное окно TUI; here — запуск в текущем терминале.'
        exit 0
    else
        echo 'Использование: ./run.sh [--terminal ghostty|kitty|here]' >&2
        exit 2
    fi
fi
if [[ $terminal_kind == auto ]]; then
    if [[ -n ${KITTY_WINDOW_ID:-} ]]; then
        terminal_kind=kitty
    elif [[ ${TERM_PROGRAM:-} == ghostty ]] || command -v ghostty >/dev/null; then
        terminal_kind=ghostty
    elif command -v kitty >/dev/null; then
        terminal_kind=kitty
    else
        terminal_kind=here
    fi
fi
case $terminal_kind in
    ghostty|kitty) command -v "$terminal_kind" >/dev/null || { echo "Не найден $terminal_kind" >&2; exit 1; } ;;
    here) ;;
    *) echo "Неизвестный терминал: $terminal_kind" >&2; exit 2 ;;
esac
tui_cargo_build build --manifest-path "$project_root/Cargo.toml" --target-dir "$project_root/target" --locked
binary="$project_root/target/debug/claude-code-tui"
terminal_args=()
for panel_digit in 1 2 3 4; do
    case $terminal_kind in
        ghostty) terminal_args+=("--keybind=alt+digit_${panel_digit}=esc:${panel_digit}" "--keybind=alt+${panel_digit}=esc:${panel_digit}") ;;
        kitty) terminal_args+=(-o "map alt+${panel_digit} send_text all \x1b${panel_digit}") ;;
    esac
done
case $terminal_kind in
    ghostty)
        exec ghostty "${terminal_args[@]}" --working-directory="$project_root" -e "$binary"
        ;;
    kitty)
        exec kitty "${terminal_args[@]}" --directory="$project_root" "$binary"
        ;;
    here) exec "$binary" ;;
esac

#!/usr/bin/env bash
set -euo pipefail
project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
terminal_kind=auto
build=true
while [[ $# -gt 0 ]]; do
    case $1 in
        --terminal) [[ $# -ge 2 ]] || { echo 'Укажите ghostty или kitty' >&2; exit 2; }; terminal_kind=$2; shift 2 ;;
        --no-build) build=false; shift ;;
        -h|--help) echo 'Использование: ./install.sh [--terminal ghostty|kitty] [--no-build]'; exit 0 ;;
        *) echo "Неизвестный аргумент: $1" >&2; exit 2 ;;
    esac
done
if [[ $terminal_kind == auto ]]; then
    if command -v ghostty >/dev/null; then terminal_kind=ghostty
    elif command -v kitty >/dev/null; then terminal_kind=kitty
    else echo 'Для запуска нужен Ghostty или Kitty' >&2; exit 1
    fi
fi
case $terminal_kind in
    ghostty|kitty) command -v "$terminal_kind" >/dev/null || { echo "Не найден $terminal_kind" >&2; exit 1; } ;;
    *) echo "Неизвестный терминал: $terminal_kind" >&2; exit 2 ;;
esac
command -v python3 >/dev/null || { echo 'Для установки нужен python3' >&2; exit 1; }
if [[ $build == true ]]; then
    cargo build --release --manifest-path "$project_root/Cargo.toml" --target-dir "$project_root/target" --locked
fi
binary="$project_root/target/release/claude-code-tui"
[[ -x $binary ]] || { echo 'Нет release-сборки; запустите без --no-build' >&2; exit 1; }
: "${HOME:?Не задан HOME}"
bin_dir="$HOME/.local/bin"
applications_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
[[ $HOME == /* && $applications_dir == /* ]] || { echo 'HOME/XDG_DATA_HOME должны быть абсолютными путями' >&2; exit 1; }
install -d "$bin_dir" "$applications_dir"
install -m 755 "$binary" "$bin_dir/claude-code-tui"
launcher="$bin_dir/claude-code-tui-launch"
{
    printf '#!/usr/bin/env bash\nset -euo pipefail\nterminal_kind=%q\n' "$terminal_kind"
    cat <<'LAUNCH'
bin_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
export PATH="$bin_dir:/usr/local/bin:$PATH"
case $terminal_kind in
    ghostty) exec ghostty --working-directory="$HOME" -e "$bin_dir/claude-code-tui" ;;
    kitty) exec kitty --directory="$HOME" "$bin_dir/claude-code-tui" ;;
esac
LAUNCH
} > "$launcher"
chmod 755 "$launcher"
python3 - "$launcher" "$applications_dir/claude-code-tui.desktop" <<'PY'
from pathlib import Path
import sys
launcher, desktop = sys.argv[1:]
# Exec quoting is applied before desktop string escaping.
quoted = '"' + ''.join('\\' + c if c in '\\"`$' else c for c in launcher) + '"'
quoted = quoted.replace('\\', '\\\\').replace('%', '%%').replace('\n', '\\n').replace('\r', '\\r')
Path(desktop).write_text('[Desktop Entry]\n'
    'Type=Application\nName=Claude Code TUI\n'
    'Comment=Claude Code chats and Markdown notes\n'
    'Comment[ru]=Чаты Claude Code и Markdown-заметки\n'
    f'Exec={quoted}\nIcon=utilities-terminal\n'
    'Terminal=false\nCategories=Development;\n'
    'Keywords=Claude;AI;TUI;Notes;\n', encoding='utf-8')
Path(desktop).chmod(0o644)
PY
if command -v desktop-file-validate >/dev/null; then
    desktop-file-validate "$applications_dir/claude-code-tui.desktop"
fi
if command -v update-desktop-database >/dev/null; then
    update-desktop-database "$applications_dir"
fi
printf 'Установлено: Claude Code TUI\nЯрлык: %s\n' "$applications_dir/claude-code-tui.desktop"

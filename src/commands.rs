#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Command {
    pub key: char,
    pub russian: char,
    pub label: &'static str,
}
impl Command {
    pub fn shortcut(&self) -> String {
        if self.key == self.russian {
            format!("Alt+{}", self.key)
        } else {
            format!(
                "Alt+{}/{}",
                self.key.to_ascii_uppercase(),
                self.russian.to_uppercase()
            )
        }
    }
}
pub fn numbered_panel(key: &crossterm::event::KeyEvent) -> Option<crate::app::FocusPanel> {
    use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
    if key.modifiers != KeyModifiers::ALT
        || !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
    {
        return None;
    }
    match key.code {
        KeyCode::Char('1') => Some(crate::app::FocusPanel::Projects),
        KeyCode::Char('2') => Some(crate::app::FocusPanel::Dialogue),
        KeyCode::Char('3') => Some(crate::app::FocusPanel::OpenSessions),
        KeyCode::Char('4') => Some(crate::app::FocusPanel::Notes),
        _ => None,
    }
}
pub const COMMANDS: &[Command] = &[
    Command {
        key: 'o',
        russian: 'щ',
        label: "Менеджер рабочих сессий",
    },
    Command {
        key: '1',
        russian: '1',
        label: "Панель проектов",
    },
    Command {
        key: '2',
        russian: '2',
        label: "Панель диалога",
    },
    Command {
        key: '3',
        russian: '3',
        label: "Панель открытых сессий",
    },
    Command {
        key: '4',
        russian: '4',
        label: "Панель заметки",
    },
    Command {
        key: 'n',
        russian: 'т',
        label: "Новый чат",
    },
    Command {
        key: 's',
        russian: 'ы',
        label: "Быстро переключить чат",
    },
    Command {
        key: 'j',
        russian: 'о',
        label: "Предыдущий чат",
    },
    Command {
        key: 'f',
        russian: 'а',
        label: "Поиск по сообщениям и заметкам",
    },
    Command {
        key: 'k',
        russian: 'л',
        label: "Палитра команд",
    },
    Command {
        key: 'h',
        russian: 'р',
        label: "Все сочетания клавиш",
    },
    Command {
        key: 'z',
        russian: 'я',
        label: "Свернуть все проекты",
    },
    Command {
        key: 'm',
        russian: 'ь',
        label: "Открыть / скрыть заметку",
    },
    Command {
        key: 'l',
        russian: 'д',
        label: "Каталог заметок",
    },
    Command {
        key: 'e',
        russian: 'у',
        label: "Сохранить выделение в заметку",
    },
    Command {
        key: 'u',
        russian: 'г',
        label: "Скрыть / показать список проектов",
    },
    Command {
        key: 'd',
        russian: 'в',
        label: "Развернуть / восстановить чат",
    },
    Command {
        key: 't',
        russian: 'е',
        label: "Включить / выключить desktop уведомления",
    },
    Command {
        key: 'a',
        russian: 'ф',
        label: "Включить / выключить звук уведомлений",
    },
    Command {
        key: 'b',
        russian: 'и',
        label: "Вывод активного чата в Neovim (также Alt+V/М)",
    },
    Command {
        key: 'c',
        russian: 'с',
        label: "Выделение мышью / мышь CLI",
    },
    Command {
        key: 'x',
        russian: 'ч',
        label: "Закрыть активный чат",
    },
    Command {
        key: 'q',
        russian: 'й',
        label: "Выйти",
    },
];

pub fn fuzzy_score(text: &str, query: &str) -> Option<usize> {
    let text = text.to_lowercase();
    let mut remaining = text.as_str();
    let mut score = 0;
    for character in query
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace())
    {
        let offset = remaining.find(character)?;
        score += remaining[..offset].chars().count();
        remaining = &remaining[offset + character.len_utf8()..];
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numbered_panels_only_reserve_exact_alt_press_or_repeat() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
        for digit in ['1', '2', '3', '4'] {
            let mut key = KeyEvent::new(KeyCode::Char(digit), KeyModifiers::ALT);
            assert!(numbered_panel(&key).is_some());
            key.kind = KeyEventKind::Repeat;
            assert!(numbered_panel(&key).is_some());
            key.kind = KeyEventKind::Release;
            assert!(numbered_panel(&key).is_none());
            for modifier in [
                KeyModifiers::NONE,
                KeyModifiers::CONTROL,
                KeyModifiers::ALT | KeyModifiers::CONTROL,
                KeyModifiers::ALT | KeyModifiers::SHIFT,
            ] {
                assert!(numbered_panel(&KeyEvent::new(KeyCode::Char(digit), modifier)).is_none());
            }
        }
        assert!(numbered_panel(&KeyEvent::new(KeyCode::Char('5'), KeyModifiers::ALT)).is_none());
    }
    #[test]
    fn fuzzy_unicode_and_rank() {
        assert_eq!(fuzzy_score("Новый чат", "нч"), Some(5));
        assert!(fuzzy_score("abc", "ca").is_none());
        assert!(fuzzy_score("abc", "ab") < fuzzy_score("a__bc", "ab"));
    }
    #[test]
    fn keys_unique() {
        let keys: std::collections::HashSet<_> =
            COMMANDS.iter().map(|command| command.key).collect();
        assert_eq!(keys.len(), COMMANDS.len());
        for command in COMMANDS {
            assert_eq!(
                crate::app::App::shortcut_character(command.russian),
                command.key
            );
        }
    }
}

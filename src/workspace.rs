use crossterm::event::KeyCode;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Target {
    Workspace(crate::saved_workspaces::Action),
    Command(char),
    Chat(String, PathBuf),
    Note(String, PathBuf),
    Search {
        id: String,
        project: PathBuf,
        text: String,
        note: Option<PathBuf>,
        line: usize,
    },
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub label: String,
    pub target: Target,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Workspaces,
    WorkspaceName,
    Commands,
    Switcher,
    Search,
    Notes,
}
pub struct Popup {
    pub kind: Kind,
    pub query: String,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub hint: String,
}
impl Popup {
    pub fn new(kind: Kind, entries: Vec<Entry>) -> Self {
        Self {
            kind,
            query: String::new(),
            entries,
            selected: 0,
            hint: String::new(),
        }
    }
    pub fn title(&self) -> &'static str {
        match self.kind {
            Kind::Workspaces => "Менеджер сессий",
            Kind::WorkspaceName => "Название сессии",
            Kind::Commands => "Команды",
            Kind::Switcher => "Переключить чат",
            Kind::Search => "Поиск сообщений и заметок",
            Kind::Notes => "База знаний · заметки",
        }
    }
    pub fn filtered(&self) -> Vec<&Entry> {
        let mut items: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| {
                crate::commands::fuzzy_score(
                    &entry.label,
                    if self.kind == Kind::Search {
                        ""
                    } else {
                        &self.query
                    },
                )
                .map(|score| (score, entry))
            })
            .collect();
        items.sort_by_key(|(score, _)| *score);
        items.into_iter().map(|(_, entry)| entry).collect()
    }
    pub fn key(&mut self, key: KeyCode) -> Option<Target> {
        let count = self.filtered().len();
        match key {
            KeyCode::Down => self.selected = (self.selected + 1).min(count.saturating_sub(1)),
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Enter => {
                if self.kind == Kind::WorkspaceName {
                    return Some(Target::Workspace(crate::saved_workspaces::Action::Name(
                        self.query.clone(),
                    )));
                }
                return self
                    .filtered()
                    .get(self.selected)
                    .map(|entry| entry.target.clone());
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected = 0;
            }
            KeyCode::Char(character) if !character.is_control() => {
                self.query.push(character);
                self.selected = 0;
            }
            _ => {}
        }
        None
    }
}

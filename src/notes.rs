use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::text::Line;

use crate::pty::PtyManager;

pub struct NotesManager {
    root: Option<PathBuf>,
    legacy_root: Option<PathBuf>,
    known: HashSet<String>,
    visible: HashSet<String>,
    editors: PtyManager,
    pub exit_requested: bool,
}

impl Default for NotesManager {
    fn default() -> Self {
        let home = std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        let root = home
            .as_ref()
            .map(|home| home.join("knowledge-base/claude-code-tui/notes"));
        let legacy = std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.map(|home| home.join(".local/share")))
            .map(|root| root.join("claude-code-tui/notes"));
        let mut notes = Self::from_root(root);
        if let Some(legacy) = &legacy {
            notes
                .known
                .extend(Self::from_root(Some(legacy.clone())).known);
        }
        notes.legacy_root = legacy;
        notes
    }
}

impl NotesManager {
    fn from_root(root: Option<PathBuf>) -> Self {
        let known = root
            .as_ref()
            .and_then(|root| fs::read_dir(root).ok())
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension()?.to_str()? != "md" {
                    return None;
                }
                let id = path.file_stem()?.to_str()?;
                valid_id(id).then(|| id.to_owned())
            })
            .collect();
        Self {
            root,
            legacy_root: None,
            known,
            visible: HashSet::new(),
            editors: PtyManager::default(),
            exit_requested: false,
        }
    }

    pub fn catalog(&self) -> Vec<(String, PathBuf)> {
        let mut entries = Vec::new();
        for root in [&self.root, &self.legacy_root].into_iter().flatten() {
            if let Ok(files) = fs::read_dir(root) {
                for file in files
                    .flatten()
                    .filter(|file| file.file_type().is_ok_and(|kind| kind.is_file()))
                {
                    let path = file.path();
                    if path.extension().is_some_and(|extension| extension == "md")
                        && let Some(id) = path
                            .file_stem()
                            .and_then(|stem| stem.to_str())
                            .filter(|id| valid_id(id))
                        && !entries.iter().any(|(known, _)| known == id)
                    {
                        entries.push((id.to_owned(), path));
                    }
                }
            }
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }
    pub fn open(&mut self, id: &str, project: &Path, area: Rect) -> io::Result<()> {
        self.exit_requested = false;
        let file = self.ensure_note(id)?;
        self.editors.start_editor(id, &file, project, area)?;
        self.visible.insert(id.to_owned());
        Ok(())
    }
    pub fn append_selection(
        &mut self,
        id: &str,
        project: &Path,
        area: Rect,
        text: &str,
    ) -> io::Result<()> {
        let file = self.ensure_note(id)?;
        let fragment = format!(
            "\n\n## Фрагмент чата\n\n{}\n",
            text.lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        if self.editors.is_running(id) {
            self.open(id, project, area)?;
            self.editors
                .send_bytes(b"\x1c\x0e:normal! G\r:startinsert!\r")?;
            self.editors.send_paste(&fragment)?;
            self.editors.send_bytes(b"\x1c\x0e")?;
        } else {
            OpenOptions::new()
                .append(true)
                .open(file)?
                .write_all(fragment.as_bytes())?;
            self.open(id, project, area)?;
        }
        Ok(())
    }
    pub fn has_note(&self, id: &str) -> bool {
        self.known.contains(id)
    }

    fn note_path(&self, id: &str) -> io::Result<PathBuf> {
        if !valid_id(id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Некорректный ID сессии",
            ));
        }
        self.root
            .as_ref()
            .map(|root| root.join(format!("{id}.md")))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Не задан HOME для базы знаний"))
    }

    fn ensure_note(&mut self, id: &str) -> io::Result<PathBuf> {
        let path = self.note_path(id)?;
        fs::create_dir_all(path.parent().expect("note directory"))?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => {
                self.known.insert(id.to_owned());
                return Ok(path);
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Заметка должна быть обычным файлом",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let legacy_content = self
            .legacy_root
            .as_ref()
            .map(|root| root.join(format!("{id}.md")))
            .filter(|old| fs::symlink_metadata(old).is_ok_and(|metadata| metadata.is_file()))
            .map(fs::read)
            .transpose()?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                file.write_all(legacy_content.as_deref().unwrap_or(b"# Notes\n\n"))?;
                file.sync_all()?;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if !fs::symlink_metadata(&path)?.is_file() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Заметка должна быть обычным файлом",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
        self.known.insert(id.to_owned());
        Ok(path)
    }

    pub fn toggle(&mut self, id: &str, project: &Path, area: Rect) -> io::Result<bool> {
        self.exit_requested = false;
        if self.visible.remove(id) {
            self.editors.select_active(None);
            return Ok(false);
        }
        let file = self.ensure_note(id)?;
        self.editors.start_editor(id, &file, project, area)?;
        self.visible.insert(id.to_owned());
        Ok(true)
    }

    // Returns true once an explicitly requested application exit is safe.
    pub fn sync(&mut self, chat_id: Option<&str>) -> bool {
        self.editors.drain_output();
        self.visible.retain(|id| self.editors.is_running(id));
        if self.exit_requested {
            let running = self.editors.running_ids();
            let active = self
                .editors
                .active_id()
                .filter(|id| self.editors.is_running(id))
                .map(str::to_owned)
                .or_else(|| running.first().cloned());
            self.editors.select_active(active.as_deref());
            return running.is_empty();
        }
        self.editors
            .select_active(chat_id.filter(|id| self.visible.contains(*id)));
        false
    }

    pub fn is_visible(&self) -> bool {
        self.editors.active_id().is_some()
    }

    pub fn resize(&mut self, area: Rect) {
        self.editors.resize(area);
    }

    pub fn lines(&self) -> Vec<Line<'static>> {
        self.editors.active_lines()
    }

    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.editors.active_cursor()
    }

    pub fn active_path(&self) -> Option<PathBuf> {
        self.editors
            .active_id()
            .and_then(|id| self.note_path(id).ok())
    }

    pub fn send_key(&mut self, key: KeyEvent) -> io::Result<()> {
        self.editors.send_key(key)
    }

    pub fn send_paste(&mut self, text: &str) -> io::Result<()> {
        self.editors.send_paste(text)
    }

    pub fn send_mouse(
        &mut self,
        mouse: crossterm::event::MouseEvent,
        area: Rect,
    ) -> io::Result<()> {
        self.editors.send_mouse(mouse, area)
    }

    pub fn cancel_exit(&mut self) {
        self.exit_requested = false;
    }

    pub fn request_exit(&mut self) -> io::Result<bool> {
        self.exit_requested = true;
        let running = self.editors.running_ids();
        for id in &running {
            self.editors.select_active(Some(id));
            // CTRL-\ CTRL-N enters Normal mode without depending on Esc mappings.
            self.editors.send_bytes(b"\x1c\x0e:confirm qall\r")?;
        }
        self.editors
            .select_active(running.first().map(String::as_str));
        Ok(running.is_empty())
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_is_not_overwritten_and_is_detected_after_restart() {
        let root = std::env::temp_dir().join(format!("claude-notes-{}", uuid::Uuid::new_v4()));
        let mut notes = NotesManager::from_root(Some(root.clone()));
        let id = uuid::Uuid::new_v4().to_string();
        let file = notes.ensure_note(&id).unwrap();
        fs::write(&file, "# My note\n\nСодержимое\n").unwrap();
        assert_eq!(notes.ensure_note(&id).unwrap(), file);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "# My note\n\nСодержимое\n"
        );
        assert!(NotesManager::from_root(Some(root.clone())).has_note(&id));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_notes_are_copied_without_overwriting_or_deleting() {
        let scratch =
            std::env::temp_dir().join(format!("notes-migration-{}", uuid::Uuid::new_v4()));
        let legacy = scratch.join("legacy");
        let root = scratch.join("knowledge-base");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("chat.md"), "old note").unwrap();
        let mut notes = NotesManager::from_root(Some(root.clone()));
        notes.legacy_root = Some(legacy.clone());
        let target = notes.ensure_note("chat").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "old note");
        fs::write(&target, "newer note").unwrap();
        notes.ensure_note("chat").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "newer note");
        assert_eq!(
            fs::read_to_string(legacy.join("chat.md")).unwrap(),
            "old note"
        );
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn invalid_ids_and_non_file_targets_are_rejected() {
        let root = std::env::temp_dir().join(format!("claude-notes-{}", uuid::Uuid::new_v4()));
        let mut notes = NotesManager::from_root(Some(root.clone()));
        for id in ["", "../outside", "a/b", ".", "\n"] {
            assert!(notes.ensure_note(id).is_err());
        }
        fs::create_dir_all(root.join("directory.md")).unwrap();
        assert!(notes.ensure_note("directory").is_err());
        let blocked = root.join("not-a-directory");
        fs::write(&blocked, b"file").unwrap();
        let mut blocked_notes = NotesManager::from_root(Some(blocked));
        assert!(blocked_notes.ensure_note("note").is_err());
        assert!(!blocked_notes.has_note("note"));
        assert!(NotesManager::from_root(None).ensure_note("note").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("outside.md"), root.join("link.md")).unwrap();
            assert!(notes.ensure_note("link").is_err());
            assert!(!root.join("outside.md").exists());
        }
        fs::remove_dir_all(root).unwrap();
    }
}

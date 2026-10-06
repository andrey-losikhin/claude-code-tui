use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use vt100::{Color as VtColor, Parser};

struct ManagedSession {
    title: String,
    project_path: PathBuf,
    master: Box<dyn MasterPty>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: Receiver<Vec<u8>>,
    parser: Parser,
    stopped: Option<String>,
}

#[derive(Clone, Debug)]
pub struct OpenSession {
    pub id: String,
    pub title: String,
    pub project_path: PathBuf,
    pub active: bool,
    pub running: bool,
}

#[derive(Default)]
pub struct PtyManager {
    sessions: HashMap<String, ManagedSession>,
    order: Vec<String>,
    active: Option<String>,
    selection: Option<DragSelection>,
}

struct DragSelection {
    id: String,
    screen: vt100::Screen,
    area: Rect,
    start: (u16, u16),
    end: (u16, u16),
    dragged: bool,
}

impl DragSelection {
    fn point(&self, mouse: MouseEvent) -> (u16, u16) {
        let (rows, cols) = self.screen.size();
        (
            mouse
                .row
                .saturating_sub(self.area.y)
                .min(rows.saturating_sub(1)),
            mouse
                .column
                .saturating_sub(self.area.x)
                .min(cols.saturating_sub(1)),
        )
    }

    fn range(&self, row: u16) -> Option<(u16, u16)> {
        let (start, end) = if self.start <= self.end {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        };
        if row < start.0 || row > end.0 {
            return None;
        }
        let (_, cols) = self.screen.size();
        let mut first = if row == start.0 { start.1 } else { 0 };
        let mut last = if row == end.0 {
            end.1.saturating_add(1).min(cols)
        } else {
            cols
        };
        if self
            .screen
            .cell(row, first)
            .is_some_and(|cell| cell.is_wide_continuation())
        {
            first = first.saturating_sub(1);
        }
        if last < cols
            && self
                .screen
                .cell(row, last)
                .is_some_and(|cell| cell.is_wide_continuation())
        {
            last += 1;
        }
        Some((first, last))
    }

    fn text(&self) -> String {
        let (rows, _) = self.screen.size();
        (0..rows)
            .filter_map(|row| {
                let (start, end) = self.range(row)?;
                Some(
                    self.screen
                        .rows(start, end - start)
                        .nth(usize::from(row))
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl PtyManager {
    pub fn start(
        &mut self,
        id: Option<&str>,
        title: String,
        project_path: &Path,
        resume_id: Option<&str>,
        model: Option<&str>,
        size: Rect,
    ) -> io::Result<String> {
        if let Some(id) = id
            && let Some(session) = self
                .sessions
                .get_mut(id)
                .filter(|session| session.stopped.is_none())
        {
            session.title = title;
            self.active = Some(id.to_owned());
            return Ok(id.to_owned());
        }

        let session_id = id
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut command = CommandBuilder::new("claude");
        command.cwd(project_path);
        if let Some(resume_id) = resume_id {
            command.arg(format!("--resume={resume_id}"));
        } else {
            command.arg(format!("--session-id={session_id}"));
        }
        if let Some(model) = model {
            command.arg(format!("--model={model}"));
        }
        self.spawn_session(session_id, title, project_path, command, size)
    }

    pub fn start_editor(
        &mut self,
        id: &str,
        file: &Path,
        cwd: &Path,
        size: Rect,
    ) -> io::Result<()> {
        if self.is_running(id) {
            self.active = Some(id.to_owned());
            return Ok(());
        }
        let mut command = CommandBuilder::new("nvim");
        command.cwd(cwd);
        command.arg("--");
        command.arg(file);
        self.spawn_session(id.to_owned(), "Заметка".to_owned(), cwd, command, size)?;
        Ok(())
    }

    pub fn start_output_viewer(&mut self, file: &Path, cwd: &Path, size: Rect) -> io::Result<()> {
        let mut command = CommandBuilder::new("nvim");
        command.cwd(cwd);
        command.arg("-R");
        command.arg("-n");
        command.arg("-c");
        command.arg("setlocal buftype=nofile bufhidden=wipe noswapfile");
        command.arg("-c");
        command.arg("normal! G");
        command.arg("--");
        command.arg(file);
        self.spawn_session(
            "output-viewer".to_owned(),
            "Вывод чата".to_owned(),
            cwd,
            command,
            size,
        )?;
        Ok(())
    }

    pub fn active_output(&self) -> Option<String> {
        let session = self.active.as_ref().and_then(|id| self.sessions.get(id))?;
        Some(screen_output(session.parser.screen()))
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.sessions
            .get(id)
            .is_some_and(|session| session.stopped.is_none())
    }

    pub fn select_active(&mut self, id: Option<&str>) {
        self.active = id
            .filter(|id| self.sessions.contains_key(*id))
            .map(str::to_owned);
    }

    pub fn running_ids(&self) -> Vec<String> {
        self.order
            .iter()
            .filter(|id| self.is_running(id))
            .cloned()
            .collect()
    }

    fn spawn_session(
        &mut self,
        session_id: String,
        title: String,
        project_path: &Path,
        command: CommandBuilder,
        size: Rect,
    ) -> io::Result<String> {
        let cols = size.width.saturating_sub(2).max(1);
        let rows = size.height.saturating_sub(2).max(1);
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io::Error::other)?;

        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(io::Error::other)?;
        drop(pair.slave);

        let handles = (|| {
            let reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
            let writer = pair.master.take_writer().map_err(io::Error::other)?;
            Ok::<_, io::Error>((reader, writer))
        })();
        let (reader, writer) = match handles {
            Ok(handles) => handles,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let (sender, output) = mpsc::sync_channel(128);
        thread::spawn(move || read_output(reader, sender));

        if !self.order.contains(&session_id) {
            self.order.push(session_id.clone());
        }
        self.active = Some(session_id.clone());
        self.sessions.insert(
            session_id.clone(),
            ManagedSession {
                title,
                project_path: project_path.to_path_buf(),
                master: pair.master,
                writer,
                child,
                output,
                parser: Parser::new(rows, cols, 2_000),
                stopped: None,
            },
        );
        Ok(session_id)
    }

    pub fn active_id(&self) -> Option<&str> {
        self.active.as_deref()
    }

    pub fn active_title(&self) -> Option<&str> {
        self.active
            .as_ref()
            .and_then(|id| self.sessions.get(id))
            .map(|session| session.title.as_str())
    }

    pub fn active_project_path(&self) -> Option<&Path> {
        self.active
            .as_ref()
            .and_then(|id| self.sessions.get(id))
            .map(|session| session.project_path.as_path())
    }

    pub fn open_sessions(&self) -> Vec<OpenSession> {
        self.order
            .iter()
            .filter_map(|id| {
                let session = self.sessions.get(id)?;
                Some(OpenSession {
                    id: id.clone(),
                    title: session.title.clone(),
                    project_path: session.project_path.clone(),
                    active: self.active.as_deref() == Some(id),
                    running: session.stopped.is_none(),
                })
            })
            .collect()
    }

    pub fn active_cursor(&self) -> Option<(u16, u16)> {
        let session = self.active.as_ref().and_then(|id| self.sessions.get(id))?;
        let screen = session.parser.screen();
        if screen.hide_cursor() || screen.scrollback() > 0 {
            None
        } else {
            Some(screen.cursor_position())
        }
    }

    pub fn cycle(&mut self, reverse: bool) -> bool {
        if self.order.is_empty() {
            return false;
        }
        let current = self
            .active
            .as_ref()
            .and_then(|id| self.order.iter().position(|item| item == id));
        let next = match (current, reverse) {
            (None, true) => self.order.len() - 1,
            (None, false) => 0,
            (Some(current), true) => (current + self.order.len() - 1) % self.order.len(),
            (Some(current), false) => (current + 1) % self.order.len(),
        };
        self.active = Some(self.order[next].clone());
        true
    }

    pub fn close_active(&mut self) -> io::Result<Option<String>> {
        let Some(id) = self.active.clone() else {
            return Ok(None);
        };
        let Some(session) = self.sessions.get_mut(&id) else {
            self.active = None;
            return Ok(None);
        };

        if session.stopped.is_none() {
            match session.child.try_wait().map_err(io::Error::other)? {
                Some(_) => {}
                None => {
                    session.child.kill().map_err(io::Error::other)?;
                    let _ = session.child.wait().map_err(io::Error::other)?;
                }
            }
        }

        self.sessions.remove(&id);
        self.order.retain(|session_id| session_id != &id);
        self.active = None;
        Ok(Some(id))
    }

    pub fn drain_output(&mut self) -> bool {
        let mut completed = false;
        for session in self.sessions.values_mut() {
            for bytes in session.output.try_iter() {
                session.parser.process(&bytes);
            }
            if session.stopped.is_none() {
                match session.child.try_wait() {
                    Ok(Some(status)) => {
                        session.stopped = Some(format!("Завершён: {status}"));
                        completed = true;
                    }
                    Ok(None) => {}
                    Err(error) => session.stopped = Some(format!("Ошибка процесса: {error}")),
                }
            }
        }
        completed
    }

    pub fn remove_stopped(&mut self) {
        self.sessions.retain(|_, session| session.stopped.is_none());
        self.order.retain(|id| self.sessions.contains_key(id));
        if self
            .active
            .as_ref()
            .is_some_and(|id| !self.sessions.contains_key(id))
        {
            self.active = None;
        }
    }

    pub fn sync_titles(&mut self, mut resolve: impl FnMut(&str) -> Option<String>) {
        for (id, session) in &mut self.sessions {
            if let Some(title) = resolve(id) {
                session.title = title;
            }
        }
    }

    pub fn resize(&mut self, area: Rect) {
        let cols = area.width.saturating_sub(2).max(1);
        let rows = area.height.saturating_sub(2).max(1);
        if self.selection.as_ref().is_some_and(|selection| {
            selection.screen.size() != (rows, cols)
                || selection.area.x != area.x.saturating_add(1)
                || selection.area.y != area.y.saturating_add(1)
        }) {
            self.selection = None;
        }
        for session in self.sessions.values_mut() {
            if session.parser.screen().size() != (rows, cols) {
                let _ = session.master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                });
                session.parser.screen_mut().set_size(rows, cols);
            }
        }
    }

    pub fn send_key(&mut self, key: KeyEvent) -> io::Result<()> {
        self.selection = None;
        let application_cursor = self
            .active
            .as_ref()
            .and_then(|id| self.sessions.get(id))
            .is_some_and(|session| session.parser.screen().application_cursor());
        if let Some(bytes) = encode_key(key, application_cursor) {
            self.send_bytes(&bytes)?;
        }
        Ok(())
    }

    pub fn send_paste(&mut self, text: &str) -> io::Result<()> {
        self.selection = None;
        let bracketed = self
            .active
            .as_ref()
            .and_then(|id| self.sessions.get(id))
            .is_some_and(|session| session.parser.screen().bracketed_paste());
        self.send_bytes(&encode_paste(text, bracketed))
    }

    pub fn send_mouse(&mut self, mouse: MouseEvent, area: Rect) -> io::Result<()> {
        let Some(session) = self
            .active
            .as_ref()
            .and_then(|id| self.sessions.get_mut(id))
        else {
            return Ok(());
        };
        if session.stopped.is_some() {
            return Ok(());
        }
        let screen = session.parser.screen();
        if let Some(bytes) = encode_mouse(
            mouse,
            area,
            screen.mouse_protocol_mode(),
            screen.mouse_protocol_encoding(),
        ) {
            session.writer.write_all(&bytes)?;
            session.writer.flush()?;
        } else if screen.mouse_protocol_mode() == vt100::MouseProtocolMode::None {
            let scroll = screen.scrollback();
            let next = match mouse.kind {
                MouseEventKind::ScrollUp => scroll.saturating_add(3),
                MouseEventKind::ScrollDown => scroll.saturating_sub(3),
                _ => scroll,
            };
            session.parser.screen_mut().set_scrollback(next);
        }
        Ok(())
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub fn select_mouse(&mut self, mouse: MouseEvent, area: Rect) -> (bool, Option<String>) {
        if self
            .selection
            .as_ref()
            .is_some_and(|selection| self.active.as_deref() != Some(&selection.id))
        {
            self.selection = None;
        }
        let inner = area.inner(ratatui::layout::Margin::new(1, 1));
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            self.selection = None;
            if inner.contains((mouse.column, mouse.row).into())
                && let Some(session) = self.active.as_ref().and_then(|id| self.sessions.get(id))
            {
                let point = (mouse.row - inner.y, mouse.column - inner.x);
                self.selection = Some(DragSelection {
                    id: self.active.clone().unwrap(),
                    screen: session.parser.screen().clone(),
                    area: inner,
                    start: point,
                    end: point,
                    dragged: false,
                });
            }
        } else if matches!(
            mouse.kind,
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
        ) {
            if let Some(selection) = &mut self.selection {
                selection.end = selection.point(mouse);
                selection.dragged |= selection.start != selection.end;
                if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
                    let selection = self.selection.take().unwrap();
                    return if selection.dragged {
                        (true, Some(selection.text()))
                    } else {
                        (false, None)
                    };
                }
                return (true, None);
            }
        } else if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            self.selection = None;
        }
        (false, None)
    }

    pub fn send_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        let Some(id) = &self.active else {
            return Ok(());
        };
        let Some(session) = self.sessions.get_mut(id) else {
            return Ok(());
        };
        if session.stopped.is_some() {
            return Ok(());
        }
        session.parser.screen_mut().set_scrollback(0);
        session.writer.write_all(bytes)?;
        session.writer.flush()?;
        Ok(())
    }

    pub fn active_lines(&self) -> Vec<Line<'static>> {
        let Some(session) = self.active.as_ref().and_then(|id| self.sessions.get(id)) else {
            return vec![Line::from("Нет активной CLI-сессии")];
        };
        let selection = self
            .selection
            .as_ref()
            .filter(|selection| self.active.as_deref() == Some(&selection.id));
        let screen = selection
            .map(|selection| &selection.screen)
            .unwrap_or_else(|| session.parser.screen());
        let (rows, cols) = screen.size();
        let mut lines = Vec::with_capacity(rows as usize + usize::from(session.stopped.is_some()));
        for row in 0..rows {
            let mut spans = Vec::with_capacity(cols as usize);
            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let content = if cell.has_contents() {
                    cell.contents().to_owned()
                } else {
                    " ".to_owned()
                };
                let mut style = Style::default()
                    .fg(map_color(cell.fgcolor()))
                    .bg(map_color(cell.bgcolor()));
                let mut modifiers = Modifier::empty();
                if cell.bold() {
                    modifiers |= Modifier::BOLD;
                }
                if cell.dim() {
                    modifiers |= Modifier::DIM;
                }
                if cell.italic() {
                    modifiers |= Modifier::ITALIC;
                }
                if cell.underline() {
                    modifiers |= Modifier::UNDERLINED;
                }
                if cell.inverse() {
                    modifiers |= Modifier::REVERSED;
                }
                style = style.add_modifier(modifiers);
                if selection.is_some_and(|selection| {
                    selection.dragged
                        && selection
                            .range(row)
                            .is_some_and(|(start, end)| col >= start && col < end)
                }) {
                    style = if modifiers.contains(Modifier::REVERSED) {
                        style.remove_modifier(Modifier::REVERSED)
                    } else {
                        style.add_modifier(Modifier::REVERSED)
                    };
                }
                spans.push(Span::styled(content, style));
            }
            lines.push(Line::from(spans));
        }
        if let Some(status) = &session.stopped {
            lines.push(Line::from(status.clone()));
        }
        lines
    }
}

fn screen_output(source: &vt100::Screen) -> String {
    let mut screen = source.clone();
    let (_, cols) = screen.size();
    screen.set_scrollback(usize::MAX);
    let history = screen.scrollback();
    let mut output = String::new();
    for offset in (1..=history).rev() {
        screen.set_scrollback(offset);
        output.push_str(&screen.rows(0, cols).next().unwrap_or_default());
        output.push('\n');
    }
    screen.set_scrollback(0);
    output.push_str(&screen.contents());
    output.push('\n');
    output
}

fn read_output(mut reader: Box<dyn Read + Send>, sender: mpsc::SyncSender<Vec<u8>>) {
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                if sender.send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
        }
    }
}

fn encode_mouse(
    mouse: MouseEvent,
    area: Rect,
    mode: vt100::MouseProtocolMode,
    encoding: vt100::MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    use vt100::{MouseProtocolEncoding as Encoding, MouseProtocolMode as Mode};
    let inner = area.inner(ratatui::layout::Margin::new(1, 1));
    if mode == Mode::None || !inner.contains((mouse.column, mouse.row).into()) {
        return None;
    }
    let button_code = |button| -> u8 {
        match button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
        }
    };
    let mut button = match mouse.kind {
        MouseEventKind::Down(button) => button_code(button),
        MouseEventKind::Up(button) if mode != Mode::Press => button_code(button),
        MouseEventKind::Drag(button) if matches!(mode, Mode::ButtonMotion | Mode::AnyMotion) => {
            32 + button_code(button)
        }
        MouseEventKind::Moved if mode == Mode::AnyMotion => 35,
        MouseEventKind::ScrollUp => 64,
        MouseEventKind::ScrollDown => 65,
        MouseEventKind::ScrollLeft => 66,
        MouseEventKind::ScrollRight => 67,
        _ => return None,
    };
    if mouse.modifiers.contains(KeyModifiers::SHIFT) {
        button += 4;
    }
    if mouse.modifiers.contains(KeyModifiers::ALT) {
        button += 8;
    }
    if mouse.modifiers.contains(KeyModifiers::CONTROL) {
        button += 16;
    }
    let x = mouse.column - inner.x + 1;
    let y = mouse.row - inner.y + 1;
    let release = matches!(mouse.kind, MouseEventKind::Up(_));
    if encoding == Encoding::Sgr {
        Some(format!("\x1b[<{button};{x};{y}{}", if release { 'm' } else { 'M' }).into_bytes())
    } else {
        let button = if release { 3 + (button & 28) } else { button };
        let values = [u32::from(button + 32), u32::from(x) + 32, u32::from(y) + 32];
        let mut bytes = b"\x1b[M".to_vec();
        for value in values {
            if encoding == Encoding::Utf8 {
                let mut buffer = [0; 4];
                bytes.extend_from_slice(char::from_u32(value)?.encode_utf8(&mut buffer).as_bytes());
            } else {
                bytes.push(u8::try_from(value).ok()?);
            }
        }
        Some(bytes)
    }
}

fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        [
            b"\x1b[200~".as_slice(),
            text.as_bytes(),
            b"\x1b[201~".as_slice(),
        ]
        .concat()
    } else {
        text.as_bytes().to_vec()
    }
}

fn encode_key(key: KeyEvent, application_cursor: bool) -> Option<Vec<u8>> {
    let mut modifiers = key.modifiers;
    let code = if key.code == KeyCode::BackTab {
        modifiers.insert(KeyModifiers::SHIFT);
        KeyCode::Tab
    } else {
        key.code
    };
    let modifier_code = 1
        + u8::from(modifiers.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(modifiers.contains(KeyModifiers::ALT))
        + 4 * u8::from(modifiers.contains(KeyModifiers::CONTROL))
        + 8 * u8::from(modifiers.contains(KeyModifiers::SUPER))
        + 16 * u8::from(modifiers.contains(KeyModifiers::HYPER))
        + 32 * u8::from(modifiers.contains(KeyModifiers::META));
    let csi_u = |code: u32| format!("\x1b[{code};{modifier_code}u").into_bytes();
    let navigation = match code {
        KeyCode::Up => Some('A'),
        KeyCode::Down => Some('B'),
        KeyCode::Right => Some('C'),
        KeyCode::Left => Some('D'),
        KeyCode::Home => Some('H'),
        KeyCode::End => Some('F'),
        _ => None,
    };
    if let Some(final_character) = navigation {
        return Some(if modifier_code > 1 {
            format!("\x1b[1;{modifier_code}{final_character}").into_bytes()
        } else if application_cursor {
            format!("\x1bO{final_character}").into_bytes()
        } else {
            format!("\x1b[{final_character}").into_bytes()
        });
    }
    let tilde_key = match code {
        KeyCode::Insert => Some(2),
        KeyCode::Delete => Some(3),
        KeyCode::PageUp => Some(5),
        KeyCode::PageDown => Some(6),
        KeyCode::F(5) => Some(15),
        KeyCode::F(6) => Some(17),
        KeyCode::F(7) => Some(18),
        KeyCode::F(8) => Some(19),
        KeyCode::F(9) => Some(20),
        KeyCode::F(10) => Some(21),
        KeyCode::F(11) => Some(23),
        KeyCode::F(12) => Some(24),
        _ => None,
    };
    if let Some(number) = tilde_key {
        return Some(if modifier_code > 1 {
            format!("\x1b[{number};{modifier_code}~").into_bytes()
        } else {
            format!("\x1b[{number}~").into_bytes()
        });
    }
    if let KeyCode::F(number @ 1..=4) = code {
        let final_character = char::from(b'P' + number - 1);
        return Some(if modifier_code > 1 {
            format!("\x1b[1;{modifier_code}{final_character}").into_bytes()
        } else {
            format!("\x1bO{final_character}").into_bytes()
        });
    }
    if code == KeyCode::Tab && modifiers == KeyModifiers::SHIFT {
        return Some(b"\x1b[Z".to_vec());
    }
    // Preserve distinctions such as Shift+Enter and Ctrl+Shift+letter with CSI-u.
    let extended = modifiers
        .intersects(KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META)
        || (modifiers.contains(KeyModifiers::SHIFT) && modifiers.contains(KeyModifiers::CONTROL));
    let mut bytes = match code {
        KeyCode::Char(character) if extended => {
            let character = if modifiers.contains(KeyModifiers::CONTROL) {
                crate::app::App::shortcut_character(character)
            } else {
                character
            };
            return Some(csi_u(u32::from(character)));
        }
        KeyCode::Char(character) if modifiers.contains(KeyModifiers::CONTROL) => {
            let character = crate::app::App::shortcut_character(character);
            let control = match character {
                'a'..='z' => character as u8 - b'a' + 1,
                ' ' | '@' | '2' => 0,
                '[' | '3' => 0x1b,
                '\\' | '4' => 0x1c,
                ']' | '5' => 0x1d,
                '^' | '6' => 0x1e,
                '_' | '/' | '7' => 0x1f,
                '8' => 0x7f,
                _ => return Some(csi_u(u32::from(character))),
            };
            vec![control]
        }
        KeyCode::Char(character) => {
            let mut encoded = [0; 4];
            character.encode_utf8(&mut encoded).as_bytes().to_vec()
        }
        KeyCode::Enter
            if modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL) || extended =>
        {
            return Some(csi_u(13));
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace
            if modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL) || extended =>
        {
            return Some(csi_u(127));
        }
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab
            if modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL) || extended =>
        {
            return Some(csi_u(9));
        }
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc
            if modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::CONTROL) || extended =>
        {
            return Some(csi_u(27));
        }
        KeyCode::Esc => vec![0x1b],
        _ => return None,
    };
    if modifiers.contains(KeyModifiers::ALT) {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

fn map_color(color: VtColor) -> Color {
    match color {
        VtColor::Default => Color::Reset,
        VtColor::Idx(index) => Color::Indexed(index),
        VtColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

impl Drop for PtyManager {
    fn drop(&mut self) {
        for session in self.sessions.values_mut() {
            if session.stopped.is_none() {
                // Stop the child before closing the PTY master; portable-pty
                // sends newline/EOT while dropping the master.
                let _ = session.child.kill();
                let _ = session.child.wait();
            }
        }
    }
}

#[cfg(test)]
mod mouse_tests {
    use super::*;
    #[test]
    fn translated_mouse_coordinates_and_modes() {
        use vt100::{MouseProtocolEncoding as Encoding, MouseProtocolMode as Mode};
        let area = Rect::new(30, 5, 70, 20);
        let mouse = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 33,
            row: 8,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            encode_mouse(mouse, area, Mode::ButtonMotion, Encoding::Sgr).unwrap(),
            b"\x1b[<0;3;3M"
        );
        assert_eq!(
            encode_mouse(
                MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    ..mouse
                },
                area,
                Mode::ButtonMotion,
                Encoding::Sgr
            )
            .unwrap(),
            b"\x1b[<0;3;3m"
        );
        assert!(encode_mouse(mouse, area, Mode::None, Encoding::Sgr).is_none());
        assert!(
            encode_mouse(
                MouseEvent {
                    column: 30,
                    ..mouse
                },
                area,
                Mode::ButtonMotion,
                Encoding::Sgr
            )
            .is_none()
        );
        assert!(
            encode_mouse(
                MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Left),
                    ..mouse
                },
                area,
                Mode::PressRelease,
                Encoding::Sgr
            )
            .is_none()
        );
    }
}

#[cfg(test)]
mod keyboard_tests {
    use super::*;

    #[test]
    fn mouse_selection_is_linewise_reversible_clamped_and_unicode_safe() {
        let mut parser = Parser::new(3, 12, 0);
        parser.process("alpha\r\nbeta\r\n界яend".as_bytes());
        let mut selection = DragSelection {
            id: "chat".to_owned(),
            screen: parser.screen().clone(),
            area: Rect::new(50, 10, 12, 3),
            start: (0, 2),
            end: (1, 1),
            dragged: true,
        };
        assert_eq!(selection.text(), "pha\nbe");
        std::mem::swap(&mut selection.start, &mut selection.end);
        assert_eq!(selection.text(), "pha\nbe");
        let mouse = MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 0,
            row: 100,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(selection.point(mouse), (2, 0));
        selection.start = (2, 1);
        selection.end = (2, 2);
        assert_eq!(selection.text(), "界я");
        selection.end = (2, 0);
        assert_eq!(selection.text(), "界");
    }

    #[test]
    fn output_snapshot_includes_scrollback_and_screen_without_ansi_or_mutation() {
        let mut parser = Parser::new(3, 30, 2000);
        parser.process(b"\x1b[31mfirst\x1b[0m\r\nsecond\r\nthird\r\nfourth");
        parser.screen_mut().set_scrollback(1);
        let output = screen_output(parser.screen());
        assert_eq!(output, "first\nsecond\nthird\nfourth\n");
        assert_eq!(parser.screen().scrollback(), 1);
        assert!(!output.contains('\x1b'));
    }

    fn encoded(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
        encode_key(KeyEvent::new(code, modifiers), false).unwrap()
    }

    #[test]
    fn enter_variants_are_distinct() {
        assert_eq!(encoded(KeyCode::Enter, KeyModifiers::NONE), b"\r");
        assert_eq!(encoded(KeyCode::Enter, KeyModifiers::SHIFT), b"\x1b[13;2u");
        assert_eq!(
            encoded(KeyCode::Enter, KeyModifiers::CONTROL),
            b"\x1b[13;5u"
        );
        assert_eq!(
            encoded(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            b"\x1b[13;6u"
        );
        assert_eq!(encoded(KeyCode::Enter, KeyModifiers::ALT), b"\x1b\r");
    }

    #[test]
    fn navigation_and_editing_preserve_modifiers() {
        assert_eq!(encoded(KeyCode::Left, KeyModifiers::CONTROL), b"\x1b[1;5D");
        assert_eq!(
            encoded(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            b"\x1b[1;6C"
        );
        assert_eq!(
            encoded(KeyCode::PageDown, KeyModifiers::SHIFT),
            b"\x1b[6;2~"
        );
        assert_eq!(
            encoded(KeyCode::Delete, KeyModifiers::CONTROL),
            b"\x1b[3;5~"
        );
        assert_eq!(
            encoded(KeyCode::Backspace, KeyModifiers::CONTROL),
            b"\x1b[127;5u"
        );
        assert_eq!(encoded(KeyCode::BackTab, KeyModifiers::NONE), b"\x1b[Z");
        assert_eq!(
            encoded(KeyCode::Tab, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            b"\x1b[9;6u"
        );
        assert_eq!(encoded(KeyCode::F(5), KeyModifiers::CONTROL), b"\x1b[15;5~");
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE), true).unwrap(),
            b"\x1bOA"
        );
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL), true).unwrap(),
            b"\x1b[1;5A"
        );
    }

    #[test]
    fn printable_and_control_keys_keep_their_semantics() {
        assert_eq!(
            encoded(KeyCode::Char('Ж'), KeyModifiers::SHIFT),
            "Ж".as_bytes()
        );
        assert_eq!(encoded(KeyCode::Char('c'), KeyModifiers::CONTROL), b"\x03");
        assert_eq!(encoded(KeyCode::Char('с'), KeyModifiers::CONTROL), b"\x03");
        assert_eq!(
            encoded(
                KeyCode::Char('J'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ),
            b"\x1b[106;6u"
        );
        assert_eq!(encoded(KeyCode::Char('p'), KeyModifiers::ALT), b"\x1bp");
        assert_eq!(
            encoded(
                KeyCode::Char('x'),
                KeyModifiers::CONTROL | KeyModifiers::ALT
            ),
            b"\x1b\x18"
        );
        assert_eq!(encoded(KeyCode::Char(' '), KeyModifiers::CONTROL), b"\x00");
    }

    #[test]
    fn multiline_paste_retains_content_and_child_paste_mode() {
        let text = "line one\nстрока два\n";
        assert_eq!(encode_paste(text, false), text.as_bytes());
        assert_eq!(
            encode_paste(text, true),
            format!("\x1b[200~{text}\x1b[201~").as_bytes()
        );
    }
}

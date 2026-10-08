use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Unknown,
    Working,
    Permission,
    Input,
    Ready,
    Error,
}
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "открыт · статус неизвестен",
            Self::Working => "работает",
            Self::Permission => "ждёт разрешения",
            Self::Input => "ждёт ввода",
            Self::Ready => "готово",
            Self::Error => "ошибка",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct HookEvent {
    pub session_id: String,
    pub hook_event_name: String,
    #[serde(default)]
    pub notification_type: String,
    #[serde(default)]
    pub prompt_id: String,
}

#[derive(Default)]
pub struct Activity {
    pub states: HashMap<String, Status>,
    pub unread: HashSet<String>,
    prompts: HashMap<String, String>,
}
impl Activity {
    pub fn apply(&mut self, event: HookEvent, visible: bool) -> bool {
        let id = event.session_id;
        if event.hook_event_name == "UserPromptSubmit" {
            self.prompts.insert(id.clone(), event.prompt_id.clone());
        } else if !event.prompt_id.is_empty()
            && self
                .prompts
                .get(&id)
                .is_some_and(|prompt| !prompt.is_empty() && *prompt != event.prompt_id)
        {
            return false;
        }
        let state = match event.hook_event_name.as_str() {
            "SessionStart" => Status::Input,
            "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
                Status::Working
            }
            "PermissionRequest" => Status::Permission,
            "Stop" => Status::Ready,
            "StopFailure" => Status::Error,
            "Notification" => match event.notification_type.as_str() {
                "permission_prompt" => Status::Permission,
                "idle_prompt" | "elicitation_dialog" | "elicitation_url" => Status::Input,
                _ => return false,
            },
            _ => return false,
        };
        let previous = self.states.insert(id.clone(), state);
        let notify = !visible
            && matches!(
                state,
                Status::Ready | Status::Error | Status::Permission | Status::Input
            )
            && previous != Some(state);
        if notify {
            self.unread.insert(id);
        }
        notify
    }
    pub fn label(&self, id: &str) -> String {
        format!(
            "{}{}",
            if self.unread.contains(id) { "◆ " } else { "" },
            self.states.get(id).copied().unwrap_or_default().label()
        )
    }
}

pub struct EventBridge {
    socket: UnixDatagram,
    directory: PathBuf,
    pub settings: String,
}
impl EventBridge {
    pub fn new() -> io::Result<Self> {
        let executable = std::env::current_exe()?;
        let directory = std::env::temp_dir().join(format!("cct-{}", uuid::Uuid::new_v4()));
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let path = directory.join("events.sock");
        let socket = match UnixDatagram::bind(&path) {
            Ok(socket) => socket,
            Err(error) => {
                let _ = std::fs::remove_dir(&directory);
                return Err(error);
            }
        };
        if let Err(error) = socket.set_nonblocking(true) {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_dir(&directory);
            return Err(error);
        }
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
        let command = format!(
            "{} --tui-hook {}",
            quote(&executable.to_string_lossy()),
            quote(&path.to_string_lossy())
        );
        let mut hooks = serde_json::Map::new();
        for event in [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PermissionRequest",
            "PostToolUse",
            "PostToolUseFailure",
            "Notification",
            "Stop",
            "StopFailure",
        ] {
            hooks.insert(event.into(), serde_json::json!([{"matcher":"*", "hooks":[{"type":"command", "command":command, "timeout":2}]}]));
        }
        Ok(Self {
            socket,
            directory,
            settings: serde_json::json!({"hooks": hooks}).to_string(),
        })
    }
    pub fn drain(&self) -> Vec<HookEvent> {
        let mut events = Vec::new();
        let mut buffer = [0; 4096];
        for _ in 0..64 {
            match self.socket.recv(&mut buffer) {
                Ok(size) => {
                    if let Ok(event) = serde_json::from_slice(&buffer[..size]) {
                        events.push(event);
                    }
                }
                Err(_) => break,
            }
        }
        events
    }
}
impl Drop for EventBridge {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.directory.join("events.sock"));
        let _ = std::fs::remove_dir(&self.directory);
    }
}

// This hook is observational: no stdout/permission decisions and always exits successfully.
pub fn emit_hook(path: &std::path::Path) {
    let mut bytes = Vec::new();
    if io::stdin()
        .take(2 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return;
    };
    let field = |name: &str| {
        value[name]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(128)
            .collect::<String>()
    };
    let event = HookEvent {
        session_id: field("session_id"),
        hook_event_name: field("hook_event_name"),
        notification_type: field("notification_type"),
        prompt_id: field("prompt_id"),
    };
    if let (Ok(socket), Ok(bytes)) = (UnixDatagram::unbound(), serde_json::to_vec(&event)) {
        let _ = socket.set_nonblocking(true);
        let _ = socket.send_to(&bytes, path);
    }
}

pub struct Notifier(std::sync::mpsc::SyncSender<(bool, bool)>);
impl Notifier {
    pub fn new() -> Self {
        let (sender, receiver) = std::sync::mpsc::sync_channel::<(bool, bool)>(8);
        std::thread::spawn(move || {
            while let Ok((desktop, sound)) = receiver.recv() {
                if sound {
                    use std::io::Write;
                    let _ = io::stdout().write_all(b"\x07");
                }
                if desktop
                    && let Ok(mut child) = std::process::Command::new("notify-send")
                        .args([
                            "--app-name=Claude Code TUI",
                            "Claude Code",
                            "Фоновый чат требует внимания",
                        ])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                {
                    let start = std::time::Instant::now();
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) | Err(_) => break,
                            _ => {}
                        }
                        if start.elapsed() > std::time::Duration::from_secs(2) {
                            let _ = child.kill();
                            let _ = child.wait();
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                }
            }
        });
        Self(sender)
    }
    pub fn notify(&self, desktop: bool, sound: bool) {
        if desktop || sound {
            let _ = self.0.try_send((desktop, sound));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(name: &str) -> HookEvent {
        HookEvent {
            session_id: "id".into(),
            hook_event_name: name.into(),
            notification_type: String::new(),
            prompt_id: "p1".into(),
        }
    }
    #[test]
    fn input_failure_and_notification_states() {
        let mut activity = Activity::default();
        activity.apply(event("SessionStart"), true);
        assert_eq!(activity.states["id"], Status::Input);
        activity.apply(event("UserPromptSubmit"), true);
        activity.apply(event("PostToolUseFailure"), true);
        assert_eq!(activity.states["id"], Status::Working);
        let mut notification = event("Notification");
        notification.notification_type = "permission_prompt".into();
        assert!(activity.apply(notification, false));
        assert_eq!(activity.states["id"], Status::Permission);
        let mut notification = event("Notification");
        notification.notification_type = "idle_prompt".into();
        activity.apply(notification, false);
        assert_eq!(activity.states["id"], Status::Input);
        activity.apply(event("StopFailure"), false);
        assert_eq!(activity.states["id"], Status::Error);
    }
    #[test]
    fn statuses_and_unread() {
        let mut state = Activity::default();
        assert!(!state.apply(event("UserPromptSubmit"), false));
        assert_eq!(state.states["id"], Status::Working);
        assert!(state.apply(event("PermissionRequest"), false));
        assert!(!state.apply(event("PermissionRequest"), false));
        state.apply(event("PostToolUse"), true);
        assert!(!state.apply(event("Stop"), true));
        assert_eq!(state.states["id"], Status::Ready);
        let mut stale = event("StopFailure");
        stale.prompt_id = "old".into();
        assert!(!state.apply(stale, false));
        assert_eq!(state.states["id"], Status::Ready);
    }
    #[test]
    fn bridge_is_private_and_observational() {
        use std::os::unix::fs::PermissionsExt;
        let bridge = EventBridge::new().unwrap();
        assert_eq!(
            std::fs::metadata(&bridge.directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let settings: serde_json::Value = serde_json::from_str(&bridge.settings).unwrap();
        assert!(settings.get("permissions").is_none());
        let path = bridge.directory.clone();
        drop(bridge);
        assert!(!path.exists());
    }
}

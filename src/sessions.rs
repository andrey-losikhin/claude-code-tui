use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub project_path: PathBuf,
    pub history_path: Option<PathBuf>,
    pub title: String,
    pub custom_title: Option<String>,
    pub git_branch: Option<String>,
    pub updated_at: SystemTime,
}

type CachedSession = (u64, SystemTime, Option<PathBuf>, Option<Session>);
thread_local! {
    static HISTORY_CACHE: RefCell<HashMap<PathBuf, CachedSession>> = RefCell::new(HashMap::new());
}

#[derive(Debug, Default)]
pub struct ScanResult {
    pub sessions: Vec<Session>,
    pub unreadable_files: usize,
}

// Keep filesystem work and the history cache on one worker, away from rendering.
pub struct HistoryScanner {
    requests: std::sync::mpsc::SyncSender<(PathBuf, HashMap<String, PathBuf>)>,
    results: std::sync::mpsc::Receiver<io::Result<ScanResult>>,
    pending: bool,
}

impl HistoryScanner {
    pub fn new() -> Self {
        Self::with_scan(scan_history)
    }

    fn with_scan(
        scan: impl Fn(&Path, &HashMap<String, PathBuf>) -> io::Result<ScanResult> + Send + 'static,
    ) -> Self {
        let (requests, receiver) =
            std::sync::mpsc::sync_channel::<(PathBuf, HashMap<String, PathBuf>)>(1);
        let (sender, results) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while let Ok((root, hints)) = receiver.recv() {
                if sender.send(scan(&root, &hints)).is_err() {
                    break;
                }
            }
        });
        Self {
            requests,
            results,
            pending: false,
        }
    }

    // Only before the event loop starts: populate the sidebar using this worker's cache.
    pub fn initial_scan(
        &mut self,
        root: PathBuf,
        hints: HashMap<String, PathBuf>,
    ) -> io::Result<ScanResult> {
        self.request(root, hints);
        let result = self.results.recv().map_err(io::Error::other)?;
        self.pending = false;
        result
    }

    pub fn request(&mut self, root: PathBuf, hints: HashMap<String, PathBuf>) {
        if !self.pending && self.requests.try_send((root, hints)).is_ok() {
            self.pending = true;
        }
    }

    pub fn take_result(&mut self) -> Option<io::Result<ScanResult>> {
        let result = self.results.try_recv().ok()?;
        self.pending = false;
        Some(result)
    }
}

pub fn default_history_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude/projects"))
}

pub fn scan_history(
    root: &Path,
    project_hints: &HashMap<String, PathBuf>,
) -> io::Result<ScanResult> {
    if !root.exists() {
        return Ok(ScanResult::default());
    }

    let mut result = ScanResult::default();
    scan_directory(root, &mut result, project_hints);
    let mut unique_sessions = HashMap::new();
    for session in result.sessions.drain(..) {
        match unique_sessions.entry(session.id.clone()) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(session);
            }
            std::collections::hash_map::Entry::Occupied(mut entry)
                if session_is_newer(&session, entry.get()) =>
            {
                entry.insert(session);
            }
            std::collections::hash_map::Entry::Occupied(_) => {}
        }
    }
    result.sessions = unique_sessions.into_values().collect();
    result.sessions.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| left.project_path.cmp(&right.project_path))
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(result)
}

fn session_is_newer(candidate: &Session, current: &Session) -> bool {
    candidate.updated_at > current.updated_at
        || (candidate.updated_at == current.updated_at
            && (candidate.project_path.as_os_str(), candidate.id.as_str())
                < (current.project_path.as_os_str(), current.id.as_str()))
}

fn scan_directory(
    directory: &Path,
    result: &mut ScanResult,
    project_hints: &HashMap<String, PathBuf>,
) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_) => {
            result.unreadable_files += 1;
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                result.unreadable_files += 1;
                continue;
            }
        };

        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => {
                result.unreadable_files += 1;
                continue;
            }
        };

        if file_type.is_dir() {
            if entry.file_name() != "subagents" {
                scan_directory(&path, result, project_hints);
            }
            continue;
        }

        if !file_type.is_file() || path.extension().is_none_or(|ext| ext != "jsonl") {
            continue;
        }

        let hint = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|id| project_hints.get(id))
            .map(PathBuf::as_path);
        match read_session(&path, hint) {
            Ok(Some(session)) => result.sessions.push(session),
            Ok(None) => {}
            Err(_) => result.unreadable_files += 1,
        }
    }
}

fn read_session(path: &Path, project_hint: Option<&Path>) -> io::Result<Option<Session>> {
    let file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    let updated_at = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    if let Some(cached) = HISTORY_CACHE.with(|cache| {
        cache
            .borrow()
            .get(path)
            .filter(|(size, modified, hint, _)| {
                *size == metadata.len()
                    && *modified == updated_at
                    && hint.as_deref() == project_hint
            })
            .map(|(_, _, _, session)| session.clone())
    }) {
        return Ok(cached);
    }
    let id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_owned();
    let mut project_path = None;
    let mut title = None;
    let mut custom_title = None;
    let mut git_branch = None;
    // Claude can write only /rename metadata before a first prompt, or place cwd
    // after large startup records. Read metadata throughout the file, not a header.
    for line in io::BufReader::new(file).lines() {
        let line = line?;
        if !line.contains("\"customTitle\"")
            && !line.contains("\"aiTitle\"")
            && !(project_path.is_none() && line.contains("\"cwd\""))
            && !(title.is_none() && line.contains("\"slug\""))
            && !(git_branch.is_none() && line.contains("\"gitBranch\""))
        {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if record
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|record_id| record_id != id)
        {
            continue;
        }
        let field = |key: &str| {
            record
                .get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
        };
        if project_path.is_none() {
            project_path = field("cwd").map(PathBuf::from);
        }
        if let Some(value) = field("customTitle") {
            custom_title = Some(value.to_owned());
        }
        if let Some(value) = field("aiTitle") {
            title = Some(value.to_owned());
        } else if title.is_none() {
            title = field("slug").map(str::to_owned);
        }
        if git_branch.is_none() {
            git_branch = field("gitBranch").map(str::to_owned);
        }
    }
    let Some(project_path) = project_path.or_else(|| project_hint.map(Path::to_path_buf)) else {
        return Ok(None);
    };
    let session = Session {
        history_path: Some(path.to_path_buf()),
        title: custom_title.clone().or(title).unwrap_or_else(|| id.clone()),
        id,
        project_path,
        custom_title,
        git_branch,
        updated_at,
    };
    HISTORY_CACHE.with(|cache| {
        cache.borrow_mut().insert(
            path.to_owned(),
            (
                metadata.len(),
                updated_at,
                project_hint.map(Path::to_path_buf),
                Some(session.clone()),
            ),
        );
    });
    Ok(Some(session))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn background_scan_does_not_block_or_queue_duplicate_work() {
        let (started, ready) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let mut scanner = HistoryScanner::with_scan(move |_, _| {
            started.send(()).unwrap();
            blocked.recv().unwrap();
            Ok(ScanResult::default())
        });
        scanner.request(PathBuf::from("/synthetic"), HashMap::new());
        ready
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        scanner.request(PathBuf::from("/duplicate"), HashMap::new());
        assert!(scanner.take_result().is_none());
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Some(result) = scanner.take_result() {
                assert!(result.unwrap().sessions.is_empty());
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(ready.try_recv().is_err());
    }

    #[test]
    fn title_only_history_uses_known_project_and_cache_checks_hint() {
        let root = std::env::temp_dir().join(format!("claude-new-title-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("chat.jsonl");
        fs::write(&path, serde_json::json!({"type":"custom-title","sessionId":"chat","customTitle":"Named before prompt"}).to_string()).unwrap();
        assert!(read_session(&path, None).unwrap().is_none());
        let session = read_session(&path, Some(Path::new("/chosen/project")))
            .unwrap()
            .unwrap();
        assert_eq!(session.project_path, Path::new("/chosen/project"));
        assert_eq!(session.title, "Named before prompt");
        assert!(read_session(&path, None).unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cwd_after_startup_records_is_read_and_beats_project_hint() {
        let root = std::env::temp_dir().join(format!("claude-late-cwd-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("chat.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        for _ in 0..40 {
            writeln!(
                file,
                "{}",
                serde_json::json!({"type":"attachment","text":"x".repeat(2048)})
            )
            .unwrap();
        }
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"user","sessionId":"chat","cwd":"/actual/project"})
        )
        .unwrap();
        writeln!(file, "{}", serde_json::json!({"type":"custom-title","sessionId":"chat","customTitle":"Late project"})).unwrap();
        let session = read_session(&path, Some(Path::new("/fallback/project")))
            .unwrap()
            .unwrap();
        assert_eq!(session.project_path, Path::new("/actual/project"));
        assert_eq!(session.title, "Late project");
        drop(file);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_rename_reads_latest_title_after_large_messages_and_invalidates_cache() {
        let root = std::env::temp_dir().join(format!("claude-title-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("chat.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({"sessionId":"chat","cwd":"/project","aiTitle":"Generated"})
        )
        .unwrap();
        for _ in 0..40 {
            writeln!(
                file,
                "{}",
                serde_json::json!({"type":"assistant","message":"x".repeat(2048)})
            )
            .unwrap();
        }
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"custom-title","sessionId":"chat","customTitle":"111"})
        )
        .unwrap();
        assert_eq!(read_session(&path, None).unwrap().unwrap().title, "111");
        assert_eq!(read_session(&path, None).unwrap().unwrap().title, "111");
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"custom-title","sessionId":"chat","customTitle":"1111"})
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"ai-title","sessionId":"chat","aiTitle":"Later generated"})
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"custom-title","sessionId":"other","customTitle":"Wrong"})
        )
        .unwrap();
        write!(file, "{{\"customTitle\":").unwrap();
        let session = read_session(&path, None).unwrap().unwrap();
        assert_eq!(session.title, "1111");
        assert_eq!(session.custom_title.as_deref(), Some("1111"));
        drop(file);
        fs::remove_dir_all(root).unwrap();
    }
}

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, Read, Seek};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MAX_HEADER_BYTES: u64 = 64 * 1024;
const MAX_HEADER_LINES: usize = 32;

#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub project_path: PathBuf,
    pub title: String,
    pub custom_title: Option<String>,
    pub git_branch: Option<String>,
    pub updated_at: SystemTime,
}

type CachedSession = (u64, SystemTime, Option<Session>);
thread_local! {
    static HISTORY_CACHE: RefCell<HashMap<PathBuf, CachedSession>> = RefCell::new(HashMap::new());
}

#[derive(Debug, Default)]
pub struct ScanResult {
    pub sessions: Vec<Session>,
    pub unreadable_files: usize,
}

pub fn default_history_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude/projects"))
}

pub fn scan_history(root: &Path) -> io::Result<ScanResult> {
    if !root.exists() {
        return Ok(ScanResult::default());
    }

    let mut result = ScanResult::default();
    scan_directory(root, &mut result);
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

fn scan_directory(directory: &Path, result: &mut ScanResult) {
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
                scan_directory(&path, result);
            }
            continue;
        }

        if !file_type.is_file() || path.extension().is_none_or(|ext| ext != "jsonl") {
            continue;
        }

        match read_session(&path) {
            Ok(Some(session)) => result.sessions.push(session),
            Ok(None) => {}
            Err(_) => result.unreadable_files += 1,
        }
    }
}

fn read_session(path: &Path) -> io::Result<Option<Session>> {
    let mut file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    let updated_at = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    if let Some(cached) = HISTORY_CACHE.with(|cache| {
        cache
            .borrow()
            .get(path)
            .filter(|(size, modified, _)| *size == metadata.len() && *modified == updated_at)
            .map(|(_, _, session)| session.clone())
    }) {
        return Ok(cached);
    }
    let limited = (&mut file).take(MAX_HEADER_BYTES);
    let reader = io::BufReader::new(limited);
    let mut id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_owned();
    let mut project_path = None;
    let mut title = None;
    let mut git_branch = None;

    for line in reader.lines().take(MAX_HEADER_LINES) {
        let line = line?;
        let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };

        if let Some(value) = record.get("sessionId").and_then(serde_json::Value::as_str)
            && !value.is_empty()
        {
            id = value.to_owned();
        }
        if project_path.is_none() {
            project_path = record
                .get("cwd")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from);
        }
        if title.is_none() {
            title = record
                .get("aiTitle")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    record
                        .get("slug")
                        .and_then(serde_json::Value::as_str)
                        .filter(|value| !value.is_empty())
                })
                .map(str::to_owned);
        }
        if git_branch.is_none() {
            git_branch = record
                .get("gitBranch")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
        }
    }

    let Some(project_path) = project_path else {
        return Ok(None);
    };
    // /rename appends metadata anywhere in the transcript. The last title wins.
    file.rewind()?;
    let mut custom_title = None;
    for line in io::BufReader::new(file).lines() {
        let line = line?;
        if !line.contains("\"customTitle\"") && !line.contains("\"aiTitle\"") {
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
        if let Some(value) = record
            .get("customTitle")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
        {
            custom_title = Some(value.to_owned());
        }
        if let Some(value) = record
            .get("aiTitle")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
        {
            title = Some(value.to_owned());
        }
    }
    let fallback_title = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("chat");

    let session = Session {
        id,
        project_path,
        title: custom_title
            .clone()
            .or(title)
            .unwrap_or_else(|| fallback_title.to_owned()),
        custom_title,
        git_branch,
        updated_at,
    };
    HISTORY_CACHE.with(|cache| {
        cache.borrow_mut().insert(
            path.to_owned(),
            (metadata.len(), updated_at, Some(session.clone())),
        );
    });
    Ok(Some(session))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
        assert_eq!(read_session(&path).unwrap().unwrap().title, "111");
        assert_eq!(read_session(&path).unwrap().unwrap().title, "111");
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
        let session = read_session(&path).unwrap().unwrap();
        assert_eq!(session.title, "1111");
        assert_eq!(session.custom_title.as_deref(), Some("1111"));
        drop(file);
        fs::remove_dir_all(root).unwrap();
    }
}

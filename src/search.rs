use crate::workspace::{Entry, Target};
use std::io::{self, BufRead, Read};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};

#[derive(Clone)]
pub struct Source {
    pub id: String,
    pub project: PathBuf,
    pub title: String,
    pub path: PathBuf,
    pub note: bool,
}
pub struct SearchResult {
    pub generation: u64,
    pub entries: Vec<Entry>,
    pub skipped: usize,
    pub limited: bool,
}
pub struct SearchWorker {
    sender: mpsc::SyncSender<(u64, String, Vec<Source>)>,
    receiver: mpsc::Receiver<SearchResult>,
    generation: Arc<AtomicU64>,
}
impl SearchWorker {
    pub fn new() -> Self {
        let (sender, requests) = mpsc::sync_channel::<(u64, String, Vec<Source>)>(1);
        let (results, receiver) = mpsc::sync_channel(1);
        let generation = Arc::new(AtomicU64::new(0));
        let revision = generation.clone();
        std::thread::spawn(move || {
            while let Ok((id, query, sources)) = requests.recv() {
                let result = search(id, &query, &sources, &revision);
                if revision.load(Ordering::Relaxed) == id && results.send(result).is_err() {
                    break;
                }
            }
        });
        Self {
            sender,
            receiver,
            generation,
        }
    }
    pub fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
    pub fn request(&self, query: String, sources: Vec<Source>) -> Option<u64> {
        let id = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.sender.try_send((id, query, sources)).ok().map(|_| id)
    }
    pub fn result(&self) -> Option<SearchResult> {
        let result = self.receiver.try_recv().ok()?;
        (result.generation == self.generation.load(Ordering::Relaxed)).then_some(result)
    }
}

pub fn sources(
    sessions: &[crate::sessions::Session],
    titles: impl Fn(&str) -> String,
    notes: Vec<(String, PathBuf)>,
) -> Vec<Source> {
    let mut sources = Vec::new();
    for session in sessions {
        if let Some(path) = &session.history_path {
            sources.push(Source {
                id: session.id.clone(),
                project: session.project_path.clone(),
                title: titles(&session.id),
                path: path.clone(),
                note: false,
            });
        }
    }
    for (id, path) in notes {
        let project = sessions
            .iter()
            .find(|session| session.id == id)
            .map(|session| session.project_path.clone())
            .unwrap_or_else(|| path.parent().unwrap_or(&path).to_path_buf());
        sources.push(Source {
            title: titles(&id),
            id,
            project,
            path,
            note: true,
        });
    }
    sources
}
fn searchable(value: &serde_json::Value) -> Option<String> {
    if !matches!(value["type"].as_str(), Some("user" | "assistant"))
        || value["isSidechain"].as_bool() == Some(true)
    {
        return None;
    }
    let content = &value["message"]["content"];
    if let Some(text) = content.as_str() {
        return Some(text.to_owned());
    }
    let text = content
        .as_array()?
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}
fn entry(source: &Source, text: &str, query: &str, line: usize) -> Option<Entry> {
    let lower = text.to_lowercase();
    let offset = lower.find(query)?;
    // Case folding may change byte lengths. Map via character position, never slice the original at a folded byte offset.
    let mut folded_bytes = 0;
    let position = text
        .chars()
        .take_while(|character| {
            let before = folded_bytes;
            folded_bytes += character.to_lowercase().map(char::len_utf8).sum::<usize>();
            before < offset
        })
        .count();
    let preview_start = position.saturating_sub(2000);
    let preview: String = text.chars().skip(preview_start).take(64000).collect();
    let preview_line = text
        .chars()
        .skip(preview_start)
        .take(position - preview_start)
        .filter(|character| *character == '\n')
        .count()
        + 1;
    let snippet: String = text
        .chars()
        .skip(position.saturating_sub(35))
        .take(150)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    Some(Entry {
        label: format!(
            "{} · {}:{} · {}",
            source.title,
            if source.note { "✎" } else { "чат" },
            line,
            snippet
        ),
        target: Target::Search {
            id: source.id.clone(),
            project: source.project.clone(),
            text: preview,
            line: if source.note { line } else { preview_line },
            note: source.note.then(|| source.path.clone()),
        },
    })
}
fn search(generation: u64, query: &str, sources: &[Source], revision: &AtomicU64) -> SearchResult {
    let mut result = SearchResult {
        generation,
        entries: Vec::new(),
        skipped: 0,
        limited: false,
    };
    let query = query.to_lowercase();
    if query.trim().is_empty() {
        return result;
    }
    let started = std::time::Instant::now();
    for source in sources {
        if revision.load(Ordering::Relaxed) != generation {
            break;
        }
        if started.elapsed() > std::time::Duration::from_secs(5) || result.entries.len() >= 200 {
            result.limited = true;
            break;
        }
        let path = &source.path;
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            continue;
        };
        if !meta.is_file() || meta.len() > 32 * 1024 * 1024 {
            result.skipped += 1;
            continue;
        }
        let Ok(file) = std::fs::File::open(path) else {
            result.skipped += 1;
            continue;
        };
        let mut reader = io::BufReader::new(file.take(32 * 1024 * 1024));
        let mut line = Vec::new();
        let mut index = 0;
        loop {
            line.clear();
            // Bound even malformed JSONL records, without allocating a whole giant line.
            let size = reader
                .by_ref()
                .take(1024 * 1024 + 1)
                .read_until(b'\n', &mut line);
            if !size.is_ok_and(|size| size > 0) {
                break;
            }
            index += 1;
            if line.len() > 1024 * 1024 {
                result.skipped += 1;
                break;
            }
            let text = if source.note {
                Some(String::from_utf8_lossy(&line).into_owned())
            } else {
                serde_json::from_slice::<serde_json::Value>(&line)
                    .ok()
                    .and_then(|value| searchable(&value))
            };
            if let Some(text) = text
                && let Some(entry) = entry(source, &text, &query, index)
            {
                result.entries.push(entry);
            }
            if result.entries.len() >= 200 || started.elapsed() > std::time::Duration::from_secs(5)
            {
                result.limited = true;
                break;
            }
            if revision.load(Ordering::Relaxed) != generation {
                break;
            }
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_uses_the_same_duplicate_transcript_as_sidebar() {
        let root =
            std::env::temp_dir().join(format!("cct-duplicate-search-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("old")).unwrap();
        std::fs::create_dir_all(root.join("new")).unwrap();
        let old = root.join("old/id.jsonl");
        let new = root.join("new/id.jsonl");
        for (path, title, timestamp) in [(&old, "old transcript", 1), (&new, "new transcript", 2)] {
            let metadata =
                serde_json::json!({"sessionId":"id","cwd":root.to_string_lossy(),"aiTitle":title});
            let message = serde_json::json!({"type":"assistant","message":{"content":title}});
            std::fs::write(path, format!("{metadata}\n{message}\n")).unwrap();
            std::fs::File::open(path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(
                    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(timestamp),
                ))
                .unwrap();
        }
        let scanned =
            crate::sessions::scan_history(&root, &std::collections::HashMap::new()).unwrap();
        assert_eq!(scanned.sessions.len(), 1);
        assert_eq!(scanned.sessions[0].history_path.as_ref(), Some(&new));
        let sources = sources(&scanned.sessions, |_| "Current title".into(), Vec::new());
        assert!(
            search(1, "old transcript", &sources, &AtomicU64::new(1))
                .entries
                .is_empty()
        );
        assert_eq!(
            search(1, "new transcript", &sources, &AtomicU64::new(1))
                .entries
                .len(),
            1
        );
        std::fs::remove_file(old).unwrap();
        std::fs::remove_file(new).unwrap();
        std::fs::remove_dir(root.join("old")).unwrap();
        std::fs::remove_dir(root.join("new")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn worker_rejects_old_results_after_new_query_or_cancel() {
        let (sender, requests) = mpsc::sync_channel(1);
        let (results, receiver) = mpsc::sync_channel(1);
        let worker = SearchWorker {
            sender,
            receiver,
            generation: Arc::new(AtomicU64::new(0)),
        };
        let first = worker.request("old".into(), Vec::new()).unwrap();
        requests.recv().unwrap();
        let second = worker.request("new".into(), Vec::new()).unwrap();
        requests.recv().unwrap();
        let result = |generation| SearchResult {
            generation,
            entries: Vec::new(),
            skipped: 0,
            limited: false,
        };
        results.send(result(first)).unwrap();
        assert!(worker.result().is_none());
        results.send(result(second)).unwrap();
        assert_eq!(worker.result().unwrap().generation, second);
        worker.cancel();
        results.send(result(second)).unwrap();
        assert!(worker.result().is_none());
    }
    #[test]
    fn bounded_saved_history_and_markdown_search() {
        let root = std::env::temp_dir().join(format!("cct-search-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("history/project")).unwrap();
        let transcript = root.join("history/project/id.jsonl");
        std::fs::write(&transcript,concat!(r#"{"type":"assistant","message":{"content":"needle reply"}}"#,"\n",r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"hidden needle"}]}}"#,"\n")).unwrap();
        let note = root.join("id.md");
        std::fs::write(&note, "# Notes\nneedle note\n").unwrap();
        let sources = vec![
            Source {
                id: "id".into(),
                project: root.clone(),
                title: "Chat".into(),
                path: transcript.clone(),
                note: false,
            },
            Source {
                id: "id".into(),
                project: root.clone(),
                title: "Chat".into(),
                path: note.clone(),
                note: true,
            },
        ];
        let revision = AtomicU64::new(1);
        let result = search(1, "needle", &sources, &revision);
        assert_eq!(result.entries.len(), 2);
        assert!(matches!(
            result.entries[1].target,
            Target::Search { line: 2, .. }
        ));
        assert!(search(0, "needle", &sources, &revision).entries.is_empty());
        std::fs::remove_file(transcript).unwrap();
        std::fs::remove_file(note).unwrap();
        std::fs::remove_dir(root.join("history/project")).unwrap();
        std::fs::remove_dir(root.join("history")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn messages_exclude_tools_and_sidechains() {
        let value = serde_json::json!({"type":"assistant","message":{"content":[{"type":"text","text":"Ответ"},{"type":"tool_use","input":{"secret":"needle"}}]}});
        assert_eq!(searchable(&value), Some("Ответ".into()));
        assert!(searchable(&serde_json::json!({"type":"user","isSidechain":true,"message":{"content":"secret"}})).is_none());
    }
    #[test]
    fn snippets_unicode_and_limits() {
        let source = Source {
            id: "id".into(),
            title: "Title".into(),
            project: PathBuf::new(),
            path: PathBuf::new(),
            note: false,
        };
        assert!(
            entry(&source, "Привет İ мир", "мир", 1)
                .unwrap()
                .label
                .contains("Привет")
        );
        assert!(entry(&source, "abc", "z", 1).is_none());
    }
}

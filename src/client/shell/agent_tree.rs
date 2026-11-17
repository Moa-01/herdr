//! fork: Claude subagents and forked sessions for the agent panel.
//!
//! Claude Code's hook reports each session's transcript path as the `fork_transcript`
//! pane token. Beside the transcript, `<session>/subagents/*.meta.json` describes every
//! subagent the session started. A forked session's transcript starts with lines copied
//! from its parent, and only copied lines carry `requestSessionId` (the session they came
//! from). A worker thread reads both; render only reads the result.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant, SystemTime};

use super::*;

const TRANSCRIPT_TOKEN: &str = "fork_transcript";
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
/// A subagent without `finished: true` counts as running only while its transcript moves;
/// one killed with its session never gets the flag.
const STALE_AFTER: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Subagent {
    pub(super) kind: String,
    pub(super) description: String,
    /// 1 for a subagent of the session, 2 for one started by a subagent, and so on.
    pub(super) depth: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Scan {
    /// Running subagents per pane, oldest first.
    subagents: HashMap<String, Vec<Subagent>>,
    /// Pane of the session each forked session was copied from.
    fork_parent: HashMap<String, String>,
}

/// How far a transcript has been read, and the last session lines were copied from.
#[derive(Clone, Debug, Default)]
struct Origin {
    offset: u64,
    source: Option<String>,
}

type JobResult = (Scan, HashMap<PathBuf, Origin>);

#[derive(Default)]
pub(crate) struct AgentTree {
    scan: Scan,
    origins: HashMap<PathBuf, Origin>,
    job: Option<Receiver<JobResult>>,
    started_at: Option<Instant>,
}

impl AgentTree {
    pub(super) fn subagents(&self, pane_id: &str) -> &[Subagent] {
        self.scan.subagents.get(pane_id).map_or(&[], Vec::as_slice)
    }

    pub(super) fn fork_parent(&self, pane_id: &str) -> Option<&str> {
        self.scan.fork_parent.get(pane_id).map(String::as_str)
    }

    /// Collects a finished scan and starts the next one every `REFRESH_INTERVAL`.
    /// Returns whether the panel needs a repaint.
    fn tick(&mut self, snapshot: Option<&ClientShellSnapshot>) -> bool {
        let mut changed = false;
        if let Some(receiver) = &self.job {
            match receiver.try_recv() {
                Ok((scan, origins)) => {
                    changed = scan != self.scan;
                    self.scan = scan;
                    self.origins = origins;
                    self.job = None;
                }
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => self.job = None,
            }
        }
        if self
            .started_at
            .is_some_and(|started| started.elapsed() < REFRESH_INTERVAL)
        {
            return changed;
        }
        let panes = snapshot.map(transcripts).unwrap_or_default();
        if panes.is_empty() {
            changed |= self.scan != Scan::default();
            self.scan = Scan::default();
            return changed;
        }
        self.started_at = Some(Instant::now());
        let origins = self.origins.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(scan(&panes, origins));
        });
        self.job = Some(receiver);
        changed
    }
}

#[cfg(test)]
impl AgentTree {
    pub(super) fn with(
        subagents: Vec<(&str, Vec<Subagent>)>,
        fork_parent: Vec<(&str, &str)>,
    ) -> Self {
        Self {
            scan: Scan {
                subagents: subagents
                    .into_iter()
                    .map(|(pane, list)| (pane.to_string(), list))
                    .collect(),
                fork_parent: fork_parent
                    .into_iter()
                    .map(|(pane, parent)| (pane.to_string(), parent.to_string()))
                    .collect(),
            },
            ..Self::default()
        }
    }
}

impl ClientShellState {
    pub(crate) fn tick_agent_tree(&mut self) -> bool {
        // Transcript paths are local files; another machine's panes are not readable here.
        let snapshot = self
            .snapshot
            .as_deref()
            .filter(|_| self.active_endpoint_id.is_local());
        self.agent_tree.tick(snapshot)
    }
}

fn transcripts(snapshot: &ClientShellSnapshot) -> Vec<(String, PathBuf)> {
    snapshot
        .agents
        .iter()
        .filter_map(|agent| {
            let (_, path) = agent
                .tokens
                .iter()
                .find(|(name, _)| name == TRANSCRIPT_TOKEN)?;
            Some((agent.pane_id.clone(), PathBuf::from(path)))
        })
        .collect()
}

fn scan(panes: &[(String, PathBuf)], mut origins: HashMap<PathBuf, Origin>) -> JobResult {
    origins.retain(|path, _| panes.iter().any(|(_, transcript)| transcript == path));
    let session_pane = panes
        .iter()
        .filter_map(|(pane, path)| Some((session_id(path)?, pane.clone())))
        .collect::<HashMap<_, _>>();
    let mut result = Scan::default();
    for (pane, transcript) in panes {
        let running = running_subagents(&transcript.with_extension("").join("subagents"));
        if !running.is_empty() {
            result.subagents.insert(pane.clone(), running);
        }
        let origin = origins.entry(transcript.clone()).or_default();
        read_copied_from(transcript, origin);
        if let Some(parent) = origin
            .source
            .as_ref()
            .and_then(|source| session_pane.get(source))
            .filter(|parent| *parent != pane)
        {
            result.fork_parent.insert(pane.clone(), parent.clone());
        }
    }
    (result, origins)
}

fn session_id(transcript: &Path) -> Option<String> {
    Some(transcript.file_stem()?.to_string_lossy().into_owned())
}

fn running_subagents(dir: &Path) -> Vec<Subagent> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let now = SystemTime::now();
    let mut running = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(".meta.json")?;
            let meta: serde_json::Value =
                serde_json::from_slice(&std::fs::read(entry.path()).ok()?).ok()?;
            if meta.get("finished").and_then(serde_json::Value::as_bool) == Some(true) {
                return None;
            }
            let modified = std::fs::metadata(dir.join(format!("{stem}.jsonl")))
                .and_then(|metadata| metadata.modified())
                .ok()?;
            if now.duration_since(modified).unwrap_or_default() > STALE_AFTER {
                return None;
            }
            let text = |key: &str| {
                meta.get(key)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let started = entry.metadata().and_then(|meta| meta.modified()).ok();
            Some((
                started,
                Subagent {
                    kind: text("agentType"),
                    description: text("description"),
                    depth: meta
                        .get("spawnDepth")
                        .and_then(serde_json::Value::as_u64)
                        .map_or(1, |depth| depth.max(1) as usize),
                },
            ))
        })
        .collect::<Vec<_>>();
    running.sort_by_key(|(started, _)| *started);
    running.into_iter().map(|(_, subagent)| subagent).collect()
}

/// Reads the transcript lines written since the last call and remembers the session the
/// latest copied line came from. Only complete lines count, so a half-written one is read
/// again next time.
fn read_copied_from(transcript: &Path, origin: &mut Origin) {
    const KEY: &str = "\"requestSessionId\":\"";
    let Ok(mut file) = std::fs::File::open(transcript) else {
        return;
    };
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(origin.offset)).is_err() || file.read_to_end(&mut bytes).is_err() {
        return;
    }
    let Some(end) = bytes.iter().rposition(|byte| *byte == b'\n') else {
        return;
    };
    let complete = String::from_utf8_lossy(&bytes[..=end]);
    for line in complete.lines() {
        let Some(start) = line.find(KEY).map(|index| index + KEY.len()) else {
            continue;
        };
        if let Some(len) = line[start..].find('"') {
            origin.source = Some(line[start..start + len].to_string());
        }
    }
    origin.offset += end as u64 + 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("herdr-agent-tree-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn forks_find_their_parent_and_subagents_show_while_running() {
        let dir = temp_dir("scan");
        let parent = dir.join("parent-session.jsonl");
        let fork = dir.join("fork-session.jsonl");
        std::fs::write(&parent, "{\"type\":\"assistant\"}\n").unwrap();
        // Copied from an older session, then from the parent, then its own lines.
        std::fs::write(
            &fork,
            "{\"requestSessionId\":\"root\"}\n{\"requestSessionId\":\"parent-session\"}\n{\"type\":\"assistant\"}\n",
        )
        .unwrap();
        let subagents = dir.join("parent-session").join("subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        std::fs::write(
            subagents.join("agent-a.meta.json"),
            r#"{"agentType":"Explore","description":"find the hook","spawnDepth":1}"#,
        )
        .unwrap();
        std::fs::write(subagents.join("agent-a.jsonl"), "{}\n").unwrap();
        std::fs::write(
            subagents.join("agent-b.meta.json"),
            r#"{"agentType":"Explore","description":"done already","finished":true}"#,
        )
        .unwrap();
        std::fs::write(subagents.join("agent-b.jsonl"), "{}\n").unwrap();

        let panes = vec![
            ("p1".to_string(), parent.clone()),
            ("p2".to_string(), fork.clone()),
        ];
        let (result, origins) = scan(&panes, HashMap::new());
        assert_eq!(result.fork_parent.get("p2").map(String::as_str), Some("p1"));
        assert!(!result.fork_parent.contains_key("p1"));
        assert_eq!(
            result.subagents.get("p1"),
            Some(&vec![Subagent {
                kind: "Explore".into(),
                description: "find the hook".into(),
                depth: 1,
            }])
        );

        // Later scans read only new lines and keep what they learned.
        let (again, _) = scan(&panes, origins);
        assert_eq!(again.fork_parent, result.fork_parent);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_half_written_line_is_read_again() {
        let dir = temp_dir("partial");
        let transcript = dir.join("s.jsonl");
        std::fs::write(&transcript, "{\"requestSessionId\":\"a\"}\n{\"requestSess").unwrap();
        let mut origin = Origin::default();
        read_copied_from(&transcript, &mut origin);
        assert_eq!(origin.source.as_deref(), Some("a"));
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript)
            .unwrap();
        std::io::Write::write_all(&mut file, b"ionId\":\"b\"}\n").unwrap();
        read_copied_from(&transcript, &mut origin);
        assert_eq!(origin.source.as_deref(), Some("b"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

//! fork: file tree for the focused pane's directory, below the agent panel.
//!
//! TUI-only presentation state. Listings come from the local filesystem outside render
//! (snapshot arrival and clicks) and render only reads the cache.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::*;

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const MAX_DIR_ENTRIES: usize = 500;
/// A file opens beside the focused pane while both halves keep this many columns;
/// narrower than that it gets a tab of its own.
const MIN_SPLIT_COLUMNS: u16 = 60;
/// Divider, title and a blank spacer, matching the agent panel header.
const FILE_PANEL_HEADER_ROWS: u16 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileEntry {
    pub(super) name: String,
    pub(super) path: PathBuf,
    pub(super) is_dir: bool,
}

#[derive(Default)]
pub(crate) struct FileTree {
    pub(super) root: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    listings: HashMap<PathBuf, Vec<FileEntry>>,
    /// Git state per changed path under `root`, plus each ancestor folder (strongest wins).
    git: HashMap<PathBuf, GitMark>,
    /// Git runs on a worker thread; its marks land here for the root they were made for.
    git_job: Option<(
        PathBuf,
        std::sync::mpsc::Receiver<HashMap<PathBuf, GitMark>>,
    )>,
    loaded_at: Option<Instant>,
    /// Show the space's directory (the pane's own cwd) even when an agent in the
    /// focused pane reports working somewhere else.
    pub(super) follow_space: bool,
}

/// Where a changed file sits on its way to the remote; ordered weakest to strongest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum GitMark {
    /// Committed but not on the upstream branch yet.
    Unpushed,
    /// Staged, waiting for a commit.
    Staged,
    /// New file git does not track yet.
    Untracked,
    /// Changed in the working tree, not staged.
    Modified,
    /// Merge conflict.
    Conflict,
}

impl GitMark {
    fn letter(self) -> &'static str {
        match self {
            Self::Unpushed => "↑",
            Self::Staged => "S",
            Self::Untracked => "U",
            Self::Modified => "M",
            Self::Conflict => "!",
        }
    }

    fn color(self, palette: &crate::app::state::Palette) -> ratatui::style::Color {
        match self {
            Self::Unpushed => palette.blue,
            Self::Staged => palette.teal,
            Self::Untracked => palette.green,
            Self::Modified => palette.yellow,
            Self::Conflict => palette.red,
        }
    }
}

pub(super) struct FileRow<'a> {
    pub(super) entry: &'a FileEntry,
    pub(super) depth: usize,
    pub(super) expanded: bool,
}

impl FileTree {
    /// Follows the focused pane's directory; an unchanged root reloads at most every
    /// `REFRESH_INTERVAL` so created and deleted files show up. Returns whether the root moved.
    pub(super) fn set_root(&mut self, root: Option<PathBuf>) -> bool {
        if self.root == root {
            if self
                .loaded_at
                .is_none_or(|loaded| loaded.elapsed() >= REFRESH_INTERVAL)
            {
                self.reload();
            }
            return false;
        }
        self.root = root;
        self.expanded.clear();
        self.git.clear();
        self.reload();
        true
    }

    pub(super) fn reload(&mut self) {
        self.listings.clear();
        self.loaded_at = Some(Instant::now());
        let Some(root) = self.root.clone() else {
            return;
        };
        self.expanded
            .retain(|dir| dir.starts_with(&root) && dir.is_dir());
        let dirs = std::iter::once(root.clone())
            .chain(self.expanded.iter().cloned())
            .collect::<Vec<_>>();
        for dir in dirs {
            let entries = read_dir_sorted(&dir);
            self.listings.insert(dir, entries);
        }
        self.start_git_job(root);
    }

    /// Every repo the tree shows: the one holding the root, plus listed folders that are
    /// repos themselves (a root like `~/projects` holds several).
    fn start_git_job(&mut self, root: PathBuf) {
        if self.git_job.is_some() {
            return;
        }
        let nested = self
            .listings
            .values()
            .flatten()
            .filter(|entry| entry.is_dir && entry.path.join(".git").exists())
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        let (sender, receiver) = std::sync::mpsc::channel();
        let job_root = root.clone();
        std::thread::spawn(move || {
            let _ = sender.send(git_marks(&job_root, &nested));
        });
        self.git_job = Some((root, receiver));
    }

    /// Picks up finished git marks and refreshes on schedule even when no snapshot
    /// arrives. Returns whether the panel needs a repaint.
    pub(super) fn tick(&mut self) -> bool {
        let mut changed = false;
        if let Some((root, receiver)) = &self.git_job {
            match receiver.try_recv() {
                Ok(marks) => {
                    if Some(root) == self.root.as_ref() && marks != self.git {
                        self.git = marks;
                        changed = true;
                    }
                    self.git_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.git_job = None,
            }
        }
        if self.root.is_some()
            && self
                .loaded_at
                .is_some_and(|loaded| loaded.elapsed() >= REFRESH_INTERVAL)
        {
            self.reload();
            changed = true;
        }
        changed
    }

    pub(super) fn toggle(&mut self, dir: &Path) {
        if !self.expanded.remove(dir) {
            self.expanded.insert(dir.to_path_buf());
            self.listings
                .insert(dir.to_path_buf(), read_dir_sorted(dir));
        }
    }

    pub(super) fn rows(&self) -> Vec<FileRow<'_>> {
        let mut rows = Vec::new();
        if let Some(root) = &self.root {
            self.push_rows(root, 0, &mut rows);
        }
        rows
    }

    fn push_rows<'a>(&'a self, dir: &Path, depth: usize, rows: &mut Vec<FileRow<'a>>) {
        for entry in self.listings.get(dir).into_iter().flatten() {
            let expanded = entry.is_dir && self.expanded.contains(&entry.path);
            rows.push(FileRow {
                entry,
                depth,
                expanded,
            });
            if expanded {
                self.push_rows(&entry.path, depth + 1, rows);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn with_listings(root: PathBuf, listings: Vec<(PathBuf, Vec<FileEntry>)>) -> Self {
        Self {
            root: Some(root),
            listings: listings.into_iter().collect(),
            git: HashMap::new(),
            loaded_at: Some(Instant::now()),
            ..Self::default()
        }
    }
}

fn read_dir_sorted(dir: &Path) -> Vec<FileEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    // ponytail: huge directories show an arbitrary first MAX_DIR_ENTRIES, sorted.
    let mut entries = read
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Hide only .git, like VS Code's default files.exclude.
            if name == ".git" {
                return None;
            }
            let path = entry.path();
            // Follow symlinks so linked directories still expand.
            let is_dir = path.is_dir();
            Some(FileEntry { name, path, is_dir })
        })
        .take(MAX_DIR_ENTRIES)
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    entries
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("--no-optional-locks")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Marks for the repo holding `root` and for each repo in `nested`, keyed by tree path.
fn git_marks(root: &Path, nested: &[PathBuf]) -> HashMap<PathBuf, GitMark> {
    let mut marks = HashMap::new();
    // The prefix finds the repo top in the tree's own spelling of the path, even when
    // `root` was reached through a symlink.
    if let Some(prefix) = git(root, &["rev-parse", "--show-prefix"]) {
        let depth = Path::new(prefix.trim_end()).components().count();
        if let Some(top) = root.ancestors().nth(depth) {
            add_repo_marks(top, root, &mut marks);
        }
    }
    for repo in nested {
        add_repo_marks(repo, root, &mut marks);
    }
    marks
}

fn add_repo_marks(top: &Path, root: &Path, marks: &mut HashMap<PathBuf, GitMark>) {
    let status = git(
        top,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .unwrap_or_default();
    // No upstream (or no commits) simply means no unpushed marks.
    let unpushed =
        git(top, &["diff", "--name-only", "-z", "@{upstream}...HEAD"]).unwrap_or_default();
    let mut changes = parse_status(&status);
    changes.extend(
        unpushed
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(|path| (path.to_string(), GitMark::Unpushed)),
    );
    for (path, mark) in changes {
        // The file and every folder above it, up to the tree's root.
        let mut current = top.join(path);
        while current.starts_with(root) && current != root {
            let slot = marks.entry(current.clone()).or_insert(mark);
            *slot = (*slot).max(mark);
            if !current.pop() {
                break;
            }
        }
    }
}

/// `git status --porcelain=v1 -z` entries as (repo-relative path, mark).
fn parse_status(status: &str) -> Vec<(String, GitMark)> {
    let mut fields = status.split('\0').filter(|field| !field.is_empty());
    let mut changes = Vec::new();
    while let Some(field) = fields.next() {
        let (Some(code), Some(path)) = (field.get(..2), field.get(3..)) else {
            continue;
        };
        let (index, worktree) = (code.as_bytes()[0], code.as_bytes()[1]);
        // Renames and copies carry their old path as the next field.
        if matches!(index, b'R' | b'C') {
            fields.next();
        }
        let mark = match (index, worktree) {
            (b'?', b'?') => GitMark::Untracked,
            (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => GitMark::Conflict,
            (_, worktree) if worktree != b' ' => GitMark::Modified,
            _ => GitMark::Staged,
        };
        changes.push((path.to_string(), mark));
    }
    changes
}

/// Pane token an agent hook reports its working directory under. Claude Code's `cd`
/// moves only its own tracking, never the process cwd, so herdr cannot see it otherwise.
const AGENT_CWD_TOKEN: &str = "fork_cwd";

/// The focused pane's directory: where its agent says it works (unless `follow_space`),
/// else where its foreground process runs, else its shell cwd.
pub(super) fn focused_pane_dir(
    snapshot: &ClientShellSnapshot,
    follow_space: bool,
) -> Option<PathBuf> {
    let focused = snapshot.focused_pane_id.as_deref()?;
    let agent_dir = snapshot
        .agents
        .iter()
        .filter(|_| !follow_space)
        .find(|agent| agent.pane_id == focused)
        .and_then(|agent| {
            agent
                .tokens
                .iter()
                .find(|(name, _)| name == AGENT_CWD_TOKEN)
        })
        .map(|(_, dir)| PathBuf::from(dir));
    if agent_dir.is_some() {
        return agent_dir;
    }
    let pane = snapshot.panes.iter().find(|pane| pane.pane_id == focused)?;
    pane.foreground_cwd
        .as_deref()
        .or(pane.cwd.as_deref())
        .map(PathBuf::from)
}

pub(super) fn render_file_panel(
    buffer: &mut Buffer,
    area: Rect,
    tree: &FileTree,
    scroll: &mut usize,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    if area.height < FILE_PANEL_HEADER_ROWS + 1 {
        return;
    }
    let palette = &config.palette;
    render::put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        &"─".repeat(area.width as usize),
        Style::default().fg(palette.surface_dim),
    );
    render::put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        " files",
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    hits.file_header = Rect::new(area.x, area.y + 1, area.width, 1);
    // Which directory the panel follows; click to switch.
    let mode = if tree.follow_space { "space" } else { "claude" };
    let toggle_x = area.x + 7;
    let toggle_end = render::put_segment(
        buffer,
        toggle_x,
        area.y + 1,
        area.right(),
        &format!("⇄ {mode}"),
        Style::default().fg(palette.accent),
    );
    hits.file_mode_toggle = Rect::new(toggle_x, area.y + 1, toggle_end.saturating_sub(toggle_x), 1);
    hits.file_open_dir = Rect::default();
    if let Some(name) = tree
        .root
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy())
    {
        // The folder name plus `↗` is one button: open the folder in the file manager.
        let room = area.width.saturating_sub(19) as usize;
        let label = format!("{} ↗ ", crate::ui::truncate_end(&name, room));
        let width = render::display_width(&label).min(area.width);
        let button = Rect::new(area.right().saturating_sub(width), area.y + 1, width, 1);
        render::put_right_text(
            buffer,
            area,
            area.y + 1,
            &label,
            Style::default().fg(palette.overlay0),
        );
        if let Some(arrow) = buffer.cell_mut((button.right().saturating_sub(2), button.y)) {
            arrow.set_fg(palette.accent);
        }
        hits.file_open_dir = button;
    }

    let body = Rect::new(
        area.x,
        area.y + FILE_PANEL_HEADER_ROWS,
        area.width,
        area.height - FILE_PANEL_HEADER_ROWS,
    );
    hits.file_body = body;
    let rows = tree.rows();
    if rows.is_empty() {
        *scroll = 0;
        render::put_text(
            buffer,
            body.x,
            body.y,
            body.width,
            " empty",
            Style::default()
                .fg(palette.overlay0)
                .add_modifier(Modifier::DIM),
        );
        return;
    }
    let heights = vec![1; rows.len()];
    let gaps = vec![0; rows.len()];
    let metrics = super::scroll::list_scroll_metrics(&heights, &gaps, body.height, *scroll);
    hits.file_max_scroll = metrics.max_offset_from_bottom;
    *scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    for (offset, row) in rows
        .iter()
        .skip(*scroll)
        .take(body.height as usize)
        .enumerate()
    {
        let rect = Rect::new(body.x, body.y + offset as u16, content_width, 1);
        let indent = 1 + row.depth as u16 * 2;
        let x = rect.x.saturating_add(indent);
        let (marker, name_style) = if row.entry.is_dir {
            (
                if row.expanded { "▾ " } else { "▸ " },
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            ("  ", Style::default().fg(palette.subtext0))
        };
        let x = render::put_segment(
            buffer,
            x,
            rect.y,
            rect.right(),
            marker,
            Style::default().fg(palette.overlay0),
        );
        let mark = tree.git.get(&row.entry.path).copied();
        let name_style = match mark {
            Some(mark) => name_style.fg(mark.color(palette)),
            None => name_style,
        };
        // Leave two columns for the status letter at the row's end.
        let name_end = rect
            .right()
            .saturating_sub(if mark.is_some() { 2 } else { 0 });
        let room = name_end.saturating_sub(x) as usize;
        render::put_segment(
            buffer,
            x,
            rect.y,
            name_end,
            &crate::ui::truncate_end(&row.entry.name, room),
            name_style,
        );
        if let Some(mark) = mark {
            // Folders show a dot like VS Code; files show their status letter.
            let label = if row.entry.is_dir {
                "•"
            } else {
                mark.letter()
            };
            render::put_right_text(
                buffer,
                rect,
                rect.y,
                &format!("{label} "),
                Style::default().fg(mark.color(palette)),
            );
        }
        hits.files
            .push((rect, row.entry.path.clone(), row.entry.is_dir));
    }
    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        super::scroll::render_list_scrollbar(buffer, track, metrics, palette);
    }
}

/// Shell line typed into the new pane to open `path` in the user's editor and close
/// the pane when the editor exits. `shell` is `[terminal] default_shell`; empty means
/// the platform default, which is PowerShell on Windows.
pub(super) fn editor_command(path: &Path, shell: &str, windows: bool) -> String {
    let path = path.display().to_string();
    if is_powershell(shell, windows) {
        return powershell_editor_command(&path);
    }
    // Single-quoted arguments read the same in POSIX shells and fish.
    format!(
        "exec sh -c '${{VISUAL:-${{EDITOR:-vi}}}} \"$1\"' sh {}",
        shell_quote(&path)
    )
}

fn is_powershell(shell: &str, windows: bool) -> bool {
    let shell = shell.trim();
    if shell.is_empty() {
        return windows;
    }
    // Last path component first, so `C:\Program Files\...\pwsh.exe` survives its space.
    let tail = shell.rsplit(['/', '\\']).next().unwrap_or(shell);
    let name = tail.split_whitespace().next().unwrap_or("");
    let name = name.trim_matches(['"', '\'']).to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    matches!(name, "pwsh" | "powershell")
}

/// Runs under Windows PowerShell 5.1 and PowerShell 7. Editor: `$VISUAL`, then
/// `$EDITOR`, then Windows' terminal `edit`, then `notepad`. An editor value that is
/// not itself a command (`code --wait`) is split into program and arguments. The pane
/// closes only when the editor succeeds, so a failure stays readable.
fn powershell_editor_command(path: &str) -> String {
    format!(
        "$e = $env:VISUAL, $env:EDITOR, $(if (Get-Command edit -ErrorAction Ignore) {{ 'edit' }} else {{ 'notepad' }}) | Where-Object {{ $_ }} | Select-Object -First 1; \
         $a = @(if (Get-Command $e -ErrorAction Ignore) {{ $e }} else {{ -split $e }}); \
         & $a[0] @($a | Select-Object -Skip 1) '{}'; if ($?) {{ exit }}",
        path.replace('\'', "''")
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

impl ClientShellState {
    /// Called whenever a snapshot lands: follow the focused pane's directory.
    pub(super) fn sync_file_tree(&mut self) {
        let root = self
            .snapshot
            .as_deref()
            .filter(|_| self.active_endpoint_id.is_local())
            .and_then(|snapshot| focused_pane_dir(snapshot, self.file_tree.follow_space))
            .filter(|dir| dir.is_dir());
        if self.file_tree.set_root(root) {
            self.file_scroll = 0;
        }
    }

    pub(crate) fn tick_file_tree(&mut self) -> bool {
        self.file_tree.tick()
    }

    pub(super) fn handle_file_click(
        &mut self,
        point: (u16, u16),
        outcome: &mut ClientShellInput,
    ) -> bool {
        if super::contains(self.hits.file_open_dir, point) {
            if let Some(root) = self.file_tree.root.clone() {
                outcome.actions.push(ClientShellAction::OpenLocalPath(root));
            }
            return true;
        }
        if super::contains(self.hits.file_mode_toggle, point) {
            self.file_tree.follow_space = !self.file_tree.follow_space;
            self.sync_file_tree();
            self.persist_chrome_preferences(outcome);
            outcome.repaint = true;
            return true;
        }
        if super::contains(self.hits.file_header, point) {
            self.file_tree.reload();
            outcome.repaint = true;
            return true;
        }
        let Some((path, is_dir)) = self
            .hits
            .files
            .iter()
            .find(|(rect, _, _)| super::contains(*rect, point))
            .map(|(_, path, is_dir)| (path.clone(), *is_dir))
        else {
            return false;
        };
        if is_dir {
            self.file_tree.toggle(&path);
            outcome.repaint = true;
        } else {
            self.open_file(path, outcome);
        }
        true
    }

    /// Opens `path` in an editor pane beside the focused pane, or in a new tab when a
    /// split would leave either half too narrow to read.
    fn open_file(&mut self, path: PathBuf, outcome: &mut ClientShellInput) {
        use crate::api::schema::{Method, PaneSplitParams, SplitDirection, TabCreateParams};
        let Some(snapshot) = self.snapshot.as_deref() else {
            return;
        };
        let workspace_id = snapshot.focused_workspace_id.clone();
        let focused_pane = snapshot.focused_pane_id.clone();
        let focused_width = focused_pane.as_deref().and_then(|focused| {
            self.hits
                .panes
                .iter()
                .find(|hit| hit.pane_id == focused && !hit.popup)
                .map(|hit| hit.rect.width)
        });
        let cwd = path.parent().map(|parent| parent.display().to_string());
        let method = if focused_width.is_some_and(|width| width >= MIN_SPLIT_COLUMNS * 2) {
            Method::PaneSplit(PaneSplitParams {
                workspace_id,
                target_pane_id: focused_pane,
                direction: SplitDirection::Right,
                ratio: None,
                cwd,
                focus: true,
                right_click: Default::default(),
                env: Default::default(),
            })
        } else {
            Method::TabCreate(TabCreateParams {
                workspace_id,
                cwd,
                focus: true,
                label: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
                env: Default::default(),
            })
        };
        self.push_endpoint_method_with_kind(
            method,
            PendingEndpointKind::OpenFile { path },
            outcome,
        );
    }

    /// Second step of `open_file`: start the editor in the pane the server just made.
    pub(super) fn complete_open_file(
        &mut self,
        path: PathBuf,
        result: Result<crate::api::schema::ResponseResult, ClientShellEndpointError>,
    ) -> (bool, Vec<ClientShellAction>) {
        use crate::api::schema::{Method, PaneSendInputParams, ResponseResult};
        let pane_id = match result {
            Ok(ResponseResult::PaneInfo { pane }) => pane.pane_id,
            Ok(ResponseResult::TabCreated { root_pane, .. }) => root_pane.pane_id,
            _ => return (false, Vec::new()),
        };
        let mut outcome = ClientShellInput::default();
        self.push_endpoint_method(
            Method::PaneSendInput(PaneSendInputParams {
                pane_id,
                text: editor_command(&path, &self.config.default_shell, cfg!(windows)),
                keys: vec!["Enter".into()],
            }),
            &mut outcome,
        );
        (outcome.repaint, outcome.actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, is_dir: bool) -> FileEntry {
        FileEntry {
            name: Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: PathBuf::from(path),
            is_dir,
        }
    }

    #[test]
    fn rows_nest_only_expanded_directories() {
        let mut tree = FileTree::with_listings(
            PathBuf::from("/p"),
            vec![
                (
                    PathBuf::from("/p"),
                    vec![entry("/p/src", true), entry("/p/README.md", false)],
                ),
                (
                    PathBuf::from("/p/src"),
                    vec![entry("/p/src/main.rs", false)],
                ),
            ],
        );
        let names = |tree: &FileTree| {
            tree.rows()
                .iter()
                .map(|row| (row.entry.name.clone(), row.depth))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(&tree),
            vec![("src".into(), 0), ("README.md".into(), 0)]
        );
        tree.expanded.insert(PathBuf::from("/p/src"));
        assert_eq!(
            names(&tree),
            vec![
                ("src".into(), 0),
                ("main.rs".into(), 1),
                ("README.md".into(), 0)
            ]
        );
    }

    #[test]
    fn listing_puts_directories_first_and_hides_git() {
        let dir = std::env::temp_dir().join(format!("herdr-file-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("zeta")).unwrap();
        std::fs::write(dir.join("Alpha.txt"), "").unwrap();
        std::fs::write(dir.join("beta.txt"), "").unwrap();
        let names = read_dir_sorted(&dir)
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(names, vec!["zeta", "Alpha.txt", "beta.txt"]);
    }

    #[test]
    fn status_maps_each_stage_to_a_mark() {
        let status = " M a.rs\0M  b.rs\0MM c.rs\0?? d.rs\0UU e.rs\0R  new.rs\0old.rs\0";
        assert_eq!(
            parse_status(status),
            vec![
                ("a.rs".into(), GitMark::Modified),
                ("b.rs".into(), GitMark::Staged),
                ("c.rs".into(), GitMark::Modified),
                ("d.rs".into(), GitMark::Untracked),
                ("e.rs".into(), GitMark::Conflict),
                ("new.rs".into(), GitMark::Staged),
            ]
        );
    }

    #[test]
    fn git_marks_follow_a_file_from_edit_to_push() {
        let base = std::env::temp_dir().join(format!("herdr-file-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let work = base.join("work");
        let run = |dir: &Path, args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .env_remove("GIT_DIR")
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        };
        std::fs::create_dir_all(work.join("src")).unwrap();
        if !run(&base, &["init", "-q", "--bare", "remote.git"])
            || !run(&work, &["init", "-q", "-b", "main"])
        {
            std::fs::remove_dir_all(&base).unwrap();
            return; // no git available
        }
        std::fs::write(work.join("src/lib.rs"), "a").unwrap();
        let file = work.join("src/lib.rs");
        let mark = |path: &Path| git_marks(&work, &[]).get(path).copied();

        assert_eq!(mark(&file), Some(GitMark::Untracked));
        assert_eq!(mark(&work.join("src")), Some(GitMark::Untracked));
        assert!(run(&work, &["add", "."]));
        assert_eq!(mark(&file), Some(GitMark::Staged));
        assert!(run(&work, &["commit", "-qm", "one"]));
        assert!(run(&work, &["remote", "add", "origin", "../remote.git"]));
        assert!(run(&work, &["push", "-q", "-u", "origin", "main"]));
        assert_eq!(mark(&file), None);
        std::fs::write(&file, "b").unwrap();
        assert_eq!(mark(&file), Some(GitMark::Modified));
        assert!(run(&work, &["commit", "-qam", "two"]));
        assert_eq!(mark(&file), Some(GitMark::Unpushed));
        // A folder opened as the root still maps repo-relative paths onto itself.
        assert_eq!(
            git_marks(&work.join("src"), &[]).get(&file).copied(),
            Some(GitMark::Unpushed)
        );
        // A root that is no repo itself, holding repos, still shows their changes.
        let outside = git_marks(&base, std::slice::from_ref(&work));
        assert_eq!(outside.get(&file).copied(), Some(GitMark::Unpushed));
        assert_eq!(outside.get(&work).copied(), Some(GitMark::Unpushed));
        assert!(run(&work, &["push", "-q"]));
        assert_eq!(mark(&file), None);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn panel_follows_the_agent_dir_unless_set_to_the_space() {
        let mut snapshot = super::super::tests::snapshot();
        assert_eq!(
            focused_pane_dir(&snapshot, false),
            Some(PathBuf::from("/repo"))
        );
        snapshot.agents.push(crate::protocol::ClientShellAgent {
            pane_id: "pane_1".into(),
            workspace_id: "ws_1".into(),
            tab_id: "tab_1".into(),
            name: None,
            display_agent: None,
            agent: Some("claude".into()),
            title: None,
            terminal_title: None,
            terminal_title_stripped: None,
            agent_status: crate::api::schema::AgentStatus::Idle,
            state_change_seq: 0,
            state_labels: Vec::new(),
            tokens: vec![(AGENT_CWD_TOKEN.into(), "/repo/api".into())],
            focused: true,
        });
        assert_eq!(
            focused_pane_dir(&snapshot, false),
            Some(PathBuf::from("/repo/api"))
        );
        assert_eq!(
            focused_pane_dir(&snapshot, true),
            Some(PathBuf::from("/repo"))
        );
    }

    #[test]
    fn editor_command_quotes_the_path() {
        assert_eq!(
            editor_command(Path::new("/tmp/it's here.rs"), "", false),
            r#"exec sh -c '${VISUAL:-${EDITOR:-vi}} "$1"' sh '/tmp/it'\''s here.rs'"#
        );
    }

    #[test]
    fn editor_command_follows_the_pane_shell() {
        let path = Path::new(r"C:\work\it's.rs");
        let powershell = |shell: &str, windows: bool| {
            editor_command(path, shell, windows).ends_with(r"'C:\work\it''s.rs'; if ($?) { exit }")
        };
        assert!(powershell("", true));
        assert!(powershell("pwsh.exe", true));
        assert!(powershell(r"C:\Program Files\PowerShell\7\pwsh.exe", true));
        assert!(powershell("powershell -NoLogo", false));
        assert!(powershell(
            r#""C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo"#,
            true
        ));
        assert!(!powershell("", false));
        assert!(!powershell("bash", true));
        assert!(
            editor_command(path, r"C:\Program Files\Git\bin\bash.exe", true)
                .starts_with("exec sh -c")
        );
    }
}

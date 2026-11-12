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
    loaded_at: Option<Instant>,
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
        let dirs = std::iter::once(root)
            .chain(self.expanded.iter().cloned())
            .collect::<Vec<_>>();
        for dir in dirs {
            let entries = read_dir_sorted(&dir);
            self.listings.insert(dir, entries);
        }
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

/// The focused pane's directory: where its foreground process runs, else its shell cwd.
pub(super) fn focused_pane_dir(snapshot: &ClientShellSnapshot) -> Option<PathBuf> {
    let focused = snapshot.focused_pane_id.as_deref()?;
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
    if let Some(name) = tree
        .root
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy())
    {
        let room = area.width.saturating_sub(8) as usize;
        let label = crate::ui::truncate_end(&name, room);
        render::put_right_text(
            buffer,
            area,
            area.y + 1,
            &label,
            Style::default().fg(palette.overlay0),
        );
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
        let room = rect.right().saturating_sub(x) as usize;
        render::put_segment(
            buffer,
            x,
            rect.y,
            rect.right(),
            &crate::ui::truncate_end(&row.entry.name, room),
            name_style,
        );
        hits.files
            .push((rect, row.entry.path.clone(), row.entry.is_dir));
    }
    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        super::scroll::render_list_scrollbar(buffer, track, metrics, palette);
    }
}

/// Shell line that replaces the new pane's shell with the user's editor on `path`.
/// Single-quoted arguments read the same in POSIX shells and fish.
pub(super) fn editor_command(path: &Path) -> String {
    format!(
        "exec sh -c '${{VISUAL:-${{EDITOR:-vi}}}} \"$1\"' sh {}",
        shell_quote(&path.display().to_string())
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
            .and_then(focused_pane_dir)
            .filter(|dir| dir.is_dir());
        if self.file_tree.set_root(root) {
            self.file_scroll = 0;
        }
    }

    pub(super) fn handle_file_click(
        &mut self,
        point: (u16, u16),
        outcome: &mut ClientShellInput,
    ) -> bool {
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
                text: editor_command(&path),
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
    fn editor_command_quotes_the_path() {
        assert_eq!(
            editor_command(Path::new("/tmp/it's here.rs")),
            r#"exec sh -c '${VISUAL:-${EDITOR:-vi}} "$1"' sh '/tmp/it'\''s here.rs'"#
        );
    }
}

//! Terminal-pane management: the file-pane tab strip (`PaneTabs`), a shell tab
//! and its split tree (`ShellTab`), and the shell panel (`ShellPane`) — async
//! PTY spawn, splits, tab/pane focus, backgrounds, and the alt-screen check.
//! The structs themselves stay in lib.rs; only their impls live here.

use super::*;

impl PaneTabs {
    pub fn single(p: Pane) -> Self {
        Self { tabs: vec![p], active: 0 }
    }
    pub fn active_ref(&self) -> &Pane { &self.tabs[self.active] }
    pub fn active_mut(&mut self) -> &mut Pane { &mut self.tabs[self.active] }
    /// Every tab's pane, for settings (like show-hidden) that apply to all.
    pub fn all_mut(&mut self) -> impl Iterator<Item = &mut Pane> {
        self.tabs.iter_mut()
    }
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }
    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
        }
    }
    pub fn select(&mut self, idx: usize) {
        if idx < self.tabs.len() { self.active = idx; }
    }
    pub fn add_clone(&mut self) -> Result<()> {
        let cwd = self.active_ref().cwd.clone();
        self.tabs.push(Pane::new(cwd)?);
        self.active = self.tabs.len() - 1;
        Ok(())
    }
    pub fn close_active(&mut self) {
        if self.tabs.len() > 1 {
            self.tabs.remove(self.active);
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len() - 1;
            }
        }
    }
}

impl ShellTab {
    pub(crate) fn new(session: PtySession) -> Self {
        Self { nodes: vec![Some(Node::Leaf { session, bg: None })], root: 0, active: 0 }
    }

    pub(crate) fn alloc(&mut self, node: Node) -> usize {
        if let Some(i) = self.nodes.iter().position(|n| n.is_none()) {
            self.nodes[i] = Some(node);
            i
        } else {
            self.nodes.push(Some(node));
            self.nodes.len() - 1
        }
    }

    pub(crate) fn active_pane(&self) -> Option<&PtySession> {
        match self.nodes.get(self.active).and_then(|n| n.as_ref()) {
            Some(Node::Leaf { session, .. }) => Some(session),
            _ => None,
        }
    }
    pub(crate) fn active_pane_mut(&mut self) -> Option<&mut PtySession> {
        match self.nodes.get_mut(self.active).and_then(|n| n.as_mut()) {
            Some(Node::Leaf { session, .. }) => Some(session),
            _ => None,
        }
    }

    pub(crate) fn collect_leaves(&self, i: usize, out: &mut Vec<usize>) {
        match self.nodes.get(i).and_then(|n| n.as_ref()) {
            Some(Node::Leaf { .. }) => out.push(i),
            Some(Node::Split { first, second, .. }) => {
                self.collect_leaves(*first, out);
                self.collect_leaves(*second, out);
            }
            None => {}
        }
    }
    pub(crate) fn leaves(&self) -> Vec<usize> {
        let mut v = Vec::new();
        if self.nodes.get(self.root).map(|n| n.is_some()).unwrap_or(false) {
            self.collect_leaves(self.root, &mut v);
        }
        v
    }

    pub(crate) fn first_leaf(&self, i: usize) -> usize {
        match self.nodes.get(i).and_then(|n| n.as_ref()) {
            Some(Node::Split { first, .. }) => self.first_leaf(*first),
            _ => i,
        }
    }

    pub(crate) fn parent_of(&self, child: usize) -> Option<(usize, bool)> {
        for (i, n) in self.nodes.iter().enumerate() {
            if let Some(Node::Split { first, second, .. }) = n {
                if *first == child {
                    return Some((i, true));
                }
                if *second == child {
                    return Some((i, false));
                }
            }
        }
        None
    }

    /// Walk up from the active leaf to the nearest split laid out along `want`,
    /// returning its node index. Used to pick which boundary a resize key moves
    /// — a Left/Right key resizes the nearest side-by-side split, Up/Down the
    /// nearest stacked one.
    pub(crate) fn nearest_split(&self, want: SplitDir) -> Option<usize> {
        let mut child = self.active;
        while let Some((parent, _)) = self.parent_of(child) {
            if let Some(Node::Split { dir, .. }) = self.nodes.get(parent).and_then(|n| n.as_ref()) {
                if *dir == want {
                    return Some(parent);
                }
            }
            child = parent;
        }
        None
    }

    /// Nudge a split's ratio by `delta`, clamped so neither child vanishes.
    pub(crate) fn nudge_split(&mut self, node: usize, delta: i16) {
        if let Some(Node::Split { ratio, .. }) =
            self.nodes.get_mut(node).and_then(|n| n.as_mut())
        {
            let next = (*ratio as i16 + delta).clamp(MIN_SPLIT_PCT as i16, 100 - MIN_SPLIT_PCT as i16);
            *ratio = next as u16;
        }
    }

    /// Split the active leaf into (old, new) along `dir`; new becomes active.
    pub(crate) fn split(&mut self, dir: SplitDir, new_session: PtySession) {
        let old = self.active;
        if !matches!(self.nodes.get(old).and_then(|n| n.as_ref()), Some(Node::Leaf { .. })) {
            return;
        }
        let new_leaf = self.alloc(Node::Leaf { session: new_session, bg: None });
        let split_idx = self.alloc(Node::Split { dir, first: old, second: new_leaf, ratio: 50 });
        if old == self.root {
            self.root = split_idx;
        } else if let Some((p, is_first)) = self.parent_of(old) {
            if let Some(Node::Split { first, second, .. }) = self.nodes[p].as_mut() {
                if is_first {
                    *first = split_idx;
                } else {
                    *second = split_idx;
                }
            }
        }
        self.active = new_leaf;
    }

    pub(crate) fn focus_next(&mut self, forward: bool) {
        let leaves = self.leaves();
        if leaves.is_empty() {
            return;
        }
        let pos = leaves.iter().position(|&l| l == self.active).unwrap_or(0);
        let n = leaves.len();
        let np = if forward { (pos + 1) % n } else { (pos + n - 1) % n };
        self.active = leaves[np];
    }

    /// Close the active leaf; its sibling takes the parent's place. Returns true
    /// if the tab is now empty.
    pub(crate) fn close_active(&mut self) -> bool {
        let leaf = self.active;
        if !matches!(self.nodes.get(leaf).and_then(|n| n.as_ref()), Some(Node::Leaf { .. })) {
            return self.leaves().is_empty();
        }
        if leaf == self.root {
            self.nodes[leaf] = None;
            return true;
        }
        let (p, leaf_is_first) = match self.parent_of(leaf) {
            Some(x) => x,
            None => {
                self.nodes[leaf] = None;
                return self.leaves().is_empty();
            }
        };
        let sib = match self.nodes[p].as_ref() {
            Some(Node::Split { first, second, .. }) => {
                if leaf_is_first { *second } else { *first }
            }
            _ => return false,
        };
        if p == self.root {
            self.root = sib;
        } else if let Some((gp, p_is_first)) = self.parent_of(p) {
            if let Some(Node::Split { first, second, .. }) = self.nodes[gp].as_mut() {
                if p_is_first {
                    *first = sib;
                } else {
                    *second = sib;
                }
            }
        }
        self.nodes[leaf] = None;
        self.nodes[p] = None;
        self.active = self.first_leaf(sib);
        false
    }

    pub(crate) fn for_each_leaf_mut(&mut self, f: &mut dyn FnMut(&mut PtySession)) {
        for n in self.nodes.iter_mut() {
            if let Some(Node::Leaf { session: s, .. }) = n {
                f(s);
            }
        }
    }
}

impl ShellPane {
    /// The configured shell program (path/name), for prompts that need it.
    pub(crate) fn command(&self) -> &str {
        &self.shell_cmd
    }

    pub(crate) fn new(shell_cmd: String) -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            zoom_pane: false,
            rows: 24,
            cols: 80,
            shell_cmd,
            error: None,
            pending: Vec::new(),
            just_split: None,
        }
    }

    pub(crate) fn count(&self) -> usize {
        self.tabs.len()
    }

    pub(crate) fn active_tab(&self) -> Option<&ShellTab> {
        self.tabs.get(self.active)
    }

    /// How many split panes the active tab has.
    pub(crate) fn active_pane_count(&self) -> usize {
        self.active_tab().map(|t| t.leaves().len()).unwrap_or(0)
    }

    /// The active pane's terminal title (what the shell/program set via OSC) —
    /// usually `user@host: cwd`. Empty titles return None.
    pub(crate) fn active_title(&self) -> Option<String> {
        let t = self.active_tab()?;
        let s = match t.nodes.get(t.active).and_then(|n| n.as_ref()) {
            Some(Node::Leaf { session, .. }) => session,
            _ => return None,
        };
        let title = s.parser().lock().ok()?.screen().title().trim().to_string();
        if title.is_empty() { None } else { Some(title) }
    }

    /// The active pane's position among the tab's panes, `(index, total)`,
    /// 1-based — for the "1 of 3" hint while one pane is maximized.
    pub(crate) fn active_pane_position(&self) -> (usize, usize) {
        match self.active_tab() {
            Some(t) => {
                let leaves = t.leaves();
                let pos = leaves.iter().position(|&l| l == t.active).map(|i| i + 1).unwrap_or(1);
                (pos, leaves.len())
            }
            None => (1, 1),
        }
    }

    /// Set the active pane's background. Per pane, not per panel: the point is
    /// to tell one split from another.
    pub(crate) fn set_active_pane_bg(&mut self, color: Option<Color>) {
        let active = self.active;
        if let Some(t) = self.tabs.get_mut(active) {
            let leaf = t.active;
            if let Some(Node::Leaf { bg, .. }) = t.nodes.get_mut(leaf).and_then(|n| n.as_mut()) {
                *bg = color;
            }
        }
    }

    /// The active pane's background, for pre-selecting it in the picker.
    pub(crate) fn active_pane_bg(&self) -> Option<Color> {
        let t = self.active_tab()?;
        match t.nodes.get(t.active).and_then(|n| n.as_ref()) {
            Some(Node::Leaf { bg, .. }) => *bg,
            _ => None,
        }
    }

    pub(crate) fn active_session(&self) -> Option<&PtySession> {
        self.active_tab().and_then(|t| t.active_pane())
    }

    pub(crate) fn active_session_mut(&mut self) -> Option<&mut PtySession> {
        self.tabs.get_mut(self.active).and_then(|t| t.active_pane_mut())
    }

    /// Start a PTY spawn on a background thread.
    ///
    /// Spawning (openpty + fork/exec of the shell) must never run on the UI
    /// thread: the event loop is single-threaded, so a slow shell startup
    /// (heavy rc files, a hung `$SHELL`, a stalled network home directory)
    /// would block *all* input until it returned — the app looked frozen.
    /// Every spawn path goes through here; results are installed by
    /// [`ShellPane::poll_pending`].
    pub(crate) fn spawn_async(&mut self, cwd: &Path, kind: PendingKind) {
        let cwd = cwd.to_path_buf();
        let shell_cmd = self.shell_cmd.clone();
        let rows = self.rows.max(1);
        let cols = self.cols.max(1);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if cian_core::log::enabled() {
                cian_core::log::log(&format!("spawning shell {:?} in {}", shell_cmd, cwd.display()));
            }
            let result = PtySession::new(&cwd, &shell_cmd, rows, cols).map_err(|e| e.to_string());
            if cian_core::log::enabled() {
                match &result {
                    Ok(_) => cian_core::log::log("shell spawned"),
                    Err(e) => cian_core::log::log(&format!("shell spawn failed: {}", e)),
                }
            }
            let _ = tx.send(result);
        });
        self.pending.push(PendingSpawn { rx, kind });
        self.error = None;
    }

    /// Whether a spawn of this kind is already in flight.
    pub(crate) fn is_pending(&self, kind: PendingKind) -> bool {
        self.pending.iter().any(|p| p.kind == kind)
    }

    /// True while any spawn is in flight. A macro waits for this to clear
    /// between splits, so each pane lands before the next is built.
    pub(crate) fn busy(&self) -> bool {
        !self.pending.is_empty()
    }

    /// True while the panel has no pane yet but one is on its way.
    pub(crate) fn is_starting(&self) -> bool {
        self.tabs.is_empty() && !self.pending.is_empty()
    }

    /// Spawn the first tab if none exists yet (lazy start on first focus).
    pub(crate) fn ensure(&mut self, cwd: &Path) {
        if self.tabs.is_empty() && !self.is_pending(PendingKind::FirstTab) {
            self.spawn_async(cwd, PendingKind::FirstTab);
        }
    }

    /// Install any background spawns that have completed. Returns true if the
    /// panel's state changed (so the caller should repaint).
    pub(crate) fn poll_pending(&mut self) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        let mut changed = false;
        let mut still_pending = Vec::with_capacity(self.pending.len());
        for p in std::mem::take(&mut self.pending) {
            match p.rx.try_recv() {
                Ok(Ok(session)) => {
                    self.install(session, p.kind);
                    changed = true;
                }
                Ok(Err(e)) => {
                    self.error = Some(e);
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => still_pending.push(p),
                // The worker vanished without sending (it panicked). Drop it
                // rather than waiting forever.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("shell spawn failed unexpectedly".to_string());
                    changed = true;
                }
            }
        }
        self.pending = still_pending;
        changed
    }

    /// Place a freshly-spawned session according to what asked for it.
    pub(crate) fn install(&mut self, session: PtySession, kind: PendingKind) {
        match kind {
            PendingKind::FirstTab => {
                self.tabs.push(ShellTab::new(session));
                self.active = self.tabs.len() - 1;
            }
            PendingKind::NewTab => {
                self.tabs.push(ShellTab::new(session));
                self.active = self.tabs.len() - 1;
                self.zoom_pane = false;
            }
            PendingKind::Split { tab, dir } => match self.tabs.get_mut(tab) {
                Some(t) => {
                    t.split(dir, session);
                    // `split` makes the new leaf active, so its parent is the
                    // split node that was just created.
                    self.just_split = t.parent_of(t.active).map(|(p, _)| (tab, p));
                    // A split must be visible, so leave single-pane zoom.
                    self.zoom_pane = false;
                }
                // The target tab was closed while we were spawning; the
                // session is dropped here, which kills the shell.
                None => return,
            },
        }
        self.error = None;
    }

    /// Open an additional shell tab.
    pub(crate) fn new_tab(&mut self, cwd: &Path) {
        self.spawn_async(cwd, PendingKind::NewTab);
    }

    /// Split the active tab's active pane in `dir`, spawning a new pane.
    pub(crate) fn split_active(&mut self, cwd: &Path, dir: SplitDir) {
        if self.tabs.get(self.active).is_none() {
            return;
        }
        let kind = PendingKind::Split { tab: self.active, dir };
        self.spawn_async(cwd, kind);
    }

    pub(crate) fn next_pane(&mut self) {
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.focus_next(true);
        }
        self.zoom_pane = false;
    }

    pub(crate) fn prev_pane(&mut self) {
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.focus_next(false);
        }
        self.zoom_pane = false;
    }

    /// Close the active pane. If its tab becomes empty the tab is removed.
    /// Returns true if no tabs remain (caller should leave the shell).
    pub(crate) fn close_active_pane(&mut self) -> bool {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if tab.close_active() {
                self.tabs.remove(self.active);
                if self.active >= self.tabs.len() && self.active > 0 {
                    self.active -= 1;
                }
            }
        }
        self.zoom_pane = false;
        self.tabs.is_empty()
    }

    /// Switch to shell tab `i` (no-op if out of range).
    pub(crate) fn select(&mut self, i: usize) {
        if i < self.tabs.len() {
            self.active = i;
            self.zoom_pane = false;
        }
    }

    /// Close the whole active tab. Returns true if no tabs remain.
    pub(crate) fn close_active(&mut self) -> bool {
        if self.active < self.tabs.len() {
            self.tabs.remove(self.active);
            if self.active >= self.tabs.len() && self.active > 0 {
                self.active -= 1;
            }
        }
        self.zoom_pane = false;
        self.tabs.is_empty()
    }

    /// Clear and report whether any pane in the active tab produced new output.
    pub(crate) fn take_active_tab_dirty(&mut self) -> bool {
        let mut dirty = false;
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.for_each_leaf_mut(&mut |p| {
                if p.take_dirty() {
                    dirty = true;
                }
            });
        }
        dirty
    }

    /// `(alternate_screen, application_cursor)` for the active pane.
    pub(crate) fn active_modes(&self) -> (bool, bool) {
        if let Some(s) = self.active_session() {
            if let Ok(p) = s.parser().lock() {
                let scr = p.screen();
                return (scr.alternate_screen(), scr.application_cursor());
            }
        }
        (false, false)
    }
}

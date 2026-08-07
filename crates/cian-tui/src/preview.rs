//! Cursor-follow preview (`:preview`): while it is on and a file pane has
//! focus, the shell panel's area shows what the cursor is on — text with
//! syntax colour, images (real pixels where the terminal can), directory and
//! archive listings, Office/PDF text. Focusing the shell (Shift+J / click)
//! shows the real shell again; the PTY runs underneath the whole time, only
//! the pixels are borrowed.
//!
//! Loading is synchronous but bounded (the viewer's own 4MB cap, listing
//! caps below) and cached by path, so holding `j` costs one load per file,
//! not one per frame.

use std::path::{Path, PathBuf};

use crate::{tr, App, FocusedPane};

/// How many rows a directory / archive listing shows at most. A preview is a
/// glance, not a browser — entering it is one keypress away.
const LIST_CAP: usize = 500;

/// What the preview pane is showing for [`PreviewState::path`].
pub(crate) enum PreviewBody {
    /// Text lines with per-char highlight categories (empty when plain).
    Text { lines: Vec<String>, hl: Vec<Vec<cian_core::highlight::Category>> },
    /// A directory or archive listing, pre-rendered as rows.
    List { rows: Vec<String>, truncated: bool },
    /// An image; pixels are rendered at draw time (protocol or half-blocks).
    Image,
    /// A one-line explanation instead of content.
    Note(String),
}

pub(crate) struct PreviewState {
    pub path: PathBuf,
    pub body: PreviewBody,
    /// Half-block fallback thumbnail, cached by the box it was rendered for.
    pub thumb: Option<(u16, u16, cian_core::image::Thumb)>,
}

impl App {
    /// `:preview` — flip the cursor-follow preview.
    pub(crate) fn toggle_preview(&mut self) {
        self.preview_on = !self.preview_on;
        if !self.preview_on {
            self.preview = None;
            self.preview_gfx = None;
        }
        self.message = Some(if self.preview_on {
            tr(
                self.lang,
                "preview on — the shell panel follows the cursor (Shift+J for the shell)",
                "プレビューON — シェル枠がカーソルに追従（シェルは Shift+J）",
            )
            .into()
        } else {
            tr(self.lang, "preview off", "プレビューOFF").into()
        });
    }

    /// Make sure the cached preview matches `path`, loading it if not.
    /// Called from the render pass, so it must stay cheap on the cached path.
    pub(crate) fn ensure_preview(&mut self, path: &Path) {
        if self.preview.as_ref().map(|p| p.path == *path).unwrap_or(false) {
            return;
        }
        // A different image invalidates the protocol state too.
        if self.preview_gfx.as_ref().map(|(p, _)| p != path).unwrap_or(false) {
            self.preview_gfx = None;
        }
        let body = load_preview(path, self.lang);
        self.preview = Some(PreviewState { path: path.to_path_buf(), body, thumb: None });
    }
}

/// Classify and load, with the same dispatch order as F3 (`look_inside`).
fn load_preview(path: &Path, lang: crate::Lang) -> PreviewBody {
    if path.is_dir() {
        return match std::fs::read_dir(path) {
            Ok(rd) => {
                let mut dirs: Vec<String> = Vec::new();
                let mut files: Vec<String> = Vec::new();
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    if is_dir {
                        dirs.push(format!("{}/", name));
                    } else {
                        files.push(name);
                    }
                    if dirs.len() + files.len() > LIST_CAP {
                        break;
                    }
                }
                dirs.sort();
                files.sort();
                let truncated = dirs.len() + files.len() > LIST_CAP;
                dirs.extend(files);
                dirs.truncate(LIST_CAP);
                PreviewBody::List { rows: dirs, truncated }
            }
            Err(e) => PreviewBody::Note(e.to_string()),
        };
    }
    if cian_core::archive::is_archive(path) {
        return match cian_core::archive::list(path) {
            Ok(members) => {
                let truncated = members.len() > LIST_CAP;
                let rows = members
                    .into_iter()
                    .take(LIST_CAP)
                    .map(|m| {
                        if m.is_dir {
                            m.name
                        } else {
                            format!("{}  ({})", m.name, cian_core::human_size(m.size))
                        }
                    })
                    .collect();
                PreviewBody::List { rows, truncated }
            }
            Err(e) => PreviewBody::Note(e.to_string()),
        };
    }
    if cian_core::image::is_image(path) {
        return PreviewBody::Image;
    }
    if cian_core::office::classify(path).is_some() {
        return match cian_core::office::extract(path) {
            Ok((_, lines)) => PreviewBody::Text { lines, hl: Vec::new() },
            Err(e) => PreviewBody::Note(e.to_string()),
        };
    }
    match cian_core::viewer::view_file(path) {
        Ok(view) => {
            let hl = if view.kind == cian_core::viewer::ViewKind::Text {
                cian_core::highlight::detect(path)
                    .map(|lang| cian_core::highlight::highlight(&view.lines, lang))
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            PreviewBody::Text { lines: view.lines, hl }
        }
        Err(_) => PreviewBody::Note(
            tr(lang, "no preview for this file", "このファイルはプレビューできません").into(),
        ),
    }
}

/// The path the preview should follow: the cursor entry of the focused file
/// pane — unless there is a reason not to look (remote pane: reading a file
/// would download it; nothing selected). `Err` carries the note to show.
pub(crate) fn preview_target(app: &App) -> Result<PathBuf, String> {
    let pane = match app.focused {
        FocusedPane::Left => app.left.active_ref(),
        FocusedPane::Right => app.right.active_ref(),
        FocusedPane::Shell => return Err(String::new()), // caller never asks
    };
    if pane.is_remote() {
        return Err(tr(
            app.lang,
            "remote pane — no preview (it would download every file)",
            "リモートペイン — プレビューなし（毎回ダウンロードになるため）",
        )
        .into());
    }
    match pane.selected() {
        Some(e) if e.is_parent => Ok(e.path.clone()), // `..` previews the parent dir
        Some(e) => Ok(e.path.clone()),
        None => Err(tr(app.lang, "empty folder", "空のフォルダ").into()),
    }
}

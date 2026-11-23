//! Map Git diff ranges onto editor visual lines for gutter and scrollbar markers.

use floem::{
    peniko::Color,
    reactive::SignalGet,
    views::editor::{Editor, visual_line::VLine},
};
use itertools::Itertools;
use sparrow_core::buffer::diff::DiffLines;

use crate::config::{SparrowConfig, color::SparrowColor};

/// Iterator over (len, color, modified) for each change in the diff
fn changes_color_iter<'a>(
    changes: &'a im::Vector<DiffLines>,
    config: &'a SparrowConfig,
) -> impl Iterator<Item = (usize, Option<Color>, bool)> + 'a {
    let mut last_change = None;
    changes.iter().map(move |change| {
        let len = match change {
            DiffLines::Left(_range) => 0,
            DiffLines::Both(info) => info.right.len(),
            DiffLines::Right(range) => range.len(),
        };
        let mut modified = false;
        let color = match change {
            DiffLines::Left(_range) => {
                Some(config.color(SparrowColor::SOURCE_CONTROL_REMOVED))
            }
            DiffLines::Right(_range) => {
                if let Some(DiffLines::Left(_)) = last_change.as_ref() {
                    modified = true;
                }
                if modified {
                    Some(config.color(SparrowColor::SOURCE_CONTROL_MODIFIED))
                } else {
                    Some(config.color(SparrowColor::SOURCE_CONTROL_ADDED))
                }
            }
            _ => None,
        };

        last_change = Some(change.clone());

        (len, color, modified)
    })
}

// TODO: both of the changes color functions could easily return iterators

/// Get the position and coloring information for over the entire current [`ScreenLines`]
/// Returns `(y, height_idx, removed, color)`
pub fn changes_colors_screen(
    config: &SparrowConfig,
    editor: &Editor,
    changes: im::Vector<DiffLines>,
) -> Vec<(f64, usize, bool, Color)> {
    let screen_lines = editor.screen_lines.get_untracked();

    let Some((min, max)) = screen_lines.rvline_range() else {
        return Vec::new();
    };

    let line_height = config.editor.line_height();
    let mut line = 0;
    let mut colors = Vec::new();

    for (len, color, modified) in changes_color_iter(&changes, config) {
        let pre_line = line;

        line += len;
        if line < min.line {
            continue;
        }

        if let Some(color) = color {
            if modified {
                colors.pop();
            }

            let rvline = editor.rvline_of_line(pre_line);
            let vline = editor.vline_of_line(pre_line);
            let y = (vline.0 * line_height) as f64;
            let height = {
                // Accumulate the number of line indices each potentially wrapped line spans
                let end_line = rvline.line + len;

                editor.iter_rvlines_over(false, rvline, end_line).count()
            };
            let removed = len == 0;

            colors.push((y, height, removed, color));
        }

        if line > max.line {
            break;
        }
    }

    colors
}

// TODO: limit the visual line that changes are considered past to some reasonable number
// TODO(minor): This could be a `changes_colors_range` with some minor changes, but it isn't needed
/// Get the position and coloring information for over the entire current [`ScreenLines`]
/// Returns `(y, height_idx, removed, color)`
pub fn changes_colors_all(
    config: &SparrowConfig,
    ed: &Editor,
    changes: im::Vector<DiffLines>,
) -> Vec<(f64, usize, bool, Color)> {
    let line_height = config.editor.line_height();

    let mut line = 0;
    let mut colors = Vec::new();

    let mut vline_iter = ed.iter_vlines(false, VLine(0)).peekable();

    for (len, color, modified) in changes_color_iter(&changes, config) {
        let pre_line = line;

        line += len;

        // Skip over all vlines that are before the current line
        vline_iter
            .by_ref()
            .peeking_take_while(|info| info.rvline.line < pre_line)
            .count();

        if let Some(color) = color {
            if modified {
                colors.pop();
            }

            // Find the info with a line == pre_line
            let Some(info) = vline_iter.peek() else {
                continue;
            };

            let y = info.vline.get() * line_height;
            let end_line = info.rvline.line + len;
            let height = vline_iter
                .by_ref()
                .peeking_take_while(|info| info.rvline.line < end_line)
                .count();
            let removed = len == 0;

            colors.push((y as f64, height, removed, color));
        }

        if vline_iter.peek().is_none() {
            break;
        }
    }

    colors
}

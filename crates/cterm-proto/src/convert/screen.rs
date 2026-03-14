//! Screen and cell conversion between cterm-core and proto

use crate::convert::color::{color_to_proto, proto_to_color};
use crate::proto;
use cterm_core::term::Terminal;
use cterm_core::{Cell, CellAttrs, Screen};

/// Convert cell attributes to proto
pub fn attrs_to_proto(attrs: CellAttrs) -> proto::CellAttributes {
    proto::CellAttributes {
        bold: attrs.contains(CellAttrs::BOLD),
        italic: attrs.contains(CellAttrs::ITALIC),
        underline: attrs.contains(CellAttrs::UNDERLINE),
        double_underline: attrs.contains(CellAttrs::DOUBLE_UNDERLINE),
        curly_underline: attrs.contains(CellAttrs::CURLY_UNDERLINE),
        dotted_underline: attrs.contains(CellAttrs::DOTTED_UNDERLINE),
        dashed_underline: attrs.contains(CellAttrs::DASHED_UNDERLINE),
        blink: attrs.contains(CellAttrs::BLINK),
        inverse: attrs.contains(CellAttrs::INVERSE),
        hidden: attrs.contains(CellAttrs::HIDDEN),
        strikethrough: attrs.contains(CellAttrs::STRIKETHROUGH),
        dim: attrs.contains(CellAttrs::DIM),
        overline: attrs.contains(CellAttrs::OVERLINE),
        wide: attrs.contains(CellAttrs::WIDE),
        wide_spacer: attrs.contains(CellAttrs::WIDE_SPACER),
    }
}

/// Convert proto attributes to cell attributes
pub fn proto_to_attrs(attrs: &proto::CellAttributes) -> CellAttrs {
    let mut result = CellAttrs::empty();
    if attrs.bold {
        result |= CellAttrs::BOLD;
    }
    if attrs.italic {
        result |= CellAttrs::ITALIC;
    }
    if attrs.underline {
        result |= CellAttrs::UNDERLINE;
    }
    if attrs.double_underline {
        result |= CellAttrs::DOUBLE_UNDERLINE;
    }
    if attrs.curly_underline {
        result |= CellAttrs::CURLY_UNDERLINE;
    }
    if attrs.dotted_underline {
        result |= CellAttrs::DOTTED_UNDERLINE;
    }
    if attrs.dashed_underline {
        result |= CellAttrs::DASHED_UNDERLINE;
    }
    if attrs.blink {
        result |= CellAttrs::BLINK;
    }
    if attrs.inverse {
        result |= CellAttrs::INVERSE;
    }
    if attrs.hidden {
        result |= CellAttrs::HIDDEN;
    }
    if attrs.strikethrough {
        result |= CellAttrs::STRIKETHROUGH;
    }
    if attrs.dim {
        result |= CellAttrs::DIM;
    }
    if attrs.overline {
        result |= CellAttrs::OVERLINE;
    }
    if attrs.wide {
        result |= CellAttrs::WIDE;
    }
    if attrs.wide_spacer {
        result |= CellAttrs::WIDE_SPACER;
    }
    result
}

/// Convert a cell to proto
pub fn cell_to_proto(cell: &Cell) -> proto::Cell {
    proto::Cell {
        char: cell.c.to_string(),
        fg: Some(color_to_proto(&cell.fg)),
        bg: Some(color_to_proto(&cell.bg)),
        attrs: Some(attrs_to_proto(cell.attrs)),
        underline_color: cell.underline_color.as_ref().map(color_to_proto),
        hyperlink: cell.hyperlink.as_ref().map(|h| proto::Hyperlink {
            id: h.id.clone(),
            uri: h.uri.clone(),
        }),
    }
}

/// Convert a row of cells to proto
pub fn row_to_proto(cells: &[Cell]) -> proto::Row {
    proto::Row {
        cells: cells.iter().map(cell_to_proto).collect(),
    }
}

/// Convert screen to proto representation
pub fn screen_to_proto(screen: &Screen, include_scrollback: bool) -> proto::GetScreenResponse {
    let cursor = proto::CursorPosition {
        row: screen.cursor.row as u32,
        col: screen.cursor.col as u32,
        visible: screen.modes.show_cursor,
        style: proto::CursorStyle::Block as i32,
    };

    // Get visible rows
    let visible_rows: Vec<proto::Row> = (0..screen.height())
        .map(|row_idx| {
            let cells: Vec<Cell> = (0..screen.width())
                .map(|col| screen.get_cell(row_idx, col).cloned().unwrap_or_default())
                .collect();
            row_to_proto(&cells)
        })
        .collect();

    // Get scrollback if requested
    let scrollback = if include_scrollback {
        screen
            .scrollback()
            .iter()
            .map(|row| {
                let cells: Vec<Cell> = row.iter().cloned().collect();
                row_to_proto(&cells)
            })
            .collect()
    } else {
        Vec::new()
    };

    proto::GetScreenResponse {
        cols: screen.width() as u32,
        rows: screen.height() as u32,
        cursor: Some(cursor),
        visible_rows,
        scrollback,
        title: screen.title.clone(),
        modes: Some(proto::TerminalModes {
            application_cursor: screen.modes.application_cursor,
            application_keypad: screen.modes.application_keypad,
            bracketed_paste: screen.modes.bracketed_paste,
            focus_events: screen.modes.focus_events,
        }),
    }
}

/// Convert a single visible row from the screen to proto
pub fn visible_row_to_proto(screen: &Screen, row_idx: usize) -> proto::Row {
    let cells: Vec<Cell> = (0..screen.width())
        .map(|col| screen.get_cell(row_idx, col).cloned().unwrap_or_default())
        .collect();
    row_to_proto(&cells)
}

/// Convert all visible rows to proto (no scrollback)
pub fn visible_rows_to_proto(screen: &Screen) -> Vec<proto::Row> {
    (0..screen.height())
        .map(|row_idx| visible_row_to_proto(screen, row_idx))
        .collect()
}

/// Build a cursor position proto from the screen state
pub fn cursor_to_proto(screen: &Screen) -> proto::CursorPosition {
    proto::CursorPosition {
        row: screen.cursor.row as u32,
        col: screen.cursor.col as u32,
        visible: screen.modes.show_cursor,
        style: proto::CursorStyle::Block as i32,
    }
}

/// Build terminal modes proto from the screen state
pub fn modes_to_proto(screen: &Screen) -> proto::TerminalModes {
    proto::TerminalModes {
        application_cursor: screen.modes.application_cursor,
        application_keypad: screen.modes.application_keypad,
        bracketed_paste: screen.modes.bracketed_paste,
        focus_events: screen.modes.focus_events,
    }
}

/// Get screen text as lines
pub fn screen_to_text(
    screen: &Screen,
    include_scrollback: bool,
    start_row: Option<u32>,
    end_row: Option<u32>,
) -> Vec<String> {
    let mut lines = Vec::new();

    // Add scrollback if requested
    if include_scrollback {
        for row in screen.scrollback().iter() {
            let text: String = row.iter().map(|c| c.c).collect();
            lines.push(text.trim_end().to_string());
        }
    }

    // Add visible rows
    let start = start_row.unwrap_or(0) as usize;
    let end = end_row.map(|e| e as usize + 1).unwrap_or(screen.height());
    let end = end.min(screen.height());

    for row_idx in start..end {
        let text: String = (0..screen.width())
            .map(|col| screen.get_cell(row_idx, col).map(|c| c.c).unwrap_or(' '))
            .collect();
        lines.push(text.trim_end().to_string());
    }

    lines
}

/// Apply a proto screen snapshot to a local terminal.
///
/// Restores full screen content including visible rows, scrollback,
/// cursor position, title, and terminal modes from the proto snapshot.
pub fn apply_screen_snapshot(terminal: &mut Terminal, screen_data: &proto::GetScreenResponse) {
    let screen = terminal.screen_mut();

    // Resize if needed
    if screen_data.cols > 0 && screen_data.rows > 0 {
        screen.resize(screen_data.cols as usize, screen_data.rows as usize);
    }

    // Restore visible rows
    for (row_idx, row) in screen_data.visible_rows.iter().enumerate() {
        for (col_idx, cell) in row.cells.iter().enumerate() {
            if let Some(grid_cell) = screen.grid_mut().get_mut(row_idx, col_idx) {
                grid_cell.c = cell.char.chars().next().unwrap_or(' ');
                if let Some(fg) = &cell.fg {
                    grid_cell.fg = proto_to_color(fg);
                }
                if let Some(bg) = &cell.bg {
                    grid_cell.bg = proto_to_color(bg);
                }
                if let Some(attrs) = &cell.attrs {
                    grid_cell.attrs = proto_to_attrs(attrs);
                }
            }
        }
    }

    // Restore scrollback
    if !screen_data.scrollback.is_empty() {
        use cterm_core::grid::Row;
        for proto_row in &screen_data.scrollback {
            let mut row = Row::new(screen_data.cols as usize);
            for (col_idx, cell) in proto_row.cells.iter().enumerate() {
                if let Some(grid_cell) = row.get_mut(col_idx) {
                    grid_cell.c = cell.char.chars().next().unwrap_or(' ');
                    if let Some(fg) = &cell.fg {
                        grid_cell.fg = proto_to_color(fg);
                    }
                    if let Some(bg) = &cell.bg {
                        grid_cell.bg = proto_to_color(bg);
                    }
                    if let Some(attrs) = &cell.attrs {
                        grid_cell.attrs = proto_to_attrs(attrs);
                    }
                }
            }
            screen.scrollback_mut().push_back(row);
        }
    }

    // Restore cursor
    if let Some(cursor) = &screen_data.cursor {
        screen.cursor.row = cursor.row as usize;
        screen.cursor.col = cursor.col as usize;
        screen.modes.show_cursor = cursor.visible;
    }

    // Restore title
    if !screen_data.title.is_empty() {
        screen.title = screen_data.title.clone();
    }

    // Restore terminal modes
    if let Some(modes) = &screen_data.modes {
        screen.modes.application_cursor = modes.application_cursor;
        screen.modes.application_keypad = modes.application_keypad;
        screen.modes.bracketed_paste = modes.bracketed_paste;
        screen.modes.focus_events = modes.focus_events;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_roundtrip() {
        let attrs = CellAttrs::BOLD | CellAttrs::ITALIC | CellAttrs::UNDERLINE;
        let proto = attrs_to_proto(attrs);
        let back = proto_to_attrs(&proto);
        assert_eq!(attrs, back);
    }

    #[test]
    fn test_cell_to_proto() {
        let cell = Cell::new('A');
        let proto = cell_to_proto(&cell);
        assert_eq!(proto.char, "A");
    }
}

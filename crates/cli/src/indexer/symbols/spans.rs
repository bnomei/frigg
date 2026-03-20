use super::*;

pub(crate) fn source_span_from_offsets(
    source: &str,
    start_byte: usize,
    end_byte: usize,
) -> SourceSpan {
    let start_byte = start_byte.min(source.len());
    let end_byte = end_byte.max(start_byte).min(source.len());
    let (start_line, start_column) = line_column_for_offset(source, start_byte);
    let (end_line, end_column) = line_column_for_offset(source, end_byte);
    SourceSpan {
        start_byte,
        end_byte,
        start_line,
        start_column,
        end_line,
        end_column,
    }
}

pub(crate) fn line_column_for_offset(source: &str, offset: usize) -> (usize, usize) {
    let clamped = offset.min(source.len());
    let bytes = source.as_bytes();
    let prefix = &bytes[..clamped];
    let line = prefix.iter().filter(|byte| **byte == b'\n').count() + 1;
    let line_start = prefix
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let column = clamped.saturating_sub(line_start) + 1;
    (line, column)
}

pub(crate) fn byte_offset_for_line_column(
    source: &str,
    line: usize,
    column: usize,
) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }
    let bytes = source.as_bytes();
    let mut current_line = 1usize;
    let mut line_start = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if current_line == line {
            let line_end = bytes[line_start..]
                .iter()
                .position(|candidate| *candidate == b'\n')
                .map(|offset| line_start + offset)
                .unwrap_or(bytes.len());
            let line_len = line_end.saturating_sub(line_start);
            let column_offset = column.saturating_sub(1).min(line_len);
            return Some(line_start + column_offset);
        }
        if *byte == b'\n' {
            current_line = current_line.saturating_add(1);
            line_start = index.saturating_add(1);
        }
    }
    if current_line == line {
        let line_len = bytes.len().saturating_sub(line_start);
        let column_offset = column.saturating_sub(1).min(line_len);
        return Some(line_start + column_offset);
    }
    None
}

pub(crate) fn source_span(node: Node<'_>) -> SourceSpan {
    let start = node.start_position();
    let end = node.end_position();
    SourceSpan {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_line: start.row + 1,
        start_column: start.column + 1,
        end_line: end.row + 1,
        end_column: end.column + 1,
    }
}

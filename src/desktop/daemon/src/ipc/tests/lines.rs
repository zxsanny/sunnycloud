use crate::ipc::lines::{CappedLines, LineTooLong};

#[test]
fn lines_split_across_chunks_come_out_whole() {
    let mut lines = CappedLines::new(16);

    lines.push(b"{\"a\":").unwrap();
    assert_eq!(lines.next_line(), None);
    lines.push(b"1}\n{\"b\"").unwrap();

    assert_eq!(lines.next_line(), Some(b"{\"a\":1}".to_vec()));
    assert_eq!(lines.next_line(), None);
}

#[test]
fn a_line_of_exactly_the_cap_is_accepted() {
    let mut lines = CappedLines::new(4);

    lines.push(b"abcd\n").unwrap();

    assert_eq!(lines.next_line(), Some(b"abcd".to_vec()));
}

#[test]
fn a_line_one_over_the_cap_is_refused_in_one_chunk() {
    let mut lines = CappedLines::new(4);

    assert_eq!(lines.push(b"abcde\n"), Err(LineTooLong));
}

#[test]
fn a_line_that_grows_past_the_cap_over_several_chunks_is_refused() {
    let mut lines = CappedLines::new(4);

    lines.push(b"ab").unwrap();
    lines.push(b"cd").unwrap();

    assert_eq!(lines.push(b"e"), Err(LineTooLong));
}

#[test]
fn the_cap_applies_per_line_not_per_stream() {
    let mut lines = CappedLines::new(4);

    for _ in 0..1000 {
        lines.push(b"abcd\n").unwrap();
    }

    assert_eq!(lines.next_line(), Some(b"abcd".to_vec()));
}

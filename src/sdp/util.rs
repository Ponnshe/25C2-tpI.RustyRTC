use std::fmt;
pub(crate) fn push_crlf(out: &mut String, args: fmt::Arguments) {
    use std::fmt::Write as _;
    let _ = out.write_fmt(args);
    let _ = out.write_str("\r\n");
}

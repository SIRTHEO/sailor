//! Which terminal **this** process is on.
//!
//! `ttyname` asks about a descriptor the process already holds: it runs
//! nothing, crosses no sandbox boundary, and cannot answer "empty" in place of
//! "denied". That is the difference from `ps`, and the reason the tracking
//! anchor starts here.

/// This process's tty, under the short name `ps` uses.
///
/// Standard error is tried first. A process invoked from a hook has its input
/// taken by the pipe carrying the payload, and often its output captured too:
/// descriptor 2 is the last one still attached to the window. This order is
/// what makes the real case work rather than the convenient one.
pub fn current() -> Option<String> {
    for descriptor in [libc::STDERR_FILENO, libc::STDOUT_FILENO, libc::STDIN_FILENO] {
        if let Some(found) = name_of(descriptor) {
            return Some(found);
        }
    }
    None
}

fn name_of(descriptor: i32) -> Option<String> {
    // `ttyname` writes into a static area of the process: copy the string at
    // once, before any other call could reuse it.
    let device = unsafe {
        let raw = libc::ttyname(descriptor);
        if raw.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned()
    };
    if device.is_empty() {
        return None;
    }
    Some(short_name(&device))
}

/// `/dev/ttys004` becomes `ttys004`.
///
/// Two names for one thing would be two keys: `ps` writes the short form,
/// `ttyname` the long one. If both reached the store as they come, one terminal
/// would have two rows and a detach would hold on only one of them.
pub fn short_name(device: &str) -> String {
    device.strip_prefix("/dev/").unwrap_or(device).to_owned()
}

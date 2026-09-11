//! Where a process is standing: the directory it would resolve a relative
//! path against, right now.
//!
//! Asked of the process, not remembered from the opening, because a shell
//! walks: a window showing the opening directory shows a place the person
//! standing in that terminal has long left.

use std::path::PathBuf;

/// The directory process `pid` is standing in, or nothing when it cannot be
/// asked — it has ended, or it belongs to somebody else.
pub fn directory_of(pid: u32) -> Option<PathBuf> {
    if pid == 0 {
        return None;
    }
    asked_of_the_system(pid)
}

#[cfg(target_os = "macos")]
fn asked_of_the_system(pid: u32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    let mut about: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let room = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    let filled = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            &mut about as *mut _ as *mut libc::c_void,
            room,
        )
    };
    if filled != room {
        return None;
    }
    let path: Vec<u8> = about
        .pvi_cdir
        .vip_path
        .iter()
        .flatten()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    if path.is_empty() {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(path)))
}

#[cfg(not(target_os = "macos"))]
fn asked_of_the_system(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This process knows where it is standing, and a number nobody runs
    /// under answers nothing rather than a directory it invented.
    #[test]
    fn a_living_process_says_where_it_stands_and_a_dead_one_says_nothing() {
        let here = directory_of(std::process::id()).expect("this process is standing somewhere");
        assert_eq!(
            here.canonicalize().ok(),
            std::env::current_dir().expect("a current directory").canonicalize().ok()
        );
        assert_eq!(directory_of(0), None);
    }
}

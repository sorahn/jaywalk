//! Best-effort same-user ancestry. Failure loses enrichment, never the supplied diff.
use std::path::PathBuf;
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Identity {
    pub pid: u32,
    pub started: u64,
    pub boot: [u64; 2],
}
pub(super) struct Process {
    pub identity: Identity,
    pub parent: u32,
    pub executable: PathBuf,
    pub arguments: Vec<String>,
}
pub(super) struct Invocation {
    pub identity: Identity,
    pub arguments: Vec<String>,
    pub file: Option<Vec<String>>,
}
pub(super) fn capture() -> (Option<Invocation>, Option<Vec<String>>) {
    // Metadata is only read from this launcher's ancestors, never a process scan.
    let mut pid = std::process::id();
    let mut file = None;
    for _ in 0..24 {
        let Some(process) = read(pid) else {
            break;
        };
        if process.executable.file_name().is_some_and(|n| n == "git")
            && command_index(&process.arguments).is_some_and(|i| process.arguments[i] == "difftool")
        {
            return (
                Some(Invocation {
                    identity: process.identity,
                    arguments: process.arguments,
                    file: file.clone(),
                }),
                file,
            );
        }
        if let Some(index) = process
            .arguments
            .iter()
            .position(|a| a.ends_with("/git-difftool--helper"))
        {
            let args = &process.arguments[index + 1..];
            if args.len() >= 7 {
                file = Some(args.to_vec());
            }
        }
        if process.parent <= 1 {
            break;
        }
        pid = process.parent;
    }
    (None, file)
}
#[cfg(target_os = "linux")]
pub(super) fn read(pid: u32) -> Option<Process> {
    use std::os::unix::fs::MetadataExt;
    let path = PathBuf::from(format!("/proc/{pid}"));
    let metadata = std::fs::metadata(&path).ok()?;
    // SAFETY: geteuid has no preconditions or borrowed memory.
    if metadata.uid() != unsafe { libc::geteuid() } {
        return None;
    }
    let stat = std::fs::read_to_string(path.join("stat")).ok()?;
    let fields: Vec<_> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let started = fields.get(19)?.parse().ok()?;
    let parent = fields.get(1)?.parse().ok()?;
    let executable = std::fs::read_link(path.join("exe")).ok()?;
    let bytes = std::fs::read(path.join("cmdline")).ok()?;
    let bytes = bytes.strip_suffix(&[0])?;
    let arguments = bytes
        .split(|b| *b == 0)
        .map(|a| String::from_utf8(a.to_vec()).ok())
        .collect::<Option<Vec<_>>>()?;
    let after = std::fs::read_to_string(path.join("stat")).ok()?;
    if after
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse::<u64>()
        .ok()?
        != started
    {
        return None;
    }
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()?
        .trim()
        .replace('-', "");
    if boot.len() != 32 {
        return None;
    }
    let boot = [
        u64::from_str_radix(&boot[..16], 16).ok()?,
        u64::from_str_radix(&boot[16..], 16).ok()?,
    ];
    Some(Process {
        identity: Identity { pid, started, boot },
        parent,
        executable,
        arguments,
    })
}
#[cfg(target_os = "macos")]
pub(super) fn read(pid: u32) -> Option<Process> {
    use std::mem::{MaybeUninit, size_of};
    let info = || {
        let mut info = MaybeUninit::<libc::proc_bsdinfo>::uninit();
        // SAFETY: proc_pidinfo writes at most the supplied size into a correctly
        // aligned buffer. Only a full successful write is assumed initialized.
        let count = unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size_of::<libc::proc_bsdinfo>() as i32,
            )
        };
        if count as usize != size_of::<libc::proc_bsdinfo>() {
            return None;
        }
        Some(unsafe { info.assume_init() })
    };
    let before = info()?;
    // SAFETY: geteuid has no preconditions.
    if before.pbi_uid != unsafe { libc::geteuid() } {
        return None;
    }
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
    let mut length = 0usize;
    // SAFETY: mib and length point to live, writable storage; null output requests size.
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
        || length > 1_048_576
    {
        return None;
    }
    let mut bytes = vec![0u8; length];
    // SAFETY: output has the queried capacity, with length passed to the kernel.
    if unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            bytes.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return None;
    }
    bytes.truncate(length);
    let argc = i32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?);
    if argc < 1 {
        return None;
    }
    let end = bytes.get(4..)?.iter().position(|b| *b == 0)? + 4;
    let executable = PathBuf::from(std::str::from_utf8(&bytes[4..end]).ok()?);
    let mut start = end + 1;
    while bytes.get(start) == Some(&0) {
        start += 1;
    }
    let arguments = bytes
        .get(start..)?
        .split(|b| *b == 0)
        .take(argc as usize)
        .map(|a| String::from_utf8(a.to_vec()).ok())
        .collect::<Option<Vec<_>>>()?;
    if arguments.len() != argc as usize {
        return None;
    }
    let after = info()?;
    if (before.pbi_start_tvsec, before.pbi_start_tvusec)
        != (after.pbi_start_tvsec, after.pbi_start_tvusec)
    {
        return None;
    }
    Some(Process {
        identity: Identity {
            pid,
            boot: [0, 0], // macOS creation time is an absolute wall-clock timestamp.
            started: before
                .pbi_start_tvsec
                .checked_mul(1_000_000)?
                .checked_add(before.pbi_start_tvusec)?,
        },
        parent: before.pbi_ppid,
        executable,
        arguments,
    })
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub(super) fn read(_: u32) -> Option<Process> {
    None
}

pub(super) fn command_index(args: &[String]) -> Option<usize> {
    let mut index = 1;
    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--config-env" => {
                index += 2
            }
            "--no-pager"
            | "--paginate"
            | "-P"
            | "-p"
            | "--literal-pathspecs"
            | "--no-optional-locks" => index += 1,
            a if [
                "--git-dir=",
                "--work-tree=",
                "--namespace=",
                "--config-env=",
            ]
            .iter()
            .any(|prefix| a.starts_with(prefix)) =>
            {
                index += 1
            }
            a if a.starts_with('-') => return None,
            _ => return Some(index),
        }
    }
    None
}

#[cfg(unix)]
pub(super) fn alive(identity: &Identity) -> Option<bool> {
    if let Some(process) = read(identity.pid) {
        return Some(process.identity == *identity);
    }
    // SAFETY: signal zero checks existence and sends no signal.
    if unsafe { libc::kill(identity.pid as i32, 0) } == 0 {
        return None;
    }
    (std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)).then_some(false)
}

#[cfg(not(unix))]
pub(super) fn alive(_: &Identity) -> Option<bool> {
    None
}

//! Check effective kernel restrictions before starting a browser, not just
//! options echoed by the container engine.
#[cfg(target_os = "linux")]
pub(super) fn ephemeral_auth_storage() -> bool {
    use std::mem::MaybeUninit;
    std::fs::read_to_string("/sys/fs/cgroup/memory.swap.max").is_ok_and(|v| v.trim() == "0")
        && [c"/tmp", c"/profiles"].iter().all(|path| {
            let mut stat = MaybeUninit::<libc::statfs>::uninit();
            // SAFETY: static terminated paths; read the structure only on success.
            unsafe {
                libc::statfs(path.as_ptr(), stat.as_mut_ptr()) == 0
                    && stat.assume_init().f_type == libc::TMPFS_MAGIC
            }
        })
}
#[cfg(not(target_os = "linux"))]
pub(super) fn ephemeral_auth_storage() -> bool {
    false
}

#[cfg(target_os = "linux")]
pub(super) fn enforced(google_auth: bool) -> bool {
    use std::{ffi::CStr, mem::MaybeUninit};
    fn mounted_with(path: &CStr, required: libc::c_ulong) -> bool {
        let mut stat = MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: static NUL-terminated paths and a valid output allocation.
        if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
            return false;
        }
        // SAFETY: statvfs initialized the output on success.
        unsafe { stat.assume_init().f_flag & required == required }
    }
    let restricted = libc::ST_NOEXEC | libc::ST_NOSUID;
    if !mounted_with(c"/", libc::ST_RDONLY)
        || ![c"/profiles", c"/tmp", c"/run"]
            .iter()
            .all(|path| mounted_with(path, restricted))
    {
        return false;
    }
    // A null clone3 argument cannot create a child. Require the explicit denial
    // used by the profile; libc can then use the separately filtered clone path.
    if unsafe { libc::syscall(libc::SYS_clone3, std::ptr::null::<u8>(), 0usize) } != -1
        || std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOSYS)
    {
        return false;
    }
    let read = |name| std::fs::read_to_string(format!("/sys/fs/cgroup/{name}")).ok();
    match (read("memory.max"), read("pids.max"), read("cpu.max")) {
        (Some(memory), Some(pids), Some(cpu)) => limits(&memory, &pids, &cpu, google_auth),
        _ => false,
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn enforced(_google_auth: bool) -> bool {
    false
}

// Authentication can hold an opener and a provider/verification page at once.
// Keep ordinary browsing at its existing bound and auth finite with no swap.
pub(super) const fn memory_limit_bytes(google_auth: bool) -> u64 {
    if google_auth {
        1536 * 1024 * 1024
    } else {
        768 * 1024 * 1024
    }
}

// Provider verification adds renderers and their threads to the same capsule.
// Keep their PID budget finite and separate from ordinary browsing.
pub(super) const fn process_limit(google_auth: bool) -> u64 {
    if google_auth { 512 } else { 256 }
}

#[cfg(any(target_os = "linux", test))]
fn limits(memory: &str, pids: &str, cpu: &str, google_auth: bool) -> bool {
    let bounded = |raw: &str, maximum| {
        raw.trim()
            .parse::<u64>()
            .is_ok_and(|v| v > 0 && v <= maximum)
    };
    let parts: Vec<_> = cpu.split_whitespace().collect();
    bounded(memory, memory_limit_bytes(google_auth))
        && bounded(pids, process_limit(google_auth))
        && parts.len() == 2
        && parts[1]
            .parse::<u64>()
            .is_ok_and(|period| period > 0 && bounded(parts[0], period))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auth_resources_are_bounded_without_relaxing_cpu_or_ordinary_limits() {
        assert!(limits("1610612736", "512", "100000 100000", true));
        assert!(limits("805306368", "256", "100000 100000", true));
        assert!(!limits("1610612736", "256", "100000 100000", false));
        assert!(!limits("805306368", "512", "100000 100000", false));
        for (memory, pids, cpu) in [
            ("1610612737", "256", "100000 100000"),
            ("max", "256", "100000 100000"),
            ("0", "256", "100000 100000"),
            ("1610612736", "513", "100000 100000"),
            ("1610612736", "0", "100000 100000"),
            ("1610612736", "max", "100000 100000"),
            ("1610612736", "256", "100001 100000"),
            ("1610612736", "256", "max 100000"),
        ] {
            assert!(!limits(memory, pids, cpu, true));
        }
    }
    #[test]
    fn missing_unlimited_or_excessive_kernel_limits_are_refused() {
        assert!(limits("805306368\n", "256\n", "100000 100000\n", false));
        assert!(limits("268435456", "64", "50000 100000", false));
        for (memory, pids, cpu) in [
            ("max", "256", "100000 100000"),
            ("805306369", "256", "100000 100000"),
            ("805306368", "max", "100000 100000"),
            ("805306368", "257", "100000 100000"),
            ("805306368", "256", "max 100000"),
            ("805306368", "256", "100001 100000"),
            ("0", "256", "100000 100000"),
            ("805306368", "256", "0 0"),
            ("805306368", "256", "100000 100000 extra"),
        ] {
            assert!(!limits(memory, pids, cpu, false));
        }
    }
}

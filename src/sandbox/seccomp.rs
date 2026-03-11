//! Seccomp-BPF system call filtering for sandboxes.
//!
//! Seccomp (Secure Computing Mode) restricts which system calls a sandboxed
//! process can invoke. This is the innermost security layer — applied last,
//! just before `execve`. Once installed, the filter cannot be removed or
//! weakened, even by root inside the sandbox.
//!
//! # Filter profiles
//!
//! - **Default**: allows ~80 commonly needed syscalls (file I/O, networking,
//!   process management, memory). Suitable for most workloads (Python, Node.js,
//!   shell scripts).
//! - **Strict**: allows only ~40 essential syscalls. No networking, no process
//!   creation beyond the initial exec. Suitable for pure computation tasks.
//!
//! # How it works
//!
//! 1. Build an allow-list of syscalls using `seccompiler`
//! 2. Set `PR_SET_NO_NEW_PRIVS` (required for unprivileged seccomp)
//! 3. Install the BPF filter via `seccomp(SECCOMP_SET_MODE_FILTER)`
//! 4. Any disallowed syscall triggers EPERM (or optionally SIGSYS/kill)
//!
//! The filter is inherited across `execve` and cannot be relaxed.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use seccompiler::{
    apply_filter_all_threads, BpfProgram, SeccompAction, SeccompFilter, SeccompRule, TargetArch,
};
use tracing::{debug, info};

/// Available seccomp filter profiles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SeccompProfile {
    /// Default profile: allows common syscalls for general workloads.
    #[default]
    Default,
    /// Strict profile: minimal syscalls for pure computation tasks.
    Strict,
    /// No seccomp filtering (not recommended for production).
    Disabled,
}

impl SeccompProfile {
    /// Parses a profile name from a string.
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "default" => Ok(Self::Default),
            "strict" => Ok(Self::Strict),
            "disabled" | "none" => Ok(Self::Disabled),
            _ => anyhow::bail!(
                "unknown seccomp profile '{}' — use default, strict, or disabled",
                s
            ),
        }
    }
}

/// Installs a seccomp-BPF filter on the calling thread.
///
/// **IMPORTANT**: This must be the last security step before `execve`.
/// The filter cannot be removed once installed.
///
/// Requires `PR_SET_NO_NEW_PRIVS` to be set first (which we do in
/// `drop_capabilities`).
pub fn install_seccomp_filter(profile: SeccompProfile) -> Result<()> {
    if profile == SeccompProfile::Disabled {
        info!("seccomp filtering disabled");
        return Ok(());
    }

    let filter = build_filter(profile).context("failed to build seccomp filter")?;

    apply_filter_all_threads(&filter).context("failed to install seccomp filter")?;

    info!(
        profile = ?profile,
        "seccomp filter installed"
    );

    Ok(())
}

/// Builds a seccomp BPF program for the given profile.
fn build_filter(profile: SeccompProfile) -> Result<BpfProgram> {
    let syscalls = match profile {
        SeccompProfile::Default => default_syscalls(),
        SeccompProfile::Strict => strict_syscalls(),
        SeccompProfile::Disabled => unreachable!(),
    };

    // Build the allow-list: each listed syscall maps to Allow.
    // Everything else gets the default action (Errno with EPERM).
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
    for &syscall_nr in &syscalls {
        rules.insert(syscall_nr, vec![SeccompRule::new(vec![]).unwrap()]);
    }

    let filter = SeccompFilter::new(
        rules,
        // Default action: return EPERM for any unlisted syscall.
        SeccompAction::Errno(libc::EPERM as u32),
        // Action for matching (allowed) syscalls: let them through.
        SeccompAction::Allow,
        TargetArch::x86_64,
    )
    .context("failed to create seccomp filter")?;

    let bpf: BpfProgram = filter
        .try_into()
        .map_err(|e| anyhow::anyhow!("failed to compile seccomp BPF: {e}"))?;

    debug!(
        profile = ?profile,
        num_syscalls = syscalls.len(),
        "seccomp filter compiled"
    );

    Ok(bpf)
}

/// Default profile: ~80 syscalls for general workloads.
///
/// Covers: file I/O, networking, process management, memory, signals,
/// time, and common infrastructure. Blocks dangerous syscalls like
/// `mount`, `reboot`, `kexec_load`, `init_module`, `ptrace`, etc.
fn default_syscalls() -> Vec<i64> {
    vec![
        // File I/O
        libc::SYS_read,
        libc::SYS_write,
        libc::SYS_open,
        libc::SYS_openat,
        libc::SYS_close,
        libc::SYS_stat,
        libc::SYS_fstat,
        libc::SYS_lstat,
        libc::SYS_newfstatat,
        libc::SYS_lseek,
        libc::SYS_pread64,
        libc::SYS_pwrite64,
        libc::SYS_readv,
        libc::SYS_writev,
        libc::SYS_access,
        libc::SYS_faccessat,
        libc::SYS_faccessat2,
        libc::SYS_dup,
        libc::SYS_dup2,
        libc::SYS_dup3,
        libc::SYS_fcntl,
        libc::SYS_flock,
        libc::SYS_fsync,
        libc::SYS_fdatasync,
        libc::SYS_truncate,
        libc::SYS_ftruncate,
        libc::SYS_getdents64,
        libc::SYS_getcwd,
        libc::SYS_chdir,
        libc::SYS_fchdir,
        libc::SYS_rename,
        libc::SYS_renameat,
        libc::SYS_renameat2,
        libc::SYS_mkdir,
        libc::SYS_mkdirat,
        libc::SYS_rmdir,
        libc::SYS_link,
        libc::SYS_linkat,
        libc::SYS_unlink,
        libc::SYS_unlinkat,
        libc::SYS_symlink,
        libc::SYS_symlinkat,
        libc::SYS_readlink,
        libc::SYS_readlinkat,
        libc::SYS_chmod,
        libc::SYS_fchmod,
        libc::SYS_fchmodat,
        libc::SYS_chown,
        libc::SYS_fchown,
        libc::SYS_fchownat,
        libc::SYS_umask,
        libc::SYS_statfs,
        libc::SYS_fstatfs,
        libc::SYS_utimensat,
        // Memory management
        libc::SYS_mmap,
        libc::SYS_mprotect,
        libc::SYS_munmap,
        libc::SYS_brk,
        libc::SYS_mremap,
        libc::SYS_madvise,
        libc::SYS_membarrier,
        // Process management
        libc::SYS_execve,
        libc::SYS_execveat,
        libc::SYS_exit,
        libc::SYS_exit_group,
        libc::SYS_wait4,
        libc::SYS_waitid,
        libc::SYS_clone,
        libc::SYS_clone3,
        libc::SYS_fork,
        libc::SYS_vfork,
        libc::SYS_getpid,
        libc::SYS_getppid,
        libc::SYS_gettid,
        libc::SYS_getuid,
        libc::SYS_getgid,
        libc::SYS_geteuid,
        libc::SYS_getegid,
        libc::SYS_getgroups,
        libc::SYS_setpgid,
        libc::SYS_getpgid,
        libc::SYS_getpgrp,
        libc::SYS_setsid,
        libc::SYS_prctl,
        libc::SYS_arch_prctl,
        libc::SYS_set_tid_address,
        libc::SYS_set_robust_list,
        libc::SYS_get_robust_list,
        libc::SYS_futex,
        libc::SYS_sched_yield,
        libc::SYS_sched_getaffinity,
        libc::SYS_sched_setaffinity,
        libc::SYS_rseq,
        // Signals
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_rt_sigsuspend,
        libc::SYS_kill,
        libc::SYS_tgkill,
        libc::SYS_sigaltstack,
        // Networking
        libc::SYS_socket,
        libc::SYS_connect,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_sendto,
        libc::SYS_recvfrom,
        libc::SYS_sendmsg,
        libc::SYS_recvmsg,
        libc::SYS_shutdown,
        libc::SYS_getsockname,
        libc::SYS_getpeername,
        libc::SYS_setsockopt,
        libc::SYS_getsockopt,
        libc::SYS_socketpair,
        // I/O multiplexing
        libc::SYS_poll,
        libc::SYS_ppoll,
        libc::SYS_select,
        libc::SYS_pselect6,
        libc::SYS_epoll_create1,
        libc::SYS_epoll_ctl,
        libc::SYS_epoll_wait,
        libc::SYS_epoll_pwait,
        libc::SYS_eventfd2,
        libc::SYS_timerfd_create,
        libc::SYS_timerfd_settime,
        libc::SYS_timerfd_gettime,
        libc::SYS_signalfd4,
        // Pipes
        libc::SYS_pipe,
        libc::SYS_pipe2,
        // Time
        libc::SYS_clock_gettime,
        libc::SYS_clock_getres,
        libc::SYS_clock_nanosleep,
        libc::SYS_gettimeofday,
        libc::SYS_nanosleep,
        // Misc
        libc::SYS_getrandom,
        libc::SYS_ioctl,
        libc::SYS_uname,
        libc::SYS_sysinfo,
        libc::SYS_prlimit64,
        libc::SYS_getrlimit,
        libc::SYS_setrlimit,
        libc::SYS_getrusage,
        libc::SYS_times,
        libc::SYS_close_range,
        libc::SYS_copy_file_range,
        libc::SYS_sendfile,
        libc::SYS_splice,
        libc::SYS_tee,
    ]
}

/// Strict profile: ~40 syscalls for pure computation.
///
/// No networking, no process creation (beyond initial exec).
/// Suitable for running untrusted code that only needs to compute and produce output.
fn strict_syscalls() -> Vec<i64> {
    vec![
        // File I/O (read-heavy, limited write)
        libc::SYS_read,
        libc::SYS_write,
        libc::SYS_open,
        libc::SYS_openat,
        libc::SYS_close,
        libc::SYS_stat,
        libc::SYS_fstat,
        libc::SYS_lstat,
        libc::SYS_newfstatat,
        libc::SYS_lseek,
        libc::SYS_pread64,
        libc::SYS_pwrite64,
        libc::SYS_readv,
        libc::SYS_writev,
        libc::SYS_access,
        libc::SYS_faccessat,
        libc::SYS_fcntl,
        libc::SYS_getdents64,
        libc::SYS_getcwd,
        libc::SYS_readlink,
        libc::SYS_readlinkat,
        libc::SYS_dup,
        libc::SYS_dup2,
        libc::SYS_dup3,
        // Memory management
        libc::SYS_mmap,
        libc::SYS_mprotect,
        libc::SYS_munmap,
        libc::SYS_brk,
        libc::SYS_mremap,
        libc::SYS_madvise,
        // Process (minimal — only exit and exec)
        libc::SYS_execve,
        libc::SYS_exit,
        libc::SYS_exit_group,
        libc::SYS_getpid,
        libc::SYS_gettid,
        libc::SYS_getuid,
        libc::SYS_getgid,
        libc::SYS_geteuid,
        libc::SYS_getegid,
        libc::SYS_set_tid_address,
        libc::SYS_set_robust_list,
        libc::SYS_futex,
        libc::SYS_arch_prctl,
        libc::SYS_prctl,
        libc::SYS_rseq,
        // Signals (minimal)
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_sigaltstack,
        // Time (read-only)
        libc::SYS_clock_gettime,
        libc::SYS_clock_getres,
        libc::SYS_gettimeofday,
        libc::SYS_nanosleep,
        libc::SYS_clock_nanosleep,
        // Misc
        libc::SYS_getrandom,
        libc::SYS_uname,
        libc::SYS_sysinfo,
        libc::SYS_prlimit64,
        libc::SYS_getrlimit,
        libc::SYS_ioctl,
        libc::SYS_close_range,
        // Pipe (needed for shell pipelines)
        libc::SYS_pipe,
        libc::SYS_pipe2,
        libc::SYS_poll,
        libc::SYS_sched_yield,
    ]
}

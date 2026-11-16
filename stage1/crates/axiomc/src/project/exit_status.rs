//! How a finished child process ended: exit code or terminating signal.

use std::process::ExitStatus;

/// The signal that terminated a child process, when the platform reports one.
fn terminating_signal(status: &ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        std::os::unix::process::ExitStatusExt::signal(status)
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

/// Exit code to report for a finished child: its own code, or the shell
/// convention `128 + signal` when it was killed by a signal. A signal death
/// must stay distinguishable from a program that chose to exit 1.
pub(super) fn run_exit_code(status: &ExitStatus) -> i32 {
    status
        .code()
        .or_else(|| terminating_signal(status).map(|signal| 128 + signal))
        .unwrap_or(1)
}

fn signal_name(signal: i32) -> String {
    #[cfg(unix)]
    {
        let name = match signal {
            libc::SIGHUP => "SIGHUP",
            libc::SIGINT => "SIGINT",
            libc::SIGQUIT => "SIGQUIT",
            libc::SIGILL => "SIGILL",
            libc::SIGTRAP => "SIGTRAP",
            libc::SIGABRT => "SIGABRT",
            libc::SIGBUS => "SIGBUS",
            libc::SIGFPE => "SIGFPE",
            libc::SIGKILL => "SIGKILL",
            libc::SIGUSR1 => "SIGUSR1",
            libc::SIGSEGV => "SIGSEGV",
            libc::SIGUSR2 => "SIGUSR2",
            libc::SIGPIPE => "SIGPIPE",
            libc::SIGALRM => "SIGALRM",
            libc::SIGTERM => "SIGTERM",
            libc::SIGSYS => "SIGSYS",
            _ => return format!("signal {signal}"),
        };
        format!("{name} ({signal})")
    }
    #[cfg(not(unix))]
    {
        format!("signal {signal}")
    }
}

fn signal_termination_message(binary: &str, signal: i32) -> String {
    format!(
        "run: {binary} was terminated by {}; exit code {}",
        signal_name(signal),
        128 + signal
    )
}

/// How a failed test process ended, for diagnostics.
pub(super) fn exit_description(status: &ExitStatus) -> String {
    match (status.code(), terminating_signal(status)) {
        (Some(code), _) => format!("exited with status {code}"),
        (None, Some(signal)) => format!("was terminated by {}", signal_name(signal)),
        (None, None) => "exited with status 1".to_owned(),
    }
}

/// Reports a signal death on axiomc's own stderr. `run --json` keeps its
/// envelope on stdout, so the signal is visible there only through the
/// conventional `128 + signal` exit code.
pub(super) fn report_signal_termination(binary: &str, status: &ExitStatus) {
    if let Some(signal) = terminating_signal(status) {
        eprintln!("{}", signal_termination_message(binary, signal));
    }
}

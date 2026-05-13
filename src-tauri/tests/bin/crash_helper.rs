//! Crash-safety helper: writes `bytes` to a temp sibling of `target` via
//! `cc_switch_lib::fs::atomic::write_temp_only`, fsyncs, then `abort()`s
//! before the rename can happen.
//!
//! Usage: `crash_helper <target> <payload>`
//!
//! The parent test then verifies the canonical `target` is untouched (or
//! absent if it never existed) — proving the atomic-write invariant under a
//! real subprocess crash, not the in-process pseudo-crash.

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let target = PathBuf::from(args.next().expect("arg1: target"));
    let payload = args.next().expect("arg2: payload");

    cc_switch_lib::fs::atomic::write_temp_only(&target, payload.as_bytes())
        .expect("write_temp_only");

    // Real crash. SIGKILL-equivalent: abort() bypasses unwinding and any
    // destructors that might attempt cleanup.
    std::process::abort();
}

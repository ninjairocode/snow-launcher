//! Integration tests for `ProcfsProcessTable` against a fake `/proc` tree.

use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::Path;

use snowlauncher_core::process::{Pid, ProcessTable, processes_in_prefix};
use snowlauncher_wine::procfs::ProcfsProcessTable;

fn fake_process(proc_root: &Path, pid: u32, comm: &str, environ: &[u8]) -> io::Result<()> {
    let dir = proc_root.join(pid.to_string());
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("comm"), format!("{comm}\n"))?;
    fs::write(dir.join("environ"), environ)
}

#[test]
fn lists_processes_and_finds_ours_through_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let proc_root = tmp.path().join("proc");
    let prefix = tmp.path().join("prefixes/battlenet");
    fs::create_dir_all(&prefix).unwrap();
    // umu/Proton may expose the prefix through a symlink such as `pfx`.
    let pfx_link = tmp.path().join("pfx");
    symlink(&prefix, &pfx_link).unwrap();

    let ours = format!("WINEPREFIX={}\0", pfx_link.display());
    fake_process(&proc_root, 200, "Battle.net.exe", ours.as_bytes()).unwrap();
    fake_process(&proc_root, 100, "Agent.exe", ours.as_bytes()).unwrap();
    fake_process(
        &proc_root,
        300,
        "Battle.net.exe",
        b"WINEPREFIX=/home/u/Games/lutris\0",
    )
    .unwrap();
    fake_process(&proc_root, 400, "bash", b"HOME=/home/u\0").unwrap();
    // Non-process entries in /proc must be ignored.
    fs::create_dir_all(proc_root.join("self")).unwrap();
    // A process that exited between listing and reading (no comm file).
    fs::create_dir_all(proc_root.join("500")).unwrap();

    let table = ProcfsProcessTable::with_root(&proc_root);
    let processes = table.processes().unwrap();

    let pids: Vec<u32> = processes.iter().map(|p| p.pid.0).collect();
    assert_eq!(pids, [100, 200, 300, 400]);
    assert_eq!(processes[0].name, "Agent.exe");

    let canonical_prefix = fs::canonicalize(&prefix).unwrap();
    let ours: Vec<Pid> = processes_in_prefix(&processes, &canonical_prefix)
        .map(|p| p.pid)
        .collect();
    assert_eq!(ours, [Pid(100), Pid(200)]);
}

#[test]
fn missing_proc_root_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let table = ProcfsProcessTable::with_root(tmp.path().join("nope"));
    assert!(table.processes().is_err());
}

#[test]
fn real_proc_contains_this_test() {
    let processes = ProcfsProcessTable::new().processes().unwrap();
    let me = std::process::id();
    assert!(processes.iter().any(|p| p.pid.0 == me));
}

#[test]
fn finds_only_the_processes_own_tcp_sockets() {
    use snowlauncher_core::tcp::SocketTable;

    let tmp = tempfile::tempdir().unwrap();
    let proc_root = tmp.path().join("proc");
    let dir = proc_root.join("4242");
    fs::create_dir_all(dir.join("fd")).unwrap();
    fs::create_dir_all(dir.join("net")).unwrap();
    // fd links: one TCP socket of ours, a pipe, a regular file.
    symlink("socket:[1001]", dir.join("fd/10")).unwrap();
    symlink("pipe:[2002]", dir.join("fd/11")).unwrap();
    symlink("/dev/null", dir.join("fd/12")).unwrap();
    let header = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n";
    fs::write(
        dir.join("net/tcp"),
        format!(
            "{header}\
   0: 0F02000A:C5A2 52861022:045F 01 00000000:00000000 00:00000000 00000000  1000        0 1001 1 0 20 4 30 10 -1\n\
   1: 0F02000A:C5A3 52861022:045F 01 00000000:00000000 00:00000000 00000000  1000        0 9999 1 0 20 4 30 10 -1\n"
        ),
    )
    .unwrap();
    // No tcp6 file: IPv6 disabled must not be an error.

    let table = ProcfsProcessTable::with_root(&proc_root);
    let sockets = table.tcp_sockets(Pid(4242)).unwrap().unwrap();
    let inodes: Vec<u64> = sockets.iter().map(|s| s.inode).collect();
    assert_eq!(inodes, [1001]);

    // A process that is gone gives None, not an error.
    assert!(table.tcp_sockets(Pid(1)).unwrap().is_none());
}

#[test]
fn real_proc_lists_tcp_sockets_of_this_process() {
    use snowlauncher_core::tcp::SocketTable;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let table = ProcfsProcessTable::new();
    let sockets = table.tcp_sockets(Pid(std::process::id())).unwrap().unwrap();
    // 0x0A = TCP_LISTEN
    assert!(sockets.iter().any(|s| s.state == 0x0A));
    drop(listener);
}

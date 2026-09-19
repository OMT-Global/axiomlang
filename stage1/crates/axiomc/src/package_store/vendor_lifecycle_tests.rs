use super::*;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Barrier};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.is_file() {
        assert!(Instant::now() < deadline, "child did not execute its selected test");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn empty_lock_file_is_not_stolen_and_identity_persists() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join(VENDOR_LIFECYCLE_LOCK_FILE);
    File::create(&path).unwrap(); // no owner marker, including before initialization
    let held = VendorLifecycleLock::acquire(temp.path()).unwrap();
    let started = Instant::now();
    let contender = VendorLifecycleLock::acquire(temp.path());
    assert!(matches!(contender, Err(StoreError { code: "vendor_lifecycle_busy", .. })));
    assert!(started.elapsed() < Duration::from_secs(5), "contention must remain bounded");
    assert!(path.is_file());
    drop(held);
    assert!(path.is_file(), "release must not unlink the shared lock identity");
    let _next = VendorLifecycleLock::acquire(temp.path()).unwrap();
}

#[test]
fn concurrent_lock_acquisitions_are_exclusive() {
    let temp = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let active = Arc::new(AtomicUsize::new(0));
    let mut threads = Vec::new();
    for _ in 0..4 {
        let root = temp.path().to_path_buf();
        let barrier = barrier.clone();
        let active = active.clone();
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            let _lock = VendorLifecycleLock::acquire(&root).unwrap();
            assert_eq!(active.fetch_add(1, AtomicOrdering::SeqCst), 0, "two writers acquired the lifecycle lock");
            std::thread::sleep(Duration::from_millis(25));
            assert_eq!(active.fetch_sub(1, AtomicOrdering::SeqCst), 1);
        }));
    }
    for thread in threads { thread.join().unwrap(); }
}

#[test]
fn crashed_owner_releases_lock_without_age_delay() {
    let temp = tempfile::tempdir().unwrap();
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "package_store::vendor_lifecycle_tests::crashed_owner_child", "--nocapture"])
        .env("AXIOM_VENDOR_CRASH_CHILD_ROOT", temp.path())
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let mut child = ChildGuard(child);
    wait_for_file(&temp.path().join("ready"));
    assert!(matches!(VendorLifecycleLock::acquire(temp.path()), Err(StoreError { code: "vendor_lifecycle_busy", .. })));
    fs::write(temp.path().join("exit-now"), b"exit").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() { break status; }
        assert!(Instant::now() < deadline, "crash child did not exit");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(status.code(), Some(37), "child must exit without dropping the Rust lock guard");
    let started = Instant::now();
    let _recovered = VendorLifecycleLock::acquire(temp.path()).unwrap();
    assert!(started.elapsed() < Duration::from_secs(2), "dead owner must not impose an age delay");
}

#[test]
fn crashed_owner_child() {
    let Some(root) = std::env::var_os("AXIOM_VENDOR_CRASH_CHILD_ROOT") else { return; };
    let root = PathBuf::from(root);
    let _lock = VendorLifecycleLock::acquire(&root).unwrap();
    fs::write(root.join("ready"), b"held").unwrap();
    wait_for_file(&root.join("exit-now"));
    std::process::exit(37); // deliberately bypasses destructors, like abrupt owner death
}

#[test]
fn uninitialized_legacy_directory_is_never_reclaimed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join(VENDOR_LIFECYCLE_LOCK_FILE);
    fs::create_dir(&path).unwrap();
    assert!(VendorLifecycleLock::acquire(temp.path()).is_err());
    assert!(path.is_dir(), "must not steal an uninitialized directory lock");
}

#[cfg(unix)]
#[test]
fn lock_refuses_symlink_and_hardlink_without_touching_target() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let outside = temp.path().join("outside");
    fs::write(&outside, b"sentinel").unwrap();
    let root = temp.path().join("vendor");
    fs::create_dir(&root).unwrap();
    let path = root.join(VENDOR_LIFECYCLE_LOCK_FILE);
    symlink(&outside, &path).unwrap();
    assert!(VendorLifecycleLock::acquire(&root).is_err());
    fs::remove_file(&path).unwrap();
    fs::hard_link(&outside, &path).unwrap();
    assert!(VendorLifecycleLock::acquire(&root).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"sentinel");
}

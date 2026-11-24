//! What the tests of the readers of a tree share: a pipe made in a tree, and a read
//! that is held to end. It holds no test, so that a verification over a file that
//! uses it names it whole, and the edit of a test elsewhere makes nothing stale here.
//! A pipe is made where the system has one (unix).

#![cfg(unix)]

use std::path::Path;
use std::process::Stdio;

use tokio::process::Command;

/// A pipe at `at` under `dir`, when the system can make one (`mkfifo`): opened for
/// reading, it would wait for a writer, so a test that ends shows it was not opened.
pub(crate) async fn pipe(dir: &Path, at: &str) -> bool {
    let full = dir.join(at);
    if let Some(parent) = full.parent() {
        tokio::fs::create_dir_all(parent).await.unwrap();
    }
    Command::new("mkfifo")
        .arg(full)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|s| s.success())
}

/// What `read` gives, when it ends. A read that opened the pipe at `pipe` would wait
/// for a writer: after a while the pipe is opened for writing, which lets that read
/// end, and the test fails rather than waits.
pub(crate) async fn ends<F: std::future::Future>(pipe: &Path, read: F) -> F::Output {
    let wait = std::time::Duration::from_secs(30);
    match tokio::time::timeout(wait, read).await {
        Ok(read) => read,
        Err(_) => {
            let mut writer = tokio::fs::OpenOptions::new();
            writer.write(true);
            let _ = tokio::time::timeout(wait, writer.open(pipe)).await;
            panic!(
                "{} was opened, and the read waited for a writer",
                pipe.display()
            );
        }
    }
}

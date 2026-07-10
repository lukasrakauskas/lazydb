use anyhow::Result;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use super::Connection;

/// A local SSH port forward managed as a subprocess. Killed on drop.
/// ponytail: shells out to the system `ssh` command instead of embedding
/// an SSH library (no new deps). Works on any system with OpenSSH installed.
/// upgrade: `russh` crate for embedded SSH when Windows/embedded users need
/// it without the external dependency.
#[allow(dead_code)]
pub struct SshTunnel {
    child: Option<Child>,
    pub local_port: u16,
}

impl SshTunnel {
    /// Start an SSH tunnel for the given connection. Returns a modified
    /// connection with host=127.0.0.1 and port=local_port, along with the
    /// tunnel handle. The caller must keep the handle alive.
    pub fn start(conn: &Connection) -> Result<(Connection, Self)> {
        let local_port = pick_free_port()?;
        let mut cmd = Command::new("ssh");
        cmd.args([
            "-N",
            "-L",
            &format!("{}:{}:{}", local_port, conn.host, conn.port),
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "ExitOnForwardFailure=yes",
            "-p",
            &conn.ssh_port.to_string(),
        ]);
        if !conn.ssh_keyfile.is_empty() {
            cmd.arg("-i").arg(&conn.ssh_keyfile);
        }
        cmd.arg(format!("{}@{}", conn.ssh_user, conn.ssh_host));
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
        let mut child = cmd.spawn()?;

        // Give the tunnel a moment to establish.
        std::thread::sleep(Duration::from_millis(500));

        // If the process already exited, the tunnel failed.
        if let Some(status) = child.try_wait()? {
            return Err(anyhow::anyhow!(
                "SSH tunnel failed to start (exit: {status})"
            ));
        }

        let mut tuned = conn.clone();
        tuned.host = "127.0.0.1".into();
        tuned.port = local_port;

        Ok((
            tuned,
            Self {
                child: Some(child),
                local_port,
            },
        ))
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Find a free TCP port on localhost by binding to port 0.
fn pick_free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

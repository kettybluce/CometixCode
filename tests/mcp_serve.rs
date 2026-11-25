//! Real stdio protocol and file-side-effect tests; no model or network needed.
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

struct Server {
    child: Child,
    lines: Receiver<String>,
    root: PathBuf,
    next_id: u64,
}

impl Server {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("cometix-mcp-serve-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("home")).unwrap();
        fs::create_dir_all(root.join("project")).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_cometix"))
            .args(["mcp", "serve"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root.join("home"))
            .env("USERPROFILE", root.join("home"))
            .env("CLAUDE_CONFIG_DIR", root.join("home/config"))
            .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
            .env("DISABLE_TELEMETRY", "1")
            .env("DISABLE_AUTOUPDATER", "1")
            .env("COMETIX_WRITE_ENABLED", "1")
            .current_dir(root.join("project"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(fs::File::create(root.join("stderr.log")).unwrap())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        let mut server = Self {
            child,
            lines,
            root,
            next_id: 0,
        };
        let init = server.request(
            "initialize",
            json!({
                "protocolVersion": "2025-03-26", "capabilities": {},
                "clientInfo": {"name": "cometix-contract", "version": "1"}
            }),
        );
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(
            init["result"]["capabilities"]["tools"].is_object(),
            "{init}"
        );
        server.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        server
    }

    fn send(&mut self, message: Value) {
        let input = self.child.stdin.as_mut().unwrap();
        writeln!(input, "{message}").unwrap();
        input.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        self.send(
            json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params}),
        );
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|error| {
                    panic!(
                        "{method}: {error}; stderr: {}",
                        fs::read_to_string(self.root.join("stderr.log")).unwrap_or_default()
                    )
                });
            // Any non-JSON stdout output (including a TUI/log) fails the test.
            let response: Value =
                serde_json::from_str(&line).expect("stdout must contain only MCP JSON");
            if response["id"] == self.next_id {
                return response;
            }
        }
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))
    }

    fn raw_output(response: &Value) -> Value {
        assert!(response.get("error").is_none(), "{response}");
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    fn close(&mut self) {
        drop(self.child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "{status}");
                assert!(fs::read(self.root.join("stderr.log")).unwrap().is_empty());
                return;
            }
            assert!(
                Instant::now() < deadline,
                "MCP server did not exit after EOF"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(target_os = "linux")]
struct OwnedShell {
    pid: i32,
    start_time: String,
}

#[cfg(target_os = "linux")]
impl OwnedShell {
    fn process_start(pid: i32) -> Option<String> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // /proc stat's comm field may contain spaces. Field 22 follows the
        // closing parenthesis and state (field 3), not whitespace token 22.
        Some(
            stat.rsplit_once(')')?
                .1
                .split_whitespace()
                .nth(19)?
                .to_string(),
        )
    }

    fn start(server: &mut Server) -> Self {
        server.next_id += 1;
        server.send(json!({
            "jsonrpc": "2.0", "id": server.next_id, "method": "tools/call",
            "params": {"name": "Bash", "arguments": {
                "command": "printf '%s' $$ > shell.pid; exec sleep 30",
                "timeout": 30000,
                "description": "MCP cancellation contract fixture"
            }}
        }));
        let pid_file = server.root.join("project/shell.pid");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(pid) = fs::read_to_string(&pid_file) {
                if let Ok(pid) = pid.trim().parse::<i32>() {
                    if let Some(start_time) = Self::process_start(pid) {
                        return Self { pid, start_time };
                    }
                }
            }
            assert!(
                Instant::now() < deadline,
                "Bash did not reach the real shell"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn assert_reaped(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Self::process_start(self.pid).as_ref() == Some(&self.start_time) {
            assert!(Instant::now() < deadline, "MCP left its shell child alive");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for OwnedShell {
    fn drop(&mut self) {
        // Failure cleanup is restricted to the PID and kernel start time
        // obtained from this fixture. Never kill by a process-name pattern.
        if Self::process_start(self.pid).as_ref() == Some(&self.start_time) {
            unsafe {
                libc::kill(self.pid, libc::SIGKILL);
            }
        }
    }
}

#[test]
fn stdio_lists_real_tools_and_preserves_read_edit_write_state() {
    let mut server = Server::new();
    let listed = server.request("tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    for name in ["Bash", "Read", "Edit", "Write", "NotebookEdit"] {
        let schema = tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(schema["inputSchema"]["type"], "object");
        assert!(!schema["description"].as_str().unwrap().is_empty());
    }
    let path = server.root.join("project/中文文件.txt");
    fs::write(&path, "MCP_ORIGINAL\nsecond line\n").unwrap();
    let read = server.call("Read", json!({"file_path": path}));
    assert_eq!(
        Server::raw_output(&read)["file"]["content"],
        "MCP_ORIGINAL\nsecond line\n"
    );
    let edit = server.call(
        "Edit",
        json!({
            "file_path": path, "old_string": "MCP_ORIGINAL", "new_string": "MCP_EDITED"
        }),
    );
    assert_eq!(Server::raw_output(&edit)["newString"], "MCP_EDITED");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "MCP_EDITED\nsecond line\n"
    );
    let written = server.root.join("project/written.txt");
    let write = server.call(
        "Write",
        json!({"file_path": written, "content": "MCP_WRITTEN\n"}),
    );
    assert_eq!(Server::raw_output(&write)["type"], "create");
    assert_eq!(fs::read_to_string(written).unwrap(), "MCP_WRITTEN\n");
    let reread = server.call("Read", json!({"file_path": path}));
    assert_eq!(
        Server::raw_output(&reread)["file"]["content"],
        "MCP_EDITED\nsecond line\n"
    );
    server.close();
}

#[test]
fn stdio_errors_do_not_close_transport_or_write_invalid_input() {
    let mut server = Server::new();
    let unknown = server.call("NoSuchTool", json!({}));
    assert_eq!(unknown["error"]["code"], -32603);
    assert_eq!(unknown["error"]["message"], "Tool NoSuchTool not found");
    let invalid = server.call(
        "Write",
        json!({"file_path": 42, "content": "must not write"}),
    );
    assert_eq!(invalid["error"]["code"], -32602);
    assert_eq!(
        fs::read_dir(server.root.join("project")).unwrap().count(),
        0
    );
    let missing = server.call(
        "Read",
        json!({"file_path": server.root.join("project/missing.txt")}),
    );
    assert_eq!(missing["error"]["code"], -32603);
    let ping = server.request("ping", json!({}));
    assert_eq!(ping["result"], json!({}));
    server.close();
}

#[test]
fn serve_help_and_unknown_arguments_exit_without_starting_protocol() {
    let help = Command::new(env!("CARGO_BIN_EXE_cometix"))
        .args(["mcp", "serve", "--help"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(
        String::from_utf8_lossy(&help.stdout).starts_with("Usage: cometix mcp serve [options]")
    );
    let invalid = Command::new(env!("CARGO_BIN_EXE_cometix"))
        .args(["mcp", "serve", "--bogus"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
}

#[test]
#[cfg(target_os = "linux")]
fn stdio_cancellation_reaps_shell_and_keeps_connection_usable() {
    let mut server = Server::new();
    let shell = OwnedShell::start(&mut server);
    server.send(
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled",
                       "params": {"requestId": server.next_id, "reason": "contract cancellation"}}),
    );
    let ping = server.request("ping", json!({}));
    assert_eq!(ping["result"], json!({}));
    shell.assert_reaped();
    server.close();
}

#[test]
#[cfg(target_os = "linux")]
fn stdio_eof_reaps_an_active_shell() {
    let mut server = Server::new();
    let shell = OwnedShell::start(&mut server);
    server.close();
    shell.assert_reaped();
}

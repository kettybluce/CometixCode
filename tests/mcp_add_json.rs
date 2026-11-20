use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const FAKE_TOKEN: &str = "cometix-stdin-test-token";
const STDIO_CONFIG: &str = r#"{"command":"example-mcp-server"}"#;

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    cwd: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("cometix-mcp-add-json-{}", uuid::Uuid::new_v4()));
        let repo = root.join("repo");
        let fixture = Self {
            cwd: repo.join("nested"),
            home: root.join("config"),
            repo,
            root,
        };
        fs::create_dir_all(fixture.repo.join(".git")).unwrap();
        fs::create_dir_all(&fixture.cwd).unwrap();
        fs::create_dir(&fixture.home).unwrap();
        fixture
    }

    fn global_file(&self) -> PathBuf {
        self.home.join(".claude.json")
    }

    fn project_file(&self) -> PathBuf {
        self.cwd.join(".mcp.json")
    }

    fn run(&self, args: &[&str], stdin: Option<&[u8]>, env: &[(&str, &str)]) -> Output {
        self.run_command(&["mcp", "add-json"], args, stdin, env)
    }

    fn run_command(
        &self,
        command_args: &[&str],
        args: &[&str],
        stdin: Option<&[u8]>,
        env: &[(&str, &str)],
    ) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cometix"));
        command
            .args(command_args)
            .args(args)
            .current_dir(&self.cwd)
            .env_clear()
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("CLAUDE_CONFIG_DIR", &self.home)
            .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
            .envs(env.iter().copied());

        let Some(input) = stdin else {
            return command.stdin(Stdio::null()).output().unwrap();
        };
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn assert_exit(output: Output, success: bool) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains(FAKE_TOKEN) && !stderr.contains(FAKE_TOKEN),
        "the CLI exposed the token in its output"
    );
    assert_eq!(
        output.status.success(),
        success,
        "unexpected status {}\nstdout: {stdout}\nstderr: {stderr}",
        output.status
    );
    if !success {
        assert!(
            !stderr.trim().is_empty(),
            "failure did not include a diagnostic"
        );
    }
}

#[test]
fn stdin_adds_to_local_scope_under_git_root() {
    let fixture = Fixture::new();
    let path = fixture.global_file();
    let project_key = fixture
        .repo
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let mut expected = json!({
        "otherToolSetting": true,
        "mcpServers": {"global-docs": {"command": "existing-server"}},
        "projects": {
            (project_key.as_str()): {
                "otherProjectSetting": [1, 2],
                "mcpServers": {"existing": {"command": "existing-local-server"}}
            }
        }
    });
    fs::write(&path, serde_json::to_vec(&expected).unwrap()).unwrap();

    let server = json!({
        "type": "http",
        "url": "https://example.invalid/mcp",
        "headers": {"Authorization": format!("Bearer {FAKE_TOKEN}")}
    });
    let input = format!("\n{server:#}\n");
    assert_exit(
        fixture.run(
            &["docs", "-"],
            Some(input.as_bytes()),
            &[("CLAUDECODE", "1")],
        ),
        true,
    );

    expected["projects"][project_key.as_str()]["mcpServers"]["docs"] = server;
    assert_eq!(read_json(&path), expected);
    assert!(!fixture.project_file().exists());
}

#[test]
fn inline_json_adds_to_user_and_project_scopes() {
    let fixture = Fixture::new();
    assert_exit(
        fixture.run(&["stdio-docs", STDIO_CONFIG, "--scope=user"], None, &[]),
        true,
    );
    let global_path = fixture.global_file();
    let global_before_project_add = fs::read(&global_path).unwrap();
    let global = read_json(&global_path);
    assert_eq!(
        global["mcpServers"]["stdio-docs"]["command"],
        "example-mcp-server"
    );
    assert_eq!(global["mcpServers"]["stdio-docs"]["args"], json!([]));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&global_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    let project_path = fixture.project_file();
    let mut expected = json!({
        "$schema": "https://example.invalid/schema.json",
        "servers": {"other-cli": {"command": "other-tool"}},
        "mcpServers": {"existing": {"command": "existing-server"}}
    });
    fs::write(&project_path, serde_json::to_vec(&expected).unwrap()).unwrap();
    assert_exit(
        fixture.run(
            &[
                "-s",
                "project",
                "http-docs",
                r#"{"type":"streamable-http","url":"https://example.invalid/mcp"}"#,
            ],
            None,
            &[],
        ),
        true,
    );
    expected["mcpServers"]["http-docs"] =
        json!({"type": "http", "url": "https://example.invalid/mcp"});
    assert_eq!(read_json(&project_path), expected);
    assert!(!fixture.repo.join(".mcp.json").exists());
    assert_eq!(fs::read(global_path).unwrap(), global_before_project_add);
}

#[test]
fn empty_and_oversized_stdin_are_rejected_without_creating_config() {
    let fixture = Fixture::new();
    assert_exit(fixture.run(&["docs", "-"], Some(b" \n\t"), &[]), false);
    assert!(!fixture.global_file().exists());

    let oversized = vec![b' '; 1024 * 1024 + 1];
    assert_exit(fixture.run(&["docs", "-"], Some(&oversized), &[]), false);
    assert!(!fixture.global_file().exists());
}

#[test]
fn refused_adds_leave_existing_config_bytes_unchanged() {
    let fixture = Fixture::new();
    let project_path = fixture.project_file();
    let global_path = fixture.global_file();
    let project = b"{\"mcpServers\":{\"existing\":{\"command\":\"original\"}}}\n";
    let global = b"{\"otherToolSetting\":true}\n";
    fs::write(&project_path, project).unwrap();
    fs::write(&global_path, global).unwrap();

    let malformed_input = format!(r#"{{"headers":{{"Authorization":"{FAKE_TOKEN}""#);
    assert_exit(
        fixture.run(
            &["-s", "project", "docs", "-"],
            Some(malformed_input.as_bytes()),
            &[],
        ),
        false,
    );
    for (name, payload) in [
        ("existing", STDIO_CONFIG),
        (
            "docs",
            r#"{"type":"unsupported","command":"example-mcp-server"}"#,
        ),
    ] {
        assert_exit(
            fixture.run(&["-s", "project", name, payload], None, &[]),
            false,
        );
    }
    for scope in ["local", "user", "project"] {
        assert_exit(
            fixture.run(
                &["-s", scope, "docs", STDIO_CONFIG],
                None,
                &[("COMETIX_WRITE_ENABLED", "0")],
            ),
            false,
        );
    }
    assert_eq!(fs::read(project_path).unwrap(), project);
    assert_eq!(fs::read(global_path).unwrap(), global);
}

#[test]
fn corrupt_config_is_not_replaced() {
    let fixture = Fixture::new();
    let project_path = fixture.project_file();
    let global_path = fixture.global_file();
    let invalid_project = b"{ broken project JSON";
    let invalid_global = b"{ broken global JSON";
    fs::write(&project_path, invalid_project).unwrap();
    fs::write(&global_path, invalid_global).unwrap();

    for scope in ["local", "user", "project"] {
        assert_exit(
            fixture.run(&["-s", scope, "docs", STDIO_CONFIG], None, &[]),
            false,
        );
    }
    assert_eq!(fs::read(project_path).unwrap(), invalid_project);
    assert_eq!(fs::read(global_path).unwrap(), invalid_global);
}

#[test]
fn invalid_existing_server_prevents_project_config_mutation() {
    let fixture = Fixture::new();
    let project_path = fixture.project_file();
    let original = br#"{"$schema":"https://example.invalid/schema.json","servers":{},"mcpServers":{"broken":{"type":"http"}}}"#;
    fs::write(&project_path, original).unwrap();

    assert_exit(
        fixture.run(
            &[
                "new-server",
                r#"{"command":"example-mcp-server"}"#,
                "--scope=project",
            ],
            None,
            &[],
        ),
        false,
    );
    assert_eq!(fs::read(project_path).unwrap(), original);
}

#[test]
fn mcp_list_reports_empty_configuration() {
    let fixture = Fixture::new();
    let output = fixture.run_command(&["mcp", "list"], &[], None, &[]);
    assert_exit(output, true);
}

#[test]
fn mcp_list_reports_health_and_pending_project_servers_without_secrets() {
    let fixture = Fixture::new();
    let secret = "list-secret-token";
    fs::write(
        fixture.global_file(),
        serde_json::to_vec(&json!({
            "mcpServers": {
                "broken": {
                    "command": "/bin/sh",
                    "args": ["-c", "exit 1"],
                    "env": {"TOKEN": secret}
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        fixture.project_file(),
        serde_json::to_vec(&json!({
            "mcpServers": {
                "pending-docs": {
                    "type": "http",
                    "url": "https://example.invalid/mcp?token=project-secret"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let output = fixture.run_command(&["mcp", "list"], &[], None, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stdout={stdout}\nstderr={stderr}");
    assert!(stdout.contains("broken (user) - failed"), "stdout={stdout}");
    assert!(
        stdout.contains("pending-docs (project) - pending approval"),
        "stdout={stdout}"
    );
    assert!(!stdout.contains(secret));
    assert!(!stdout.contains("project-secret"));
    assert!(!stderr.contains(secret));
    assert!(!stderr.contains("project-secret"));
}

#[test]
fn mcp_list_does_not_print_failed_http_url_with_credentials() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break Some(stream),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("HTTP fixture accept failed: {error}"),
            }
        };

        if let Some(stream) = stream.as_mut() {
            stream
                .write_all(
                    b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        }
        stream.is_some()
    });

    let fixture = Fixture::new();
    let secret = "mcp-list-query-token-sentinel";
    let url = format!("http://{address}/mcp?access_token={secret}");
    fs::write(
        fixture.global_file(),
        serde_json::to_vec(&json!({
            "mcpServers": {
                "remote": {
                    "type": "http",
                    "url": url
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let output = fixture.run_command(&["mcp", "list"], &[], None, &[]);
    assert!(
        server.join().unwrap(),
        "MCP client did not reach the HTTP fixture"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stdout={stdout}\nstderr={stderr}");
    assert!(stdout.contains("remote (user) - failed"), "stdout={stdout}");
    assert!(!stdout.contains(secret));
    assert!(!stdout.contains(&url));
    assert!(!stderr.contains(secret));
    assert!(!stderr.contains(&url));
}

#[test]
fn mcp_get_reports_stdio_details_and_health() {
    let fixture = Fixture::new();
    fs::write(
        fixture.global_file(),
        serde_json::to_vec(&json!({
            "mcpServers": {
                "probe": {
                    "command": "/bin/false",
                    "args": ["sentinel-arg"],
                    "env": {"SENTINEL": "sentinel-value"},
                    "timeout": 500
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let output = fixture.run_command(&["mcp", "get", "probe"], &[], None, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stdout={stdout}\nstderr={stderr}");
    assert!(stdout.contains("probe:"), "stdout={stdout}");
    assert!(
        stdout.contains("Scope: User config (available in all your projects)"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("Status: ✘ Failed to connect"),
        "stdout={stdout}"
    );
    assert!(
        stdout.lines().any(|line| line.starts_with("  Issue: ")),
        "stdout={stdout}"
    );
    assert!(stdout.contains("Type: stdio"), "stdout={stdout}");
    assert!(stdout.contains("Command: /bin/false"), "stdout={stdout}");
    assert!(stdout.contains("Args: sentinel-arg"), "stdout={stdout}");
    assert!(
        stdout.contains("SENTINEL=sentinel-value"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("Timeout: 500ms (ignored: below 1000ms minimum)"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("To remove this server, run: claude mcp remove probe -s user"),
        "stdout={stdout}"
    );
    assert!(stderr.is_empty(), "stderr={stderr}");
}

#[test]
fn mcp_get_reports_normal_timeout() {
    let fixture = Fixture::new();
    fs::write(
        fixture.global_file(),
        serde_json::to_vec(&json!({
            "mcpServers": {
                "probe": {
                    "command": "/bin/false",
                    "timeout": 1500
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let output = fixture.run_command(&["mcp", "get", "probe"], &[], None, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stdout={stdout}\nstderr={stderr}");
    assert!(stdout.contains("Timeout: 1500ms"), "stdout={stdout}");
    assert!(!stdout.contains("below 1000ms minimum"), "stdout={stdout}");
    assert!(stderr.is_empty(), "stderr={stderr}");
}

#[test]
fn mcp_get_reports_pending_project_server_without_connecting() {
    let fixture = Fixture::new();
    fs::write(
        fixture.project_file(),
        serde_json::to_vec(&json!({
            "mcpServers": {
                "project-probe": {"command": "/bin/false"}
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let output = fixture.run_command(&["mcp", "get", "project-probe"], &[], None, &[]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stdout={stdout}\nstderr={stderr}");
    assert!(
        stdout.contains("Scope: Project config (shared via .mcp.json)"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("Status: ⏸ Pending approval (run `claude` to approve)"),
        "stdout={stdout}"
    );
    assert!(!stdout.contains("Failed to connect"), "stdout={stdout}");
    assert!(stderr.is_empty(), "stderr={stderr}");
}

#[test]
fn mcp_get_redacts_failed_remote_issue_but_keeps_details() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break Some(stream),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("HTTP fixture accept failed: {error}"),
            }
        };

        if let Some(stream) = stream.as_mut() {
            stream
                .write_all(
                    b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        }
        stream.is_some()
    });

    let fixture = Fixture::new();
    let secret = "mcp-get-query-token-sentinel";
    let header_secret = "mcp-get-header-token-sentinel";
    let url = format!("http://{address}/mcp?access_token={secret}");
    fs::write(
        fixture.global_file(),
        serde_json::to_vec(&json!({
            "mcpServers": {
                "remote": {
                    "type": "http",
                    "url": url,
                    "headers": {"Authorization": format!("Bearer {header_secret}")}
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let output = fixture.run_command(&["mcp", "get", "remote"], &[], None, &[]);
    assert!(
        server.join().unwrap(),
        "MCP client did not reach the HTTP fixture"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stdout={stdout}\nstderr={stderr}");
    assert!(
        stdout.contains("Status: ✘ Failed to connect"),
        "stdout={stdout}"
    );
    let issue = stdout
        .lines()
        .find(|line| line.starts_with("  Issue: "))
        .expect("failed remote server should report an Issue");
    assert!(!issue.contains(secret), "issue leaked query token: {issue}");
    assert!(!issue.contains(&url), "issue leaked full URL: {issue}");
    assert!(stdout.contains(&format!("URL: {url}")), "stdout={stdout}");
    assert!(
        stdout.contains(&format!("Authorization: Bearer {header_secret}")),
        "stdout={stdout}"
    );
    assert!(stderr.is_empty(), "stderr={stderr}");
}

#[test]
fn mcp_get_reports_missing_server() {
    let fixture = Fixture::new();
    let output = fixture.run_command(&["mcp", "get", "missing"], &[], None, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("No MCP server named \"missing\". Run \"claude mcp add\" to add one."),
        "stderr={stderr}"
    );
}

#[test]
fn mcp_remove_deletes_user_server_and_preserves_other_configuration() {
    let fixture = Fixture::new();
    fs::write(
        fixture.global_file(),
        serde_json::to_vec(&json!({
            "otherSetting": {"keep": true},
            "mcpServers": {
                "remove-me": {"command": "/bin/false"},
                "keep-me": {"command": "/bin/false"}
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let original_project = json!({
        "otherProjectSetting": true,
        "mcpServers": {"project-keep": {"command": "/bin/false"}}
    });
    fs::write(
        fixture.project_file(),
        serde_json::to_vec(&original_project).unwrap(),
    )
    .unwrap();

    let output = fixture.run_command(
        &["mcp", "remove"],
        &["remove-me", "ignored-extra", "--scope="],
        None,
        &[],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stdout={stdout}\nstderr={stderr}");
    assert!(
        stdout.contains("Removed MCP server \"remove-me\" from user config"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("File modified:"), "stdout={stdout}");
    let saved = read_json(&fixture.global_file());
    assert_eq!(saved["otherSetting"]["keep"], true);
    assert!(saved["mcpServers"].get("remove-me").is_none());
    assert!(saved["mcpServers"].get("keep-me").is_some());
    assert_eq!(read_json(&fixture.project_file()), original_project);
}

#[test]
fn mcp_remove_deletes_project_and_local_scopes_explicitly() {
    let fixture = Fixture::new();
    fs::write(
        fixture.global_file(),
        serde_json::to_vec(&json!({
            "projects": {
                (fixture.repo.canonicalize().unwrap().to_string_lossy().as_ref()): {
                    "otherProjectState": 7,
                    "mcpServers": {
                        "local-me": {"command": "/bin/false"},
                        "local-keep": {"command": "/bin/false"}
                    }
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        fixture.project_file(),
        serde_json::to_vec(&json!({
            "otherProjectFileSetting": "keep",
            "mcpServers": {
                "project-me": {"command": "/bin/false"},
                "project-keep": {"command": "/bin/false"}
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let local = fixture.run_command(
        &["mcp", "remove"],
        &["local-me", "--scope", "local"],
        None,
        &[],
    );
    assert!(
        local.status.success(),
        "{}",
        String::from_utf8_lossy(&local.stderr)
    );
    assert!(String::from_utf8_lossy(&local.stdout).contains("from local config"));
    let project_key = fixture
        .repo
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let global = read_json(&fixture.global_file());
    assert_eq!(global["projects"][&project_key]["otherProjectState"], 7);
    assert!(
        global["projects"][&project_key]["mcpServers"]
            .get("local-me")
            .is_none()
    );
    assert!(
        global["projects"][&project_key]["mcpServers"]
            .get("local-keep")
            .is_some()
    );

    let project = fixture.run_command(
        &["mcp", "remove"],
        &["project-me", "-s", "project"],
        None,
        &[],
    );
    let stdout = String::from_utf8_lossy(&project.stdout);
    assert!(project.status.success(), "{stdout}");
    assert!(stdout.contains("from project config"), "{stdout}");
    let saved_project = read_json(&fixture.project_file());
    assert_eq!(saved_project["otherProjectFileSetting"], "keep");
    assert!(saved_project["mcpServers"].get("project-me").is_none());
    assert!(saved_project["mcpServers"].get("project-keep").is_some());
}

#[test]
fn mcp_remove_without_scope_refuses_ambiguous_server_without_mutation() {
    let fixture = Fixture::new();
    let project_key = fixture
        .repo
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let global_original = json!({
        "mcpServers": {"same": {"command": "/bin/false"}},
        "projects": {
            (project_key.as_str()): {
                "mcpServers": {"same": {"command": "/bin/false"}}
            }
        }
    });
    fs::write(
        &fixture.global_file(),
        serde_json::to_vec(&global_original).unwrap(),
    )
    .unwrap();
    let project_original = json!({
        "mcpServers": {"same": {"command": "/bin/false"}}
    });
    fs::write(
        &fixture.project_file(),
        serde_json::to_vec(&project_original).unwrap(),
    )
    .unwrap();
    let global_bytes = fs::read(fixture.global_file()).unwrap();
    let project_bytes = fs::read(fixture.project_file()).unwrap();

    let output = fixture.run_command(&["mcp", "remove"], &["same"], None, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        stderr.contains("MCP server \"same\" exists in multiple scopes:"),
        "{stderr}"
    );
    assert!(
        stderr.contains("- Local config (private to you in this project)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("- Project config (shared via .mcp.json)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("- User config (available in all your projects)"),
        "{stderr}"
    );
    assert_eq!(fs::read(fixture.global_file()).unwrap(), global_bytes);
    assert_eq!(fs::read(fixture.project_file()).unwrap(), project_bytes);
}

#[test]
fn mcp_remove_reports_missing_invalid_scope_and_managed_protection() {
    let fixture = Fixture::new();

    let missing = fixture.run_command(&["mcp", "remove"], &["missing"], None, &[]);
    assert!(!missing.status.success());
    assert_eq!(
        String::from_utf8_lossy(&missing.stderr).trim(),
        "No MCP server named \"missing\". Run `claude mcp add` to add one."
    );

    let invalid = fixture.run_command(&["mcp", "remove"], &["missing", "-s", "bogus"], None, &[]);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr)
        .contains("Invalid scope: bogus. Must be one of: local, user, project, dynamic, enterprise, claudeai, managed, agent"));

    let managed = fixture.run_command(&["mcp", "remove"], &["missing", "-s", "managed"], None, &[]);
    assert!(!managed.status.success());
    assert!(
        String::from_utf8_lossy(&managed.stderr)
            .contains("Cannot remove MCP server from scope: managed")
    );
}

#[test]
fn mcp_remove_suggests_close_names_and_lists_unrelated_servers() {
    let fixture = Fixture::new();
    fs::write(
        fixture.global_file(),
        serde_json::to_vec(&json!({
            "mcpServers": {
                "existing": {"command": "/bin/false"},
                "docs": {"command": "/bin/false"}
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let original = fs::read(fixture.global_file()).unwrap();

    let typo = fixture.run_command(&["mcp", "remove"], &["exiting"], None, &[]);
    assert!(!typo.status.success());
    assert_eq!(
        String::from_utf8_lossy(&typo.stderr).trim(),
        "No MCP server named \"exiting\". Did you mean \"existing\"? Run `claude mcp list` to see all."
    );

    let unrelated = fixture.run_command(&["mcp", "remove"], &["xyz"], None, &[]);
    assert!(!unrelated.status.success());
    assert_eq!(
        String::from_utf8_lossy(&unrelated.stderr).trim(),
        "No MCP server named \"xyz\". Configured servers: docs, existing"
    );
    assert_eq!(fs::read(fixture.global_file()).unwrap(), original);
}

#[test]
fn mcp_remove_matches_cli_argument_error_boundaries() {
    let fixture = Fixture::new();

    let missing_name = fixture.run_command(&["mcp", "remove"], &[], None, &[]);
    assert!(!missing_name.status.success());
    assert_eq!(
        String::from_utf8_lossy(&missing_name.stderr).trim(),
        "error: missing required argument 'name'"
    );

    let missing_scope = fixture.run_command(&["mcp", "remove"], &["missing", "--scope"], None, &[]);
    assert!(!missing_scope.status.success());
    assert_eq!(
        String::from_utf8_lossy(&missing_scope.stderr).trim(),
        "error: option '-s, --scope <scope>' argument missing"
    );

    let unknown_option =
        fixture.run_command(&["mcp", "remove"], &["missing", "--bogus"], None, &[]);
    assert!(!unknown_option.status.success());
    assert_eq!(
        String::from_utf8_lossy(&unknown_option.stderr).trim(),
        "error: unknown option '--bogus'"
    );
}

#[test]
fn mcp_remove_respects_disabled_write_gate() {
    let fixture = Fixture::new();
    fs::write(
        fixture.global_file(),
        serde_json::to_vec(&json!({"mcpServers": {"probe": {"command": "/bin/false"}}})).unwrap(),
    )
    .unwrap();
    let original = fs::read(fixture.global_file()).unwrap();

    let output = fixture.run_command(
        &["mcp", "remove"],
        &["probe"],
        None,
        &[("COMETIX_WRITE_ENABLED", "0")],
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("configuration writes are disabled by COMETIX_WRITE_ENABLED=0")
    );
    assert_eq!(fs::read(fixture.global_file()).unwrap(), original);
}

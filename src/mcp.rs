use std::io::{BufRead, Write};
use std::path::Path;

use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ssh_config::model::{SshConfigFile, is_host_pattern};

/// A JSON-RPC 2.0 request.
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

impl JsonRpcResponse {
    fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<Value>, code: i64, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

/// Helper to build an MCP tool result (success).
fn mcp_tool_result(text: &str) -> Value {
    serde_json::json!({
        "content": [{"type": "text", "text": text}]
    })
}

/// Helper to build an MCP tool error result.
fn mcp_tool_error(text: &str) -> Value {
    serde_json::json!({
        "content": [{"type": "text", "text": text}],
        "isError": true
    })
}

/// Verify that an alias exists in the SSH config. Returns error Value if not found.
fn verify_alias_exists(alias: &str, config_path: &Path) -> Result<(), Value> {
    let config = match SshConfigFile::parse(config_path) {
        Ok(c) => c,
        Err(e) => return Err(mcp_tool_error(&format!("Failed to parse SSH config: {e}"))),
    };
    let exists = config.host_entries().iter().any(|h| h.alias == alias);
    if !exists {
        return Err(mcp_tool_error(&format!("Host not found: {alias}")));
    }
    Ok(())
}

/// Run an SSH command with a timeout. Returns (exit_code, stdout, stderr).
fn ssh_exec(
    alias: &str,
    config_path: &Path,
    command: &str,
    timeout_secs: u64,
) -> Result<(i32, String, String), Value> {
    let config_str = config_path.to_string_lossy();
    let mut child = match std::process::Command::new("ssh")
        .args([
            "-F",
            &config_str,
            "-o",
            "ConnectTimeout=10",
            "-o",
            "BatchMode=yes",
            "--",
            alias,
            command,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return Err(mcp_tool_error(&format!("Failed to spawn ssh: {e}"))),
    };

    let timeout = std::time::Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child
                    .stdout
                    .take()
                    .map(|mut s| {
                        let mut buf = String::new();
                        std::io::Read::read_to_string(&mut s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();
                let stderr = child
                    .stderr
                    .take()
                    .map(|mut s| {
                        let mut buf = String::new();
                        std::io::Read::read_to_string(&mut s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();
                return Ok((status.code().unwrap_or(-1), stdout, stderr));
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(mcp_tool_error(&format!(
                        "SSH command timed out after {timeout_secs} seconds"
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(mcp_tool_error(&format!("Failed to wait for ssh: {e}"))),
        }
    }
}

/// Dispatch a JSON-RPC method to the appropriate handler.
pub(crate) fn dispatch(method: &str, params: Option<Value>, config_path: &Path) -> JsonRpcResponse {
    match method {
        "initialize" => handle_initialize(),
        "tools/list" => handle_tools_list(),
        "tools/call" => handle_tools_call(params, config_path),
        _ => JsonRpcResponse::error(None, -32601, format!("Method not found: {method}")),
    }
}

fn handle_initialize() -> JsonRpcResponse {
    JsonRpcResponse::success(
        None,
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "purple",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

fn handle_tools_list() -> JsonRpcResponse {
    let tools = serde_json::json!({
        "tools": [
            {
                "name": "list_hosts",
                "description": "List all SSH hosts available to connect to. Returns alias, hostname, user, port, tags and provider for each host. Use the tag parameter to filter by tag, provider tag or provider name (fuzzy match). Call this first to discover available hosts.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "tag": {
                            "type": "string",
                            "description": "Filter hosts by tag (fuzzy match against tags, provider_tags and provider name)"
                        }
                    }
                }
            },
            {
                "name": "get_host",
                "description": "Get detailed information for a single SSH host including identity file, proxy jump, provider metadata, password source and tunnel count.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "alias": {
                            "type": "string",
                            "description": "The host alias to look up"
                        }
                    },
                    "required": ["alias"]
                }
            },
            {
                "name": "run_command",
                "description": "Run a shell command on a remote host via SSH. Non-interactive (BatchMode). Returns exit code, stdout and stderr. Suitable for diagnostic commands, not interactive programs.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "alias": {
                            "type": "string",
                            "description": "The host alias to connect to"
                        },
                        "command": {
                            "type": "string",
                            "description": "The command to execute"
                        },
                        "timeout": {
                            "type": "integer",
                            "description": "Timeout in seconds (default 30)",
                            "default": 30,
                            "minimum": 1,
                            "maximum": 300
                        }
                    },
                    "required": ["alias", "command"]
                }
            },
            {
                "name": "list_containers",
                "description": "List all Docker or Podman containers on a remote host via SSH. Auto-detects the container runtime. Returns container ID, name, image, state, status and ports.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "alias": {
                            "type": "string",
                            "description": "The host alias to list containers for"
                        }
                    },
                    "required": ["alias"]
                }
            },
            {
                "name": "container_action",
                "description": "Start, stop or restart a Docker or Podman container on a remote host via SSH. Auto-detects the container runtime.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "alias": {
                            "type": "string",
                            "description": "The host alias"
                        },
                        "container_id": {
                            "type": "string",
                            "description": "The container ID or name"
                        },
                        "action": {
                            "type": "string",
                            "description": "The action to perform",
                            "enum": ["start", "stop", "restart"]
                        }
                    },
                    "required": ["alias", "container_id", "action"]
                }
            }
        ]
    });
    JsonRpcResponse::success(None, tools)
}

fn handle_tools_call(params: Option<Value>, config_path: &Path) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None => {
            return JsonRpcResponse::error(
                None,
                -32602,
                "Invalid params: missing params object".to_string(),
            );
        }
    };

    let tool_name = match params.get("name").and_then(|n| n.as_str()) {
        Some(n) => n,
        None => {
            return JsonRpcResponse::error(
                None,
                -32602,
                "Invalid params: missing tool name".to_string(),
            );
        }
    };

    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    let result = match tool_name {
        "list_hosts" => tool_list_hosts(&args, config_path),
        "get_host" => tool_get_host(&args, config_path),
        "run_command" => tool_run_command(&args, config_path),
        "list_containers" => tool_list_containers(&args, config_path),
        "container_action" => tool_container_action(&args, config_path),
        _ => mcp_tool_error(&format!("Unknown tool: {tool_name}")),
    };

    JsonRpcResponse::success(None, result)
}

fn tool_list_hosts(args: &Value, config_path: &Path) -> Value {
    let config = match SshConfigFile::parse(config_path) {
        Ok(c) => c,
        Err(e) => return mcp_tool_error(&format!("Failed to parse SSH config: {e}")),
    };

    let entries = config.host_entries();
    let tag_filter = args.get("tag").and_then(|t| t.as_str());

    let hosts: Vec<Value> = entries
        .iter()
        .filter(|entry| {
            // Skip host patterns (already filtered by host_entries, but be safe)
            if is_host_pattern(&entry.alias) {
                return false;
            }

            // Apply tag filter (fuzzy: substring match on tags, provider_tags, provider name)
            if let Some(tag) = tag_filter {
                let tag_lower = tag.to_lowercase();
                let matches_tags = entry
                    .tags
                    .iter()
                    .any(|t| t.to_lowercase().contains(&tag_lower));
                let matches_provider_tags = entry
                    .provider_tags
                    .iter()
                    .any(|t| t.to_lowercase().contains(&tag_lower));
                let matches_provider = entry
                    .provider
                    .as_ref()
                    .is_some_and(|p| p.to_lowercase().contains(&tag_lower));
                if !matches_tags && !matches_provider_tags && !matches_provider {
                    return false;
                }
            }

            true
        })
        .map(|entry| {
            serde_json::json!({
                "alias": entry.alias,
                "hostname": entry.hostname,
                "user": entry.user,
                "port": entry.port,
                "tags": entry.tags,
                "provider": entry.provider,
                "stale": entry.stale.is_some(),
            })
        })
        .collect();

    let json_str = serde_json::to_string_pretty(&hosts).unwrap_or_default();
    mcp_tool_result(&json_str)
}

fn tool_get_host(args: &Value, config_path: &Path) -> Value {
    let alias = match args.get("alias").and_then(|a| a.as_str()) {
        Some(a) => a,
        None => return mcp_tool_error("Missing required parameter: alias"),
    };

    let config = match SshConfigFile::parse(config_path) {
        Ok(c) => c,
        Err(e) => return mcp_tool_error(&format!("Failed to parse SSH config: {e}")),
    };

    let entries = config.host_entries();
    let entry = entries.iter().find(|e| e.alias == alias);

    match entry {
        Some(entry) => {
            let meta: serde_json::Map<String, Value> = entry
                .provider_meta
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect();

            let host = serde_json::json!({
                "alias": entry.alias,
                "hostname": entry.hostname,
                "user": entry.user,
                "port": entry.port,
                "identity_file": entry.identity_file,
                "proxy_jump": entry.proxy_jump,
                "tags": entry.tags,
                "provider_tags": entry.provider_tags,
                "provider": entry.provider,
                "provider_meta": meta,
                "askpass": entry.askpass,
                "tunnel_count": entry.tunnel_count,
                "stale": entry.stale.is_some(),
            });

            let json_str = serde_json::to_string_pretty(&host).unwrap_or_default();
            mcp_tool_result(&json_str)
        }
        None => mcp_tool_error(&format!("Host not found: {alias}")),
    }
}

fn tool_run_command(args: &Value, config_path: &Path) -> Value {
    let alias = match args.get("alias").and_then(|a| a.as_str()) {
        Some(a) if !a.is_empty() => a,
        _ => return mcp_tool_error("Missing required parameter: alias"),
    };
    let command = match args.get("command").and_then(|c| c.as_str()) {
        Some(c) if !c.is_empty() => c,
        _ => return mcp_tool_error("Missing required parameter: command"),
    };
    let timeout_secs = args.get("timeout").and_then(|t| t.as_u64()).unwrap_or(30);

    if let Err(e) = verify_alias_exists(alias, config_path) {
        return e;
    }

    info!("MCP tool: ssh_exec alias={alias} command={command}");
    match ssh_exec(alias, config_path, command, timeout_secs) {
        Ok((exit_code, stdout, stderr)) => {
            if exit_code != 0 {
                error!("[external] MCP ssh_exec failed: alias={alias} exit={exit_code}");
            }
            let result = serde_json::json!({
                "exit_code": exit_code,
                "stdout": stdout,
                "stderr": stderr
            });
            let json_str = serde_json::to_string_pretty(&result).unwrap_or_default();
            mcp_tool_result(&json_str)
        }
        Err(e) => e,
    }
}

fn tool_list_containers(args: &Value, config_path: &Path) -> Value {
    let alias = match args.get("alias").and_then(|a| a.as_str()) {
        Some(a) if !a.is_empty() => a,
        _ => return mcp_tool_error("Missing required parameter: alias"),
    };

    if let Err(e) = verify_alias_exists(alias, config_path) {
        return e;
    }

    // Build the combined detection + listing command
    let command = crate::containers::container_list_command(None);

    let (exit_code, stdout, stderr) = match ssh_exec(alias, config_path, &command, 30) {
        Ok(r) => r,
        Err(e) => return e,
    };

    if exit_code != 0 {
        return mcp_tool_error(&format!("SSH command failed: {}", stderr.trim()));
    }

    match crate::containers::parse_container_output(&stdout, None) {
        Ok((runtime, containers)) => {
            let containers_json: Vec<Value> = containers
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "id": c.id,
                        "name": c.names,
                        "image": c.image,
                        "state": c.state,
                        "status": c.status,
                        "ports": c.ports,
                    })
                })
                .collect();
            let result = serde_json::json!({
                "runtime": runtime.as_str(),
                "containers": containers_json,
            });
            let json_str = serde_json::to_string_pretty(&result).unwrap_or_default();
            mcp_tool_result(&json_str)
        }
        Err(e) => mcp_tool_error(&e),
    }
}

fn tool_container_action(args: &Value, config_path: &Path) -> Value {
    let alias = match args.get("alias").and_then(|a| a.as_str()) {
        Some(a) if !a.is_empty() => a,
        _ => return mcp_tool_error("Missing required parameter: alias"),
    };
    let container_id = match args.get("container_id").and_then(|c| c.as_str()) {
        Some(c) if !c.is_empty() => c,
        _ => return mcp_tool_error("Missing required parameter: container_id"),
    };
    let action_str = match args.get("action").and_then(|a| a.as_str()) {
        Some(a) => a,
        None => return mcp_tool_error("Missing required parameter: action"),
    };

    // Validate container ID (injection prevention)
    if let Err(e) = crate::containers::validate_container_id(container_id) {
        return mcp_tool_error(&e);
    }

    let action = match action_str {
        "start" => crate::containers::ContainerAction::Start,
        "stop" => crate::containers::ContainerAction::Stop,
        "restart" => crate::containers::ContainerAction::Restart,
        _ => {
            return mcp_tool_error(&format!(
                "Invalid action: {action_str}. Must be start, stop or restart"
            ));
        }
    };

    if let Err(e) = verify_alias_exists(alias, config_path) {
        return e;
    }

    // First detect runtime
    let detect_cmd = crate::containers::container_list_command(None);

    let (detect_exit, detect_stdout, _detect_stderr) =
        match ssh_exec(alias, config_path, &detect_cmd, 30) {
            Ok(r) => r,
            Err(e) => return e,
        };

    if detect_exit != 0 {
        return mcp_tool_error("Failed to detect container runtime");
    }

    let runtime = match crate::containers::parse_container_output(&detect_stdout, None) {
        Ok((rt, _)) => rt,
        Err(e) => return mcp_tool_error(&format!("Failed to detect container runtime: {e}")),
    };

    let action_command = crate::containers::container_action_command(runtime, action, container_id);

    let (action_exit, _action_stdout, action_stderr) =
        match ssh_exec(alias, config_path, &action_command, 30) {
            Ok(r) => r,
            Err(e) => return e,
        };

    if action_exit == 0 {
        let result = serde_json::json!({
            "success": true,
            "message": format!("Container {container_id} {}ed", action_str),
        });
        let json_str = serde_json::to_string_pretty(&result).unwrap_or_default();
        mcp_tool_result(&json_str)
    } else {
        mcp_tool_error(&format!(
            "Container action failed: {}",
            action_stderr.trim()
        ))
    }
}

/// Run the MCP server, reading JSON-RPC requests from stdin and writing
/// responses to stdout. Blocks until stdin is closed.
pub fn run(config_path: &Path) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let reader = stdin.lock();
    let mut writer = stdout.lock();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(_) => {
                let resp = JsonRpcResponse::error(None, -32700, "Parse error".to_string());
                let json = serde_json::to_string(&resp)?;
                writeln!(writer, "{json}")?;
                writer.flush()?;
                continue;
            }
        };

        // Notifications (no id) don't get responses
        if request.id.is_none() {
            debug!("MCP notification: {}", request.method);
            continue;
        }

        debug!("MCP request: method={}", request.method);
        let mut response = dispatch(&request.method, request.params, config_path);
        debug!(
            "MCP response: method={} success={}",
            request.method,
            response.error.is_none()
        );
        response.id = request.id;

        let json = serde_json::to_string(&response)?;
        writeln!(writer, "{json}")?;
        writer.flush()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Task 1: JSON-RPC types and parsing ---

    #[test]
    fn parse_valid_request() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "initialize");
        assert_eq!(req.id, Some(Value::Number(1.into())));
    }

    #[test]
    fn parse_notification_no_id() {
        let json = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert!(req.id.is_none());
        assert!(req.params.is_none());
    }

    #[test]
    fn parse_invalid_json() {
        let result: Result<JsonRpcRequest, _> = serde_json::from_str("not json");
        assert!(result.is_err());
    }

    #[test]
    fn response_success_serialization() {
        let resp = JsonRpcResponse::success(Some(Value::Number(1.into())), Value::Bool(true));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""result":true"#));
        assert!(!json.contains("error"));
    }

    #[test]
    fn response_error_serialization() {
        let resp = JsonRpcResponse::error(
            Some(Value::Number(1.into())),
            -32601,
            "Method not found".to_string(),
        );
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("-32601"));
        assert!(!json.contains("result"));
    }

    // --- Task 2: MCP initialize and tools/list handlers ---

    #[test]
    fn test_handle_initialize() {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "1.0"}
        });
        let resp = dispatch(
            "initialize",
            Some(params),
            &std::path::PathBuf::from("/dev/null"),
        );
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(result["serverInfo"]["name"], "purple");
    }

    #[test]
    fn test_handle_tools_list() {
        let resp = dispatch("tools/list", None, &std::path::PathBuf::from("/dev/null"));
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 5);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"list_hosts"));
        assert!(names.contains(&"get_host"));
        assert!(names.contains(&"run_command"));
        assert!(names.contains(&"list_containers"));
        assert!(names.contains(&"container_action"));
    }

    #[test]
    fn test_handle_unknown_method() {
        let resp = dispatch("bogus/method", None, &std::path::PathBuf::from("/dev/null"));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    // --- Task 3: list_hosts and get_host tool handlers ---

    #[test]
    fn tool_list_hosts_returns_all_concrete_hosts() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "list_hosts", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0]["alias"], "web-1");
        assert_eq!(hosts[1]["alias"], "db-1");
    }

    #[test]
    fn tool_list_hosts_filter_by_tag() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"tag": "database"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "list_hosts", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0]["alias"], "db-1");
    }

    #[test]
    fn tool_get_host_found() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "web-1"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "get_host", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let host: Value = serde_json::from_str(text).unwrap();
        assert_eq!(host["alias"], "web-1");
        assert_eq!(host["hostname"], "10.0.1.5");
        assert_eq!(host["user"], "deploy");
        assert_eq!(host["identity_file"], "~/.ssh/id_ed25519");
        assert_eq!(host["provider"], "aws");
    }

    #[test]
    fn tool_get_host_not_found() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "nonexistent"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "get_host", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn tool_get_host_missing_alias() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "get_host", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    // --- Task 4: run_command tool handler ---

    #[test]
    fn tool_run_command_missing_alias() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"command": "uptime"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "run_command", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn tool_run_command_missing_command() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "web-1"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "run_command", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn tool_run_command_empty_alias() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "", "command": "uptime"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "run_command", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn tool_run_command_empty_command() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "web-1", "command": ""});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "run_command", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    // --- Task 5: list_containers and container_action tool handlers ---

    #[test]
    fn tool_list_containers_missing_alias() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "list_containers", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn tool_container_action_missing_fields() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "web-1", "action": "start"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "container_action", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn tool_container_action_invalid_action() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args =
            serde_json::json!({"alias": "web-1", "container_id": "abc", "action": "destroy"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "container_action", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn tool_container_action_invalid_container_id() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "web-1", "container_id": "abc;rm -rf /", "action": "start"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "container_action", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    // --- Protocol-level tests ---

    #[test]
    fn tools_call_missing_params() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let resp = dispatch("tools/call", None, &config_path);
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("missing params"));
    }

    #[test]
    fn tools_call_missing_tool_name() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"arguments": {}})),
            &config_path,
        );
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("missing tool name"));
    }

    #[test]
    fn tools_call_unknown_tool() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "nonexistent_tool", "arguments": {}})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
        assert!(
            result["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Unknown tool")
        );
    }

    #[test]
    fn tools_call_name_is_number_not_string() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": 42, "arguments": {}})),
            &config_path,
        );
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn tools_call_no_arguments_field() {
        // arguments defaults to {} when missing
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "list_hosts"})),
            &config_path,
        );
        let result = resp.result.unwrap();
        // Should succeed - list_hosts with no args returns all hosts
        assert!(result.get("isError").is_none());
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(hosts.len(), 2);
    }

    // --- list_hosts additional tests ---

    #[test]
    fn tool_list_hosts_empty_config() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_empty_config");
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "list_hosts", "arguments": {}})),
            &config_path,
        );
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        assert!(hosts.is_empty());
    }

    #[test]
    fn tool_list_hosts_filter_by_provider_name() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"tag": "aws"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "list_hosts", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0]["alias"], "web-1");
    }

    #[test]
    fn tool_list_hosts_filter_case_insensitive() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"tag": "PROD"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "list_hosts", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(hosts.len(), 2); // both web-1 and db-1 have "prod" tag
    }

    #[test]
    fn tool_list_hosts_filter_no_match() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"tag": "nonexistent-tag"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "list_hosts", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        assert!(hosts.is_empty());
    }

    #[test]
    fn tool_list_hosts_filter_by_provider_tags() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_provider_tags_config");
        let args = serde_json::json!({"tag": "backend"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "list_hosts", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0]["alias"], "tagged-1");
    }

    #[test]
    fn tool_list_hosts_stale_field_is_boolean() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_stale_config");
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "list_hosts", "arguments": {}})),
            &config_path,
        );
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        let stale_host = hosts.iter().find(|h| h["alias"] == "stale-1").unwrap();
        let active_host = hosts.iter().find(|h| h["alias"] == "active-1").unwrap();
        assert_eq!(stale_host["stale"], true);
        assert_eq!(active_host["stale"], false);
    }

    #[test]
    fn tool_list_hosts_output_fields() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "list_hosts", "arguments": {}})),
            &config_path,
        );
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let hosts: Vec<Value> = serde_json::from_str(text).unwrap();
        let host = &hosts[0];
        // Verify all expected fields are present
        assert!(host.get("alias").is_some());
        assert!(host.get("hostname").is_some());
        assert!(host.get("user").is_some());
        assert!(host.get("port").is_some());
        assert!(host.get("tags").is_some());
        assert!(host.get("provider").is_some());
        assert!(host.get("stale").is_some());
        // Verify types
        assert!(host["port"].is_number());
        assert!(host["tags"].is_array());
        assert!(host["stale"].is_boolean());
    }

    // --- get_host additional tests ---

    #[test]
    fn tool_get_host_empty_alias() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": ""});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "get_host", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        // get_host doesn't check for empty string (unlike run_command), just does lookup
        // Empty string won't match any host
        assert!(
            result["isError"].as_bool().unwrap_or(false) || {
                let text = result["content"][0]["text"].as_str().unwrap_or("");
                text.contains("not found") || text.contains("Missing")
            }
        );
    }

    #[test]
    fn tool_get_host_alias_is_number() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": 42});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "get_host", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn tool_get_host_output_fields() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "web-1"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "get_host", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let host: Value = serde_json::from_str(text).unwrap();
        // Verify all expected fields
        assert_eq!(host["port"], 22);
        assert!(host["tags"].is_array());
        assert!(host["provider_tags"].is_array());
        assert!(host["provider_meta"].is_object());
        assert!(host["stale"].is_boolean());
        assert_eq!(host["stale"], false);
        assert_eq!(host["tunnel_count"], 0);
        // Verify provider_meta content
        assert_eq!(host["provider_meta"]["region"], "us-east-1");
        assert_eq!(host["provider_meta"]["instance"], "t3.micro");
    }

    #[test]
    fn tool_get_host_no_provider() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "db-1"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "get_host", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let host: Value = serde_json::from_str(text).unwrap();
        assert!(host["provider"].is_null());
        assert!(host["provider_meta"].as_object().unwrap().is_empty());
        assert_eq!(host["port"], 5432);
    }

    #[test]
    fn tool_get_host_stale_is_boolean() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_stale_config");
        let args = serde_json::json!({"alias": "stale-1"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "get_host", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        let host: Value = serde_json::from_str(text).unwrap();
        assert_eq!(host["stale"], true);
    }

    #[test]
    fn tool_get_host_case_sensitive() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "WEB-1"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "get_host", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    // --- run_command additional tests ---

    #[test]
    fn tool_run_command_nonexistent_alias() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "nonexistent-host", "command": "uptime"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "run_command", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
        assert!(
            result["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("not found")
        );
    }

    #[test]
    fn tool_run_command_alias_is_number() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": 42, "command": "uptime"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "run_command", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn tool_run_command_command_is_number() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "web-1", "command": 123});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "run_command", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn tool_run_command_timeout_is_string() {
        // timeout as string should be ignored, defaulting to 30
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args =
            serde_json::json!({"alias": "web-1", "command": "uptime", "timeout": "not-a-number"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "run_command", "arguments": args})),
            &config_path,
        );
        // This should not error on parsing - timeout defaults to 30
        // It will fail on SSH but not on input validation
        let result = resp.result.unwrap();
        // The alias exists so it will try SSH (which may fail), but no input validation error
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(!text.contains("Missing required parameter"));
    }

    // --- container_action additional tests ---

    #[test]
    fn tool_container_action_empty_alias() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "", "container_id": "abc", "action": "start"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "container_action", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn tool_container_action_empty_container_id() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "web-1", "container_id": "", "action": "start"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "container_action", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn tool_container_action_nonexistent_alias() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args =
            serde_json::json!({"alias": "nonexistent", "container_id": "abc", "action": "start"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "container_action", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
        assert!(
            result["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("not found")
        );
    }

    #[test]
    fn tool_container_action_uppercase_action() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "web-1", "container_id": "abc", "action": "START"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "container_action", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
        assert!(
            result["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Invalid action")
        );
    }

    #[test]
    fn tool_container_action_container_id_with_dots_and_hyphens() {
        // Valid container IDs can have dots, hyphens, underscores
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "web-1", "container_id": "my-container_v1.2", "action": "start"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "container_action", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        // Should NOT error on validation - container_id is valid
        // Will proceed to alias check and SSH (which may fail), but no validation error
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(!text.contains("invalid character"));
    }

    #[test]
    fn tool_container_action_container_id_with_spaces() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "web-1", "container_id": "my container", "action": "start"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "container_action", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
        assert!(
            result["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("invalid character")
        );
    }

    #[test]
    fn tool_list_containers_missing_empty_alias() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": ""});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "list_containers", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
    }

    #[test]
    fn tool_list_containers_nonexistent_alias() {
        let config_path = std::path::PathBuf::from("tests/fixtures/mcp_test_config");
        let args = serde_json::json!({"alias": "nonexistent"});
        let resp = dispatch(
            "tools/call",
            Some(serde_json::json!({"name": "list_containers", "arguments": args})),
            &config_path,
        );
        let result = resp.result.unwrap();
        assert!(result["isError"].as_bool().unwrap());
        assert!(
            result["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("not found")
        );
    }

    // --- initialize and tools/list output tests ---

    #[test]
    fn initialize_contains_version() {
        let resp = dispatch("initialize", None, &std::path::PathBuf::from("/dev/null"));
        let result = resp.result.unwrap();
        assert!(!result["serverInfo"]["version"].as_str().unwrap().is_empty());
    }

    #[test]
    fn tools_list_schema_has_required_fields() {
        let resp = dispatch("tools/list", None, &std::path::PathBuf::from("/dev/null"));
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        for tool in tools {
            assert!(tool["name"].is_string(), "Tool missing name");
            assert!(tool["description"].is_string(), "Tool missing description");
            assert!(tool["inputSchema"].is_object(), "Tool missing inputSchema");
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }
}

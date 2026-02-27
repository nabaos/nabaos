//! MCP server subprocess spawning and stdio JSON-RPC communication.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use super::config::McpServerConfig;
use super::transport::{
    parse_tool_call_result, parse_tools_list, JsonRpcRequest, JsonRpcResponse, McpToolDef,
};
use crate::core::error::{NyayaError, Result};

/// A running MCP server subprocess with stdio JSON-RPC communication.
pub struct McpProcess {
    child: Child,
    stdin: ChildStdin,
    reader: Lines<BufReader<ChildStdout>>,
    next_id: AtomicU64,
}

impl McpProcess {
    /// Spawn a new MCP server process.
    pub async fn spawn(config: &McpServerConfig, env: HashMap<String, String>) -> Result<Self> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| NyayaError::Config("Failed to capture MCP server stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| NyayaError::Config("Failed to capture MCP server stdout".to_string()))?;
        let reader = BufReader::new(stdout).lines();

        Ok(Self {
            child,
            stdin,
            reader,
            next_id: AtomicU64::new(1),
        })
    }

    /// Send a JSON-RPC request and read the response.
    pub async fn send_request(&mut self, method: &str, params: Value) -> Result<JsonRpcResponse> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest::new(id, method, params);
        let mut line = serde_json::to_string(&request)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;

        let response_line = self.reader.next_line().await?.ok_or_else(|| {
            NyayaError::Config("MCP server closed stdout unexpectedly".to_string())
        })?;
        let resp: JsonRpcResponse = serde_json::from_str(&response_line)?;
        Ok(resp)
    }

    /// Send the MCP initialize handshake.
    pub async fn initialize(&mut self) -> Result<()> {
        let resp = self
            .send_request(
                "initialize",
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "nabaos", "version": "0.1.0" }
                }),
            )
            .await?;

        if let Some(ref err) = resp.error {
            return Err(NyayaError::Config(format!(
                "MCP initialize error {}: {}",
                err.code, err.message
            )));
        }
        Ok(())
    }

    /// Request the list of available tools from the server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpToolDef>> {
        let resp = self
            .send_request("tools/list", serde_json::json!({}))
            .await?;
        parse_tools_list(&resp)
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<String> {
        let resp = self
            .send_request(
                "tools/call",
                serde_json::json!({
                    "name": name,
                    "arguments": arguments
                }),
            )
            .await?;
        parse_tool_call_result(&resp)
    }

    /// Check if the child process is still running.
    pub fn is_alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_json_rpc_request_serialization() {
        let req = JsonRpcRequest::new(1, "initialize", json!({"foo": "bar"}));
        let serialized = serde_json::to_string(&req).unwrap();
        assert!(serialized.contains("\"jsonrpc\":\"2.0\""));
        assert!(serialized.contains("\"id\":1"));
        assert!(serialized.contains("\"method\":\"initialize\""));
    }

    #[test]
    fn test_initialize_request_format() {
        let req = JsonRpcRequest::new(
            1,
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "nabaos", "version": "0.1.0" }
            }),
        );
        assert_eq!(req.method, "initialize");
        assert_eq!(req.params["protocolVersion"], json!("2024-11-05"));
        assert_eq!(req.params["clientInfo"]["name"], json!("nabaos"));
    }

    #[test]
    fn test_tool_call_request_format() {
        let req = JsonRpcRequest::new(
            3,
            "tools/call",
            json!({
                "name": "read_file",
                "arguments": { "path": "/tmp/test.txt" }
            }),
        );
        assert_eq!(req.params["name"], json!("read_file"));
        assert_eq!(req.params["arguments"]["path"], json!("/tmp/test.txt"));
    }
}

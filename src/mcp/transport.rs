use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::error::{NyayaError, Result};

/// A tool definition as returned by the MCP `tools/list` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema", default)]
    pub input_schema: Value,
}

/// A JSON-RPC 2.0 request.
#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: Value,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        }
    }
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

/// Parse the `tools/list` response into a vector of tool definitions.
///
/// Expects `resp.result.tools` to be an array of tool objects.
pub fn parse_tools_list(resp: &JsonRpcResponse) -> Result<Vec<McpToolDef>> {
    if let Some(ref err) = resp.error {
        return Err(NyayaError::Config(format!(
            "MCP tools/list error {}: {}",
            err.code, err.message
        )));
    }
    let result = resp
        .result
        .as_ref()
        .ok_or_else(|| NyayaError::Config("MCP tools/list response missing result".to_string()))?;
    let tools_val = result.get("tools").ok_or_else(|| {
        NyayaError::Config("MCP tools/list result missing 'tools' field".to_string())
    })?;
    let tools: Vec<McpToolDef> = serde_json::from_value(tools_val.clone())?;
    Ok(tools)
}

/// Parse a `tools/call` response, extracting concatenated text from content blocks.
///
/// Expects `resp.result.content` to be an array of objects with `type` and `text` fields.
/// Only blocks where `type == "text"` are included.
pub fn parse_tool_call_result(resp: &JsonRpcResponse) -> Result<String> {
    if let Some(ref err) = resp.error {
        return Err(NyayaError::Config(format!(
            "MCP tools/call error {}: {}",
            err.code, err.message
        )));
    }
    let result = resp
        .result
        .as_ref()
        .ok_or_else(|| NyayaError::Config("MCP tools/call response missing result".to_string()))?;
    let content = result.get("content").ok_or_else(|| {
        NyayaError::Config("MCP tools/call result missing 'content' field".to_string())
    })?;
    let arr = content
        .as_array()
        .ok_or_else(|| NyayaError::Config("MCP tools/call content is not an array".to_string()))?;

    let mut text_parts = Vec::new();
    for item in arr {
        if item.get("type").and_then(|v| v.as_str()) == Some("text") {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                text_parts.push(text.to_string());
            }
        }
    }
    Ok(text_parts.join(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_build_initialize_request() {
        let req = JsonRpcRequest::new(
            1,
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "nabaos", "version": "0.1.0" }
            }),
        );
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.id, 1);
        assert_eq!(req.method, "initialize");
        assert_eq!(req.params["clientInfo"]["name"], json!("nabaos"));
    }

    #[test]
    fn test_parse_success_response() {
        let raw = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "protocolVersion": "2024-11-05", "capabilities": {} }
        }"#;
        let resp: JsonRpcResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.jsonrpc, "2.0");
        assert_eq!(resp.id, Some(json!(1)));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["protocolVersion"], json!("2024-11-05"));
    }

    #[test]
    fn test_parse_error_response() {
        let raw = r#"{
            "jsonrpc": "2.0",
            "id": 2,
            "error": { "code": -32601, "message": "Method not found" }
        }"#;
        let resp: JsonRpcResponse = serde_json::from_str(raw).unwrap();
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method not found");
        assert!(err.data.is_none());
    }

    #[test]
    fn test_build_tools_list_request() {
        let req = JsonRpcRequest::new(2, "tools/list", json!({}));
        assert_eq!(req.method, "tools/list");
        assert_eq!(req.id, 2);
        assert_eq!(req.params, json!({}));
    }

    #[test]
    fn test_build_tools_call_request() {
        let req = JsonRpcRequest::new(
            3,
            "tools/call",
            json!({
                "name": "read_file",
                "arguments": { "path": "/tmp/test.txt" }
            }),
        );
        assert_eq!(req.method, "tools/call");
        assert_eq!(req.id, 3);
        assert_eq!(req.params["name"], json!("read_file"));
        assert_eq!(req.params["arguments"]["path"], json!("/tmp/test.txt"));
    }

    #[test]
    fn test_parse_tools_list_result() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            result: Some(json!({
                "tools": [
                    {
                        "name": "read_file",
                        "description": "Read a file from the filesystem",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" }
                            },
                            "required": ["path"]
                        }
                    },
                    {
                        "name": "list_directory",
                        "description": "List contents of a directory",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" }
                            }
                        }
                    }
                ]
            })),
            error: None,
        };
        let tools = parse_tools_list(&resp).unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[0].description, "Read a file from the filesystem");
        assert!(tools[0].input_schema.get("properties").is_some());
        assert_eq!(tools[1].name, "list_directory");
    }

    #[test]
    fn test_parse_tool_call_result() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(3)),
            result: Some(json!({
                "content": [
                    { "type": "text", "text": "Hello, " },
                    { "type": "image", "data": "base64..." },
                    { "type": "text", "text": "world!" }
                ]
            })),
            error: None,
        };
        let text = parse_tool_call_result(&resp).unwrap();
        assert_eq!(text, "Hello, world!");
    }

    #[test]
    fn test_parse_tool_call_error() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(4)),
            result: None,
            error: Some(JsonRpcError {
                code: -32000,
                message: "File not found".to_string(),
                data: Some(json!({ "path": "/nonexistent" })),
            }),
        };
        let err = parse_tools_list(&resp).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("-32000"));
        assert!(msg.contains("File not found"));

        let err2 = parse_tool_call_result(&resp).unwrap_err();
        let msg2 = format!("{}", err2);
        assert!(msg2.contains("-32000"));
    }

    #[test]
    fn test_parse_tools_list_missing_result() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(5)),
            result: None,
            error: None,
        };
        let err = parse_tools_list(&resp).unwrap_err();
        assert!(format!("{}", err).contains("missing result"));
    }

    #[test]
    fn test_parse_tool_call_result_empty_content() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(6)),
            result: Some(json!({ "content": [] })),
            error: None,
        };
        let text = parse_tool_call_result(&resp).unwrap();
        assert_eq!(text, "");
    }

    #[test]
    fn test_parse_tool_call_result_no_text_blocks() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(7)),
            result: Some(json!({
                "content": [
                    { "type": "image", "data": "base64..." }
                ]
            })),
            error: None,
        };
        let text = parse_tool_call_result(&resp).unwrap();
        assert_eq!(text, "");
    }
}

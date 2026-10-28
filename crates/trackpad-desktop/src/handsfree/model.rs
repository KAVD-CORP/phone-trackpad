//! Model client for Handsfree.
//!
//! [`ModelClient`] is the seam. [`MockModelClient`] scripts tool calls for
//! tests and, with the offline planner, demos the loop without the network.
//! [`MetaModelClient`] calls Muse Voice Transcribe and Muse Spark on
//! `https://api.meta.ai/v1`. The API key is read from the environment
//! (`META_API_KEY`, then `MODEL_API_KEY`). A missing key is an error the
//! caller shows; it does not touch the cursor.

use serde_json::{json, Value};
use std::collections::VecDeque;
use std::time::Duration;

pub const API_BASE: &str = "https://api.meta.ai/v1";
pub const SPARK_MODEL: &str = "muse-spark-1.3";
pub const TRANSCRIBE_MODEL: &str = "muse-voice-transcribe-1.0";

pub const MISSING_KEY_MESSAGE: &str = "\
META_API_KEY is not set. Export META_API_KEY (Meta's docs also call this \
MODEL_API_KEY) and restart the desktop app. The trackpad still works; \
Handsfree will not call Muse until the key is set. Set HANDSFREE_MODEL=mock \
to run the offline planner instead.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    MissingKey,
    Http { status: u16, message: String },
    BadResponse(String),
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelError::MissingKey => f.write_str(MISSING_KEY_MESSAGE),
            ModelError::Http { status, message } => {
                write!(f, "Meta API returned HTTP {status}: {message}")
            }
            ModelError::BadResponse(msg) => write!(f, "Meta API response could not be read: {msg}"),
        }
    }
}

impl std::error::Error for ModelError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SparkResponse {
    pub id: String,
    pub calls: Vec<ToolCall>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub enum SparkInput {
    User(String),
    ToolOutputs(Vec<(String, String)>),
}

#[derive(Debug, Clone)]
pub struct SparkRequest {
    pub previous_response_id: Option<String>,
    pub input: SparkInput,
}

pub trait ModelClient {
    fn transcribe(&mut self, wav: &[u8]) -> Result<String, ModelError>;
    fn complete(&mut self, request: &SparkRequest) -> Result<SparkResponse, ModelError>;
}

/// Reads `META_API_KEY`, then `MODEL_API_KEY`. Empty values count as missing.
/// `file` supplies keys from a local `.env` when the process environment
/// does not already have them.
pub fn resolve_api_key(env_get: impl Fn(&str) -> Option<String>) -> Result<String, ModelError> {
    for name in ["META_API_KEY", "MODEL_API_KEY"] {
        if let Some(value) = env_get(name) {
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if !value.is_empty() {
                return Ok(value);
            }
        }
    }
    Err(ModelError::MissingKey)
}

pub fn tool_definitions() -> Value {
    json!([
        {
            "type": "function",
            "name": "move_pointer",
            "description": "Move the pointer by a relative pixel delta. dx and dy are not screen coordinates. Positive dx moves right, positive dy moves down. Keep each step within a few hundred pixels.",
            "parameters": {
                "type": "object",
                "properties": {
                    "dx": { "type": "integer" },
                    "dy": { "type": "integer" }
                },
                "required": ["dx", "dy"],
                "additionalProperties": false
            }
        },
        {
            "type": "function",
            "name": "click",
            "description": "Click the mouse. button is left, right, or middle. count is 1, or 2 for a double click.",
            "parameters": {
                "type": "object",
                "properties": {
                    "button": { "type": "string", "enum": ["left", "right", "middle"] },
                    "count": { "type": "integer" }
                },
                "required": ["button"],
                "additionalProperties": false
            }
        },
        {
            "type": "function",
            "name": "scroll",
            "description": "Scroll by finger pixels. About 40 pixels is one wheel notch. Positive dy scrolls content down.",
            "parameters": {
                "type": "object",
                "properties": {
                    "dx": { "type": "integer" },
                    "dy": { "type": "integer" }
                },
                "required": ["dx", "dy"],
                "additionalProperties": false
            }
        },
        {
            "type": "function",
            "name": "type_text",
            "description": "Type unicode text at the caret. Use this for form fields. A newline presses Enter.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                },
                "required": ["text"],
                "additionalProperties": false
            }
        },
        {
            "type": "function",
            "name": "key_combo",
            "description": "Press a key combo such as enter, tab, escape, backspace, or ctrl+enter. Modifiers: ctrl, alt, shift, win.",
            "parameters": {
                "type": "object",
                "properties": {
                    "combo": { "type": "string" }
                },
                "required": ["combo"],
                "additionalProperties": false
            }
        },
        {
            "type": "function",
            "name": "capture_screenshot",
            "description": "Capture the desktop. The result is the image size. Use it before guessing where a control is.",
            "parameters": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "type": "function",
            "name": "segment_screen",
            "description": "Ask SAM 3.1 to find a UI element in the latest screenshot. prompt is a short noun phrase such as \"submit button\". This may be unavailable; if it fails, keep using relative motion.",
            "parameters": {
                "type": "object",
                "properties": {
                    "prompt": { "type": "string" }
                },
                "required": ["prompt"],
                "additionalProperties": false
            }
        }
    ])
}

pub fn parse_spark_response(body: &str) -> Result<SparkResponse, ModelError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|e| ModelError::BadResponse(format!("spark json: {e}")))?;
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("resp")
        .to_string();
    let mut calls = Vec::new();
    let mut text = String::new();
    let output = value
        .get("output")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for item in output {
        match item.get("type").and_then(|v| v.as_str()) {
            Some("function_call") => {
                let name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ModelError::BadResponse("function_call missing name".into()))?
                    .to_string();
                let call_id = item
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ModelError::BadResponse("function_call missing call_id".into()))?
                    .to_string();
                let arguments = match item.get("arguments") {
                    Some(Value::String(s)) => s.clone(),
                    Some(other) => other.to_string(),
                    None => "{}".into(),
                };
                calls.push(ToolCall {
                    call_id,
                    name,
                    arguments,
                });
            }
            Some("message") => {
                if let Some(parts) = item.get("content").and_then(|v| v.as_array()) {
                    for part in parts {
                        if part.get("type").and_then(|v| v.as_str()) == Some("output_text") {
                            if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                                if !text.is_empty() {
                                    text.push('\n');
                                }
                                text.push_str(t);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if text.is_empty() {
        if let Some(t) = value.get("output_text").and_then(|v| v.as_str()) {
            text = t.to_string();
        }
    }
    Ok(SparkResponse { id, calls, text })
}

pub fn parse_transcript(body: &str) -> Result<String, ModelError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|e| ModelError::BadResponse(format!("transcript json: {e}")))?;
    if let Some(text) = value.get("transcript").and_then(|v| v.as_str()) {
        let text = text.trim();
        if !text.is_empty() {
            return Ok(text.to_string());
        }
    }
    let mut parts = Vec::new();
    if let Some(turns) = value.get("turns").and_then(|v| v.as_array()) {
        for turn in turns {
            if let Some(text) = turn.get("transcript").and_then(|v| v.as_str()) {
                let text = text.trim();
                if !text.is_empty() {
                    parts.push(text.to_string());
                }
            }
        }
    }
    if parts.is_empty() {
        return Err(ModelError::BadResponse(
            "transcript was empty. Speak again and hold the button until you finish.".into(),
        ));
    }
    Ok(parts.join(" "))
}

pub fn spark_request_body(request: &SparkRequest) -> Value {
    let input = match &request.input {
        SparkInput::User(text) => Value::String(text.clone()),
        SparkInput::ToolOutputs(outputs) => Value::Array(
            outputs
                .iter()
                .map(|(call_id, output)| {
                    json!({
                        "type": "function_call_output",
                        "call_id": call_id,
                        "output": output,
                    })
                })
                .collect(),
        ),
    };
    let mut body = json!({
        "model": SPARK_MODEL,
        "input": input,
        "tools": tool_definitions(),
    });
    if let Some(id) = &request.previous_response_id {
        body["previous_response_id"] = Value::String(id.clone());
    }
    body
}

fn wav_ok(wav: &[u8]) -> Result<(), ModelError> {
    if wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err(ModelError::BadResponse(
            "audio is not a WAV file. Handsfree sends Muse a WAV recording.".into(),
        ));
    }
    if wav.len() > trackpad_core::handsfree::MAX_VOICE_LEN {
        return Err(ModelError::BadResponse(
            "recording is too long. Keep a Handsfree command under about twenty seconds.".into(),
        ));
    }
    Ok(())
}

/// Scripted client for tests. With `planner` set, an empty script follows
/// [`plan_offline`] so a demo can run with `HANDSFREE_MODEL=mock`.
#[derive(Debug)]
pub struct MockModelClient {
    pub transcript: String,
    script: VecDeque<SparkResponse>,
    planner: bool,
    next_id: u32,
    pub seen: Vec<SparkInput>,
}

impl Default for MockModelClient {
    fn default() -> Self {
        Self::scripted()
    }
}

impl MockModelClient {
    pub fn scripted() -> Self {
        Self {
            transcript: String::new(),
            script: VecDeque::new(),
            planner: false,
            next_id: 1,
            seen: Vec::new(),
        }
    }

    pub fn planner(transcript: impl Into<String>) -> Self {
        Self {
            transcript: transcript.into(),
            planner: true,
            ..Self::scripted()
        }
    }

    pub fn push_tools(&mut self, calls: &[(&str, Value)]) {
        let id = self.alloc_id();
        let calls = calls
            .iter()
            .enumerate()
            .map(|(i, (name, args))| ToolCall {
                call_id: format!("{id}-{i}"),
                name: (*name).to_string(),
                arguments: args.to_string(),
            })
            .collect();
        self.script.push_back(SparkResponse {
            id,
            calls,
            text: String::new(),
        });
    }

    pub fn push_final(&mut self, text: &str) {
        let id = self.alloc_id();
        self.script.push_back(SparkResponse {
            id,
            calls: Vec::new(),
            text: text.to_string(),
        });
    }

    fn alloc_id(&mut self) -> String {
        let id = format!("mock-{}", self.next_id);
        self.next_id += 1;
        id
    }
}

impl ModelClient for MockModelClient {
    fn transcribe(&mut self, wav: &[u8]) -> Result<String, ModelError> {
        wav_ok(wav)?;
        if self.transcript.trim().is_empty() {
            Ok("click".into())
        } else {
            Ok(self.transcript.trim().to_string())
        }
    }

    fn complete(&mut self, request: &SparkRequest) -> Result<SparkResponse, ModelError> {
        self.seen.push(request.input.clone());
        if let Some(scripted) = self.script.pop_front() {
            return Ok(scripted);
        }
        if self.planner {
            if let SparkInput::User(text) = &request.input {
                let actions = plan_offline(extract_command(text));
                if actions.is_empty() {
                    return Ok(SparkResponse {
                        id: self.alloc_id(),
                        calls: Vec::new(),
                        text: "Say a move, click, scroll, or type request.".into(),
                    });
                }
                let id = self.alloc_id();
                let calls = actions
                    .into_iter()
                    .enumerate()
                    .map(|(i, (name, args))| ToolCall {
                        call_id: format!("{id}-{i}"),
                        name,
                        arguments: args.to_string(),
                    })
                    .collect();
                return Ok(SparkResponse {
                    id,
                    calls,
                    text: String::new(),
                });
            }
            return Ok(SparkResponse {
                id: self.alloc_id(),
                calls: Vec::new(),
                text: "Done.".into(),
            });
        }
        Ok(SparkResponse {
            id: self.alloc_id(),
            calls: Vec::new(),
            text: "Done.".into(),
        })
    }
}

/// Pull the spoken line back out of the tool-loop prompt.
fn extract_command(text: &str) -> &str {
    text.split_once("The spoken command is:\n")
        .map(|(_, rest)| rest.lines().next().unwrap_or(rest))
        .unwrap_or(text)
        .trim()
}

/// Small keyword planner for `HANDSFREE_MODEL=mock`. Not Muse Spark.
pub fn plan_offline(transcript: &str) -> Vec<(String, Value)> {
    let lower = transcript.to_ascii_lowercase();
    let mut actions = Vec::new();
    if let Some(idx) = lower.find("type ") {
        let rest = &transcript[idx + "type ".len()..];
        let end = rest
            .to_ascii_lowercase()
            .find(" and ")
            .unwrap_or(rest.len());
        let text = rest[..end].trim().trim_matches(|c| c == '"' || c == '\'');
        if !text.is_empty() {
            actions.push(("type_text".into(), json!({ "text": text })));
        }
    }
    if lower.contains("scroll up") {
        actions.push(("scroll".into(), json!({ "dx": 0, "dy": -160 })));
    } else if lower.contains("scroll") {
        actions.push(("scroll".into(), json!({ "dx": 0, "dy": 160 })));
    }
    if lower.contains("double") {
        actions.push(("click".into(), json!({ "button": "left", "count": 2 })));
    } else if lower.contains("right click") || lower.contains("right-click") {
        actions.push(("click".into(), json!({ "button": "right", "count": 1 })));
    } else if lower.contains("click") {
        actions.push(("click".into(), json!({ "button": "left", "count": 1 })));
    }
    if lower.contains("press enter") || lower.contains("hit enter") {
        actions.push(("key_combo".into(), json!({ "combo": "enter" })));
    }
    if lower.contains("press tab") || lower.contains("hit tab") {
        actions.push(("key_combo".into(), json!({ "combo": "tab" })));
    }
    actions
}

pub struct HttpReply {
    pub status: u16,
    pub body: String,
}

pub trait ByteTransport {
    fn post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<HttpReply, ModelError>;
}

pub struct MetaModelClient<T> {
    key: String,
    pub(crate) http: T,
}

impl<T> MetaModelClient<T> {
    pub fn new(key: String, http: T) -> Self {
        Self { key, http }
    }
}

impl<T: ByteTransport> ModelClient for MetaModelClient<T> {
    fn transcribe(&mut self, wav: &[u8]) -> Result<String, ModelError> {
        wav_ok(wav)?;
        let request = json!({
            "model": TRANSCRIBE_MODEL,
            "audioEncoding": "WAV",
            "mode": "PUSH_TO_TALK",
        });
        let boundary = "----handsfree-muse";
        let mut body = Vec::new();
        let request_bytes = request.to_string().into_bytes();
        push_part(
            &mut body,
            boundary,
            "request",
            "application/json",
            None,
            &request_bytes,
        );
        push_part(
            &mut body,
            boundary,
            "audio",
            "audio/wav",
            Some("command.wav"),
            wav,
        );
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        let reply = self.http.post(
            &format!("{API_BASE}/asr/transcribe"),
            &[
                ("Authorization", &format!("Bearer {}", self.key)),
                ("Accept", "application/json"),
                (
                    "Content-Type",
                    &format!("multipart/form-data; boundary={boundary}"),
                ),
            ],
            &body,
        )?;
        interpret(reply, parse_transcript)
    }

    fn complete(&mut self, request: &SparkRequest) -> Result<SparkResponse, ModelError> {
        let body = spark_request_body(request).to_string();
        let reply = self.http.post(
            &format!("{API_BASE}/responses"),
            &[
                ("Authorization", &format!("Bearer {}", self.key)),
                ("Content-Type", "application/json"),
                ("Accept", "application/json"),
            ],
            body.as_bytes(),
        )?;
        interpret(reply, parse_spark_response)
    }
}

fn interpret<T>(
    reply: HttpReply,
    parse: impl Fn(&str) -> Result<T, ModelError>,
) -> Result<T, ModelError> {
    if reply.status == 401 {
        return Err(ModelError::Http {
            status: 401,
            message: "the API key was rejected. Check META_API_KEY.".into(),
        });
    }
    if !(200..300).contains(&reply.status) {
        let snippet: String = reply.body.chars().take(180).collect();
        return Err(ModelError::Http {
            status: reply.status,
            message: snippet.replace('\n', " "),
        });
    }
    parse(&reply.body)
}

fn push_part(
    body: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    content_type: &str,
    filename: Option<&str>,
    bytes: &[u8],
) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    match filename {
        Some(filename) => body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n")
                .as_bytes(),
        ),
        None => body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n").as_bytes(),
        ),
    }
    body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
}

pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    pub fn new() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(45)))
            .http_status_as_error(false)
            .build()
            .into();
        Self { agent }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl ByteTransport for UreqTransport {
    fn post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<HttpReply, ModelError> {
        let mut request = self.agent.post(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        match request.send(body) {
            Ok(mut response) => {
                let status = response.status().as_u16();
                let text = response
                    .body_mut()
                    .read_to_string()
                    .unwrap_or_else(|e| format!("(body unreadable: {e})"));
                Ok(HttpReply { status, body: text })
            }
            Err(err) => Err(ModelError::Http {
                status: 0,
                message: err.to_string(),
            }),
        }
    }
}

pub fn meta_from_env(
    env_get: impl Fn(&str) -> Option<String>,
) -> Result<MetaModelClient<UreqTransport>, ModelError> {
    let key = resolve_api_key(env_get)?;
    Ok(MetaModelClient::new(key, UreqTransport::new()))
}

/// `HANDSFREE_MODEL=mock` selects the offline planner. Otherwise the live
/// Muse client is built, or a missing-key error is returned.
pub fn client_from_env(
    env_get: impl Fn(&str) -> Option<String>,
) -> Result<Box<dyn ModelClient + Send>, ModelError> {
    if env_get("HANDSFREE_MODEL")
        .map(|v| v.trim().eq_ignore_ascii_case("mock"))
        .unwrap_or(false)
    {
        let transcript = env_get("HANDSFREE_MOCK_TRANSCRIPT").unwrap_or_default();
        return Ok(Box::new(MockModelClient::planner(transcript)));
    }
    let client = meta_from_env(env_get)?;
    Ok(Box::new(client))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_key_is_a_clear_error() {
        let err = resolve_api_key(|_| None).unwrap_err();
        assert_eq!(err, ModelError::MissingKey);
        assert!(err.to_string().contains("META_API_KEY"));
        let key = resolve_api_key(|name| {
            if name == "MODEL_API_KEY" {
                Some("  sk-test  ".into())
            } else {
                None
            }
        })
        .unwrap();
        assert_eq!(key, "sk-test");
        assert!(resolve_api_key(|name| (name == "META_API_KEY").then(|| "  ".into())).is_err());
    }

    struct SeqHttp {
        replies: VecDeque<HttpReply>,
        bodies: Vec<String>,
        urls: Vec<String>,
        headers: Vec<Vec<(String, String)>>,
    }

    impl ByteTransport for SeqHttp {
        fn post(
            &mut self,
            url: &str,
            headers: &[(&str, &str)],
            body: &[u8],
        ) -> Result<HttpReply, ModelError> {
            self.urls.push(url.to_string());
            self.headers.push(
                headers
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                    .collect(),
            );
            self.bodies.push(String::from_utf8_lossy(body).into_owned());
            self.replies
                .pop_front()
                .ok_or_else(|| ModelError::BadResponse("no reply".into()))
        }
    }

    #[test]
    fn meta_client_posts_spark_tools_and_transcript() {
        let spark = r#"{"id":"resp_9","output":[{"type":"function_call","name":"type_text","arguments":"{\"text\":\"Ada\"}","call_id":"c1"}]}"#;
        let http = SeqHttp {
            replies: VecDeque::from([
                HttpReply {
                    status: 200,
                    body: spark.into(),
                },
                HttpReply {
                    status: 200,
                    body: r#"{"transcript":"fill in our team name"}"#.into(),
                },
            ]),
            bodies: Vec::new(),
            urls: Vec::new(),
            headers: Vec::new(),
        };
        let mut client = MetaModelClient::new("secret-key".into(), http);
        let response = client
            .complete(&SparkRequest {
                previous_response_id: Some("resp_1".into()),
                input: SparkInput::ToolOutputs(vec![("c0".into(), "{\"ok\":true}".into())]),
            })
            .unwrap();
        assert_eq!(response.calls[0].name, "type_text");
        assert!(client.http.urls[0].ends_with("/v1/responses"));
        let posted: Value = serde_json::from_str(&client.http.bodies[0]).unwrap();
        assert_eq!(posted["model"], SPARK_MODEL);
        assert_eq!(posted["previous_response_id"], "resp_1");
        assert_eq!(posted["input"][0]["type"], "function_call_output");
        assert!(posted["tools"][0]["name"].as_str().is_some());
        assert!(client.http.headers[0]
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer secret-key"));

        let wav = {
            let mut w = b"RIFF".to_vec();
            w.extend_from_slice(&[0, 0, 0, 0]);
            w.extend_from_slice(b"WAVE");
            w.resize(44, 0);
            w
        };
        let text = client.transcribe(&wav).unwrap();
        assert_eq!(text, "fill in our team name");
        assert!(client.http.urls[1].ends_with("/v1/asr/transcribe"));
        assert!(client.http.bodies[1].contains("muse-voice-transcribe-1.0"));
        assert!(client.http.bodies[1].contains("PUSH_TO_TALK"));
    }

    #[test]
    fn http_401_does_not_look_like_a_transcript() {
        let http = SeqHttp {
            replies: VecDeque::from([HttpReply {
                status: 401,
                body: r#"{"error":"invalid_api_key"}"#.into(),
            }]),
            bodies: Vec::new(),
            urls: Vec::new(),
            headers: Vec::new(),
        };
        let mut client = MetaModelClient::new("nope".into(), http);
        let err = client
            .complete(&SparkRequest {
                previous_response_id: None,
                input: SparkInput::User("hi".into()),
            })
            .unwrap_err();
        assert!(err.to_string().contains("401"), "{err}");
        assert!(err.to_string().contains("META_API_KEY"), "{err}");
    }

    #[test]
    fn offline_planner_types_then_clicks() {
        let plan = plan_offline("Type hello and click");
        assert_eq!(plan[0].0, "type_text");
        assert_eq!(plan[0].1["text"], "hello");
        assert_eq!(plan[1].0, "click");
        let mut model = MockModelClient::planner("");
        let first = model
            .complete(&SparkRequest {
                previous_response_id: None,
                input: SparkInput::User("type hello and click".into()),
            })
            .unwrap();
        assert_eq!(first.calls.len(), 2);
        let second = model
            .complete(&SparkRequest {
                previous_response_id: Some(first.id),
                input: SparkInput::ToolOutputs(vec![("a".into(), "{}".into())]),
            })
            .unwrap();
        assert!(second.calls.is_empty());
    }

    #[test]
    fn joins_transcript_turns_when_the_top_level_field_is_missing() {
        let text =
            parse_transcript(r#"{"turns":[{"transcript":"fill in"},{"transcript":"the form"}]}"#)
                .unwrap();
        assert_eq!(text, "fill in the form");
    }
}

//! Handsfree: a spoken command becomes pointer, click, scroll, and typing.
//!
//! The phone sends one sealed TCP frame. Muse Voice Transcribe turns a WAV
//! into text. Muse Spark calls tools. The tools run on the same
//! [`InputInjector`] as the trackpad, on the UDP thread, so a swipe and a
//! command cannot interleave inside one key combo. Nothing runs unless a
//! paired session is live. Cancel is checked before every tool.

mod model;
mod vision;

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write}; // read_exact / write_all on the Handsfree TCP frame
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use trackpad_core::button;
use trackpad_core::handsfree::{self, kind};

use crate::keyboard::{self, KeyCombo};
use crate::pairing_server::PairingShared;
use crate::InputInjector;

pub use model::{
    client_from_env, resolve_api_key, MockModelClient, ModelClient, ModelError, SparkInput,
    SparkRequest, MISSING_KEY_MESSAGE,
};
pub use vision::{
    CapturedImage, MemoryCapture, PlatformCapture, Sam31Stub, ScreenCapture, Segmenter,
};

/// Tool executions per command, counting each tool call.
pub const MAX_STEPS: u32 = 8;
const MOVE_LIMIT: i32 = 2_000;
const TEXT_LIMIT: usize = 4_000;
const APPLY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct HandsfreeStatus {
    pub running: bool,
    pub step: u32,
    pub max_steps: u32,
    pub transcript: String,
    pub detail: String,
    pub error: String,
}

impl Default for HandsfreeStatus {
    fn default() -> Self {
        Self {
            running: false,
            step: 0,
            max_steps: MAX_STEPS,
            transcript: String::new(),
            detail: "Idle".into(),
            error: String::new(),
        }
    }
}

#[derive(Debug)]
pub(crate) enum OsAction {
    Move { dx: i32, dy: i32 },
    Click { button: u8, count: u8 },
    Scroll { dx: i32, dy: i32 },
    TypeText(String),
    KeyCombo(KeyCombo),
}

struct Pending {
    action: OsAction,
    done: Sender<String>,
}

struct RunningGuard {
    hub: Arc<HandsfreeHub>,
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.hub.running.store(false, Ordering::SeqCst);
    }
}

/// Shared by the UDP loop, the pairing TCP thread, and the desktop window.
pub struct HandsfreeHub {
    queue: Mutex<VecDeque<Pending>>,
    inline: Mutex<Option<Box<dyn InputInjector + Send>>>,
    status: Mutex<HandsfreeStatus>,
    cancel: AtomicBool,
    running: AtomicBool,
    last_seq: Mutex<Option<(u64, u64)>>,
    env_file: HashMap<String, String>,
}

impl HandsfreeHub {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            inline: Mutex::new(None),
            status: Mutex::new(HandsfreeStatus::default()),
            cancel: AtomicBool::new(false),
            running: AtomicBool::new(false),
            last_seq: Mutex::new(None),
            env_file: load_env_file(),
        }
    }

    pub fn status(&self) -> HandsfreeStatus {
        self.status.lock().expect("handsfree status").clone()
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.note("Cancel requested. Handsfree stops before the next action.");
    }

    /// Test injector. Production leaves this empty and [`Self::drain`]s on
    /// the UDP thread instead.
    pub fn set_inline_injector(&self, injector: Box<dyn InputInjector + Send>) {
        *self.inline.lock().expect("handsfree inline") = Some(injector);
    }

    pub(crate) fn apply_os(&self, action: OsAction) -> String {
        if let Some(injector) = self.inline.lock().expect("handsfree inline").as_mut() {
            return perform_os(action, injector.as_mut());
        }
        let (tx, rx) = mpsc::channel();
        self.queue
            .lock()
            .expect("handsfree queue")
            .push_back(Pending { action, done: tx });
        rx.recv_timeout(APPLY_TIMEOUT)
            .unwrap_or_else(|_| "the input loop did not apply the action".into())
    }

    pub fn drain(&self, injector: &mut dyn InputInjector) {
        let batch: Vec<Pending> = self
            .queue
            .lock()
            .expect("handsfree queue")
            .drain(..)
            .collect();
        for job in batch {
            let obs = perform_os(job.action, injector);
            let _ = job.done.send(obs);
        }
    }

    pub fn pending_count(&self) -> usize {
        self.queue.lock().expect("handsfree queue").len()
    }

    fn note(&self, detail: &str) {
        self.status.lock().expect("handsfree status").detail = detail.to_string();
    }

    fn env_get(&self, name: &str) -> Option<String> {
        match std::env::var(name) {
            Ok(value) if !value.trim().is_empty() => Some(value),
            _ => self.env_file.get(name).cloned(),
        }
    }

    fn try_begin(self: &Arc<Self>) -> Option<RunningGuard> {
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return None;
        }
        self.cancel.store(false, Ordering::SeqCst);
        let mut status = self.status.lock().expect("handsfree status");
        status.running = true;
        status.step = 0;
        status.error.clear();
        status.detail = "Starting.".into();
        drop(status);
        Some(RunningGuard {
            hub: Arc::clone(self),
        })
    }

    fn finish(&self, detail: &str, error: &str) {
        let mut status = self.status.lock().expect("handsfree status");
        status.running = false;
        status.detail = detail.to_string();
        status.error = error.to_string();
    }
}

impl Default for HandsfreeHub {
    fn default() -> Self {
        Self::new()
    }
}

struct LiveAuth {
    session_id: u64,
    phone_key: [u8; 32],
}

fn live_auth(shared: &Mutex<PairingShared>) -> Option<LiveAuth> {
    let guard = shared.lock().expect("pairing lock");
    let live = guard.live()?;
    if live.session_id == 0 {
        return None;
    }
    if !guard
        .trust_peers()
        .iter()
        .any(|peer| peer.matches(&live.peer_pubkey))
    {
        return None;
    }
    Some(LiveAuth {
        session_id: live.session_id,
        phone_key: live.keys.phone_key,
    })
}

fn session_still_live(shared: &Mutex<PairingShared>, session_id: u64) -> Result<(), String> {
    match live_auth(shared) {
        Some(auth) if auth.session_id == session_id => Ok(()),
        _ => Err("the paired session ended. Handsfree stopped.".into()),
    }
}

/// Local command from the desktop window. Requires a live paired session.
/// The model loop runs on a background thread so Cancel stays responsive.
pub fn submit_text(
    hub: &Arc<HandsfreeHub>,
    shared: &Arc<Mutex<PairingShared>>,
    text: &str,
) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Type a command first.".into());
    }
    let auth = live_auth(shared)
        .ok_or_else(|| "Pair a phone before Handsfree can move the cursor.".to_string())?;
    let guard = hub.try_begin().ok_or_else(|| {
        "Handsfree is already running. Cancel it before starting another.".to_string()
    })?;
    let hub = Arc::clone(hub);
    let shared = Arc::clone(shared);
    let text = text.to_string();
    std::thread::spawn(move || {
        run_command(
            &hub,
            &shared,
            auth.session_id,
            kind::COMMAND,
            text.as_bytes(),
            guard,
        );
    });
    Ok(())
}

/// One sealed phone frame. The reply is plain UTF-8 on the already-local TCP
/// connection. The command itself is authenticated.
pub fn accept(
    mut stream: TcpStream,
    shared: Arc<Mutex<PairingShared>>,
    hub: Arc<HandsfreeHub>,
) -> std::io::Result<()> {
    let body = read_u32_frame(&mut stream)?;
    let reply = handle_sealed(&shared, &hub, &body);
    write_u32_frame(&mut stream, reply.as_bytes())
}

fn handle_sealed(
    shared: &Arc<Mutex<PairingShared>>,
    hub: &Arc<HandsfreeHub>,
    body: &[u8],
) -> String {
    let Some(auth) = live_auth(shared) else {
        return reject(hub, "Handsfree needs a paired phone. Nothing was injected.");
    };
    let plain = match handsfree::open(&auth.phone_key, body) {
        Ok(plain) => plain,
        Err(_) => return reject(hub, "Handsfree command was rejected. Nothing was injected."),
    };
    if plain.session_id != auth.session_id {
        return reject(
            hub,
            "Handsfree command was for a different session. Nothing was injected.",
        );
    }
    if let Err(msg) = note_seq(hub, plain.session_id, plain.seq) {
        return reject(hub, &msg);
    }
    match plain.kind {
        kind::CANCEL => {
            hub.request_cancel();
            "Cancelling Handsfree.".into()
        }
        kind::TYPE_TEXT => manual_type(hub, shared, plain.session_id, &plain.payload),
        kind::KEY_COMBO => manual_combo(hub, shared, plain.session_id, &plain.payload),
        kind::VOICE | kind::COMMAND => {
            let Some(guard) = hub.try_begin() else {
                return "Handsfree is already running. Cancel it before starting another.".into();
            };
            run_command(
                hub,
                shared,
                plain.session_id,
                plain.kind,
                &plain.payload,
                guard,
            )
        }
        other => format!("unsupported Handsfree kind {other}"),
    }
}

fn reject(hub: &HandsfreeHub, msg: &str) -> String {
    if !hub.running.load(Ordering::SeqCst) {
        hub.finish(msg, msg);
    }
    msg.to_string()
}

fn note_seq(hub: &HandsfreeHub, session: u64, seq: u64) -> Result<(), String> {
    let mut last = hub.last_seq.lock().expect("handsfree seq");
    if let Some((seen_session, seen_seq)) = *last {
        if seen_session == session && seq <= seen_seq {
            return Err("repeated Handsfree command ignored".into());
        }
    }
    *last = Some((session, seq));
    Ok(())
}

fn manual_type(
    hub: &HandsfreeHub,
    shared: &Mutex<PairingShared>,
    session_id: u64,
    payload: &[u8],
) -> String {
    if let Err(msg) = session_still_live(shared, session_id) {
        return msg;
    }
    let Ok(text) = std::str::from_utf8(payload) else {
        return "keyboard text was not UTF-8. Nothing was typed.".into();
    };
    match expand_text(text) {
        Ok(actions) => {
            let mut notes = Vec::new();
            for action in actions {
                notes.push(hub.apply_os(action));
            }
            let detail = if notes.is_empty() {
                "nothing to type".to_string()
            } else {
                notes.join("; ")
            };
            hub.note(&detail);
            detail
        }
        Err(msg) => msg,
    }
}

fn manual_combo(
    hub: &HandsfreeHub,
    shared: &Mutex<PairingShared>,
    session_id: u64,
    payload: &[u8],
) -> String {
    if let Err(msg) = session_still_live(shared, session_id) {
        return msg;
    }
    let Ok(text) = std::str::from_utf8(payload) else {
        return "key combo was not UTF-8. Nothing was pressed.".into();
    };
    match keyboard::parse_combo(text) {
        Ok(combo) => {
            let detail = hub.apply_os(OsAction::KeyCombo(combo));
            hub.note(&detail);
            detail
        }
        Err(msg) => msg,
    }
}

fn run_command(
    hub: &HandsfreeHub,
    shared: &Mutex<PairingShared>,
    session_id: u64,
    kind_byte: u8,
    payload: &[u8],
    _guard: RunningGuard,
) -> String {
    let mut model = match client_from_env(|name| hub.env_get(name)) {
        Ok(model) => model,
        Err(err) => {
            let msg = err.to_string();
            hub.finish(&msg, &msg);
            return msg;
        }
    };
    let transcript = if kind_byte == kind::VOICE {
        match model.transcribe(payload) {
            Ok(text) => text,
            Err(err) => {
                let msg = err.to_string();
                hub.finish(&msg, &msg);
                return msg;
            }
        }
    } else {
        match std::str::from_utf8(payload) {
            Ok(text) => text.trim().to_string(),
            Err(_) => {
                let msg = "command was not UTF-8. Nothing was injected.".to_string();
                hub.finish(&msg, &msg);
                return msg;
            }
        }
    };
    if transcript.is_empty() {
        let msg = "the command was empty. Nothing was injected.".to_string();
        hub.finish(&msg, &msg);
        return msg;
    }
    {
        let mut status = hub.status.lock().expect("handsfree status");
        status.transcript = transcript.clone();
        status.detail = "Planning.".into();
    }
    let hub_apply = hub;
    let mut apply = |action: OsAction| hub_apply.apply_os(action);
    let mut capture = PlatformCapture;
    let mut segmenter = Sam31Stub;
    let mut env = ToolEnv {
        apply_os: &mut apply,
        capture: &mut capture,
        segmenter: &mut segmenter,
        last_image: None,
    };
    let mut session_ok = || session_still_live(shared, session_id);
    let report = run_loop(
        model.as_mut(),
        &transcript,
        &mut env,
        &hub.cancel,
        &mut session_ok,
        MAX_STEPS,
        &mut |step, detail| {
            let mut status = hub.status.lock().expect("handsfree status");
            status.step = step;
            status.detail = detail.to_string();
        },
    );
    let text = report.summary();
    let error = if report.failed() {
        text.clone()
    } else {
        String::new()
    };
    drop(_guard);
    hub.finish(&text, &error);
    text
}

#[derive(Debug, PartialEq, Eq)]
pub enum StopReason {
    Done,
    Cancelled,
    MaxSteps,
    Failed,
}

pub struct LoopReport {
    pub steps: u32,
    pub text: String,
    pub stop: StopReason,
}

impl LoopReport {
    fn summary(&self) -> String {
        match self.stop {
            StopReason::Done => {
                if self.text.is_empty() {
                    format!("Done in {} steps.", self.steps)
                } else {
                    self.text.clone()
                }
            }
            StopReason::Cancelled => format!("Cancelled after {} steps.", self.steps),
            StopReason::MaxSteps => {
                format!("Stopped after {} steps (limit {MAX_STEPS}).", self.steps)
            }
            StopReason::Failed => self.text.clone(),
        }
    }

    fn failed(&self) -> bool {
        self.stop == StopReason::Failed
    }
}

pub(crate) struct ToolEnv<'a> {
    pub apply_os: &'a mut dyn FnMut(OsAction) -> String,
    pub capture: &'a mut dyn ScreenCapture,
    pub segmenter: &'a mut dyn Segmenter,
    pub last_image: Option<CapturedImage>,
}

/// Act, observe, continue. `cancel` is checked before every tool.
pub(crate) fn run_loop(
    model: &mut dyn ModelClient,
    transcript: &str,
    env: &mut ToolEnv<'_>,
    cancel: &AtomicBool,
    session_ok: &mut dyn FnMut() -> Result<(), String>,
    max_steps: u32,
    on_step: &mut dyn FnMut(u32, &str),
) -> LoopReport {
    let prompt = format!(
        "You control a PC for someone who cannot use a mouse or keyboard. \
         The spoken command is:\n{transcript}\n\
         Use the tools. move_pointer takes a relative pixel delta, not a screen position. \
         capture_screenshot grabs the desktop. segment_screen asks SAM 3.1 and may be unavailable; \
         if it fails, keep using relative move, click, scroll, and typing. \
         Stop when the command is done."
    );
    let mut request = SparkRequest {
        previous_response_id: None,
        input: SparkInput::User(prompt),
    };
    let mut steps = 0;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return LoopReport {
                steps,
                text: String::new(),
                stop: StopReason::Cancelled,
            };
        }
        if let Err(msg) = session_ok() {
            return LoopReport {
                steps,
                text: msg,
                stop: StopReason::Failed,
            };
        }
        let response = match model.complete(&request) {
            Ok(response) => response,
            Err(err) => {
                return LoopReport {
                    steps,
                    text: err.to_string(),
                    stop: StopReason::Failed,
                }
            }
        };
        if response.calls.is_empty() {
            return LoopReport {
                steps,
                text: response.text,
                stop: StopReason::Done,
            };
        }
        let mut outputs = Vec::new();
        for call in &response.calls {
            if cancel.load(Ordering::SeqCst) {
                return LoopReport {
                    steps,
                    text: String::new(),
                    stop: StopReason::Cancelled,
                };
            }
            if steps >= max_steps {
                return LoopReport {
                    steps,
                    text: String::new(),
                    stop: StopReason::MaxSteps,
                };
            }
            if let Err(msg) = session_ok() {
                return LoopReport {
                    steps,
                    text: msg,
                    stop: StopReason::Failed,
                };
            }
            steps += 1;
            on_step(steps, &format!("{} {}", call.name, call.arguments));
            outputs.push((call.call_id.clone(), exec_tool(call, env)));
        }
        request = SparkRequest {
            previous_response_id: Some(response.id),
            input: SparkInput::ToolOutputs(outputs),
        };
    }
}

fn exec_tool(call: &model::ToolCall, env: &mut ToolEnv<'_>) -> String {
    let args: Value = if call.arguments.trim().is_empty() {
        json!({})
    } else {
        match serde_json::from_str(&call.arguments) {
            Ok(value) => value,
            Err(_) => return tool_result(false, "arguments were not JSON"),
        }
    };
    let detail = match call.name.as_str() {
        "move_pointer" => {
            let dx = match json_i32(&args, "dx") {
                Ok(v) => clamp_delta(v),
                Err(msg) => return msg,
            };
            let dy = match json_i32(&args, "dy") {
                Ok(v) => clamp_delta(v),
                Err(msg) => return msg,
            };
            (env.apply_os)(OsAction::Move { dx, dy })
        }
        "click" => {
            let button = match json_button(&args) {
                Ok(v) => v,
                Err(msg) => return msg,
            };
            let count = match json_count(&args) {
                Ok(v) => v,
                Err(msg) => return msg,
            };
            (env.apply_os)(OsAction::Click { button, count })
        }
        "scroll" => {
            let dx = match json_i32(&args, "dx") {
                Ok(v) => clamp_delta(v),
                Err(msg) => return msg,
            };
            let dy = match json_i32(&args, "dy") {
                Ok(v) => clamp_delta(v),
                Err(msg) => return msg,
            };
            (env.apply_os)(OsAction::Scroll { dx, dy })
        }
        "type_text" => {
            let Some(text) = args.get("text").and_then(|v| v.as_str()) else {
                return tool_result(false, "type_text needs text");
            };
            let actions = match expand_text(text) {
                Ok(actions) => actions,
                Err(msg) => return tool_result(false, &msg),
            };
            let mut notes = Vec::new();
            for action in actions {
                notes.push((env.apply_os)(action));
            }
            if notes.is_empty() {
                return tool_result(false, "nothing to type");
            }
            notes.join("; ")
        }
        "key_combo" => {
            let Some(combo) = args.get("combo").and_then(|v| v.as_str()) else {
                return tool_result(false, "key_combo needs combo");
            };
            let combo = match keyboard::parse_combo(combo) {
                Ok(combo) => combo,
                Err(msg) => return tool_result(false, &msg),
            };
            (env.apply_os)(OsAction::KeyCombo(combo))
        }
        "capture_screenshot" => match env.capture.capture() {
            Ok(image) => {
                let detail = format!("captured {}x{}", image.width, image.height);
                env.last_image = Some(image);
                return tool_result(true, &detail);
            }
            Err(msg) => return tool_result(false, &msg),
        },
        "segment_screen" => {
            let prompt = args
                .get("prompt")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if env.last_image.is_none() {
                match env.capture.capture() {
                    Ok(image) => env.last_image = Some(image),
                    Err(msg) => return tool_result(false, &msg),
                }
            }
            let image = env.last_image.as_ref().expect("image just stored");
            match env.segmenter.segment(image, &prompt) {
                Ok(segments) if segments.is_empty() => {
                    return tool_result(false, "no matching UI element");
                }
                Ok(segments) => {
                    let listed = segments
                        .iter()
                        .map(|seg| {
                            format!(
                                "{} at {},{} {}x{}",
                                seg.label, seg.x, seg.y, seg.width, seg.height
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("; ");
                    return tool_result(true, &listed);
                }
                Err(msg) => return tool_result(false, &msg),
            }
        }
        other => return tool_result(false, &format!("unknown tool {other}")),
    };
    tool_result(true, &detail)
}

fn tool_result(ok: bool, detail: &str) -> String {
    json!({ "ok": ok, "detail": detail }).to_string()
}

fn json_i32(args: &Value, name: &str) -> Result<i32, String> {
    args.get(name)
        .and_then(|v| v.as_i64())
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| tool_result(false, &format!("{name} must be an integer")))
}

fn json_button(args: &Value) -> Result<u8, String> {
    match args.get("button").and_then(|v| v.as_str()) {
        Some("left") => Ok(button::LEFT),
        Some("right") => Ok(button::RIGHT),
        Some("middle") => Ok(button::MIDDLE),
        _ => Err(tool_result(false, "button must be left, right, or middle")),
    }
}

fn json_count(args: &Value) -> Result<u8, String> {
    match args.get("count") {
        None => Ok(1),
        Some(value) => match value.as_i64() {
            Some(1) => Ok(1),
            Some(2) => Ok(2),
            _ => Err(tool_result(false, "count must be 1 or 2")),
        },
    }
}

fn clamp_delta(value: i32) -> i32 {
    value.clamp(-MOVE_LIMIT, MOVE_LIMIT)
}

fn expand_text(text: &str) -> Result<Vec<OsAction>, String> {
    if text.chars().count() > TEXT_LIMIT {
        return Err(format!("text is longer than {TEXT_LIMIT} characters"));
    }
    let mut actions = Vec::new();
    let mut buf = String::new();
    let flush = |buf: &mut String, actions: &mut Vec<OsAction>| {
        if !buf.is_empty() {
            actions.push(OsAction::TypeText(std::mem::take(buf)));
        }
    };
    for ch in text.chars() {
        match ch {
            '\n' => {
                flush(&mut buf, &mut actions);
                actions.push(OsAction::KeyCombo(
                    keyboard::parse_combo("enter").expect("enter parses"),
                ));
            }
            '\t' => {
                flush(&mut buf, &mut actions);
                actions.push(OsAction::KeyCombo(
                    keyboard::parse_combo("tab").expect("tab parses"),
                ));
            }
            '\r' => {}
            other if other.is_control() => {}
            other => buf.push(other),
        }
    }
    flush(&mut buf, &mut actions);
    Ok(actions)
}

pub(crate) fn perform_os(action: OsAction, injector: &mut dyn InputInjector) -> String {
    match action {
        OsAction::Move { dx, dy } => {
            injector.move_relative(dx, dy);
            format!("moved dx={dx} dy={dy}")
        }
        OsAction::Click { button, count } => {
            let name = match button {
                button::LEFT => "left",
                button::RIGHT => "right",
                button::MIDDLE => "middle",
                _ => "button",
            };
            if button == button::LEFT && count == 2 {
                injector.click_double();
                "double-clicked left".into()
            } else {
                for _ in 0..count {
                    match button {
                        button::RIGHT => injector.click_right(),
                        button::MIDDLE => injector.click_middle(),
                        _ => injector.click_left(),
                    }
                }
                if count == 2 {
                    format!("double-clicked {name}")
                } else {
                    format!("clicked {name}")
                }
            }
        }
        OsAction::Scroll { dx, dy } => {
            injector.scroll(dx, dy);
            format!("scrolled dx={dx} dy={dy}")
        }
        OsAction::TypeText(text) => {
            let n = text.chars().count();
            injector.type_text(&text);
            format!("typed {n} characters")
        }
        OsAction::KeyCombo(combo) => {
            let name = combo.canonical();
            injector.key_combo(&combo);
            format!("pressed {name}")
        }
    }
}

const FRAME_MAX: usize = handsfree::MAX_VOICE_LEN + handsfree::HEADER_LEN + handsfree::TAG_LEN;

pub fn write_u32_frame(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    if data.len() > FRAME_MAX {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "frame too large",
        ));
    }
    stream.write_all(&(data.len() as u32).to_be_bytes())?;
    stream.write_all(data)
}

pub fn read_u32_frame(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > FRAME_MAX {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

fn load_env_file() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(text) = std::fs::read_to_string(".env") else {
        return map;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || key.contains(char::is_whitespace) {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'');
        map.insert(key.to_string(), value.to_string());
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockInjector;
    use serde_json::json;
    use std::sync::atomic::AtomicBool;
    use trackpad_core::crypto::derive_udp_keys;
    use trackpad_core::handsfree::{self, kind};

    struct LoopHarness {
        injector: MockInjector,
        capture: MemoryCapture,
    }

    fn harness() -> LoopHarness {
        LoopHarness {
            injector: MockInjector::default(),
            capture: MemoryCapture {
                image: CapturedImage::new(2, 2, vec![7; 16]).unwrap(),
                calls: 0,
            },
        }
    }

    fn run_script(
        harness: &mut LoopHarness,
        model: &mut MockModelClient,
        cancel: &AtomicBool,
        max_steps: u32,
    ) -> LoopReport {
        let mut apply = |action: OsAction| perform_os(action, &mut harness.injector);
        let mut segmenter = Sam31Stub;
        let mut env = ToolEnv {
            apply_os: &mut apply,
            capture: &mut harness.capture,
            segmenter: &mut segmenter,
            last_image: None,
        };
        let mut session_ok = || Ok(());
        run_loop(
            model,
            "fill in our team name and click submit",
            &mut env,
            cancel,
            &mut session_ok,
            max_steps,
            &mut |_, _| {},
        )
    }

    #[test]
    fn scripted_loop_moves_clicks_types_and_stops() {
        let mut model = MockModelClient::scripted();
        model.push_tools(&[("move_pointer", json!({"dx": 12, "dy": -4}))]);
        model.push_tools(&[("click", json!({"button": "left", "count": 1}))]);
        model.push_tools(&[("type_text", json!({"text": "Ada\n"}))]);
        model.push_tools(&[("key_combo", json!({"combo": "ctrl+enter"}))]);
        model.push_final("submitted");
        let mut harness = harness();
        let report = run_script(&mut harness, &mut model, &AtomicBool::new(false), MAX_STEPS);
        assert_eq!(report.stop, StopReason::Done);
        assert_eq!(report.text, "submitted");
        assert_eq!(harness.injector.moved, (12, -4));
        assert_eq!(harness.injector.clicks, 1);
        assert_eq!(harness.injector.typed, "Ada");
        assert_eq!(
            harness.injector.combos,
            vec!["enter".to_string(), "ctrl+enter".to_string()]
        );
        assert_eq!(report.steps, 4);
    }

    #[test]
    fn cancel_skips_the_rest_of_the_tool_list() {
        let mut model = MockModelClient::scripted();
        model.push_tools(&[
            ("click", json!({"button": "left"})),
            ("type_text", json!({"text": "should-not-type"})),
        ]);
        model.push_final("nope");
        let cancel = AtomicBool::new(false);
        let mut harness = harness();
        let mut apply = |action: OsAction| {
            cancel.store(true, Ordering::SeqCst);
            perform_os(action, &mut harness.injector)
        };
        let mut segmenter = Sam31Stub;
        let mut env = ToolEnv {
            apply_os: &mut apply,
            capture: &mut harness.capture,
            segmenter: &mut segmenter,
            last_image: None,
        };
        let mut session_ok = || Ok(());
        let report = run_loop(
            &mut model,
            "click then type",
            &mut env,
            &cancel,
            &mut session_ok,
            MAX_STEPS,
            &mut |_, _| {},
        );
        assert_eq!(report.stop, StopReason::Cancelled);
        assert_eq!(harness.injector.clicks, 1);
        assert!(harness.injector.typed.is_empty());
    }

    #[test]
    fn max_steps_stops_a_model_that_never_finishes() {
        let mut model = MockModelClient::scripted();
        for _ in 0..6 {
            model.push_tools(&[("click", json!({"button": "right", "count": 1}))]);
        }
        let mut harness = harness();
        let report = run_script(&mut harness, &mut model, &AtomicBool::new(false), 2);
        assert_eq!(report.stop, StopReason::MaxSteps);
        assert_eq!(harness.injector.right_clicks, 2);
        assert_eq!(report.steps, 2);
    }

    #[test]
    fn bad_tool_arguments_are_observed_and_do_not_inject() {
        let mut model = MockModelClient::scripted();
        model.push_tools(&[("click", json!({}))]);
        model.push_final("stopped asking");
        let mut harness = harness();
        let report = run_script(&mut harness, &mut model, &AtomicBool::new(false), MAX_STEPS);
        assert_eq!(report.stop, StopReason::Done);
        assert_eq!(harness.injector.clicks, 0);
        let SparkInput::ToolOutputs(outputs) = &model.seen[1] else {
            panic!("second turn should be tool output");
        };
        assert!(outputs[0].1.contains("button"), "{}", outputs[0].1);
    }

    #[test]
    fn voice_wav_transcript_drives_the_offline_planner() {
        let mut model = MockModelClient::planner("type Ada and click");
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&0u32.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.resize(44, 0);
        let transcript = model.transcribe(&wav).unwrap();
        assert_eq!(transcript, "type Ada and click");
        let mut harness = harness();
        let mut apply = |action: OsAction| perform_os(action, &mut harness.injector);
        let mut segmenter = Sam31Stub;
        let mut env = ToolEnv {
            apply_os: &mut apply,
            capture: &mut harness.capture,
            segmenter: &mut segmenter,
            last_image: None,
        };
        let mut session_ok = || Ok(());
        let report = run_loop(
            &mut model,
            &transcript,
            &mut env,
            &AtomicBool::new(false),
            &mut session_ok,
            MAX_STEPS,
            &mut |_, _| {},
        );
        assert_eq!(report.stop, StopReason::Done);
        assert_eq!(harness.injector.typed, "Ada");
        assert_eq!(harness.injector.clicks, 1);
    }

    #[test]
    fn segment_stub_is_a_tool_result_and_capture_remembers_the_image() {
        let mut model = MockModelClient::scripted();
        model.push_tools(&[("capture_screenshot", json!({}))]);
        model.push_tools(&[("segment_screen", json!({"prompt": "submit button"}))]);
        model.push_final("still relative");
        let mut harness = harness();
        let report = run_script(&mut harness, &mut model, &AtomicBool::new(false), MAX_STEPS);
        assert_eq!(report.stop, StopReason::Done);
        assert_eq!(harness.capture.calls, 1);
        assert_eq!(harness.injector.clicks, 0);
        let SparkInput::ToolOutputs(second) = &model.seen[2] else {
            panic!("segment output missing");
        };
        assert!(second[0].1.contains("sam-3.1"), "{}", second[0].1);
        assert!(second[0].1.contains("\"ok\":false"), "{}", second[0].1);
    }

    #[test]
    fn double_click_and_scroll_hit_the_injector() {
        let mut model = MockModelClient::scripted();
        model.push_tools(&[
            ("click", json!({"button": "left", "count": 2})),
            ("scroll", json!({"dx": 0, "dy": 80})),
        ]);
        model.push_final("ok");
        let mut harness = harness();
        let report = run_script(&mut harness, &mut model, &AtomicBool::new(false), MAX_STEPS);
        assert_eq!(report.stop, StopReason::Done);
        assert_eq!(harness.injector.double_clicks, 1);
        assert_eq!(harness.injector.scrolled, (0, 80));
    }

    #[test]
    fn revoked_session_injects_nothing_further() {
        let mut model = MockModelClient::scripted();
        model.push_tools(&[("click", json!({"button": "left"}))]);
        model.push_final("no");
        let mut harness = harness();
        let mut checks = 0;
        let mut apply = |action: OsAction| perform_os(action, &mut harness.injector);
        let mut segmenter = Sam31Stub;
        let mut env = ToolEnv {
            apply_os: &mut apply,
            capture: &mut harness.capture,
            segmenter: &mut segmenter,
            last_image: None,
        };
        let mut session_ok = || {
            checks += 1;
            if checks > 1 {
                Err("the paired session ended. Handsfree stopped.".into())
            } else {
                Ok(())
            }
        };
        let report = run_loop(
            &mut model,
            "click",
            &mut env,
            &AtomicBool::new(false),
            &mut session_ok,
            MAX_STEPS,
            &mut |_, _| {},
        );
        assert_eq!(report.stop, StopReason::Failed);
        assert_eq!(harness.injector.clicks, 0);
        assert!(report.text.contains("paired session"));
    }

    #[test]
    fn drain_applies_a_queued_action_on_the_injector_thread() {
        let hub = Arc::new(HandsfreeHub::new());
        let worker = Arc::clone(&hub);
        let handle = std::thread::spawn(move || worker.apply_os(OsAction::Move { dx: 3, dy: -5 }));
        for _ in 0..50 {
            if hub.pending_count() > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut injector = MockInjector::default();
        hub.drain(&mut injector);
        let obs = handle.join().expect("worker");
        assert_eq!(injector.moved, (3, -5));
        assert!(obs.contains("moved"), "{obs}");
    }

    #[test]
    fn unpaired_frame_does_not_inject() {
        let (shared, _, _) = crate::pairing_server::PairingShared::test_parts(1);
        let shared = Arc::new(Mutex::new(shared));
        let hub = Arc::new(HandsfreeHub::new());
        hub.set_inline_injector(Box::new(MockInjector::default()));
        let phone_key = [9u8; 32];
        let sealed = handsfree::seal(&phone_key, 4, 1, kind::TYPE_TEXT, b"nope").unwrap();
        let reply = handle_sealed(&shared, &hub, &sealed);
        assert!(reply.contains("paired"), "{reply}");
    }

    #[test]
    fn paired_type_text_reaches_the_inline_injector() {
        let (mut state, _, _) = crate::pairing_server::PairingShared::test_parts(1);
        let keys = derive_udp_keys(b"handsfree-test-handshake").unwrap();
        let peer = [3u8; 32];
        state.test_publish_live(peer, keys.clone(), 42);
        let shared = Arc::new(Mutex::new(state));
        let hub = Arc::new(HandsfreeHub::new());
        hub.set_inline_injector(Box::new(MockInjector::default()));
        let sealed = handsfree::seal(&keys.phone_key, 42, 1, kind::TYPE_TEXT, b"Hi").unwrap();
        let reply = handle_sealed(&shared, &hub, &sealed);
        assert!(reply.contains("typed"), "{reply}");
        let combo =
            handsfree::seal(&keys.phone_key, 42, 2, kind::KEY_COMBO, b"ctrl+enter").unwrap();
        let reply = handle_sealed(&shared, &hub, &combo);
        assert!(reply.contains("ctrl+enter"), "{reply}");
        let wrong = handsfree::seal(&keys.phone_key, 99, 3, kind::TYPE_TEXT, b"x").unwrap();
        let reply = handle_sealed(&shared, &hub, &wrong);
        assert!(reply.contains("different session"), "{reply}");
        let replay = handsfree::seal(&keys.phone_key, 42, 2, kind::KEY_COMBO, b"tab").unwrap();
        let reply = handle_sealed(&shared, &hub, &replay);
        assert!(reply.contains("repeated"), "{reply}");
    }

    #[test]
    fn missing_key_message_names_the_variable() {
        assert!(MISSING_KEY_MESSAGE.contains("META_API_KEY"));
    }
}

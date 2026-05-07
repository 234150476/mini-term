//! Hook HTTP 服务器模块
//!
//! 在后台线程监听 `127.0.0.1` 的 HTTP 请求，接收 Claude Code / Codex 的
//! hook 事件上报，并通过 Tauri event 通知前端。

use crate::process_monitor::PtyStatusChangePayload;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

/// 默认监听端口
const DEFAULT_PORT: u16 = 23456;
/// 端口冲突时最多尝试的端口数
const MAX_PORT_ATTEMPTS: u16 = 5;
/// hook 事件有效期（秒），超过此时间降级回 process_monitor 轮询
const HOOK_ACTIVE_TIMEOUT_SECS: u64 = 30;

/// Hook 事件的 JSON payload
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // 保留完整字段供未来 UI 细化使用
pub struct HookPayload {
    /// PTY ID（由 MINITERM_PTY_ID 环境变量传递）
    pub pty_id: Option<u32>,
    /// 事件名（如 UserPromptSubmit, PreToolUse 等）
    pub event: Option<String>,
    /// 来源 agent（claude-code / codex）
    pub agent: Option<String>,
    /// 会话 ID
    pub session_id: Option<String>,
    /// 工作目录
    pub cwd: Option<String>,
    /// 工具名称（PreToolUse/PostToolUse 时有值）
    pub tool_name: Option<String>,
}

/// Hook 状态信息，供前端查询
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookStatusInfo {
    pub port: u16,
    pub running: bool,
}

/// Hook 状态管理器，记录每个 PTY 的最后 hook 事件时间和状态
#[derive(Clone)]
pub struct HookState {
    last_hook_time: Arc<Mutex<HashMap<u32, Instant>>>,
    last_hook_status: Arc<Mutex<HashMap<u32, String>>>,
    port: Arc<Mutex<u16>>,
}

impl HookState {
    pub fn new() -> Self {
        Self {
            last_hook_time: Arc::new(Mutex::new(HashMap::new())),
            last_hook_status: Arc::new(Mutex::new(HashMap::new())),
            port: Arc::new(Mutex::new(0)),
        }
    }

    /// 检查指定 PTY 在最近 30s 内是否收到过 hook 事件
    pub fn is_hook_active(&self, pty_id: u32) -> bool {
        let map = self.last_hook_time.lock().unwrap();
        map.get(&pty_id).map_or(false, |t| {
            t.elapsed().as_secs() < HOOK_ACTIVE_TIMEOUT_SECS
        })
    }

    /// 获取指定 PTY 的 hook 状态
    pub fn get_status(&self, pty_id: u32) -> Option<String> {
        self.last_hook_status.lock().unwrap().get(&pty_id).cloned()
    }

    /// 更新指定 PTY 的 hook 状态
    fn update(&self, pty_id: u32, status: String) {
        self.last_hook_time
            .lock()
            .unwrap()
            .insert(pty_id, Instant::now());
        self.last_hook_status
            .lock()
            .unwrap()
            .insert(pty_id, status);
    }

    /// 移除指定 PTY 的 hook 状态（PTY 关闭时调用）
    pub fn remove(&self, pty_id: u32) {
        self.last_hook_time.lock().unwrap().remove(&pty_id);
        self.last_hook_status.lock().unwrap().remove(&pty_id);
    }

    /// 获取当前服务器端口
    pub fn get_port(&self) -> u16 {
        *self.port.lock().unwrap()
    }

    /// 设置服务器端口
    fn set_port(&self, port: u16) {
        *self.port.lock().unwrap() = port;
    }
}

/// 将 hook 事件名映射为 PTY 状态
///
/// - ai-working: 表示 AI 正在处理（思考/工具调用/子代理/压缩）
/// - ai-idle: 表示 AI 等待用户输入（会话开始/结束/停止/权限请求/通知等）
fn map_event_to_status(event: &str) -> Option<&'static str> {
    match event {
        // ai-working 状态：AI 正在积极工作
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "SubagentStart" | "PreCompact"
        | "PostCompact" => Some("ai-working"),
        // ai-idle 状态：AI 等待用户输入或已完成
        "SessionStart" | "SessionEnd" | "Stop" | "PermissionRequest" | "Notification"
        | "Elicitation" | "SubagentStop" => Some("ai-idle"),
        _ => None,
    }
}

/// 启动 hook HTTP 服务器
///
/// 在后台线程监听，接收 hook 事件后通过 Tauri event 通知前端。
/// 端口从 DEFAULT_PORT 开始尝试，冲突时自动递增。
pub fn start_hook_server(app: AppHandle, hook_state: HookState) {
    std::thread::spawn(move || {
        // 尝试绑定端口
        let server = {
            let mut bound = None;
            for offset in 0..MAX_PORT_ATTEMPTS {
                let port = DEFAULT_PORT + offset;
                let addr = format!("127.0.0.1:{}", port);
                match tiny_http::Server::http(&addr) {
                    Ok(s) => {
                        eprintln!("[hook-server] 监听 {}", addr);
                        hook_state.set_port(port);
                        bound = Some((s, port));
                        break;
                    }
                    Err(e) => {
                        eprintln!("[hook-server] 端口 {} 被占用: {}", port, e);
                    }
                }
            }
            bound
        };

        let (server, port) = match server {
            Some(s) => s,
            None => {
                eprintln!("[hook-server] 无法绑定任何端口，hook 服务器未启动");
                return;
            }
        };

        // 写入端口文件
        write_port_file(&app, port);

        // 处理请求
        for mut request in server.incoming_requests() {
            if request.method() != &tiny_http::Method::Post {
                let response = tiny_http::Response::from_string("Method Not Allowed")
                    .with_status_code(405);
                let _ = request.respond(response);
                continue;
            }

            let url = request.url().to_string();
            if url != "/hook" {
                let response =
                    tiny_http::Response::from_string("Not Found").with_status_code(404);
                let _ = request.respond(response);
                continue;
            }

            // 读取 body
            let mut body = String::new();
            if request.as_reader().read_to_string(&mut body).is_err() {
                let response =
                    tiny_http::Response::from_string("Bad Request").with_status_code(400);
                let _ = request.respond(response);
                continue;
            }

            // 解析 JSON payload
            let payload: HookPayload = match serde_json::from_str(&body) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("[hook-server] JSON 解析失败: {}", e);
                    let response = tiny_http::Response::from_string("Bad Request")
                        .with_status_code(400);
                    let _ = request.respond(response);
                    continue;
                }
            };

            // 立即响应 200，不阻塞 hook 脚本
            let response = tiny_http::Response::from_string("OK").with_status_code(200);
            let _ = request.respond(response);

            // 处理事件
            if let (Some(pty_id), Some(ref event)) = (payload.pty_id, &payload.event) {
                if let Some(status) = map_event_to_status(event) {
                    hook_state.update(pty_id, status.to_string());

                    // 通过 Tauri event 通知前端（复用现有 pty-status-change 事件）
                    let _ = app.emit(
                        "pty-status-change",
                        PtyStatusChangePayload {
                            pty_id,
                            status: status.to_string(),
                        },
                    );

                    eprintln!(
                        "[hook-server] pty_id={} event={} -> status={}",
                        pty_id, event, status
                    );
                }
            }
        }
    });
}

/// 将端口信息写入 app_data_dir/hook-server.json
fn write_port_file(app: &AppHandle, port: u16) {
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("hook-server.json");
        let content = format!("{{\"port\":{}}}", port);
        if let Err(e) = std::fs::write(&path, &content) {
            eprintln!(
                "[hook-server] 写入端口文件失败 {}: {}",
                path.display(),
                e
            );
        } else {
            eprintln!("[hook-server] 端口文件已写入 {}", path.display());
        }
    }
}

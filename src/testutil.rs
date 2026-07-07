//! 测试共享工具:临时项目、需求夹具与顺序应答的假聊天端点。

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::requirements::{RequirementReport, analyze_requirements};

pub(crate) fn temp_project(prefix: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("{prefix}-{suffix}"));
    fs::create_dir_all(&root).expect("create temp project");
    root
}

/// 生成一份只含 REQ-001(“系统必须显示登录成功提示”)的需求报告。
pub(crate) fn requirement_fixture(prefix: &str) -> RequirementReport {
    let root = temp_project(prefix);
    let file = root.join("PRD.md");
    fs::write(&file, "- 系统必须显示登录成功提示。").expect("write requirement");
    let report = analyze_requirements(&file).expect("requirement analysis succeeds");
    fs::remove_dir_all(root).expect("remove test project");
    report
}

/// 把 content 包装成 OpenAI chat completions 响应体(带固定 usage)。
pub(crate) fn chat_response(content: &str) -> String {
    serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": content}}],
        "usage": {"prompt_tokens": 11, "completion_tokens": 7, "total_tokens": 18},
    })
    .to_string()
}

/// 顺序应答的假聊天端点:每个连接读完整请求后返回预置响应,
/// 线程返回捕获到的原始请求文本。
pub(crate) fn spawn_chat_server(
    responses: Vec<(u16, String)>,
) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake ai server");
    let port = listener.local_addr().expect("read local addr").port();
    let handle = thread::spawn(move || {
        let mut captured = Vec::new();
        for (status, body) in responses {
            let (mut stream, _) = listener.accept().expect("accept connection");
            captured.push(read_http_request(&mut stream));
            let reason = match status {
                200 => "OK",
                400 => "Bad Request",
                401 => "Unauthorized",
                429 => "Too Many Requests",
                500 => "Internal Server Error",
                _ => "Status",
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        }
        captured
    });
    (format!("http://127.0.0.1:{port}"), handle)
}

fn read_http_request(stream: &mut TcpStream) -> String {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut chunk).expect("read request");
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = find_subslice(&buffer, b"\r\n\r\n") {
            break position + 4;
        }
        if read == 0 {
            return String::from_utf8_lossy(&buffer).into_owned();
        }
    };

    let headers = String::from_utf8_lossy(&buffer[..header_end]).to_ascii_lowercase();
    let content_length = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buffer.len() < header_end + content_length {
        let read = stream.read(&mut chunk).expect("read request body");
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8_lossy(&buffer).into_owned()
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

//! 出站内容脱敏(ROADMAP 3.4)。
//!
//! 发送给外部 LLM 的内容(需求文档、源码片段、进程输出、DOM 摘要)可能夹带密钥。
//! 在出站前按行脱敏:①敏感键(api_key/token/password 等)的值;②已知前缀的密钥令牌
//! (sk-、ghp_ 等),即便不在 key=value 形式也脱敏。保守起见宁可多脱敏一行,
//! 也不把疑似密钥发给第三方。运行期日志有自己的整行脱敏(runtime),此处针对出站文本。

/// 敏感键:出现即认为其后的值敏感。
const SENSITIVE_KEYS: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "secret",
    "password",
    "passwd",
    "passphrase",
    "token",
    "authorization",
    "auth_token",
    "access_key",
    "private_key",
    "client_secret",
    "connection_string",
];

/// 高辨识度的密钥令牌前缀(不易与普通词冲突)。
const TOKEN_PREFIXES: &[&str] = &[
    "sk-",
    "ghp_",
    "gho_",
    "ghs_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
];

const REDACTED: &str = "[REDACTED]";

/// 按行脱敏文本中的疑似密钥。保留非敏感内容与整体结构。
pub fn redact_secrets(text: &str) -> String {
    text.lines().map(redact_line).collect::<Vec<_>>().join("\n")
}

fn redact_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    if SENSITIVE_KEYS.iter().any(|key| lower.contains(key)) {
        // 保留分隔符前的键结构,隐藏其后的值;无分隔符则整行脱敏。
        if let Some(pos) = line.rfind(['=', ':']) {
            let head = &line[..=pos];
            return format!("{head} {REDACTED}");
        }
        return REDACTED.to_owned();
    }
    redact_tokens(line)
}

/// 无敏感键时,按空白切分,脱敏已知前缀的令牌。
fn redact_tokens(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for (index, token) in line.split(' ').enumerate() {
        if index > 0 {
            out.push(' ');
        }
        if looks_like_secret_token(token) {
            out.push_str(REDACTED);
        } else {
            out.push_str(token);
        }
    }
    out
}

fn looks_like_secret_token(token: &str) -> bool {
    let trimmed = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
    if trimmed.len() < 12 {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    TOKEN_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::redact_secrets;

    #[test]
    fn redacts_sensitive_key_values() {
        let input =
            "line one\nOPENAI_API_KEY=sk-abcdef1234567890\npassword: hunter2secret\nline two";
        let out = redact_secrets(input);
        assert!(out.contains("line one"));
        assert!(out.contains("line two"));
        assert!(!out.contains("sk-abcdef1234567890"));
        assert!(!out.contains("hunter2secret"));
        assert!(out.contains("OPENAI_API_KEY= [REDACTED]"));
        assert!(out.contains("password: [REDACTED]"));
    }

    #[test]
    fn redacts_bare_known_tokens() {
        let out = redact_secrets("const t = ghp_abcdefghijklmnop1234;");
        assert!(!out.contains("ghp_abcdefghijklmnop1234"));
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn keeps_ordinary_content_untouched() {
        let input = "user clicks the login button\npage should show a welcome message";
        assert_eq!(redact_secrets(input), input);
    }

    #[test]
    fn does_not_flag_ordinary_long_words() {
        // 不以已知密钥前缀开头的长词不脱敏。
        let input = "internationalization and responsibilities are long words";
        assert_eq!(redact_secrets(input), input);
    }
}

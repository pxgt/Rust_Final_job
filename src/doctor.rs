use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub tools: Vec<ToolCheck>,
    pub ai_providers: Vec<AiProviderCheck>,
    pub core_ready: bool,
    pub web_testing_ready: bool,
    pub ai_ready: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ToolCheck {
    pub name: String,
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AiProviderCheck {
    pub name: String,
    pub configured: bool,
    pub source: String,
}

pub fn inspect_environment() -> DoctorReport {
    let mut tools = vec![
        inspect_command("rustc", &["--version"]),
        inspect_command("cargo", &["--version"]),
        inspect_command("git", &["--version"]),
        inspect_command("node", &["--version"]),
        inspect_command("npm", &["--version"]),
        inspect_command("docker", &["--version"]),
        inspect_command("ollama", &["--version"]),
    ];

    tools.push(inspect_msvc());

    let ai_providers = vec![
        inspect_env_provider("OpenAI-compatible API", "OPENAI_API_KEY"),
        inspect_env_provider("Anthropic", "ANTHROPIC_API_KEY"),
        inspect_env_provider("DeepSeek", "DEEPSEEK_API_KEY"),
        AiProviderCheck {
            name: "Ollama".to_owned(),
            configured: tool_available(&tools, "ollama"),
            source: "local command".to_owned(),
        },
    ];

    let core_ready = ["rustc", "cargo", "git"]
        .iter()
        .all(|name| tool_available(&tools, name));
    let web_testing_ready = ["node", "npm"]
        .iter()
        .all(|name| tool_available(&tools, name));
    let ai_ready = ai_providers.iter().any(|provider| provider.configured);

    let mut notes = Vec::new();
    if !web_testing_ready {
        notes.push(
            "Node.js and npm are required before browser automation can be enabled.".to_owned(),
        );
    }
    if !ai_ready {
        notes.push(
            "No AI provider is configured yet; static inspection remains available.".to_owned(),
        );
    }
    if tools
        .iter()
        .find(|tool| tool.name == "msvc")
        .is_some_and(|tool| tool.available && tool.version.is_none())
    {
        notes.push(
            "MSVC is installed but not active in this shell; use scripts/cargo-msvc.ps1."
                .to_owned(),
        );
    }

    DoctorReport {
        tools,
        ai_providers,
        core_ready,
        web_testing_ready,
        ai_ready,
        notes,
    }
}

fn inspect_command(name: &str, args: &[&str]) -> ToolCheck {
    let resolved = find_on_path(name);
    let executable = resolved.as_deref().unwrap_or_else(|| Path::new(name));

    match run_executable(executable, args) {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let text = if (name == "cl" || stdout.trim().is_empty()) && !stderr.trim().is_empty() {
                stderr.trim()
            } else {
                stdout.trim()
            };

            ToolCheck {
                name: name.to_owned(),
                available: output.status.success(),
                version: first_non_empty_line(text),
                path: resolved.map(|path| path.display().to_string()),
                note: (!output.status.success()).then(|| "Command returned an error.".to_owned()),
            }
        }
        Err(error) => ToolCheck {
            name: name.to_owned(),
            available: false,
            version: None,
            path: None,
            note: Some(error.to_string()),
        },
    }
}

fn run_executable(executable: &Path, args: &[&str]) -> io::Result<Output> {
    let is_windows_script = cfg!(windows)
        && executable
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
            });

    if is_windows_script {
        Command::new("cmd.exe")
            .args(["/d", "/c"])
            .arg(executable)
            .args(args)
            .output()
    } else {
        Command::new(executable).args(args).output()
    }
}

fn inspect_msvc() -> ToolCheck {
    let active = inspect_command("cl", &[]);
    if active.available {
        return ToolCheck {
            name: "msvc".to_owned(),
            ..active
        };
    }

    let discovered = find_msvc_compiler();
    ToolCheck {
        name: "msvc".to_owned(),
        available: discovered.is_some(),
        version: None,
        path: discovered.as_ref().map(|path| path.display().to_string()),
        note: Some(if discovered.is_some() {
            "Installed, but the Visual Studio developer environment is not active.".to_owned()
        } else {
            "Visual C++ build tools were not found.".to_owned()
        }),
    }
}

fn inspect_env_provider(name: &str, variable: &str) -> AiProviderCheck {
    AiProviderCheck {
        name: name.to_owned(),
        configured: env::var_os(variable).is_some_and(|value| !value.is_empty()),
        source: variable.to_owned(),
    }
}

fn tool_available(tools: &[ToolCheck], name: &str) -> bool {
    tools
        .iter()
        .find(|tool| tool.name == name)
        .is_some_and(|tool| tool.available)
}

fn first_non_empty_line(value: &str) -> Option<String> {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

fn find_on_path(command: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let extensions: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };

    env::split_paths(&path)
        .flat_map(|directory| {
            extensions
                .iter()
                .map(move |extension| directory.join(format!("{command}{extension}")))
        })
        .find(|candidate| candidate.is_file())
}

fn find_msvc_compiler() -> Option<PathBuf> {
    let roots = [
        Path::new(r"C:\Program Files\Microsoft Visual Studio\2022"),
        Path::new(r"C:\Program Files (x86)\Microsoft Visual Studio\2022"),
    ];

    for root in roots {
        let Ok(editions) = std::fs::read_dir(root) else {
            continue;
        };

        for edition in editions.flatten() {
            let tools = edition.path().join("VC").join("Tools").join("MSVC");
            let Ok(versions) = std::fs::read_dir(tools) else {
                continue;
            };

            for version in versions.flatten() {
                let compiler = version
                    .path()
                    .join("bin")
                    .join("Hostx64")
                    .join("x64")
                    .join("cl.exe");
                if compiler.is_file() {
                    return Some(compiler);
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{first_non_empty_line, inspect_environment};

    #[test]
    fn extracts_first_non_empty_line() {
        assert_eq!(
            first_non_empty_line("\n cargo 1.96.0\nsecond line"),
            Some("cargo 1.96.0".to_owned())
        );
    }

    #[test]
    fn doctor_report_always_contains_core_tools() {
        let report = inspect_environment();
        for expected in ["rustc", "cargo", "git", "msvc"] {
            assert!(report.tools.iter().any(|tool| tool.name == expected));
        }
    }
}

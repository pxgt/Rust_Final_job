//! 项目配置文件 `specprobe.toml`(ROADMAP 2.2)。
//!
//! 解决"每条命令都要带一串 flag"的问题:公共参数(base_url、provider、超时、
//! 需求源、缓存开关)可写进项目根的 `specprobe.toml`,`specprobe init` 生成模板。
//! 优先级:**CLI 显式参数 > 环境变量 > 项目配置 > 内置默认**。环境层做成
//! 显式结构注入,避免测试依赖进程级环境变量。
//!
//! 本轮范围:项目级配置。用户级 `~/.config/specprobe/config.toml`、启动命令
//! 覆盖与就绪探测配置属 runtime 层管道,列为遗留(见 ROADMAP 2.2 记录)。

use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::Deserialize;
use thiserror::Error;

use crate::ai::AiProviderKind;

pub const CONFIG_FILE_NAME: &str = "specprobe.toml";

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:3000";
pub const DEFAULT_LAUNCH_TIMEOUT_SECS: u64 = 15;
pub const DEFAULT_BROWSER_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error("invalid provider '{value}' in {path} (expected mock | openai-compatible | ollama)")]
    InvalidProvider { path: PathBuf, value: String },
}

/// `specprobe.toml` 的原始内容。未知字段报错,以便及时发现拼写错误。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub base_url: Option<String>,
    /// mock | openai-compatible | ollama
    pub provider: Option<String>,
    /// 需求文档或目录,相对项目根。
    pub requirements: Option<PathBuf>,
    pub launch_timeout_secs: Option<u64>,
    pub browser_timeout_secs: Option<u64>,
    pub no_cache: Option<bool>,
}

/// 已加载的配置与其来源文件(用于提示)。
#[derive(Debug)]
pub struct LoadedConfig {
    pub source: PathBuf,
    pub config: ProjectConfig,
}

/// 依次尝试 `<project>/specprobe.toml` 与 `./specprobe.toml`(相同路径只读一次)。
/// 文件不存在返回 Ok(None);存在但非法则报错(配置写错应显式失败,不静默忽略)。
pub fn load_project_config(project_path: &Path) -> Result<Option<LoadedConfig>, ConfigError> {
    let mut candidates = vec![project_path.join(CONFIG_FILE_NAME)];
    let cwd_candidate = PathBuf::from(CONFIG_FILE_NAME);
    let same = fs::canonicalize(&candidates[0]).ok() == fs::canonicalize(&cwd_candidate).ok();
    if !same {
        candidates.push(cwd_candidate);
    }

    for path in candidates {
        if !path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(|source| ConfigError::Io {
            path: path.clone(),
            source,
        })?;
        let config =
            toml::from_str::<ProjectConfig>(&text).map_err(|source| ConfigError::Parse {
                path: path.clone(),
                source: Box::new(source),
            })?;
        return Ok(Some(LoadedConfig {
            source: path,
            config,
        }));
    }
    Ok(None)
}

/// 环境变量层(显式注入,便于测试)。
#[derive(Debug, Default)]
pub struct EnvOverrides {
    pub base_url: Option<String>,
    pub provider: Option<String>,
}

impl EnvOverrides {
    pub fn from_process_env() -> Self {
        Self {
            base_url: std::env::var("SPECPROBE_BASE_URL").ok(),
            provider: std::env::var("SPECPROBE_PROVIDER").ok(),
        }
    }
}

/// CLI 层可缺省的公共参数(None = 用户未显式传)。
#[derive(Debug, Default)]
pub struct CliOverrides {
    pub base_url: Option<String>,
    pub provider: Option<AiProviderKind>,
    pub requirements: Option<PathBuf>,
    pub launch_timeout_secs: Option<u64>,
    pub browser_timeout_secs: Option<u64>,
    /// CLI 上的 --no-cache 是开关:true 表示显式禁用;false 表示未指定。
    pub no_cache: bool,
}

/// 逐字段解析后的最终生效值。
#[derive(Debug)]
pub struct ResolvedSettings {
    pub base_url: String,
    pub provider: AiProviderKind,
    pub requirements: Option<PathBuf>,
    pub launch_timeout_secs: u64,
    pub browser_timeout_secs: u64,
    pub no_cache: bool,
}

/// 按 CLI > 环境 > 配置 > 默认 逐字段合并。
/// 配置文件中的 `requirements` 相对项目根解析。
pub fn resolve_settings(
    project_path: &Path,
    cli: CliOverrides,
    env: EnvOverrides,
    loaded: Option<&LoadedConfig>,
) -> Result<ResolvedSettings, ConfigError> {
    let config = loaded.map(|value| &value.config);
    let config_provider = match config.and_then(|c| c.provider.as_deref()) {
        Some(text) => Some(
            parse_provider(text).ok_or_else(|| ConfigError::InvalidProvider {
                path: loaded.expect("loaded when provider set").source.clone(),
                value: text.to_owned(),
            })?,
        ),
        None => None,
    };
    let env_provider = match env.provider.as_deref() {
        // 环境变量拼写错误同样显式失败。
        Some(text) => Some(
            parse_provider(text).ok_or_else(|| ConfigError::InvalidProvider {
                path: PathBuf::from("SPECPROBE_PROVIDER"),
                value: text.to_owned(),
            })?,
        ),
        None => None,
    };

    let requirements = cli.requirements.or_else(|| {
        config
            .and_then(|c| c.requirements.clone())
            .map(|relative| project_path.join(relative))
    });

    Ok(ResolvedSettings {
        base_url: cli
            .base_url
            .or(env.base_url)
            .or_else(|| config.and_then(|c| c.base_url.clone()))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned()),
        provider: cli
            .provider
            .or(env_provider)
            .or(config_provider)
            .unwrap_or_default(),
        requirements,
        launch_timeout_secs: cli
            .launch_timeout_secs
            .or_else(|| config.and_then(|c| c.launch_timeout_secs))
            .unwrap_or(DEFAULT_LAUNCH_TIMEOUT_SECS),
        browser_timeout_secs: cli
            .browser_timeout_secs
            .or_else(|| config.and_then(|c| c.browser_timeout_secs))
            .unwrap_or(DEFAULT_BROWSER_TIMEOUT_SECS),
        no_cache: cli.no_cache || config.and_then(|c| c.no_cache).unwrap_or(false),
    })
}

fn parse_provider(text: &str) -> Option<AiProviderKind> {
    AiProviderKind::from_str(text, true).ok()
}

/// `specprobe init` 写出的模板。
pub const CONFIG_TEMPLATE: &str = r#"# SpecProbe 项目配置(specprobe.toml)
# 优先级:CLI 显式参数 > 环境变量(SPECPROBE_BASE_URL / SPECPROBE_PROVIDER)> 本文件 > 内置默认。

# 被测 Web 应用的基础 URL。
base_url = "http://127.0.0.1:3000"

# AI Provider:mock(离线,默认)| openai-compatible | ollama。
# 凭据仍走环境变量:OPENAI_API_KEY / OPENAI_MODEL / OPENAI_BASE_URL,OLLAMA_MODEL。
# provider = "openai-compatible"

# 需求文档或目录(相对本文件所在目录);默认在项目内自动查找 README/PRD/SPEC/REQUIREMENTS/docs。
# requirements = "REQUIREMENTS.md"

# 超时(秒)。
# launch_timeout_secs = 15
# browser_timeout_secs = 10

# 禁用 AI 响应缓存(.specprobe/cache)。
# no_cache = true
"#;

/// 在 `path` 下生成配置模板;已存在且未 `force` 时报错。返回写出的文件路径。
pub fn write_template(path: &Path, force: bool) -> Result<PathBuf, std::io::Error> {
    let target = path.join(CONFIG_FILE_NAME);
    if target.exists() && !force {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists (use --force to overwrite)",
                target.display()
            ),
        ));
    }
    fs::write(&target, CONFIG_TEMPLATE)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        CliOverrides, DEFAULT_BASE_URL, EnvOverrides, load_project_config, resolve_settings,
        write_template,
    };
    use crate::ai::AiProviderKind;
    use crate::testutil::temp_project;

    #[test]
    fn resolves_precedence_cli_env_config_default() {
        let root = temp_project("specprobe-config-precedence");
        fs::write(
            root.join("specprobe.toml"),
            r#"
base_url = "http://config:1"
provider = "ollama"
launch_timeout_secs = 30
"#,
        )
        .expect("write config");
        let loaded = load_project_config(&root)
            .expect("load succeeds")
            .expect("config present");

        // CLI 覆盖一切;env 覆盖 config;config 覆盖默认。
        let resolved = resolve_settings(
            &root,
            CliOverrides {
                base_url: Some("http://cli:1".to_owned()),
                ..Default::default()
            },
            EnvOverrides {
                base_url: Some("http://env:1".to_owned()),
                provider: Some("openai-compatible".to_owned()),
            },
            Some(&loaded),
        )
        .expect("resolve succeeds");

        assert_eq!(resolved.base_url, "http://cli:1");
        assert_eq!(resolved.provider, AiProviderKind::OpenaiCompatible);
        assert_eq!(resolved.launch_timeout_secs, 30);
        // 未在任何层指定 → 内置默认。
        assert_eq!(resolved.browser_timeout_secs, 10);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn config_requirements_resolves_relative_to_project() {
        let root = temp_project("specprobe-config-req");
        fs::write(root.join("specprobe.toml"), r#"requirements = "PRD.md""#).expect("write config");
        let loaded = load_project_config(&root)
            .expect("load succeeds")
            .expect("config present");

        let resolved = resolve_settings(
            &root,
            CliOverrides::default(),
            EnvOverrides::default(),
            Some(&loaded),
        )
        .expect("resolve succeeds");

        assert_eq!(resolved.requirements, Some(root.join("PRD.md")));
        assert_eq!(resolved.base_url, DEFAULT_BASE_URL);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn unknown_field_and_bad_provider_fail_loudly() {
        let root = temp_project("specprobe-config-bad");
        fs::write(root.join("specprobe.toml"), "base_urll = \"typo\"").expect("write config");
        assert!(load_project_config(&root).is_err());

        fs::write(root.join("specprobe.toml"), "provider = \"gpt5\"").expect("write config");
        let loaded = load_project_config(&root)
            .expect("load succeeds")
            .expect("config present");
        let error = resolve_settings(
            &root,
            CliOverrides::default(),
            EnvOverrides::default(),
            Some(&loaded),
        )
        .expect_err("bad provider rejected");
        assert!(error.to_string().contains("gpt5"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn template_writes_and_refuses_overwrite() {
        let root = temp_project("specprobe-config-init");

        let path = write_template(&root, false).expect("first write succeeds");
        assert!(path.is_file());
        // 模板本身必须是可解析的合法配置。
        assert!(
            load_project_config(&root)
                .expect("parse template")
                .is_some()
        );

        assert!(write_template(&root, false).is_err());
        assert!(write_template(&root, true).is_ok());
        fs::remove_dir_all(root).expect("cleanup");
    }
}

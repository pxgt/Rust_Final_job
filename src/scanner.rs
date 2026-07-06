use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

const SKIPPED_DIRECTORIES: &[&str] = &[
    ".git",
    ".idea",
    ".next",
    ".venv",
    ".vscode",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "target",
    "vendor",
];

#[derive(Debug, Error)]
pub enum ScanError {
    #[error("project path does not exist: {0}")]
    NotFound(PathBuf),
    #[error("project path is not a directory: {0}")]
    NotDirectory(PathBuf),
    #[error("failed to inspect {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Serialize)]
pub struct ProjectProfile {
    pub root: String,
    pub git_repository: bool,
    pub manifests: Vec<String>,
    pub requirement_documents: Vec<String>,
    pub technologies: Vec<String>,
    pub languages: Vec<LanguageStat>,
    pub source_file_count: usize,
    pub test_file_count: usize,
    pub skipped_directories: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct LanguageStat {
    pub language: String,
    pub files: usize,
}

#[derive(Default)]
struct ScanState {
    manifests: BTreeSet<String>,
    requirement_documents: BTreeSet<String>,
    technologies: BTreeSet<String>,
    language_files: BTreeMap<String, usize>,
    source_file_count: usize,
    test_file_count: usize,
}

pub fn scan_project(path: &Path) -> Result<ProjectProfile, ScanError> {
    if !path.exists() {
        return Err(ScanError::NotFound(path.to_path_buf()));
    }
    if !path.is_dir() {
        return Err(ScanError::NotDirectory(path.to_path_buf()));
    }

    let root = fs::canonicalize(path).map_err(|source| ScanError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut state = ScanState::default();
    scan_directory(&root, &root, &mut state)?;
    inspect_manifest_contents(&root, &mut state);

    let languages = state
        .language_files
        .into_iter()
        .map(|(language, files)| LanguageStat { language, files })
        .collect();

    Ok(ProjectProfile {
        root: display_path(&root),
        git_repository: root.join(".git").exists(),
        manifests: state.manifests.into_iter().collect(),
        requirement_documents: state.requirement_documents.into_iter().collect(),
        technologies: state.technologies.into_iter().collect(),
        languages,
        source_file_count: state.source_file_count,
        test_file_count: state.test_file_count,
        skipped_directories: SKIPPED_DIRECTORIES
            .iter()
            .map(|directory| (*directory).to_owned())
            .collect(),
    })
}

fn scan_directory(root: &Path, directory: &Path, state: &mut ScanState) -> Result<(), ScanError> {
    let entries = fs::read_dir(directory).map_err(|source| ScanError::Io {
        path: directory.to_path_buf(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| ScanError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| ScanError::Io {
            path: path.clone(),
            source,
        })?;

        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let name = entry.file_name();
            if !SKIPPED_DIRECTORIES.iter().any(|skipped| name == *skipped) {
                scan_directory(root, &path, state)?;
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        inspect_file(root, &path, state);
    }

    Ok(())
}

fn inspect_file(root: &Path, path: &Path, state: &mut ScanState) {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let normalized = normalize_path(relative);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    if is_manifest(file_name) {
        state.manifests.insert(normalized.clone());
        add_manifest_technology(file_name, &mut state.technologies);
    }
    if is_requirement_document(relative, file_name) {
        state.requirement_documents.insert(normalized);
    }

    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return;
    };
    let Some(language) = language_for_extension(extension) else {
        return;
    };

    *state.language_files.entry(language.to_owned()).or_default() += 1;
    state.source_file_count += 1;
    if is_test_file(relative, file_name) {
        state.test_file_count += 1;
    }
}

fn inspect_manifest_contents(root: &Path, state: &mut ScanState) {
    inspect_package_json(root, state);
    inspect_cargo_toml(root, state);

    for (file, technology) in [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "Yarn"),
        ("bun.lock", "Bun"),
        ("package-lock.json", "npm"),
        ("playwright.config.ts", "Playwright"),
        ("playwright.config.js", "Playwright"),
        ("Dockerfile", "Docker"),
        ("docker-compose.yml", "Docker Compose"),
        ("compose.yml", "Docker Compose"),
    ] {
        if root.join(file).is_file() {
            state.technologies.insert(technology.to_owned());
        }
    }
}

fn inspect_package_json(root: &Path, state: &mut ScanState) {
    let path = root.join("package.json");
    let Ok(contents) = fs::read_to_string(path) else {
        return;
    };
    let Ok(document) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return;
    };

    for section in ["dependencies", "devDependencies"] {
        let Some(dependencies) = document.get(section).and_then(|value| value.as_object()) else {
            continue;
        };
        for dependency in dependencies.keys() {
            if let Some(technology) = technology_for_node_dependency(dependency) {
                state.technologies.insert(technology.to_owned());
            }
        }
    }
}

fn inspect_cargo_toml(root: &Path, state: &mut ScanState) {
    let Ok(contents) = fs::read_to_string(root.join("Cargo.toml")) else {
        return;
    };

    for (dependency, technology) in [
        ("axum", "Axum"),
        ("actix-web", "Actix Web"),
        ("tokio", "Tokio"),
        ("tauri", "Tauri"),
        ("rocket", "Rocket"),
    ] {
        if contents
            .lines()
            .map(str::trim)
            .any(|line| line.starts_with(&format!("{dependency} ")))
        {
            state.technologies.insert(technology.to_owned());
        }
    }
}

fn is_manifest(file_name: &str) -> bool {
    matches!(
        file_name,
        "Cargo.toml"
            | "package.json"
            | "pyproject.toml"
            | "requirements.txt"
            | "go.mod"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "Dockerfile"
    )
}

fn add_manifest_technology(file_name: &str, technologies: &mut BTreeSet<String>) {
    let technology = match file_name {
        "Cargo.toml" => "Rust",
        "package.json" => "Node.js",
        "pyproject.toml" | "requirements.txt" => "Python",
        "go.mod" => "Go",
        "pom.xml" | "build.gradle" | "build.gradle.kts" => "Java",
        "Dockerfile" => "Docker",
        _ => return,
    };
    technologies.insert(technology.to_owned());
}

fn technology_for_node_dependency(dependency: &str) -> Option<&'static str> {
    match dependency {
        "react" => Some("React"),
        "next" => Some("Next.js"),
        "vue" => Some("Vue"),
        "nuxt" => Some("Nuxt"),
        "svelte" => Some("Svelte"),
        "@angular/core" => Some("Angular"),
        "vite" => Some("Vite"),
        "express" => Some("Express"),
        "fastify" => Some("Fastify"),
        "@playwright/test" | "playwright" => Some("Playwright"),
        "vitest" => Some("Vitest"),
        "jest" => Some("Jest"),
        "typescript" => Some("TypeScript"),
        "tailwindcss" => Some("Tailwind CSS"),
        _ => None,
    }
}

fn language_for_extension(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "rs" => Some("Rust"),
        "ts" | "tsx" => Some("TypeScript"),
        "js" | "jsx" | "mjs" | "cjs" => Some("JavaScript"),
        "py" => Some("Python"),
        "go" => Some("Go"),
        "java" => Some("Java"),
        "kt" | "kts" => Some("Kotlin"),
        "c" | "h" => Some("C"),
        "cc" | "cpp" | "cxx" | "hpp" => Some("C++"),
        "cs" => Some("C#"),
        "html" | "htm" => Some("HTML"),
        "css" | "scss" | "sass" | "less" => Some("CSS"),
        "sql" => Some("SQL"),
        "sh" | "bash" | "ps1" => Some("Shell"),
        _ => None,
    }
}

fn is_requirement_document(relative: &Path, file_name: &str) -> bool {
    let upper_name = file_name.to_ascii_uppercase();
    let named_document = upper_name.starts_with("README")
        || upper_name.starts_with("REQUIREMENT")
        || upper_name.starts_with("SPEC")
        || upper_name.starts_with("PRD")
        || upper_name == "PROJECT.MD";
    let in_docs = relative
        .components()
        .next()
        .is_some_and(|component| component.as_os_str().eq_ignore_ascii_case("docs"));
    let markdown = relative
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("txt")
        });

    markdown && (named_document || in_docs)
}

fn is_test_file(relative: &Path, file_name: &str) -> bool {
    let lower_name = file_name.to_ascii_lowercase();
    let named_test = lower_name.starts_with("test_")
        || lower_name.contains(".test.")
        || lower_name.contains(".spec.")
        || lower_name
            .split_once('.')
            .is_some_and(|(stem, _)| stem.ends_with("_test"));
    let in_test_directory = relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(
            name.to_ascii_lowercase().as_str(),
            "test" | "tests" | "__tests__"
        )
    });

    named_test || in_test_directory
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    value.strip_prefix(r"\\?\").unwrap_or(&value).to_owned()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{display_path, scan_project};

    #[test]
    fn removes_windows_verbatim_path_prefix() {
        assert_eq!(
            display_path(std::path::Path::new(r"\\?\D:\project")),
            r"D:\project"
        );
    }

    #[test]
    fn detects_web_stack_requirements_and_tests() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("specprobe-scanner-{suffix}"));
        fs::create_dir_all(root.join("src")).expect("create source directory");
        fs::create_dir_all(root.join("tests")).expect("create test directory");
        fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"react":"latest"},"devDependencies":{"@playwright/test":"latest","typescript":"latest"}}"#,
        )
        .expect("write package manifest");
        fs::write(root.join("README.md"), "# Product requirements").expect("write README");
        fs::write(root.join("src").join("app.tsx"), "").expect("write source");
        fs::write(root.join("tests").join("login.spec.ts"), "").expect("write test");

        let profile = scan_project(&root).expect("scan should succeed");

        assert!(profile.technologies.contains(&"React".to_owned()));
        assert!(profile.technologies.contains(&"Playwright".to_owned()));
        assert_eq!(profile.source_file_count, 2);
        assert_eq!(profile.test_file_count, 1);
        assert_eq!(profile.requirement_documents, vec!["README.md"]);

        fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn ignores_dependency_directories() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("specprobe-ignore-{suffix}"));
        fs::create_dir_all(root.join("node_modules").join("package")).expect("create directory");
        fs::write(
            root.join("node_modules").join("package").join("index.js"),
            "",
        )
        .expect("write dependency source");

        let profile = scan_project(&root).expect("scan should succeed");

        assert_eq!(profile.source_file_count, 0);
        fs::remove_dir_all(root).expect("remove test directory");
    }
}

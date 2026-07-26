use std::path::{Path, PathBuf};

use ad_lib::agents::{
    builtin_registry, persist_project_codex_runtime, project_runtime_descriptor_for_base_project,
    AgentContext, ProjectCodexRuntime, ResourceKind, ResourceRef, ResourceScope,
};
use sha2::{Digest, Sha256};

fn restore_env(previous_home: Option<String>, previous_codex_home: Option<String>) {
    match previous_home {
        Some(value) => std::env::set_var("AD_HOME", value),
        None => std::env::remove_var("AD_HOME"),
    }
    match previous_codex_home {
        Some(value) => std::env::set_var("CODEX_HOME", value),
        None => std::env::remove_var("CODEX_HOME"),
    }
}

fn legacy_project_id(project_path: &str, base_installation_id: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(project_path.as_bytes());
    digest.update([0]);
    digest.update(base_installation_id.as_bytes());
    let hex = format!("{:x}", digest.finalize());
    hex[..24].to_owned()
}

fn write_legacy_runtime(
    home: &Path,
    project_path: &str,
    base_installation_id: &str,
    config: &str,
) -> ProjectCodexRuntime {
    let project_id = legacy_project_id(project_path, base_installation_id);
    let runtime_home = home.join(".ad/codex-homes").join(&project_id);
    std::fs::create_dir_all(&runtime_home).unwrap();
    std::fs::write(runtime_home.join("config.toml"), config).unwrap();
    let runtime = ProjectCodexRuntime {
        project_id: project_id.clone(),
        project_path: project_path.to_owned(),
        base_installation_id: base_installation_id.to_owned().into(),
        runtime_installation_id: format!("codex:{}", runtime_home.display()).into(),
        runtime_home,
        base_config_digest: None,
        generated_config_digest: None,
        profile_id: None,
        applied_inherit_base_config: true,
        manifest_digest: None,
    };
    let state_dir = home.join(".ad/state/codex-project-runtimes");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::write(
        state_dir.join(format!("{project_id}.json")),
        serde_json::to_vec_pretty(&runtime).unwrap(),
    )
    .unwrap();
    runtime
}

fn runtime_config_target(runtime: &ProjectCodexRuntime) -> PathBuf {
    let context = AgentContext {
        installation_id: runtime.runtime_installation_id.clone(),
        project_path: Some(runtime.project_path.clone()),
    };
    let resource = ResourceRef {
        installation_id: runtime.runtime_installation_id.clone(),
        project_path: Some(runtime.project_path.clone()),
        kind: ResourceKind::Settings,
        scope: ResourceScope::Project,
        logical_id: "runtime-config".into(),
    };
    builtin_registry()
        .resolve_resource(&context, &resource)
        .unwrap()
        .path()
        .to_path_buf()
}

#[test]
#[serial_test::serial(home_env)]
fn project_name_is_the_runtime_identity_across_base_installations() {
    let temp = tempfile::tempdir().unwrap();
    let default_home = temp.path().join(".codex");
    let alternate_home = temp.path().join("codex-work");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&default_home).unwrap();
    std::fs::create_dir_all(&alternate_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::set_var("CODEX_HOME", &alternate_home);

    let canonical_default_home = std::fs::canonicalize(&default_home).unwrap();
    let canonical_alternate_home = std::fs::canonicalize(&alternate_home).unwrap();
    let installations = builtin_registry().discover();
    let default_base = installations
        .iter()
        .find(|installation| installation.root_path == canonical_default_home.to_string_lossy())
        .unwrap();
    let alternate_base = installations
        .iter()
        .find(|installation| installation.root_path == canonical_alternate_home.to_string_lossy())
        .unwrap();
    let default_runtime = ProjectCodexRuntime::derive(default_base, &project).unwrap();
    let alternate_runtime = ProjectCodexRuntime::derive(alternate_base, &project).unwrap();

    assert_eq!(default_runtime.project_id, "project");
    assert_eq!(default_runtime.runtime_home, alternate_runtime.runtime_home);
    assert_eq!(default_runtime.runtime_home.file_name().unwrap(), "project");
    assert!(!default_runtime
        .runtime_installation_id
        .as_str()
        .contains("::base::"));
    assert!(alternate_runtime
        .runtime_installation_id
        .as_str()
        .contains("::base::codex:"));

    std::fs::create_dir_all(&default_runtime.runtime_home).unwrap();
    std::fs::write(
        default_runtime.runtime_home.join("config.toml"),
        "model = \"gpt-5.6\"\n",
    )
    .unwrap();
    persist_project_codex_runtime(&default_runtime).unwrap();
    let rebound = project_runtime_descriptor_for_base_project(&alternate_base.id, &project)
        .unwrap()
        .unwrap();

    assert_eq!(rebound.runtime_home, default_runtime.runtime_home);
    assert_eq!(rebound.base_installation_id, alternate_base.id);
    assert!(rebound
        .runtime_installation_id
        .as_str()
        .contains("::base::codex:"));
    assert!(rebound.generated_config_digest.is_none());

    restore_env(previous_home, previous_codex_home);
}

#[test]
#[serial_test::serial(home_env)]
fn same_named_projects_fail_before_sharing_a_runtime_home() {
    let temp = tempfile::tempdir().unwrap();
    let default_home = temp.path().join(".codex");
    let first_project = temp.path().join("first/project");
    let second_project = temp.path().join("second/project");
    let case_variant_project = temp.path().join("third/Project");
    std::fs::create_dir_all(&default_home).unwrap();
    std::fs::create_dir_all(&first_project).unwrap();
    std::fs::create_dir_all(&second_project).unwrap();
    std::fs::create_dir_all(&case_variant_project).unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let base = builtin_registry()
        .discover()
        .into_iter()
        .find(|installation| installation.agent_id.as_str() == "codex")
        .unwrap();
    let first = ProjectCodexRuntime::derive(&base, &first_project).unwrap();
    std::fs::create_dir_all(&first.runtime_home).unwrap();
    persist_project_codex_runtime(&first).unwrap();

    let error = ProjectCodexRuntime::derive(&base, &second_project).unwrap_err();
    let case_error = ProjectCodexRuntime::derive(&base, &case_variant_project).unwrap_err();

    assert!(error
        .to_string()
        .contains("already used by another project"));
    assert!(case_error
        .to_string()
        .contains("already used by another project"));
    let state: ProjectCodexRuntime = serde_json::from_slice(
        &std::fs::read(
            temp.path()
                .join(".ad/state/codex-project-runtimes/project.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(state.project_path, first.project_path);

    restore_env(previous_home, previous_codex_home);
}

#[test]
#[serial_test::serial(home_env)]
fn legacy_hashed_runtime_migrates_without_breaking_old_resources() {
    let temp = tempfile::tempdir().unwrap();
    let default_home = temp.path().join(".codex");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&default_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::remove_var("CODEX_HOME");

    let project_path = std::fs::canonicalize(&project)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let canonical_home = std::fs::canonicalize(temp.path()).unwrap();
    let base_id = format!(
        "codex:{}",
        std::fs::canonicalize(&default_home).unwrap().display()
    );
    let legacy = write_legacy_runtime(
        &canonical_home,
        &project_path,
        &base_id,
        "model = \"gpt-5.6\"\n",
    );
    let named_home = canonical_home.join(".ad/codex-homes/project");
    let named_id = format!("codex:{}", named_home.display());
    let installation = builtin_registry()
        .discover()
        .into_iter()
        .find(|installation| installation.id.as_str() == named_id)
        .unwrap();

    assert_eq!(installation.root_path, named_home.to_string_lossy());
    assert_eq!(
        std::fs::read_to_string(named_home.join("config.toml")).unwrap(),
        "model = \"gpt-5.6\"\n"
    );
    assert!(std::fs::symlink_metadata(&legacy.runtime_home)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        runtime_config_target(&legacy),
        named_home.join("config.toml")
    );
    assert!(temp
        .path()
        .join(".ad/state/codex-project-runtimes/project.json")
        .is_file());

    restore_env(previous_home, previous_codex_home);
}

#[test]
#[serial_test::serial(home_env)]
fn multiple_legacy_bases_remain_loadable_without_automatic_consolidation() {
    let temp = tempfile::tempdir().unwrap();
    let default_home = temp.path().join(".codex");
    let alternate_home = temp.path().join("codex-work");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&default_home).unwrap();
    std::fs::create_dir_all(&alternate_home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let previous_home = std::env::var("AD_HOME").ok();
    let previous_codex_home = std::env::var("CODEX_HOME").ok();
    std::env::set_var("AD_HOME", temp.path());
    std::env::set_var("CODEX_HOME", &alternate_home);

    let project_path = std::fs::canonicalize(&project)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let canonical_home = std::fs::canonicalize(temp.path()).unwrap();
    let default = write_legacy_runtime(
        &canonical_home,
        &project_path,
        &format!(
            "codex:{}",
            std::fs::canonicalize(&default_home).unwrap().display()
        ),
        "model = \"default\"\n",
    );
    let alternate = write_legacy_runtime(
        &canonical_home,
        &project_path,
        &format!(
            "codex:{}",
            std::fs::canonicalize(&alternate_home).unwrap().display()
        ),
        "model = \"alternate\"\n",
    );
    let discovered = builtin_registry().discover();

    assert!(discovered
        .iter()
        .any(|installation| installation.id == default.runtime_installation_id));
    assert!(discovered
        .iter()
        .any(|installation| installation.id == alternate.runtime_installation_id));
    assert!(!canonical_home.join(".ad/codex-homes/project").exists());
    assert_eq!(
        std::fs::read_to_string(runtime_config_target(&default)).unwrap(),
        "model = \"default\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(runtime_config_target(&alternate)).unwrap(),
        "model = \"alternate\"\n"
    );

    restore_env(previous_home, previous_codex_home);
}

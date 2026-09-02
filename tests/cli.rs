use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    home: PathBuf,
    repo: PathBuf,
    base: PathBuf,
    cones: PathBuf,
    global_git_config: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let repo = temp.path().join("monorepo");
        let base = temp.path().join("worktrees");
        let cones = temp.path().join("cones");
        let global_git_config = temp.path().join("gitconfig");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(repo.join("app")).unwrap();
        fs::create_dir_all(repo.join("other")).unwrap();
        fs::write(repo.join("root.txt"), "root\n").unwrap();
        fs::write(repo.join("app/code.txt"), "app\n").unwrap();
        fs::write(repo.join("other/code.txt"), "other\n").unwrap();
        fs::write(repo.join(".gitignore"), ".env\n").unwrap();
        fs::write(repo.join(".env"), "secret=local\n").unwrap();
        fs::write(&global_git_config, "").unwrap();

        run_git(&repo, &global_git_config, &["init", "-b", "main"]);
        run_git(
            &repo,
            &global_git_config,
            &["config", "user.name", "FWT Tests"],
        );
        run_git(
            &repo,
            &global_git_config,
            &["config", "user.email", "fwt@example.test"],
        );
        run_git(&repo, &global_git_config, &["add", "."]);
        run_git(&repo, &global_git_config, &["commit", "-m", "initial"]);

        Self {
            temp,
            home,
            repo,
            base,
            cones,
            global_git_config,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::cargo_bin("git-fwt").unwrap();
        command
            .current_dir(&self.repo)
            .env("HOME", &self.home)
            .env("FWT_BASE", &self.base)
            .env("FWT_CONE_DIR", &self.cones)
            .env("FWT_SEED", ".env")
            .env("GIT_CONFIG_GLOBAL", &self.global_git_config)
            .env("GIT_CONFIG_NOSYSTEM", "1");
        command
    }

    fn target(&self, branch: &str) -> PathBuf {
        self.base.join(format!("monorepo@{branch}"))
    }
}

fn run_git(repo: &Path, global_config: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", global_config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(status.success(), "git {:?} failed", args);
}

#[test]
fn sparse_new_preserves_order_invariants_and_seeds_local_state() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["cone", "set", "focused", "app"])
        .assert()
        .success();

    fixture
        .command()
        .args(["new", "feature/sparse", "--cone", "focused"])
        .assert()
        .success();

    let target = fixture.target("feature/sparse");
    assert!(target.join("app/code.txt").is_file());
    assert!(!target.join("other/code.txt").exists());
    assert_eq!(
        fs::read_to_string(target.join(".env")).unwrap(),
        "secret=local\n"
    );

    let extension = Command::new("git")
        .args(["-C"])
        .arg(&fixture.repo)
        .args(["config", "--bool", "extensions.worktreeConfig"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&extension.stdout).trim(), "true");

    let output = fixture.command().args(["ls", "--json"]).output().unwrap();
    assert!(output.status.success());
    let listing: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        listing["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| { entry["branch"] == "feature/sparse" && entry["kind"] == "worktree" })
    );

    let resolved = fixture
        .command()
        .args(["cd", "feature/sparse"])
        .output()
        .unwrap();
    assert!(resolved.status.success());
    assert_eq!(
        PathBuf::from(String::from_utf8_lossy(&resolved.stdout).trim()),
        fs::canonicalize(&target).unwrap()
    );

    fixture
        .command()
        .args(["rm", "feature/sparse"])
        .assert()
        .success();
    assert!(!target.exists());
}

#[test]
fn legacy_cones_migrate_to_self_describing_yaml_on_read() {
    let fixture = Fixture::new();
    let repo_cones = fixture.cones.join("monorepo");
    fs::create_dir_all(&repo_cones).unwrap();
    let legacy = repo_cones.join("legacy");
    fs::write(&legacy, "app\nother\n").unwrap();

    let output = fixture
        .command()
        .args(["cone", "ls", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!legacy.exists());
    let yaml_path = repo_cones.join("legacy.yaml");
    let yaml = fs::read_to_string(yaml_path).unwrap();
    assert!(yaml.contains("source: manual"));
    assert!(yaml.contains("bazel_target: null"));
    assert!(yaml.contains("derived_at: null"));
    let profiles: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(profiles[0]["name"], "legacy");
    assert_eq!(profiles[0]["staleness"], "not_applicable");
}

#[test]
fn new_uses_fwt_cone_default_when_no_cone_flag_is_given() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["cone", "set", "default-for-test", "app"])
        .assert()
        .success();
    fixture
        .command()
        .env("FWT_CONE_DEFAULT", "default-for-test")
        .args(["new", "uses-default"])
        .assert()
        .success();
    let target = fixture.target("uses-default");
    assert!(target.join("app/code.txt").is_file());
    assert!(!target.join("other/code.txt").exists());
}

#[test]
fn default_storage_paths_are_generic() {
    let fixture = Fixture::new();
    fixture
        .command()
        .env_remove("FWT_BASE")
        .env_remove("FWT_CONE_DIR")
        .env_remove("FWT_CONE_DEFAULT")
        .args(["cone", "set", "default", "app"])
        .assert()
        .success();
    assert!(
        fixture
            .home
            .join(".config/fwt/cones/monorepo/default.yaml")
            .is_file()
    );

    fixture
        .command()
        .env_remove("FWT_BASE")
        .env_remove("FWT_CONE_DIR")
        .env_remove("FWT_CONE_DEFAULT")
        .args(["new", "generic-defaults"])
        .assert()
        .success();
    assert!(
        fixture
            .home
            .join("worktrees/monorepo@generic-defaults/app/code.txt")
            .is_file()
    );
}

#[test]
fn bazel_derived_cone_records_provenance_and_maps_failures_to_exit_two() {
    let fixture = Fixture::new();
    let bin = fixture.temp.path().join("bin");
    fs::create_dir_all(&bin).unwrap();
    let bazel = bin.join("bazel");
    fs::write(
        &bazel,
        "#!/bin/sh\nif [ \"${FAIL_BAZEL-}\" = 1 ]; then echo query-failed >&2; exit 42; fi\nprintf '//app\\n//platform/utils\\n@external//ignored\\n'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&bazel).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&bazel, permissions).unwrap();
    }
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());

    fixture
        .command()
        .env("PATH", &path)
        .args(["cone", "derive", "buildable", "//app/..."])
        .assert()
        .success();
    let yaml = fs::read_to_string(fixture.cones.join("monorepo/buildable.yaml")).unwrap();
    assert!(yaml.contains("source: bazel"));
    assert!(yaml.contains("bazel_target: //app/..."));
    assert!(!yaml.contains("derived_at: null"));

    fixture
        .command()
        .env("PATH", path)
        .env("FAIL_BAZEL", "1")
        .args(["cone", "derive", "broken", "//app/..."])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("query-failed"));
}

#[test]
fn skill_install_is_versioned_and_idempotent() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["skill", "install", "--agent", "claude-code"])
        .assert()
        .success();
    let skill = fixture.home.join(".claude/skills/fwt/SKILL.md");
    let content = fs::read_to_string(&skill).unwrap();
    assert!(content.contains("generated_by: fwt 0.1.0"));
    fixture
        .command()
        .args(["skill", "install", "--agent", "claude-code"])
        .assert()
        .success()
        .stdout(predicates::str::contains("already up to date"));
}

#[test]
fn usage_errors_exit_one() {
    let fixture = Fixture::new();
    fixture.command().args(["new"]).assert().code(1);
    fixture
        .command()
        .args(["cone", "set", "../escape", "app"])
        .assert()
        .code(1);
    fixture
        .command()
        .args(["new", "main", "--full"])
        .assert()
        .code(2);
}

#[test]
fn listing_discovers_pre_registry_clones() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.base).unwrap();
    let clone = fixture.target("legacy-clone");
    let status = Command::new("git")
        .args(["clone", "--quiet"])
        .arg(&fixture.repo)
        .arg(&clone)
        .status()
        .unwrap();
    assert!(status.success());
    run_git(
        &clone,
        &fixture.global_git_config,
        &["remote", "add", "local", fixture.repo.to_str().unwrap()],
    );
    run_git(
        &clone,
        &fixture.global_git_config,
        &["checkout", "-b", "legacy-clone"],
    );

    let output = fixture.command().args(["ls", "--json"]).output().unwrap();
    assert!(output.status.success());
    let listing: Value = serde_json::from_slice(&output.stdout).unwrap();
    let entry = listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["branch"] == "legacy-clone")
        .expect("legacy clone should be listed");
    assert_eq!(entry["kind"], "cow_clone");
    assert_eq!(entry["registered"], false);
}

#[test]
fn a_branch_on_exactly_one_remote_is_checked_out_with_tracking() {
    let fixture = Fixture::new();
    let remote = fixture.temp.path().join("remote.git");
    let status = Command::new("git")
        .args(["init", "--bare"])
        .arg(&remote)
        .status()
        .unwrap();
    assert!(status.success());
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &["branch", "remote-task"],
    );
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &["push", "origin", "remote-task"],
    );
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &["branch", "-D", "remote-task"],
    );

    fixture
        .command()
        .args(["new", "remote-task", "--full"])
        .assert()
        .success();
    let target = fixture.target("remote-task");
    let upstream = Command::new("git")
        .args(["-C"])
        .arg(target)
        .args([
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ])
        .output()
        .unwrap();
    assert!(upstream.status.success());
    assert_eq!(
        String::from_utf8_lossy(&upstream.stdout).trim(),
        "origin/remote-task"
    );
}

#[test]
fn tune_applies_every_documented_git_setting() {
    let fixture = Fixture::new();
    let bin = fixture.temp.path().join("fake-git-bin");
    let log = fixture.temp.path().join("git.log");
    fs::create_dir_all(&bin).unwrap();
    let wrapper = bin.join("git");
    fs::write(
        &wrapper,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$FWT_GIT_LOG\"\ncase \" $* \" in\n  *' rev-parse '*|*' remote get-url '*) exec \"$FWT_REAL_GIT\" \"$@\" ;;\n  *) exit 0 ;;\nesac\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&wrapper).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&wrapper, permissions).unwrap();
    }
    let real_git = Command::new("which").arg("git").output().unwrap();
    assert!(real_git.status.success());
    let real_git = String::from_utf8_lossy(&real_git.stdout).trim().to_owned();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());

    fixture
        .command()
        .env("PATH", path)
        .env("FWT_GIT_LOG", &log)
        .env("FWT_REAL_GIT", real_git)
        .args(["tune"])
        .assert()
        .success();

    let log = fs::read_to_string(log).unwrap();
    for expected in [
        "config extensions.worktreeConfig true",
        "config index.version 4",
        "config core.untrackedCache true",
        "config core.fsmonitor true",
        "config core.commitGraph true",
        "config fetch.writeCommitGraph true",
        "config checkout.workers 0",
        "update-index --index-version 4",
        "commit-graph write --reachable --changed-paths",
        "maintenance start",
    ] {
        assert!(log.contains(expected), "missing `{expected}` in:\n{log}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn cow_clones_are_registered_listed_and_trashed() {
    let fixture = Fixture::new();
    fs::write(fixture.repo.join("root.txt"), "dirty source\n").unwrap();
    fixture
        .command()
        .args(["new", "cow-task", "--cow"])
        .assert()
        .success()
        .stderr(predicates::str::contains("uncommitted changes"));
    let target = fixture.target("cow-task");
    assert_eq!(
        fs::read_to_string(target.join("root.txt")).unwrap(),
        "dirty source\n"
    );
    let registry = fs::read_to_string(fixture.home.join(".config/fwt/clones.json")).unwrap();
    assert!(registry.contains("cow-task"));

    let output = fixture.command().args(["ls", "--json"]).output().unwrap();
    let listing: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        listing["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| { entry["branch"] == "cow-task" && entry["kind"] == "cow_clone" })
    );

    fixture
        .command()
        .args(["rm", "cow-task"])
        .assert()
        .success();
    assert!(!target.exists());
    assert!(
        fs::read_dir(fixture.home.join(".Trash"))
            .unwrap()
            .next()
            .is_some()
    );
}

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use assert_cmd::prelude::*;
use predicates::prelude::PredicateBooleanExt;
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
            .env("FWT_CONE_DEFAULT", "default")
            .env("GIT_CONFIG_GLOBAL", &self.global_git_config)
            .env("GIT_CONFIG_NOSYSTEM", "1");
        command
    }

    fn target(&self, branch: &str) -> PathBuf {
        self.base.join(format!("monorepo@{branch}"))
    }
}

fn run_git(repo: &Path, global_config: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", global_config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
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
    assert!(content.contains(&format!("generated_by: fwt {}", env!("CARGO_PKG_VERSION"))));
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
        .current_dir(&target)
        .args(["new", "cow-child", "--full"])
        .assert()
        .success();
    fixture
        .command()
        .args(["rm", "cow-task"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("owns linked worktrees"));
    assert!(target.exists());
    fixture
        .command()
        .args(["rm", "cow-child"])
        .assert()
        .success();

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

#[test]
fn removal_preserves_dirty_worktrees_unless_force_is_explicit() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["new", "dirty", "--full"])
        .assert()
        .success();
    let target = fixture.target("dirty");
    fs::write(target.join("root.txt"), "work in progress\n").unwrap();
    fixture.command().args(["rm", "dirty"]).assert().failure();
    assert_eq!(
        fs::read_to_string(target.join("root.txt")).unwrap(),
        "work in progress\n"
    );
    fixture
        .command()
        .args(["rm", "dirty", "--force"])
        .assert()
        .success();
    assert!(!target.exists());
}

#[test]
fn new_branch_uses_the_invoking_worktrees_head() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["new", "parent", "--full"])
        .assert()
        .success();
    let parent = fixture.target("parent");
    fs::write(parent.join("parent.txt"), "parent commit\n").unwrap();
    run_git(&parent, &fixture.global_git_config, &["add", "parent.txt"]);
    run_git(
        &parent,
        &fixture.global_git_config,
        &["commit", "-m", "feat: parent change"],
    );
    fixture
        .command()
        .current_dir(&parent)
        .args(["new", "child", "--full"])
        .assert()
        .success();
    assert!(fixture.target("child").join("parent.txt").exists());
}

#[test]
fn remote_branch_matching_does_not_match_a_suffix() {
    let fixture = Fixture::new();
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &["update-ref", "refs/remotes/origin/feature/task", "HEAD"],
    );
    fixture
        .command()
        .args(["new", "task", "--full"])
        .assert()
        .success();
    let upstream = Command::new("git")
        .arg("-C")
        .arg(fixture.target("task"))
        .args(["rev-parse", "--verify", "@{upstream}"])
        .output()
        .unwrap();
    assert!(
        !upstream.status.success(),
        "task must not track origin/feature/task"
    );
}

#[test]
fn existing_target_must_belong_to_the_requested_branch_and_repository() {
    let fixture = Fixture::new();
    let target = fixture.target("collision");
    fs::create_dir_all(&target).unwrap();
    run_git(
        &target,
        &fixture.global_git_config,
        &["init", "-b", "unrelated"],
    );
    fixture
        .command()
        .args(["new", "collision", "--full"])
        .assert()
        .code(1);
    assert!(target.join(".git").exists());
}

#[test]
fn full_worktree_does_not_inherit_legacy_shared_sparse_settings() {
    let fixture = Fixture::new();
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &["config", "core.sparseCheckout", "true"],
    );
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &["config", "core.sparseCheckoutCone", "true"],
    );
    fs::create_dir_all(fixture.repo.join(".git/info")).unwrap();
    fs::write(
        fixture.repo.join(".git/info/sparse-checkout"),
        "/*\n!/*/\n/app/\n",
    )
    .unwrap();
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &["read-tree", "-mu", "HEAD"],
    );
    assert!(!fixture.repo.join("other/code.txt").exists());
    fixture
        .command()
        .args(["new", "full", "--full"])
        .assert()
        .success();
    assert!(fixture.target("full").join("other/code.txt").exists());
    assert!(!fixture.repo.join("other/code.txt").exists());
}

#[test]
fn removal_protects_untracked_files_main_and_locked_worktrees() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["rm", "main", "--force"])
        .assert()
        .code(1);
    assert!(fixture.repo.join("root.txt").exists());
    fixture
        .command()
        .args(["new", "protected", "--full"])
        .assert()
        .success();
    let target = fixture.target("protected");
    fs::write(target.join("notes.txt"), "keep me\n").unwrap();
    fixture
        .command()
        .args(["rm", "protected"])
        .assert()
        .failure();
    assert!(target.join("notes.txt").exists());
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &["worktree", "lock", target.to_str().unwrap()],
    );
    fixture
        .command()
        .args(["rm", "protected", "--force"])
        .assert()
        .failure();
    assert!(target.join("notes.txt").exists());
}

#[test]
fn missing_cone_does_not_create_directories_or_change_git_configuration() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["new", "missing/cone"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("fwt cone set"));
    assert!(!fixture.base.exists());
    assert!(
        !fs::read_to_string(fixture.repo.join(".git/config"))
            .unwrap()
            .contains("worktreeConfig")
    );
}

#[test]
fn relative_base_is_resolved_from_the_invoking_directory() {
    let fixture = Fixture::new();
    fixture
        .command()
        .current_dir(fixture.repo.join("app"))
        .env("FWT_BASE", "../../relative worktrees")
        .args(["new", "relative", "--full"])
        .assert()
        .success();
    assert!(
        fixture
            .temp
            .path()
            .join("relative worktrees/monorepo@relative/app/code.txt")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn seed_copy_does_not_follow_a_destination_symlink_parent() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let source_outside = fixture.temp.path().join("outside");
    let target_outside = fixture.base.join("outside");
    fs::create_dir_all(&source_outside).unwrap();
    fs::create_dir_all(&target_outside).unwrap();
    fs::write(source_outside.join("secret"), "private local state\n").unwrap();
    symlink("../outside", fixture.repo.join("config")).unwrap();
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &["add", "config"],
    );
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &["commit", "-m", "test symlink"],
    );
    fixture
        .command()
        .env("FWT_SEED", "config/secret")
        .args(["new", "seed", "--full"])
        .assert()
        .success()
        .stderr(predicates::str::contains("symlink parent"));
    assert!(!target_outside.join("secret").exists());
}

#[cfg(unix)]
#[test]
fn branch_path_cannot_follow_a_symlink_outside_base() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = fixture.temp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::create_dir_all(&fixture.base).unwrap();
    symlink(&outside, fixture.target("feature")).unwrap();
    fixture
        .command()
        .args(["new", "feature/escape", "--full"])
        .assert()
        .code(1);
    assert!(!outside.join("escape").exists());
}

#[test]
fn cone_directories_cannot_inject_additional_stdin_lines() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["cone", "set", "bad", "app\nother"])
        .assert()
        .code(1);
    assert!(!fixture.cones.join("monorepo/bad.yaml").exists());
}

#[test]
fn init_appends_once_to_the_detected_shell_config_outside_a_repository() {
    for (shell, filename) in [("/bin/bash", ".bashrc"), ("/bin/zsh", ".zshrc")] {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join(filename);
        let original = b"# existing config\nexport KEEP_ME=yes";
        fs::write(&config, original).unwrap();
        let mut command = Command::cargo_bin("fwt").unwrap();
        command
            .current_dir(temp.path())
            .env("HOME", temp.path())
            .env("SHELL", shell)
            .env_remove("ZDOTDIR")
            .arg("init");
        command
            .assert()
            .success()
            .stdout(predicates::str::contains("Updated"));
        let installed = fs::read(&config).unwrap();
        assert!(installed.starts_with(original));
        assert!(
            String::from_utf8_lossy(&installed).contains("\neval \"$(git-fwt init --print)\"\n")
        );
        command
            .assert()
            .success()
            .stdout(predicates::str::contains("Already configured"));
        assert_eq!(installed, fs::read(&config).unwrap());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }
}

#[test]
fn init_supports_shell_override_and_custom_zdotdir() {
    let temp = tempfile::tempdir().unwrap();
    let zdotdir = temp.path().join("custom zsh config");
    Command::cargo_bin("fwt")
        .unwrap()
        .current_dir(temp.path())
        .env("HOME", temp.path())
        .env("SHELL", "/bin/fish")
        .env("ZDOTDIR", &zdotdir)
        .args(["init", "--shell", "zsh"])
        .assert()
        .success();
    assert!(zdotdir.join(".zshrc").is_file());
    assert!(!temp.path().join(".zshrc").exists());
    Command::cargo_bin("fwt")
        .unwrap()
        .current_dir(temp.path())
        .env("HOME", temp.path())
        .env("SHELL", "/bin/zsh")
        .env("ZDOTDIR", &zdotdir)
        .args(["init", "--shell", "bash"])
        .assert()
        .success();
    assert!(temp.path().join(".bashrc").is_file());
    assert!(!zdotdir.join(".bashrc").exists());
}

#[test]
fn init_rejects_unsupported_or_missing_shell_without_writing() {
    let temp = tempfile::tempdir().unwrap();
    for shell in [Some("/bin/fish"), Some(""), None] {
        let mut command = Command::cargo_bin("fwt").unwrap();
        command
            .current_dir(temp.path())
            .env("HOME", temp.path())
            .arg("init");
        if let Some(shell) = shell {
            command.env("SHELL", shell);
        } else {
            command.env_remove("SHELL");
        }
        command
            .assert()
            .code(1)
            .stderr(predicates::str::contains("--shell bash or --shell zsh"));
    }
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn init_preserves_manual_setup_and_rejects_incomplete_blocks() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join(".bashrc");
    let manual = "# existing setup\neval \"$(git-fwt init --print)\"\n";
    fs::write(&config, manual).unwrap();
    let mut command = Command::cargo_bin("fwt").unwrap();
    command
        .current_dir(temp.path())
        .env("HOME", temp.path())
        .args(["init", "--shell", "bash"]);
    command.assert().success();
    assert_eq!(fs::read_to_string(&config).unwrap(), manual);
    let incomplete = "# >>> fwt shell integration >>>\n";
    fs::write(&config, incomplete).unwrap();
    command
        .assert()
        .code(1)
        .stderr(predicates::str::contains("incomplete or edited"));
    assert_eq!(fs::read_to_string(&config).unwrap(), incomplete);
}

#[cfg(unix)]
#[test]
fn init_preserves_symlink_permissions_and_non_utf8_contents() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("tracked-bashrc");
    let original = b"# non-UTF8 comment: \xff\n";
    fs::write(&target, original).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    let config = temp.path().join(".bashrc");
    symlink(&target, &config).unwrap();
    Command::cargo_bin("fwt")
        .unwrap()
        .current_dir(temp.path())
        .env("HOME", temp.path())
        .args(["init", "--shell", "bash"])
        .assert()
        .success();
    assert_eq!(fs::read_link(&config).unwrap(), target);
    assert!(fs::read(&target).unwrap().starts_with(original));
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn init_print_mode_does_not_require_repository_or_settings() {
    let temp = tempfile::tempdir().unwrap();
    for binary in ["fwt", "git-fwt"] {
        Command::cargo_bin(binary)
            .unwrap()
            .current_dir(temp.path())
            .env_remove("HOME")
            .args(["init", "--print"])
            .assert()
            .success()
            .stdout(include_str!("../shell/fwt.sh"))
            .stderr("");
        Command::cargo_bin(binary)
            .unwrap()
            .current_dir(temp.path())
            .env("HOME", temp.path())
            .arg("shell-init")
            .assert()
            .code(1)
            .stderr(predicates::str::contains("unrecognized subcommand"));
        Command::cargo_bin(binary)
            .unwrap()
            .arg("--help")
            .assert()
            .success()
            .stdout(predicates::str::contains("  init "))
            .stdout(predicates::str::contains("shell-init").not());
    }
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn shell_integration_changes_directory_but_help_and_errors_do_not() {
    let fixture = Fixture::new();
    let base = fixture.temp.path().join("worktrees with spaces");
    fixture
        .command()
        .env("FWT_BASE", &base)
        .args(["new", "shell-task", "--full"])
        .assert()
        .success();
    let binary = fixture.command();
    let bin_dir = Path::new(binary.get_program()).parent().unwrap();
    for shell in ["bash", "zsh"] {
        let mut command = Command::new(shell);
        if shell == "bash" {
            command.args(["--noprofile", "--norc"]);
        } else {
            command.arg("-f");
        }
        for (name, value) in binary.get_envs() {
            if let Some(value) = value {
                command.env(name, value);
            }
        }
        let output = command
            .current_dir(&fixture.repo)
            .env(
                "PATH",
                format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap()),
            )
            .env("FWT_BASE", &base)
            .env("ZDOTDIR", &fixture.home)
            .env("FWT_TEST_SHELL", shell)
            .env(
                "FWT_TEST_RC",
                fixture
                    .home
                    .join(if shell == "bash" { ".bashrc" } else { ".zshrc" }),
            )
            .args([
                "-c",
                r#"
set -e
git-fwt init --shell "$FWT_TEST_SHELL" >/dev/null
. "$FWT_TEST_RC"
fwt init --shell "$FWT_TEST_SHELL" >/dev/null
fwt cd --help >/dev/null
test "$PWD" = "$(git rev-parse --show-toplevel)"
fwt cd shell-task
test -f app/code.txt
test "$PWD" = "$(git-fwt resolve shell-task)"
before=$PWD
if fwt cd nonexistent 2>/dev/null; then exit 1; fi
test "$PWD" = "$before"
"#,
            ])
            .output();
        match output {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("{shell} not installed; shell test skipped")
            }
            result => {
                let output = result.unwrap();
                assert!(
                    output.status.success(),
                    "{shell}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn failed_worktree_add_does_not_remove_a_competing_destination() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let bin = fixture.temp.path().join("git-wrapper");
    fs::create_dir_all(&bin).unwrap();
    let wrapper = bin.join("git");
    fs::write(
        &wrapper,
        r#"#!/bin/sh
case " $* " in
  *' worktree add '*)
    mkdir -p "$FWT_COMPETING_TARGET"
    printf 'another creator\n' > "$FWT_COMPETING_TARGET/keep.txt"
    exit 1
    ;;
  *) exec "$FWT_REAL_GIT" "$@" ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let real_git = Command::new("which").arg("git").output().unwrap();
    assert!(real_git.status.success());
    fixture
        .command()
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .env(
            "FWT_REAL_GIT",
            String::from_utf8(real_git.stdout).unwrap().trim(),
        )
        .env("FWT_COMPETING_TARGET", fixture.target("race"))
        .args(["new", "race", "--full"])
        .assert()
        .code(2);
    assert_eq!(
        fs::read_to_string(fixture.target("race").join("keep.txt")).unwrap(),
        "another creator\n"
    );
}

#[test]
fn repository_scope_does_not_include_another_sources_same_named_clone() {
    let fixture = Fixture::new();
    let other_source = fixture.temp.path().join("other-source/monorepo");
    fs::create_dir_all(other_source.parent().unwrap()).unwrap();
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &[
            "clone",
            "--quiet",
            fixture.repo.to_str().unwrap(),
            other_source.to_str().unwrap(),
        ],
    );
    let clone = fixture.target("foreign");
    fs::create_dir_all(&fixture.base).unwrap();
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &[
            "clone",
            "--quiet",
            other_source.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    run_git(
        &clone,
        &fixture.global_git_config,
        &["remote", "add", "local", other_source.to_str().unwrap()],
    );
    run_git(
        &clone,
        &fixture.global_git_config,
        &["checkout", "-b", "foreign"],
    );
    fixture.command().args(["rm", "foreign"]).assert().code(1);
    assert!(clone.join(".git").is_dir());
}

#[test]
fn registered_clone_is_listable_after_its_source_is_moved() {
    let fixture = Fixture::new();
    let clone = fixture.target("orphan");
    fs::create_dir_all(&fixture.base).unwrap();
    run_git(
        &fixture.repo,
        &fixture.global_git_config,
        &[
            "clone",
            "--quiet",
            fixture.repo.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    let registry = fixture.home.join(".config/fwt/clones.json");
    fs::create_dir_all(registry.parent().unwrap()).unwrap();
    fs::write(
        &registry,
        serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "clones": [{
                "path": fs::canonicalize(&clone).unwrap(),
                "source": fs::canonicalize(&fixture.repo).unwrap(),
                "repo": "monorepo",
                "branch": "orphan",
                "created_at": "2026-09-06T00:00:00Z"
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::rename(&fixture.repo, fixture.temp.path().join("moved-source")).unwrap();
    let output = fixture
        .command()
        .current_dir(&clone)
        .args(["ls", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let listing: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        listing["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["kind"] == "cow_clone")
    );
}

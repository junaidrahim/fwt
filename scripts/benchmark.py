#!/usr/bin/env python3
"""Reproducible full/native-sparse/fwt-sparse checkout benchmark (stdlib only).

Run from any directory. Downloads pinned, shallow snapshots into a dedicated
workspace. Never runs upstream builds, hooks, Bazel, submodules, or LFS filters.
Only benchmark-created worktrees and branches are removed, using normal Git
removal (no --force). Sources and raw results remain for inspection.
"""

import argparse
import concurrent.futures
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
MODES = ("git_full", "git_sparse", "fwt_sparse")


def run(argv, cwd, env, data=None):
    result = subprocess.run(
        [str(arg) for arg in argv], cwd=cwd, env=env, input=data,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
    )
    if result.returncode:
        raise RuntimeError(
            f"{argv!r} in {cwd} exited {result.returncode}:\n"
            + result.stderr.decode(errors="replace")
        )
    return result.stdout


def git(cwd, env, *args, data=None):
    return run(["git", *args], cwd, env, data)


def isolated_environment(workspace):
    # Keep PATH/toolchain access, not ambient Git or fwt configuration.
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(("GIT_", "FWT_"))}
    home = workspace / "home"
    home.mkdir(exist_ok=True)
    env.update({
        "HOME": str(home), "XDG_CONFIG_HOME": str(home / ".config"),
        "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_TERMINAL_PROMPT": "0", "GIT_LFS_SKIP_SMUDGE": "1",
        "GIT_CONFIG_COUNT": "3", "GIT_CONFIG_KEY_0": "core.hooksPath",
        "GIT_CONFIG_VALUE_0": os.devnull, "GIT_CONFIG_KEY_1": "gc.auto",
        "GIT_CONFIG_VALUE_1": "0", "GIT_CONFIG_KEY_2": "maintenance.auto",
        "GIT_CONFIG_VALUE_2": "false", "FWT_SEED": "",
        "FWT_BASE": str(workspace / "worktrees"),
        "FWT_CONE_DIR": str(workspace / "cones"),
        "LC_ALL": "C",
    })
    return env


def prepare(spec, workspace, env):
    source = workspace / "sources" / spec["name"]
    print(f"Preparing {spec['name']} at {spec['revision']}", flush=True)
    if not source.exists():
        source.mkdir(parents=True)
        git(source, env, "init", "--quiet")
        git(source, env, "remote", "add", "origin", spec["repository"])
        git(source, env, "fetch", "--depth=1", "--no-tags", "origin", spec["revision"])
        git(source, env, "checkout", "--detach", "--quiet", "FETCH_HEAD")
    if git(source, env, "rev-parse", "HEAD").decode().strip() != spec["revision"]:
        raise RuntimeError(f"Unexpected source revision: {source}")
    if git(source, env, "status", "--porcelain", "--untracked-files=all"):
        raise RuntimeError(f"Source is not clean: {source}")
    for directory in spec["directories"]:
        if not (source / directory).is_dir():
            raise RuntimeError(f"Missing cone directory: {source / directory}")
    markers = [name for name in ("MODULE.bazel", "WORKSPACE", "WORKSPACE.bazel")
               if (source / name).is_file()]
    if not markers:
        raise RuntimeError(f"No Bazel workspace/module found in {source}")
    tracked = git(source, env, "ls-files", "-z").split(b"\0")[:-1]
    build_files = sum(Path(os.fsdecode(path)).name in ("BUILD", "BUILD.bazel")
                      for path in tracked)
    if not build_files:
        raise RuntimeError(f"No Bazel BUILD files in {source}")
    print(f"Prepared {spec['name']}: {len(tracked)} tracked files, {build_files} BUILD files", flush=True)
    return source, tracked, markers, build_files


def expected_paths(tracked, directories):
    """Cone mode also includes immediate files in each selected path's ancestors."""
    ancestors = {"."}
    for directory in directories:
        ancestors.update(str(parent) for parent in Path(directory).parents)
    return {
        path for path in tracked
        if str(Path(os.fsdecode(path)).parent) in ancestors
        or any(os.fsdecode(path).startswith(directory + "/") for directory in directories)
    }


def trial(source, spec, tracked, mode, label, binary, workspace, env):
    branch = f"bench-{mode}-{label}"
    target = workspace / "worktrees" / f"{spec['name']}@{branch}"
    if target.exists() or git(source, env, "branch", "--list", branch):
        raise RuntimeError(f"Benchmark target already exists: {target}")
    commands = []

    def timed_command(argv, cwd, data=None):
        commands.append({"argv": [str(arg) for arg in argv], "cwd": str(cwd),
                         "stdin": None if data is None else data.decode()})
        return run(argv, cwd, env, data)

    start = time.perf_counter_ns()
    if mode == "fwt_sparse":
        timed_command([binary, "new", branch, "--cone", "bench"], source)
    else:
        args = ["git", "worktree", "add"]
        if mode == "git_sparse":
            # fwt also runs this config command on each creation.
            timed_command(["git", "config", "extensions.worktreeConfig", "true"], source)
            args.append("--no-checkout")
        timed_command([*args, "-b", branch, target, "HEAD"], source)
        if mode == "git_sparse":
            timed_command(["git", "sparse-checkout", "init", "--cone", "--sparse-index"], target)
            timed_command(["git", "sparse-checkout", "set", "--stdin"], target,
                          ("\n".join(spec["directories"]) + "\n").encode())
            timed_command(["git", "checkout"], target)
    elapsed = (time.perf_counter_ns() - start) / 1e9

    # Validation and accounting are deliberately outside the timed region.
    if git(target, env, "rev-parse", "HEAD").decode().strip() != spec["revision"]:
        raise RuntimeError(f"Wrong checkout revision: {target}")
    sparse = mode != "git_full"
    if sparse:
        actual_cone = git(target, env, "sparse-checkout", "list").decode().splitlines()
        if sorted(actual_cone) != sorted(spec["directories"]):
            raise RuntimeError(f"Unexpected cone: {actual_cone}")
        if git(target, env, "config", "--bool", "index.sparse").strip() != b"true":
            raise RuntimeError("Sparse index is not enabled")
    if git(target, env, "status", "--porcelain", "--untracked-files=all"):
        raise RuntimeError(f"Checkout is dirty: {target}")
    present = {path for path in tracked if os.path.lexists(target / os.fsdecode(path))}
    expected = expected_paths(tracked, spec["directories"]) if sparse else set(tracked)
    if present != expected:
        raise RuntimeError(f"Materialized paths do not match expected cone in {target}")
    logical_bytes = sum((target / os.fsdecode(path)).lstat().st_size for path in present)
    result = {
        "mode": mode, "label": label, "seconds": elapsed,
        "files_present": len(present), "tracked_file_logical_bytes": logical_bytes,
        "path_list_sha256": hashlib.sha256(b"\0".join(sorted(present))).hexdigest(),
        "verified_clean": True, "commands": commands,
    }
    git(source, env, "worktree", "remove", target)
    git(source, env, "branch", "-d", branch)
    print(f"  {spec['name']} {label} {mode}: {elapsed:.3f}s; {len(present)} files", flush=True)
    return result


def summarize(trials):
    summary = {}
    for mode in MODES:
        selected = [trial for trial in trials if trial["mode"] == mode]
        times = [trial["seconds"] for trial in selected]
        if len({trial["path_list_sha256"] for trial in selected}) != 1:
            raise RuntimeError(f"Inconsistent files across {mode} trials")
        summary[mode] = {
            "median_seconds": statistics.median(times), "min_seconds": min(times),
            "max_seconds": max(times), "files_present": selected[0]["files_present"],
            "tracked_file_logical_bytes": selected[0]["tracked_file_logical_bytes"],
        }
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=ROOT / "benchmarks/repos.json")
    parser.add_argument("--fwt", type=Path, default=ROOT / "target/release/fwt")
    parser.add_argument("--workspace", type=Path, required=True, help="dedicated empty scratch directory")
    parser.add_argument("--runs", type=int, default=6)
    parser.add_argument("--filesystem", required=True, help="record filesystem, e.g. APFS on internal SSD")
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    if args.runs < 5:
        parser.error("use at least five measured trials")
    workspace = args.workspace.resolve()
    workspace.mkdir(parents=True, exist_ok=True)
    manifest = args.manifest.read_bytes()
    marker = workspace / ".fwt-benchmark-manifest.json"
    if marker.exists():
        if marker.read_bytes() != manifest:
            parser.error("workspace belongs to a different manifest")
    elif any(workspace.iterdir()):
        parser.error("workspace must be empty or owned by this benchmark")
    else:
        marker.write_bytes(manifest)
    specs = json.loads(manifest)
    env = isolated_environment(workspace)
    binary = args.fwt.resolve()
    (workspace / "worktrees").mkdir(exist_ok=True)
    # Downloads may overlap, but all finish before any timed work begins.
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        prepared = list(pool.map(lambda spec: prepare(spec, workspace, env), specs))
    if args.prepare_only:
        return
    output = workspace / "results.json"
    if output.exists():
        parser.error("results.json already exists; use a new workspace to rerun")
    metadata = {
        "timestamp_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "platform": platform.platform(), "machine": platform.machine(),
        "python": platform.python_version(), "filesystem": args.filesystem,
        "git": git(ROOT, env, "--version").decode().strip(),
        "fwt_version": run([binary, "--version"], ROOT, env).decode().strip(),
        "fwt_revision": git(ROOT, env, "rev-parse", "HEAD").decode().strip(),
        "fwt_source_dirty": bool(git(ROOT, env, "status", "--porcelain", "--", "src", "shell", "Cargo.toml", "Cargo.lock", "assets")),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "harness_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "manifest_sha256": hashlib.sha256(manifest).hexdigest(),
        "runs_per_mode": args.runs, "warmups_per_mode": 1,
        "cache": "warm/uncontrolled OS cache; full source checkout and one warmup per mode",
        "clone": "depth=1, full blobs, no submodules or LFS downloads",
        "seeding": "disabled (FWT_SEED='')", "tune": "not run",
        "timing": "wall clock; creation only; downloads, profile setup, validation and removal excluded",
        "build_verified": False,
    }
    if platform.system() == "Darwin":
        metadata["cpu"] = run(["sysctl", "-n", "machdep.cpu.brand_string"], ROOT, env).decode().strip()
        metadata["memory_bytes"] = int(run(["sysctl", "-n", "hw.memsize"], ROOT, env))
        metadata["os_version"] = run(["sw_vers", "-productVersion"], ROOT, env).decode().strip()
    results = {"environment": metadata, "repositories": []}
    for spec, (source, tracked, markers, build_files) in zip(specs, prepared):
        git(source, env, "config", "extensions.worktreeConfig", "true")
        run([binary, "cone", "set", "bench", *spec["directories"]], source, env)
        repo = {**spec, "tracked_files": len(tracked), "bazel_markers": markers,
                "bazel_build_files": build_files, "warmups": [], "trials": []}
        for mode in MODES:
            repo["warmups"].append(trial(source, spec, tracked, mode, "warmup", binary, workspace, env))
        for index in range(args.runs):
            # Six runs put every mode in each position twice.
            order = MODES[index % 3:] + MODES[:index % 3]
            for mode in order:
                repo["trials"].append(trial(source, spec, tracked, mode, str(index + 1), binary, workspace, env))
        repo["summary"] = summarize(repo["trials"])
        results["repositories"].append(repo)
        output.write_text(json.dumps(results, indent=2) + "\n")
    print(f"Results: {output}", flush=True)


if __name__ == "__main__":
    main()

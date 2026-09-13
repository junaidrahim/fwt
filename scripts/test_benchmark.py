"""Local smoke tests; no network, upstream code, or user configuration."""

import os
from pathlib import Path
import tempfile
import unittest

from benchmark import MODES, ROOT, expected_paths, git, isolated_environment, summarize, trial


class BenchmarkTests(unittest.TestCase):
    def test_cone_includes_ancestor_files_not_sibling_directories(self):
        tracked = [b"README.md", b"src/BUILD", b"src/http/BUILD", b"src/http/a.cc",
                   b"src/http/internal/b.cc", b"src/tcp/c.cc", b"docs/guide.md"]
        self.assertEqual(expected_paths(tracked, ["src/http"]), set(tracked[:5]))

    def test_all_modes_create_and_clean_up_the_expected_checkout(self):
        binary = Path(os.environ.get("FWT_BENCH_BINARY", ROOT / "target/release/fwt"))
        self.assertTrue(binary.is_file(), "build the release binary before testing")
        with tempfile.TemporaryDirectory(prefix="fwt-bench-test-") as temp:
            workspace = Path(temp)
            env = isolated_environment(workspace)
            source = workspace / "sources" / "fixture"
            source.mkdir(parents=True)
            (workspace / "worktrees").mkdir()
            for path in ["README.md", "MODULE.bazel", "src/BUILD", "src/http/BUILD",
                         "src/http/handler.cc", "src/tcp/other.cc"]:
                file = source / path
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_text("fixture\n")
            git(source, env, "init", "-b", "main")
            git(source, env, "config", "user.name", "Benchmark Test")
            git(source, env, "config", "user.email", "bench@example.test")
            git(source, env, "config", "extensions.worktreeConfig", "true")
            git(source, env, "add", ".")
            git(source, env, "commit", "-m", "fixture")
            revision = git(source, env, "rev-parse", "HEAD").decode().strip()
            tracked = git(source, env, "ls-files", "-z").split(b"\0")[:-1]
            spec = {"name": "fixture", "revision": revision, "directories": ["src/http"]}
            from benchmark import run
            run([binary, "cone", "set", "bench", "src/http"], source, env)
            trials = [trial(source, spec, tracked, mode, "test", binary, workspace, env)
                      for mode in MODES]
            summary = summarize(trials)
            self.assertEqual(summary["git_full"]["files_present"], 6)
            self.assertEqual(summary["git_sparse"]["files_present"], 5)
            self.assertEqual(summary["fwt_sparse"]["files_present"], 5)
            self.assertEqual(trials[1]["path_list_sha256"], trials[2]["path_list_sha256"])
            self.assertEqual(len(list((workspace / "worktrees").iterdir())), 0)
            self.assertEqual(git(source, env, "branch", "--list", "bench-*"), b"")
            self.assertEqual(git(source, env, "status", "--porcelain"), b"")


if __name__ == "__main__":
    unittest.main()

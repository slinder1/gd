use praddle_test_server::TestServer;
use std::{fs, path::Path, process::Command};

fn git(directory: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pushes_a_new_change_through_real_clients() {
    let temp = tempfile::tempdir().unwrap();
    let remote = temp.path().join("remote.git");
    let work = temp.path().join("work");
    fs::create_dir(&work).unwrap();

    git(temp.path(), &["init", "--bare", remote.to_str().unwrap()]);
    git(&work, &["init", "--initial-branch=main"]);
    git(&work, &["config", "user.name", "Praddle Test"]);
    git(&work, &["config", "user.email", "praddle@example.com"]);
    fs::write(work.join("README"), "base\n").unwrap();
    git(&work, &["add", "README"]);
    git(&work, &["commit", "-m", "Base"]);
    git(
        &work,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&work, &["push", "--set-upstream", "origin", "main"]);
    git(&work, &["checkout", "-b", "feature"]);
    git(
        &work,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/alice/widgets",
        ],
    );
    fs::write(work.join("feature"), "content\n").unwrap();
    git(&work, &["add", "feature"]);
    git(
        &work,
        &["commit", "-m", "Add feature", "-m", "Change-Id: I0001"],
    );

    let server = TestServer::start("alice", "widgets", &remote)
        .await
        .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_praddle"));
    command.current_dir(&work).args([
        "--remote=origin",
        "--base-branch=main",
        "--user-branch-prefix=users/alice/",
        "--serial",
        "--verbose=2",
        "push",
    ]);
    server.apply_environment(&mut command);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "praddle failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let snapshot = server.snapshot();
    assert_eq!(snapshot.pull_requests.len(), 1);
    let pr = &snapshot.pull_requests[0];
    assert_eq!(pr.title, "Add feature");
    assert!(pr.body.contains("Change-Id: I0001"));
    assert_eq!(pr.head_ref_name, "refs/heads/users/alice/I0001");
    assert!(!pr.is_draft);
    assert_eq!(pr.comments.len(), 1);
    assert_eq!(snapshot.stacks.values().next().unwrap(), &[1]);

    fs::write(work.join("feature"), "updated\n").unwrap();
    fs::write(
        work.join("praddle-test.toml"),
        "remote = \"origin\"\nbase_branch = \"main\"\nuser_branch_prefix = \"users/alice/\"\n\n[reviewer_groups]\ntest = [\"bob\"]\n",
    )
    .unwrap();
    git(&work, &["add", "feature"]);
    git(
        &work,
        &[
            "commit",
            "--amend",
            "-m",
            "Update feature",
            "-m",
            "Change-Id: I0001",
        ],
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_praddle"));
    command
        .current_dir(&work)
        .env("PRADDLE_CONFIG_PATH", work.join("praddle-test.toml"))
        .args([
            "--remote=origin",
            "--base-branch=main",
            "--user-branch-prefix=users/alice/",
            "--serial",
            "--verbose=2",
            "push",
            "--reviewer-groups=test",
        ]);
    server.apply_environment(&mut command);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "second praddle run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot = server.snapshot();
    assert_eq!(snapshot.pull_requests.len(), 1);
    assert_eq!(snapshot.pull_requests[0].title, "Update feature");
    assert_eq!(snapshot.pull_requests[0].comments.len(), 2);
    assert_eq!(snapshot.pull_requests[0].reviewers, ["bob"]);

    let refs = Command::new("git")
        .args(["show-ref", "refs/heads/users/alice/I0001"])
        .current_dir(&remote)
        .output()
        .unwrap();
    assert!(refs.status.success());
}

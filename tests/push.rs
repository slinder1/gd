use praddle_test_server::TestHarness;

#[tokio::test(flavor = "multi_thread")]
async fn pushes_a_new_change_through_real_clients() {
    let harness = TestHarness::start("alice", "widgets").await.unwrap();
    harness.write("feature", "content\n").unwrap();
    harness.git(["add", "feature"]).unwrap();
    harness
        .git(["commit", "-m", "Add feature", "-m", "Change-Id: I0001"])
        .unwrap();

    let mut command = harness.command(env!("CARGO_BIN_EXE_praddle"));
    command.args([
        "--remote=origin",
        "--base-branch=main",
        "--user-branch-prefix=users/alice/",
        "--serial",
        "--verbose=2",
        "push",
    ]);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "praddle failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let snapshot = harness.snapshot();
    assert_eq!(snapshot.pull_requests.len(), 1);
    let pr = &snapshot.pull_requests[0];
    assert_eq!(pr.title, "Add feature");
    assert!(pr.body.contains("Change-Id: I0001"));
    assert_eq!(pr.head_ref_name, "refs/heads/users/alice/I0001");
    assert!(!pr.is_draft);
    assert_eq!(pr.comments.len(), 1);
    assert_eq!(snapshot.stacks.values().next().unwrap(), &[1]);

    harness.write("feature", "updated\n").unwrap();
    harness
        .write(
        "praddle-test.toml",
        "remote = \"origin\"\nbase_branch = \"main\"\nuser_branch_prefix = \"users/alice/\"\n\n[reviewer_groups]\ntest = [\"bob\"]\n",
    )
    .unwrap();
    harness.git(["add", "feature"]).unwrap();
    harness
        .git([
            "commit",
            "--amend",
            "-m",
            "Update feature",
            "-m",
            "Change-Id: I0001",
        ])
        .unwrap();
    let mut command = harness.command(env!("CARGO_BIN_EXE_praddle"));
    command
        .env(
            "PRADDLE_CONFIG_PATH",
            harness.worktree().join("praddle-test.toml"),
        )
        .args([
            "--remote=origin",
            "--base-branch=main",
            "--user-branch-prefix=users/alice/",
            "--serial",
            "--verbose=2",
            "push",
            "--reviewer-groups=test",
        ]);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "second praddle run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot = harness.snapshot();
    assert_eq!(snapshot.pull_requests.len(), 1);
    assert_eq!(snapshot.pull_requests[0].title, "Update feature");
    assert_eq!(snapshot.pull_requests[0].comments.len(), 2);
    assert_eq!(snapshot.pull_requests[0].reviewers, ["bob"]);

    assert!(
        harness
            .remote_ref_exists("refs/heads/users/alice/I0001")
            .unwrap()
    );
}

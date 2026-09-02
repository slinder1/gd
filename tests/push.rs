use praddle_test_server::TestHarness;

const INITIAL_COMMENT_PREFIX: &str =
    "<details>\n<summary>🛠️ Initial changes (click to expand):</summary>\n\n```diff\n";
const INITIAL_COMMENT_SUFFIX: &str = "\n```\n</details>";

fn initial_comment(path: &str, contents: &str) -> String {
    format!(
        "{INITIAL_COMMENT_PREFIX}diff --git b/{path} a/{path}\n@@ -0,0 +1 @@\n+{contents}\n{INITIAL_COMMENT_SUFFIX}"
    )
}

fn interdiff_comment(path: &str, before: &str, after: &str) -> String {
    format!(
        "<details>\n<summary>🛠️ Changes since last version (click to expand):</summary>\n\n```diff\ndiff --git b/{path} a/{path}\n@@ -1 +1 @@\n-{before}\n+{after}\n\n```\n</details>"
    )
}

fn push(harness: &TestHarness) {
    let mut command = harness.command(env!("CARGO_BIN_EXE_praddle"));
    command.args([
        "--remote=origin",
        "--base-branch=main",
        "--user-branch-prefix=users/alice/",
        "--serial",
        "push",
    ]);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "praddle failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn extends_a_stack_when_a_second_change_is_pushed() {
    let harness = TestHarness::start("alice", "widgets").await.unwrap();
    harness.write("first", "first\n").unwrap();
    harness.git(["add", "first"]).unwrap();
    harness
        .git([
            "commit",
            "-m",
            "First change",
            "-m",
            "First body",
            "-m",
            "Change-Id: I0001",
        ])
        .unwrap();

    push(&harness);

    let snapshot = harness.snapshot();
    assert_eq!(snapshot.stacks, [(1, vec![1])].into());
    assert_eq!(snapshot.pull_requests.len(), 1);
    let first = &snapshot.pull_requests[0];
    assert_eq!(first.title, "First change");
    assert_eq!(first.body, "First body\n\nChange-Id: I0001");
    assert_eq!(first.base_ref_name, "main");
    assert_eq!(first.head_ref_name, "refs/heads/users/alice/I0001");
    assert_eq!(first.stack, Some(1));
    assert_eq!(first.stack_position, Some(0));
    assert_eq!(first.comments, [initial_comment("first", "first")]);

    harness.write("second", "second\n").unwrap();
    harness.git(["add", "second"]).unwrap();
    harness
        .git([
            "commit",
            "-m",
            "Second change",
            "-m",
            "Second body",
            "-m",
            "Change-Id: I0002",
        ])
        .unwrap();

    push(&harness);

    let snapshot = harness.snapshot();
    assert_eq!(snapshot.stacks, [(1, vec![1, 2])].into());
    assert_eq!(snapshot.pull_requests.len(), 2);
    let first = &snapshot.pull_requests[0];
    assert_eq!(first.title, "First change");
    assert_eq!(first.body, "First body\n\nChange-Id: I0001");
    assert_eq!(first.base_ref_name, "main");
    assert_eq!(first.comments, [initial_comment("first", "first")]);
    assert_eq!(first.stack, Some(1));
    assert_eq!(first.stack_position, Some(0));
    let second = &snapshot.pull_requests[1];
    assert_eq!(second.title, "Second change");
    assert_eq!(second.body, "Second body\n\nChange-Id: I0002");
    assert_eq!(second.base_ref_name, "users/alice/I0001");
    assert_eq!(second.head_ref_name, "refs/heads/users/alice/I0002");
    assert_eq!(second.stack, Some(1));
    assert_eq!(second.stack_position, Some(1));
    assert_eq!(second.comments, [initial_comment("second", "second")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn extends_a_stack_with_two_changes_in_one_push() {
    let harness = TestHarness::start("alice", "widgets").await.unwrap();
    harness.write("first", "first\n").unwrap();
    harness.git(["add", "first"]).unwrap();
    harness
        .git([
            "commit",
            "-m",
            "First change",
            "-m",
            "First body",
            "-m",
            "Change-Id: I0001",
        ])
        .unwrap();

    push(&harness);

    let snapshot = harness.snapshot();
    assert_eq!(snapshot.stacks, [(1, vec![1])].into());
    assert_eq!(snapshot.pull_requests.len(), 1);

    harness.write("second", "second\n").unwrap();
    harness.git(["add", "second"]).unwrap();
    harness
        .git([
            "commit",
            "-m",
            "Second change",
            "-m",
            "Second body",
            "-m",
            "Change-Id: I0002",
        ])
        .unwrap();
    harness.write("third", "third\n").unwrap();
    harness.git(["add", "third"]).unwrap();
    harness
        .git([
            "commit",
            "-m",
            "Third change",
            "-m",
            "Third body",
            "-m",
            "Change-Id: I0003",
        ])
        .unwrap();

    push(&harness);

    let snapshot = harness.snapshot();
    assert_eq!(snapshot.stacks, [(1, vec![1, 3, 2])].into());
    assert_eq!(snapshot.pull_requests.len(), 3);
    let first = &snapshot.pull_requests[0];
    assert_eq!(first.title, "First change");
    assert_eq!(first.body, "First body\n\nChange-Id: I0001");
    assert_eq!(first.base_ref_name, "main");
    assert_eq!(first.comments, [initial_comment("first", "first")]);
    assert_eq!(first.stack_position, Some(0));
    let third = &snapshot.pull_requests[1];
    assert_eq!(third.title, "Third change");
    assert_eq!(third.body, "Third body\n\nChange-Id: I0003");
    assert_eq!(third.base_ref_name, "users/alice/I0002");
    assert_eq!(third.comments, [initial_comment("third", "third")]);
    assert_eq!(third.stack_position, Some(2));
    let second = &snapshot.pull_requests[2];
    assert_eq!(second.title, "Second change");
    assert_eq!(second.body, "Second body\n\nChange-Id: I0002");
    assert_eq!(second.base_ref_name, "users/alice/I0001");
    assert_eq!(second.comments, [initial_comment("second", "second")]);
    assert_eq!(second.stack_position, Some(1));
}

#[tokio::test(flavor = "multi_thread")]
async fn updates_a_change_when_it_is_edited_and_pushed_again() {
    let harness = TestHarness::start("alice", "widgets").await.unwrap();
    harness.write("change", "before\n").unwrap();
    harness.git(["add", "change"]).unwrap();
    harness
        .git([
            "commit",
            "-m",
            "Initial title",
            "-m",
            "Initial body",
            "-m",
            "Change-Id: I0001",
        ])
        .unwrap();

    push(&harness);

    let snapshot = harness.snapshot();
    assert_eq!(snapshot.stacks, [(1, vec![1])].into());
    assert_eq!(snapshot.pull_requests.len(), 1);
    let change = &snapshot.pull_requests[0];
    assert_eq!(change.title, "Initial title");
    assert_eq!(change.body, "Initial body\n\nChange-Id: I0001");
    assert_eq!(change.base_ref_name, "main");
    assert_eq!(change.head_ref_name, "refs/heads/users/alice/I0001");
    assert_eq!(change.comments, [initial_comment("change", "before")]);
    assert_eq!(change.stack, Some(1));
    assert_eq!(change.stack_position, Some(0));

    harness.write("change", "after\n").unwrap();
    harness.git(["add", "change"]).unwrap();
    harness
        .git([
            "commit",
            "--amend",
            "-m",
            "Updated title",
            "-m",
            "Updated body",
            "-m",
            "Change-Id: I0001",
        ])
        .unwrap();

    push(&harness);

    let snapshot = harness.snapshot();
    assert_eq!(snapshot.stacks, [(1, vec![1])].into());
    assert_eq!(snapshot.pull_requests.len(), 1);
    let change = &snapshot.pull_requests[0];
    assert_eq!(change.title, "Updated title");
    assert_eq!(change.body, "Updated body\n\nChange-Id: I0001");
    assert_eq!(change.base_ref_name, "main");
    assert_eq!(change.head_ref_name, "refs/heads/users/alice/I0001");
    assert_eq!(
        change.comments,
        [
            initial_comment("change", "before"),
            interdiff_comment("change", "before", "after"),
        ]
    );
    assert_eq!(change.stack, Some(1));
    assert_eq!(change.stack_position, Some(0));
}

#[tokio::test(flavor = "multi_thread")]
async fn misc() {
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

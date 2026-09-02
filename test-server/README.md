# Praddle test server

This crate implements the GitHub behavior used by Praddle. It serves:

* GitHub GraphQL and REST requests over a Unix socket for the real `gh` CLI.
* Smart Git HTTP fetches and atomic pushes over a loopback TCP socket for the real `git` CLI.
* `GET /_test/state` for test assertions.

After each successful receive-pack operation, the server marks every open pull
request as merged when its head ref is reachable from its base ref, matching
GitHub's behavior.

Run it with a single repository identity and an existing bare Git repository:

```console
cargo run -p praddle-test-server -- OWNER REPO /path/to/repo.git --socket /tmp/praddle-github.sock
```

The first stdout line contains the selected TCP address and Unix socket as JSON. Configure an isolated `gh` config with `http_unix_socket` set to the socket. Redirect Git without changing the repository URL with:

```console
git config --global url.http://ADDRESS/OWNER/REPO.insteadOf https://github.com/OWNER/REPO
```

Tests should prefer `TestHarness::start`. It creates a temporary bare remote and
worktree, configures a test identity, commits and pushes an empty `main` base,
checks out a `change` branch, starts the server, and isolates both `gh` and Git
settings from the user's configuration. `TestHarness::git`,
`TestHarness::write`, and `TestHarness::command` provide the common operations
needed by scenarios.

`TestRepository` and `TestServer` remain available separately for tests that
need nonstandard repository or server setup.

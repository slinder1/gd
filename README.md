GitHub stacked-PR builder for those who miss Gerrit

See `--help` for more.

# Etymology

<sub>(*Note:* This project was previously named `gd` and then `cgh`)</sub>

The name doesn't meaning anything, but it sounds like "prattle", which I enjoy.

A backronym might be "Pull Request, Add (Little Endian)".

# Alternatives

## `spr`

A very compelling alternative to `praddle` is https://github.com/ejoffe/spr which
differs in a few ways:

* `spr` will modify your local branches by default for logically
  non-destructive operations (i.e. when you try to `update` the remote)
* `spr` won't use Gerrit `Change-Id:`, and is very particular about the format
  of its ID; `praddle` allows any string and uses the `Change-Id:` trailer
* `spr` does not seem to have a `dry-run` option, so modifications aren't
  foreseeable
* `spr` doesn't produce an "interdiff" when force-pushing to give the reviewer
  context for the edits to the change
* `spr` installs itself as a git subcommand (this is really just an aesthetic
  quibble, but I don't think it is primarily a `git` tool, it is a GitHub tool,
  and exists only to patch a deficiency in GitHub as a service)
* `spr` warns you to only close/merge PRs through it, rather than just
  diagnosing when e.g. a PR would be created for a change which already has a
  merged PR
* `spr` uses YAML for configuration, `praddle` uses TOML
* `spr` is noisy by default, `praddle` is quiet by default
* `spr` seems slightly less aggressive with parallelizing operations
* `spr` is written in Go, `praddle` is written in Rust

In the end most of these are fairly aesthetic and minor, but rather than try to
hack on `spr` I opted to start over and make the exact tool I wanted. YMMV

## `gherrit`

A tool with a very similar core philosophy, https://github.com/joshlf/gherrit
seems to differ primarily in the UX and the structure of remote branches:

* Goes to greater lengths to reproduce the `git push`-based workflow of Gerrit
  proper. This involves intercepting the `push` through hooks.
* Retains more "phantom branches" on the remote to facilitate diffs and retain
  comments (if I understand it correctly).
* Also includes GH actions to keep the stack tidy and ready to merge. I don't
  fully understand how this works yet.

## `maiao`

I haven't actually used https://github.com/adevinta/maiao but came across it
since writing `praddle`. The biggest issue I see immediately is that it modifies
local refs to do fixups and rebases.

## `graphite`

I have had only negative experiences with https://graphite.dev/ and in
particular my issues are:

* Modifies local refs, and inserts itself into your workflow before you even
  consider creating PRs
* Requires a third-party service
* Is terribly slow (on top of the already slow GH API)
* Is closed-source

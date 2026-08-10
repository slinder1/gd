use clap::{ArgAction, Args, Parser, Subcommand};

/// GitHub stacked-PR builder for those who miss Gerrit
///
/// Main features:
///
/// * Never touches your local branches. The tool only reads from your local branch and attempts to
///   mirror it to GitHub by: fetching remote tracking branches, force-pushing namespaced refs,
///   creating PRs, and maintaining PR bodies and comments to present a pseudo-UI for the stack. If
///   you use the `merge` subcommand, it also touches namespaced git-config entries under
///   `branch.<name>.praddle-*`, but this is only for optional features.
/// * Treats one branch as one patch-stack, where each commit maps 1:1 to a PR.
/// * Uses the same "Change-Id" trailer used by Gerrit. You can install the commit-msg hook from
///   a Gerrit instance or use the install-hook subcommand to install an embedded copy.
/// * Generates "interdiff"-esque diffs for updates to changes and posts them as a comment on your
///   behalf when you push. This is a bit of a workaround to mitigate the fallout from having to
///   force-push.
/// * Quiet by default. No news is good news, but you can also get verbose output or a dry-run.
/// * Uses the official `gh` tool to interface with the GitHub API, so you don't have
///   to go through authenticating another app.
/// * Uses the actual `git` command for network operations, so however you authenticate
///   works fine. (If you have to answer a prompt for each operation, you may have a
///   less than stellar experience, but it should work. Use the `--serial` option if you
///   want to do this).
/// * Painfully slow, but at least tries to claw back performance where possible, primarily by
///   parallelizing steps across all patches in the branch. There is a lot of room to
///   optimize still, it does the least clever thing imaginable in a lot of cases.
///
/// And currently its greatest shortcomings are:
///
/// * Does not even try to avoid force pushes. Review comments will regularly end up marked as
///   stale with no relation to the latest patch contents. This seems to happen frequently anyway,
///   and avoiding it in the general case requires never rebasing which is not viable for anything
///   but an extremely short-lived review process. Ideas about how to potentially resolve this is
///   documented at https://github.com/slinder1/praddle/blob/main/IDEAS.md and contributions are
///   welcome!
/// * Can lose track of merged/closed PR if the user is not careful to use the `merge` subcommand.
///   This may be mildly confusing, but is more-or-less by design: the change commit which corresponds
///   to a merged PR will naturally disappear from the branch on rebase. The `merge` subcommand
///   notes the Change-Id of successful merges in a git-config entry tied to the branch, so
///   it doesn't forget to count them and link to them, but it is purely aesthetics.
/// * Currently lacks a lot of polish and documentation.
///
/// It reads configuration from the first of the following:
///
/// * The file identified by the environment variable `PRADDLE_CONFIG_PATH`, if that variable is set.
/// * The file `.praddle.toml` in the git repo's workdir, if it exists.
/// * The file `praddle.toml` in the git repo's workdir, if it exists.
/// * The file `praddle.toml` in platform-dependant user config dir, otherwise.
///
/// An example config file is:
///
///     remote = "origin"
///     base_branch = "main"
///     user_branch_prefix = "users/$USER/"
///
/// The title of PR number `N` in a series of `M` commits is prefixed with:
///
/// * `[<branch-desc-first-line>: N/M]: ` if the branch has a description,
///   editable via `git branch --edit-description`, or
/// * `[N/M]: ` otherwise.
///
#[derive(Parser)]
#[command(version, verbatim_doc_comment, args_override_self = true)]
pub struct Cli {
    #[clap(flatten)]
    pub globals: Globals,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args)]
pub struct Globals {
    /// The name of the git remote corresponding to the GitHub repo to operate on.
    #[arg(long, global = true)]
    pub remote: Option<String>,
    /// The branch on `remote` which acts as the "base" branch, which all PRs are ultimately
    /// relative to.
    #[arg(long, global = true)]
    pub base_branch: Option<String>,
    /// The prefix for all remote branches created by the tool. Can be empty.
    #[arg(long, global = true)]
    pub user_branch_prefix: Option<String>,
    /// Limit the global thread pool used by `praddle` to have only one thread.
    #[arg(long, global = true)]
    pub serial: bool,
    /// Give a verbose summary of what would happen if executed.
    ///
    /// Note: This still makes read-only queries to the git repo and GitHub APIs. Only operations
    /// which have the potential to mutate remote state are skipped and printed.
    #[arg(short = '#', long, global = true)]
    pub dry_run: bool,
    /// Output all commands executed, and their stdout/stderr.
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Push the current branch as a stack of GitHub PRs.
    ///
    /// The commits `${base}..HEAD` must each have a `Change-Id:` trailer. Each commit will be
    /// force-pushed to a corresponding branch named `${user_branch_prefix}${change_id}` on
    /// `${remote}`. Each commit will be matched to its existing PR or else a new PR will be
    /// created for it. The PRs will be "stacked" such that they reproduce the local branch
    /// sequence, with additional trailers in the PR message body to help reviewers navigate the
    /// stack.
    ///
    /// Note: This command will never modify your commits or refs, even their messages. No local
    /// branches are created or destroyed. All mutation occurs exclusively on the `$remote`.
    #[command(visible_alias = "p")]
    Push(Push),
    /// With no arguments, print the current stack's short-name. With an argument, set it.
    ///
    /// The short-name is tracked in the `branch.<name>.praddle-shortName` git config entry,
    /// and is used to prefix the PR title, e.g. `[${short-name} 3/5] ...`
    Name(Name),
    /// Merge the next change.
    ///
    /// If successful, this will modify the local `branch.<name>.praddle-mergedChangeIds` git config
    /// entry to record that the change was merged. This allows future `praddle push`es to include
    /// merged changes in the reviewer stack "UI", keeping the relative numbering of changes stable.
    ///
    /// If a change's PR is merged in any other way, it will "disappear" from the stack, affecting
    /// all downstream numbering and the total number of changes in the stack. If you want to
    /// manually correct this, edit `branch.<name>.praddle-mergedChangeIds` (a ':' separated list) using
    /// e.g. `git config --edit` and append the merged change's ID to the list (or create it, if it
    /// does not already exist).
    ///
    /// If you don't care about the renumbering behavior, you can safely ignore this subcommand (it
    /// is purely aesthetic).
    Merge(Merge),
    /// Print the PR URL of the top-most (i.e. last) change which already has one.
    Url(Url),
    /// Install a commit-msg hook in the current git repo to create `Change-Id:` trailers.
    InstallHook(InstallHook),
}

#[derive(Args)]
pub struct Push {
    /// A comma-separated list of reviewer groups to apply from the config file.
    ///
    /// An example config snippet defining two groups `internal` and `public`:
    ///
    ///     [reviewer_groups]
    ///     internal = [ "dev1", "dev2" ]
    ///     public = [ "dev1", "dev3", "dev4" ]
    #[arg(short, long, action = ArgAction::Set, value_delimiter = ',')]
    pub reviewer_groups: Vec<String>,
    #[arg(short, long)]
    /// Leave all the PRs as drafts
    pub draft: bool,
}

#[derive(Args)]
pub struct Name {
    pub new_name: Option<String>,
}

#[derive(Args)]
pub struct Merge;

#[derive(Args)]
pub struct Url;

#[derive(Args)]
pub struct InstallHook {
    /// Install the hook over any existing commit-msg hook.
    #[arg(short, long)]
    pub force: bool,
}

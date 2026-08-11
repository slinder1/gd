use clap::{ArgAction, Args, Parser, Subcommand};

fn parse_verbosity(value: &str) -> Result<u8, String> {
    match value {
        "0" => Ok(0),
        "1" => Ok(1),
        "2" | "v" => Ok(2),
        _ => Err("verbosity must be 0, 1, or 2/v".into()),
    }
}

/// GitHub stacked-PR builder for those who miss Gerrit
///
/// Main features:
///
/// * Never touches your local branches. The tool only reads from your local branch and attempts to
///   mirror it to GitHub by: fetching remote tracking branches, force-pushing namespaced refs,
///   creating PRs and comments with the official GitHub UI.
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
    /// Output commands executed. Repeat for command output as well.
    #[arg(
        short,
        long,
        global = true,
        value_parser = parse_verbosity,
        num_args = 0..=1,
        default_missing_value = "1",
        default_value = "0"
    )]
    pub verbose: u8,
}

#[derive(Subcommand)]
pub enum Command {
    /// Push the current branch as a stack of GitHub PRs.
    ///
    /// The commits `${base}..HEAD` must each have a `Change-Id:` trailer. Each commit will be
    /// force-pushed to a corresponding branch named `${user_branch_prefix}${change_id}` on
    /// `${remote}`. Each commit will be matched to its existing PR or else a new PR will be
    /// created for it. The PRs will be "stacked" such that they reproduce the local branch
    /// sequence.
    ///
    /// Note: This command will never modify your commits or refs, even their messages. No local
    /// branches are created or destroyed. All mutation occurs exclusively on the `$remote`.
    #[command(visible_alias = "p")]
    Push(Push),
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
pub struct Url;

#[derive(Args)]
pub struct InstallHook {
    /// Install the hook over any existing commit-msg hook.
    #[arg(short, long)]
    pub force: bool,
}

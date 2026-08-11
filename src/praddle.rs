// Copyright © 2026 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use crate::change::{self, AnyChange, Change, LocalChange};
use crate::cli;
use crate::env;
use crate::gh::{self, Pr, PrState};
use crate::util::Extract;
use anyhow::{Context, Result, bail};
use git2::Repository;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub fn praddle(cli: cli::Cli) -> Result<()> {
    if let cli::Command::InstallHook(ref args) = cli.command {
        return install_hook(args, cli.globals.dry_run);
    }
    env::init(cli)?;
    let cli = env::get().cli();
    if cli.globals.serial {
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build_global()
            .context("could not install serial thread pool")
            .extract();
    }
    match cli.command {
        cli::Command::Push(ref args) => push(args),
        cli::Command::Url(ref args) => url(args),
        cli::Command::InstallHook(_) => unreachable!(),
    }
}

fn push(args: &cli::Push) -> Result<()> {
    let env = env::get();
    let mut reviewers = vec![];
    for group_key in args.reviewer_groups.iter() {
        let group = env
            .reviewer_groups()
            .context("used a review group, but none were found in the config file")?
            .get(group_key)
            .with_context(|| format!("reviewer group {group_key:?} not found in config file"))?;
        reviewers.extend_from_slice(group);
    }
    let local_changes =
        change::get_local_changes().context("could not enumerate current local branch")?;
    let mut prs_by_change_id = gh::prs_by_change_id().context("could not enumerate remote prs")?;
    let mut any_changes = vec![];
    for local_change in local_changes {
        any_changes.push(match prs_by_change_id.remove(&local_change.id) {
            None => AnyChange::LocalChange(local_change),
            Some(pr) => {
                if pr.in_state(PrState::Merged) {
                    bail!(
                        "pr {} for unmerged change {} is already merged",
                        pr.number,
                        local_change.id
                    );
                }
                AnyChange::Change(Change { local_change, pr })
            }
        });
    }
    let has_cycles = detect_cycles(&any_changes);
    if has_cycles {
        let stacked_prs: Vec<&Pr> = any_changes
            .iter()
            .filter_map(|any_change| match any_change {
                AnyChange::Change(change) => Some(&change.pr),
                AnyChange::LocalChange(_) => None,
            })
            .collect();
        gh::remove_stack(&stacked_prs).context("could not remove pr stack for rebase")?;
        for any_change in any_changes.iter_mut() {
            if let AnyChange::Change(change) = any_change {
                change.pr.stack = None;
                change.pr.stack_entry = None;
            }
        }
    }
    if has_cycles || args.draft {
        any_changes
            .par_iter_mut()
            .filter_map(|ac| {
                if let AnyChange::Change(c) = ac {
                    Some(c)
                } else {
                    None
                }
            })
            .map(|c| {
                c.pr.mark_ready(false)
                    .with_context(|| format!("could not mark pr as draft: {:?}", c.pr))?;
                // FIXME: This is pretty coarse-grained, could find the minimal set.
                if has_cycles {
                    c.pr.set_base(env.base_branch()).with_context(|| {
                        format!(
                            "could not retarget pr {} to base branch: {:?}",
                            c.pr.number,
                            env.base_branch(),
                        )
                    })?;
                }
                Ok(())
            })
            .collect::<Result<Vec<_>>>()?;
    }
    LocalChange::fetch_all(any_changes.iter().filter_map(|ac| match ac {
        AnyChange::Change(c) => Some(&c.local_change),
        _ => None,
    }))
    .context("could not fetch base branches for all existing prs")?;
    let diffs = any_changes
        .par_iter()
        .map(|ac| ac.diff())
        .collect::<Result<Vec<_>>>()
        .context("could not build diffs")?;
    LocalChange::push_all(any_changes.iter().map(|ac| ac.local_change()))
        .context("could not push all local changes")?;
    // FIXME: Should try to restore the original branch contents if we fail from this point on. It
    // would be at least an attempt at being "atomic" about the push, and it would mean we don't
    // lose the interdiff in a future re-run.
    let mut changes = any_changes
        .into_par_iter()
        .map(|any_change| {
            let change = match any_change {
                AnyChange::LocalChange(local_change) => {
                    let pr = Pr::create(&local_change)
                        .with_context(|| format!("could not create new pr for {local_change:?}"))?;
                    Change { local_change, pr }
                }
                AnyChange::Change(change) => change,
            };
            Ok(change)
        })
        .collect::<Result<Vec<_>>>()
        .context("could not create new prs")?;
    let bases: Vec<String> = changes
        .iter()
        .enumerate()
        .map(|(i, _)| {
            changes[i + 1..]
                .iter()
                .next()
                .map(|p| p.local_change.remote_branch())
                .unwrap_or_else(|| env.base_branch().to_owned())
        })
        .collect();
    changes
        .par_iter_mut()
        .zip(bases.par_iter())
        .map(|(c, base)| {
            if c.pr.base_ref_name != base.as_str() {
                if c.pr.stack.is_some() {
                    bail!(
                        "cannot retarget pr {} while it is part of a stack",
                        c.pr.number
                    );
                }
                c.pr.set_base(base.as_ref()).with_context(|| {
                    format!(
                        "could not retarget pr {} to branch: {:?}",
                        c.pr.number, base,
                    )
                })?;
            }
            Ok(())
        })
        .collect::<Result<Vec<_>>>()
        .context("could not set pr bases and bodies")?;
    gh::reconcile_stack(&changes.iter().map(|change| &change.pr).collect::<Vec<_>>())
        .context("could not reconcile pr stack")?;
    changes
        .par_iter_mut()
        .map(|c| {
            let commit = c.local_change.commit()?;
            let title = commit
                .summary()
                .context("failed to get commit summary")?
                .context("commit has no summary")?;
            let body = commit
                .body()
                .context("failed to get commit body")?
                .context("commit has no body")?;
            c.pr.set_details(title, body)
        })
        .collect::<Result<Vec<_>>>()
        .context("could not update pr titles and bodies")?;
    changes
        .par_iter()
        .zip(diffs)
        .map(|(c, diff)| c.pr.add_details_comment(&diff))
        .collect::<Result<Vec<_>>>()
        .context("could not add interdiff comments")?;
    changes
        .par_iter()
        .map(|c| c.pr.add_reviewers(reviewers.as_ref()))
        .collect::<Result<Vec<_>>>()
        .context("could not add pr reviewers")?;
    if !args.draft {
        changes
            .par_iter()
            .map(|c| c.pr.mark_ready(true))
            .collect::<Result<Vec<_>>>()
            .context("could not mark prs as ready")?;
    }
    Ok(())
}

fn detect_cycles(any_changes: &[AnyChange]) -> bool {
    let mut parent_refs_seen: HashSet<String> = HashSet::new();
    for any_change in any_changes.iter() {
        if let AnyChange::Change(change) = any_change {
            if !parent_refs_seen.is_empty()
                && !parent_refs_seen.contains(&change.local_change.remote_branch())
            {
                return true;
            }
            parent_refs_seen.insert(change.pr.base_ref_name.clone());
        }
    }
    false
}

fn url(_args: &cli::Url) -> Result<()> {
    let local_changes =
        change::get_local_changes().context("could not enumerate current local branch")?;
    let mut prs_by_change_id = gh::prs_by_change_id().context("could not enumerate remote prs")?;
    for local_change in local_changes {
        if let Some(pr) = prs_by_change_id.remove(&local_change.id) {
            println!("{}", pr.get_url()?);
            return Ok(());
        }
    }
    bail!("no change has an existing PR");
}

static COMMIT_MSG_HOOK_SRC: &str = include_str!("commit-msg");
static EXECUTABLE_MODE_BITS: u32 = 0o111;

fn install_hook(args: &cli::InstallHook, dry_run: bool) -> Result<()> {
    let repo = Repository::open(".").context("not in a git repo")?;
    let mut hook_path = PathBuf::from(repo.commondir());
    hook_path.extend(["hooks", "commit-msg"]);
    if dry_run {
        let verb = if args.force { "overwrite" } else { "write" };
        eprintln!("would {verb} {hook_path:?}");
        return Ok(());
    }
    let mut hook_file: File = if args.force {
        File::create(&hook_path)
            .with_context(|| format!("could not create hook file: {hook_path:?}"))
    } else {
        File::create_new(&hook_path)
            .with_context(|| format!("could not create hook file (try --force): {hook_path:?}"))
    }?;
    hook_file
        .write_all(COMMIT_MSG_HOOK_SRC.as_bytes())
        .with_context(|| format!("could not write to hook file: {hook_path:?}"))?;
    hook_file
        .flush()
        .with_context(|| format!("could not flush hook file: {hook_path:?}"))?;
    let mut perms = hook_file
        .metadata()
        .with_context(|| format!("could not get metadata for hook file: {hook_path:?}"))?
        .permissions();
    perms.set_mode(perms.mode() | EXECUTABLE_MODE_BITS);
    hook_file
        .set_permissions(perms)
        .with_context(|| format!("could not set permissions for hook file: {hook_path:?}"))?;
    Ok(())
}

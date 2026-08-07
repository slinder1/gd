// Copyright © 2026 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use crate::change::{self, AnyChange, Change, LocalChange};
use crate::cli;
use crate::env;
use crate::gh::{self, Pr, PrState};
use crate::metadata::StackMetadata;
use crate::util::Extract;
use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub fn cgh() -> Result<()> {
    let cli = env::cli();
    if cli.globals.serial {
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build_global()
            .context("could not install serial thread pool")
            .extract();
    }
    env::validate().context("invalid configuration")?;
    match cli.command {
        cli::Command::Push(ref cfg) => push(cfg),
        cli::Command::Name(ref cfg) => name(cfg),
        cli::Command::Merge(ref cfg) => merge(cfg),
        cli::Command::Url(ref cfg) => url(cfg),
        cli::Command::InstallHook(ref cfg) => install_hook(cfg),
    }
}

fn push(cfg: &cli::Push) -> Result<()> {
    let stack_meta =
        StackMetadata::from_repo().context("could not parse branch description metadata")?;
    let mut reviewers = vec![];
    for group_key in cfg.reviewer_groups.iter() {
        let group = env::reviewer_groups()
            .context("used a review group, but none were found in the config file")?
            .get(group_key)
            .with_context(|| format!("reviewer group {group_key:?} not found in config file"))?;
        reviewers.extend_from_slice(group);
    }
    let local_changes =
        change::get_local_changes().context("could not enumerate current local branch")?;
    let mut prs_by_change_id = gh::prs_by_change_id(|pr| !pr.in_state(PrState::Closed))
        .context("could not enumerate remote prs")?;
    let mut merged_prs = vec![];
    for merged_change_id in stack_meta.merged_change_ids {
        let pr = prs_by_change_id
            .remove(&merged_change_id)
            .with_context(|| format!("merged change {} has no pr", merged_change_id))?;
        if pr.in_state(PrState::Open) {
            bail!(
                "pr {} for merged change {} is still open",
                pr.number,
                merged_change_id,
            );
        }
        merged_prs.push(pr);
    }
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
    if has_cycles || cfg.draft {
        any_changes
            .par_iter()
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
                    c.pr.set_base(env::base_branch()).with_context(|| {
                        format!(
                            "could not retarget pr {} to base branch: {:?}",
                            c.pr.number,
                            env::base_branch(),
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
    any_changes
        .first()
        .context("no local changes to tag")?
        .local_change()
        .push_tag()
        .context("could not push tag for final local change")?;
    // FIXME: Should try to restore the original branch contents if we fail from this point on. It
    // would be at least an attempt at being "atomic" about the push, and it would mean we don't
    // lose the interdiff in a future re-run.
    let changes = any_changes
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
    changes
        .par_iter()
        .enumerate()
        .map(|(i, c)| {
            let parents = &changes[i + 1..];
            let base = parents
                .iter()
                .next()
                .map(|p| p.local_change.remote_branch())
                .unwrap_or_else(|| env::base_branch().to_owned());
            c.pr.set_base(base.as_ref()).with_context(|| {
                format!(
                    "could not retarget pr {} to branch: {:?}",
                    c.pr.number, base,
                )
            })?;
            c.render_pr_ui(&changes, &stack_meta.short_name, &merged_prs)
                .context("could not render pseudo-ui in pr title/body")
        })
        .collect::<Result<Vec<_>>>()
        .context("could not set pr bases and bodies")?;
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
    if !cfg.draft {
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

fn name(cfg: &cli::Name) -> Result<()> {
    let mut stack_meta = StackMetadata::from_repo()?;
    match cfg.new_name {
        None => println!("{}", stack_meta.short_name),
        Some(ref new_name) => {
            stack_meta.short_name = new_name.to_string();
            stack_meta.to_repo()?;
        }
    }
    Ok(())
}

fn merge(_cfg: &cli::Merge) -> Result<()> {
    let mut stack_meta =
        StackMetadata::from_repo().context("could not parse branch description metadata")?;
    let local_change = change::get_local_changes()
        .context("could not enumerate current local branch")?
        .pop()
        .context("no local changes")?;
    if stack_meta.merged_change_ids.contains(&local_change.id) {
        bail!("change {} was already merged", local_change.id);
    }
    let mut prs_by_change_id = gh::prs_by_change_id(|pr| !pr.in_state(PrState::Closed))
        .context("could not enumerate remote prs")?;
    let pr = prs_by_change_id
        .remove(&local_change.id)
        .with_context(|| format!("change {} has no pr", local_change.id))?;
    let change = Change { local_change, pr };
    change.merge()?;
    stack_meta
        .merged_change_ids
        .push(change.local_change.id.clone());
    stack_meta.to_repo()?;
    Ok(())
}

fn url(_cfg: &cli::Url) -> Result<()> {
    let local_changes =
        change::get_local_changes().context("could not enumerate current local branch")?;
    let mut prs_by_change_id = gh::prs_by_change_id(|pr| !pr.in_state(PrState::Closed))
        .context("could not enumerate remote prs")?;
    for local_change in local_changes {
        if let Some(pr) = prs_by_change_id.remove(&local_change.id) {
            println!("{}", pr.get_url());
            return Ok(());
        }
    }
    bail!("no change has an existing PR");
}

static COMMIT_MSG_HOOK_SRC: &str = include_str!("commit-msg");
static EXECUTABLE_MODE_BITS: u32 = 0o111;

fn install_hook(cfg: &cli::InstallHook) -> Result<()> {
    let mut hook_path = PathBuf::from(env::repo().commondir());
    hook_path.extend(["hooks", "commit-msg"]);
    if env::dry_run() {
        let verb = if cfg.force { "overwrite" } else { "write" };
        eprintln!("would {verb} {hook_path:?}");
        return Ok(());
    }
    let mut hook_file: File = if cfg.force {
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

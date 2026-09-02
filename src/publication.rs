// Copyright © 2026 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use crate::change::{AnyChange, Change, LocalChange};
use crate::env;
use crate::util::{exec, print_cmd_and_files};
use anyhow::{Context, Result, bail};
use git2::{Oid, Repository};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::process::Command;

pub struct RemoteRefs {
    refs: HashMap<String, Oid>,
}

impl RemoteRefs {
    pub fn fetch(changes: &[AnyChange]) -> Result<Self> {
        let env = env::get();
        let remote_url = env.remote_url()?;
        let mut requested = BTreeSet::from([branch_ref(env.base_branch())]);
        for change in changes {
            requested.insert(change.local_change().remote_branch_ref());
            if let AnyChange::Change(change) = change {
                requested.insert(branch_ref(&change.pr.base_ref_name));
            }
        }

        let mut ls_remote = Command::new("git");
        ls_remote.args(["ls-remote", "--refs", &remote_url]);
        ls_remote.args(&requested);
        let output = exec(&mut ls_remote).context("could not enumerate remote refs")?;
        let mut refs = HashMap::new();
        for line in String::from_utf8(output.stdout)
            .context("git ls-remote output is not UTF-8")?
            .lines()
        {
            let (oid, reference) = line
                .split_once('\t')
                .with_context(|| format!("invalid git ls-remote output: {line:?}"))?;
            refs.insert(reference.to_owned(), Oid::from_str(oid)?);
        }

        validate_pr_refs(changes, &refs)?;

        let mut fetch = Command::new("git");
        fetch.args(["fetch", "--no-write-fetch-head", &remote_url]);
        fetch.args(refs.keys());
        exec(&mut fetch).context("could not fetch remote change objects")?;
        Ok(Self { refs })
    }

    pub fn get(&self, reference: &str) -> Option<Oid> {
        self.refs.get(reference).copied()
    }

    pub fn require(&self, reference: &str) -> Result<Oid> {
        self.get(reference)
            .with_context(|| format!("remote ref does not exist: {reference}"))
    }
}

fn validate_pr_refs(changes: &[AnyChange], refs: &HashMap<String, Oid>) -> Result<()> {
    let base_ref = branch_ref(env::get().base_branch());
    if !refs.contains_key(&base_ref) {
        bail!("remote base branch does not exist: {base_ref}");
    }
    for change in changes {
        if let AnyChange::Change(change) = change {
            let head_ref = change.local_change.remote_branch_ref();
            if branch_ref(&change.pr.head_ref_name) != head_ref {
                bail!(
                    "pr {} for change {} has unexpected head branch {:?}",
                    change.pr.number,
                    change.local_change.id,
                    change.pr.head_ref_name
                );
            }
            if !refs.contains_key(&head_ref) {
                bail!(
                    "remote branch for pr {} does not exist: {head_ref}",
                    change.pr.number
                );
            }
            let old_base_ref = branch_ref(&change.pr.base_ref_name);
            if !refs.contains_key(&old_base_ref) {
                bail!(
                    "base branch for pr {} does not exist: {old_base_ref}",
                    change.pr.number
                );
            }
        }
    }
    Ok(())
}

struct BranchUpdate {
    reference: String,
    old_oid: Option<Oid>,
    new_oid: Oid,
}

pub struct PublicationPlan {
    updates: Vec<BranchUpdate>,
    bases: Vec<String>,
    moving_earlier: HashSet<String>,
}

impl PublicationPlan {
    pub fn build(changes: &[AnyChange], remote_refs: &RemoteRefs) -> Result<Self> {
        let repo = env::get().repo()?;
        let mut parent_oid = remote_refs.require(&branch_ref(env::get().base_branch()))?;
        let mut updates = Vec::with_capacity(changes.len());

        for change in changes.iter().rev() {
            let local = change.local_change();
            let reference = local.remote_branch_ref();
            let old_oid = remote_refs.get(&reference);
            let new_oid = plan_commit(repo, local, old_oid, parent_oid)?;
            updates.push(BranchUpdate {
                reference,
                old_oid,
                new_oid,
            });
            parent_oid = new_oid;
        }
        let bases = desired_bases(changes);
        let moving_earlier = changes_moving_earlier(changes)?;
        Ok(Self {
            updates,
            bases,
            moving_earlier,
        })
    }

    pub fn has_changes_moving_earlier(&self) -> bool {
        !self.moving_earlier.is_empty()
    }

    pub fn is_moving_earlier(&self, change_id: &str) -> bool {
        self.moving_earlier.contains(change_id)
    }

    pub fn bases(&self) -> &[String] {
        &self.bases
    }

    pub fn push(&self) -> Result<()> {
        let refspecs: Vec<String> = self
            .updates
            .iter()
            .filter(|update| update.old_oid != Some(update.new_oid))
            .map(|update| format!("{}:{}", update.new_oid, update.reference))
            .collect();
        if refspecs.is_empty() {
            return Ok(());
        }

        let mut command = Command::new("git");
        command.args(["push", "--atomic", &env::get().remote_url()?]);
        command.args(refspecs);
        if env::get().dry_run() {
            print_cmd_and_files(&command, std::iter::empty())?;
        } else {
            exec(&mut command)?;
        }
        Ok(())
    }
}

fn desired_bases(changes: &[AnyChange]) -> Vec<String> {
    changes
        .iter()
        .enumerate()
        .map(|(index, _)| {
            changes[index + 1..]
                .first()
                .map(|change| change.local_change().remote_branch())
                .unwrap_or_else(|| env::get().base_branch().to_owned())
        })
        .collect()
}

fn changes_moving_earlier(changes: &[AnyChange]) -> Result<HashSet<String>> {
    let existing: Vec<&Change> = changes
        .iter()
        .filter_map(|change| match change {
            AnyChange::Change(change) => Some(change),
            AnyChange::LocalChange(_) => None,
        })
        .collect();
    let old_ranks = old_stack_ranks(&existing)?;
    Ok(existing
        .iter()
        .rev()
        .enumerate()
        .filter(|(new_rank, change)| new_rank < &old_ranks[&change.local_change.id])
        .map(|(_, change)| change.local_change.id.clone())
        .collect())
}

fn old_stack_ranks(changes: &[&Change]) -> Result<HashMap<String, usize>> {
    if changes.iter().all(|change| change.pr.stack_entry.is_some()) {
        let mut old_order = changes.to_vec();
        old_order.sort_unstable_by_key(|change| change.pr.stack_entry.as_ref().unwrap().position);
        return Ok(old_order
            .into_iter()
            .enumerate()
            .map(|(rank, change)| (change.local_change.id.clone(), rank))
            .collect());
    }

    let by_branch: HashMap<String, &Change> = changes
        .iter()
        .map(|change| (change.local_change.remote_branch(), *change))
        .collect();
    changes
        .iter()
        .map(|change| {
            let mut position = 0;
            let mut base = change.pr.base_ref_name.as_str();
            let mut seen = HashSet::new();
            while let Some(parent) = by_branch.get(base) {
                if !seen.insert(base) {
                    bail!("cycle in existing PR base branches at {base}");
                }
                position += 1;
                base = &parent.pr.base_ref_name;
            }
            Ok((change.local_change.id.clone(), position))
        })
        .collect()
}

fn plan_commit(
    repo: &Repository,
    local: &LocalChange,
    old_oid: Option<Oid>,
    parent_oid: Oid,
) -> Result<Oid> {
    let source = repo.find_commit(local.oid)?;
    let desired_tree = source.tree()?;
    let parent_is_ancestor = old_oid
        .map(|old_oid| is_ancestor(repo, parent_oid, old_oid))
        .transpose()?
        .unwrap_or(false);
    if let Some(old_oid) = old_oid
        && repo.find_commit(old_oid)?.tree_id() == desired_tree.id()
        && parent_is_ancestor
    {
        return Ok(old_oid);
    }

    // The old tip is always the first parent, making the remote update a fast-forward. The
    // current stack parent is included only when the old history does not already contain it.
    let old_commit = old_oid.map(|oid| repo.find_commit(oid)).transpose()?;
    let parent_commit = repo.find_commit(parent_oid)?;
    let mut parents = Vec::with_capacity(2);
    if let Some(old_commit) = old_commit.as_ref() {
        parents.push(old_commit);
    }
    if !parent_is_ancestor {
        parents.push(&parent_commit);
    }
    let message = source
        .message()
        .with_context(|| format!("commit {} message is not UTF-8", source.id()))?;
    repo.commit(
        None,
        &source.author(),
        &source.committer(),
        message,
        &desired_tree,
        &parents,
    )
    .context("could not create published commit")
}

fn is_ancestor(repo: &Repository, ancestor: Oid, descendant: Oid) -> Result<bool> {
    Ok(ancestor == descendant || repo.graph_descendant_of(descendant, ancestor)?)
}

pub fn branch_ref(name: &str) -> String {
    if name.starts_with("refs/") {
        name.to_owned()
    } else {
        format!("refs/heads/{name}")
    }
}

// Copyright © 2026 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use crate::env;
use crate::gh::Pr;
use crate::util::exec;
use anyhow::{Context, Result, bail};
use git2::{
    Commit, DiffDelta, DiffFormat, DiffHunk, DiffLine, DiffOptions, FileFavor, MergeOptions, Oid,
    Tree, message_trailers_bytes,
};
use std::process::Command;

#[derive(Debug)]
pub enum AnyChange {
    LocalChange(LocalChange),
    Change(Change),
}

impl AnyChange {
    pub fn local_change(&self) -> &LocalChange {
        match self {
            AnyChange::LocalChange(local_change) => local_change,
            AnyChange::Change(change) => &change.local_change,
        }
    }
    pub fn diff(&self) -> Result<Diff> {
        Ok(match self {
            AnyChange::LocalChange(local_change) => Diff::InitialDiff(local_change.diff()?),
            AnyChange::Change(change) => Diff::InterDiff(change.interdiff()?),
        })
    }
}

pub enum Diff {
    InitialDiff(String),
    InterDiff(String),
}

type DiffPrinter<'a> = Box<dyn FnMut(DiffDelta, Option<DiffHunk>, DiffLine) -> bool + 'a>;

fn diff_printer<'a>(out: &'a mut String) -> DiffPrinter<'a> {
    Box::new(|_delta, _hunk, line| {
        match line.origin() {
            '+' | '-' | ' ' => out.push(line.origin()),
            _ => {}
        }
        let s = str::from_utf8(line.content()).expect("non-utf8 encoded content in diff");
        // The file mode and index info is just noise here, but I don't see how to disable both
        // in the options. Just rely on the output of the 'F' origin "line" and strip
        // everything but the `diff --git <A> <B>` part.
        if line.origin() == 'F'
            && let Some((p, _)) = s.split_once('\n')
        {
            out.push_str(p);
            out.push('\n')
        } else {
            out.push_str(s)
        };
        true
    })
}

#[derive(Debug)]
pub struct LocalChange {
    pub id: String,
    pub oid: Oid,
}

impl LocalChange {
    pub fn remote_branch(&self) -> String {
        let branch_prefix = env::user_branch_prefix();
        let change_id = &self.id;
        format!("{branch_prefix}{change_id}")
    }
    pub fn remote_branch_ref(&self) -> String {
        format!("refs/heads/{}", self.remote_branch())
    }
    pub fn push_refspec(&self) -> String {
        let oid = self.oid;
        let remote_branch_ref = self.remote_branch_ref();
        format!("{oid}:{remote_branch_ref}")
    }
    pub fn tag_refspec(&self) -> String {
        let oid = self.oid;
        let tag_name = format!("{}{oid}", env::user_branch_prefix());
        format!("{oid}:refs/tags/{tag_name}")
    }
    pub fn push_all<'a, I: Iterator<Item = &'a Self>>(iterator: I) -> Result<()> {
        let local_changes: Vec<&Self> = iterator.collect();
        if local_changes.is_empty() {
            bail!("no refs to push");
        }
        let refspecs: Vec<String> = local_changes.iter().map(|lc| lc.push_refspec()).collect();
        let mut cmd = Command::new("git");
        let mut args = vec![
            "push".to_string(),
            env::remote().into(),
            "--force".into(),
            "--atomic".into(),
        ];
        args.extend(refspecs);
        cmd.args(args);
        exec!(dry_return = (), cmd);
        Ok(())
    }
    pub fn push_tag(&self) -> Result<()> {
        let tag_refspec = self.tag_refspec();
        let mut cmd = Command::new("git");
        cmd.args([
            "push",
            env::tag_remote(),
            "--atomic",
            tag_refspec.as_str(),
        ]);
        exec!(dry_return = (), cmd);
        Ok(())
    }
    pub fn fetch_all<'a, I: Iterator<Item = &'a Self>>(iterator: I) -> Result<()> {
        let refspecs: Vec<String> = iterator
            .map(|lc| format!("{}:", lc.remote_branch_ref()))
            .collect();
        if refspecs.is_empty() {
            return Ok(());
        }
        let mut cmd = Command::new("git");
        let mut args = vec!["fetch".to_string(), env::remote().into()];
        args.extend(refspecs);
        cmd.args(args);
        exec!(dry_return = (), cmd);
        Ok(())
    }
    pub fn diff(&self) -> Result<String> {
        let change = self.id.as_str();
        let repo = env::repo();
        let commit = self.commit()?;
        let parent = commit
            .parent(0)
            .with_context(|| format!("change {change} has no parent commit",))?;
        let mut diff_opts = DiffOptions::new();
        diff_opts.reverse(true);
        let diff = repo.diff_tree_to_tree(
            Some(&tree(&commit)?),
            Some(&tree(&parent)?),
            Some(&mut diff_opts),
        )?;
        let mut out = String::new();
        diff.print(DiffFormat::Patch, diff_printer(&mut out))
            .with_context(|| format!("failed to generate interdiff for change {change}"))?;
        Ok(out)
    }
    pub fn commit<'repo>(&self) -> Result<Commit<'repo>> {
        Ok(env::repo().find_commit(self.oid)?)
    }
}

#[derive(Debug)]
pub struct Change {
    pub local_change: LocalChange,
    pub pr: Pr,
}

impl Change {
    pub fn render_pr_ui(
        &self,
        changes: &[Self],
        short_name: &str,
        merged_prs: &[Pr],
    ) -> Result<()> {
        let commit = self.local_change.commit()?;
        let mut index = None;
        let title = String::from(
            commit
                .summary()
                .context("failed to get commit summary")?
                .context("commit has no summary")?,
        );
        let mut body = String::from(
            commit
                .body()
                .context("failed to get commit body")?
                .context("commit has no body")?,
        );
        body.push_str("\n\n---\n\n");
        body.push_str("**Stack**:\n");
        for (i, c) in changes.iter().enumerate() {
            body.push_str(&format!("- #{}", c.pr.number));
            if self.pr.number == c.pr.number {
                index = Some(i);
                body.push('⬅');
            }
            body.push('\n');
        }
        let index = index.expect(
            "render_pr_ui asked to render into a stack of changes which does not contain self?",
        );
        for pr in merged_prs.iter().rev() {
            body.push_str(&format!("- #{}\n", pr.number));
        }
        body.push_str(&format!("- `{}`\n\n<sub>(Note: Closed and merged PRs may not be reflected here and PR numbering is not stable.)</sub>\n", env::base_branch()));
        let count = changes.len() + merged_prs.len();
        let position = count - index;
        let prefix = if short_name.is_empty() {
            "".into()
        } else {
            format!("{}: ", short_name)
        };
        self.pr
            .set_title_and_body(&format!("[{prefix}{position}/{count}]: {title}"), &body)
    }
    /// Adapted from https://joshcannon.me/2025/04/05/pr-interdiff.html
    pub fn interdiff(&self) -> Result<String> {
        let change = self.local_change.id.as_str();
        let repo = env::repo();
        let remote_branch = format!("{}/{}", env::remote(), self.local_change.remote_branch());
        let old_commit = repo
            .revparse_single(remote_branch.as_ref())
            .with_context(|| format!("could not parse revspec for remote branch: {remote_branch}"))?
            .peel_to_commit()
            .context("revspec for remote branch did not resolve to a commit")?;
        let new_commit = self.local_change.commit()?;
        let old_merge_base = old_commit
            .parent(0)
            .with_context(|| format!("old version of change {change} has no parent commit"))?;
        let new_merge_base = new_commit
            .parent(0)
            .with_context(|| format!("new version of change {change} has no parent commit",))?;
        let mut merge_opts = MergeOptions::new();
        merge_opts
            .find_renames(true)
            .no_recursive(true)
            .file_favor(FileFavor::Theirs);
        let merge_idx = repo
            .merge_trees(
                &tree(&old_merge_base)?,
                &tree(&new_merge_base)?,
                &tree(&old_commit)?,
                Some(&merge_opts),
            )
            .with_context(|| format!("merge to calculate interdiff for change {change} failed"))?;
        let mut diff_opts = DiffOptions::new();
        diff_opts.reverse(true);
        let diff = repo.diff_tree_to_index(
            Some(&tree(&new_commit)?),
            Some(&merge_idx),
            Some(&mut diff_opts),
        )?;
        let mut out = String::new();
        diff.print(DiffFormat::Patch, diff_printer(&mut out))
            .with_context(|| format!("failed to generate interdiff for change {change}"))?;
        Ok(out)
    }
    pub fn merge(&self) -> Result<()> {
        let commit = self.local_change.commit()?;
        let subject_raw = commit.summary()?.context("couldn't get summary")?;
        let subject = format!("{} (#{})", subject_raw, self.pr.number);
        let body = commit.body()?.context("couldn't get summary")?;
        let sha = format!("{}", self.local_change.oid);
        self.pr.merge(&subject, body, &sha)?;
        Ok(())
    }
}

fn tree<'repo>(commit: &Commit<'repo>) -> Result<Tree<'repo>> {
    commit
        .tree()
        .with_context(|| format!("commit {:?} has no tree?", commit.id()))
}

pub fn get_local_changes() -> Result<Vec<LocalChange>> {
    let repo = env::repo();
    let mut local_changes = vec![];
    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.hide_ref(env::base_branch_ref())?;
    for oid in revwalk {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let trailers = message_trailers_bytes(
            commit
                .message_raw()
                .with_context(|| format!("commit lacks message: {commit:?}"))?,
        )
        .context("message_trailer_bytes failed")?;
        let mut change_ids = trailers.iter().filter_map(|(k, v)| {
            if k == b"Change-Id" {
                Some(String::from_utf8(v.into()))
            } else {
                None
            }
        });
        let id = change_ids
            .next()
            .with_context(|| format!("commit lacks Change-Id: {commit:?}"))?
            .with_context(|| format!("commit Change-Id is not valid utf8: {commit:?}"))?;
        if change_ids.next().is_some() {
            bail!("commit has multiple Change-Id: {commit:?}");
        }
        local_changes.push(LocalChange { id, oid });
    }
    Ok(local_changes)
}

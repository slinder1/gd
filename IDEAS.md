At the core of the "stacked PR" is a hack to work around a deficiency in
GitHub's PR model. This document is intended to catalog the issues that fall
out of that deficiency, and outline possible workarounds.

# Issues

## Supply-chain security

I think the most critical failing of the "stacked PR" model is normalizing the
PR author's control of the diff under review.

With some subtle manipulation of the base branch of intermediate PRs a
malicious author can effectively "launder" their changes to appear as-if they
were just part of the codebase already. Unless reviewers are careful to
meticulously review the stacking of the PRs looking for rogue commits, and
always do full re-review of the final diff before actually approving, they are
left open to someone sneaking in changes which were never actually reviewed or
approved. Put another way: the "stacked PR" crutch makes socially engineering
approval of code which was never actually reviewed far easier.

## Redundant approval

It seems like GitHub recognized the perils of allowing the stacked PR author to
carry forward approval when the PR base changes, and so patched this by
requiring another approval when the base changes.

I don't think this actually moves the needle much, though. Considering only my
own capacity for code review, I imagine that this extra approval traffic more
often leads to reviewers just hitting approve again when they are asked,
because "GitHub stacked PRs just require these extra approvals."

So, while GitHub has nominally patched this issue, I don't feel like they have
really resolved it. The issue is actually non-technical, and a technical patch
can't fix that, it can only push around the blame. At the root there is a
technical fix: supporting a patch series workflow in PRs without the "stacked
PR" hack.

## Loss of comment context

Any force-push means PR feedback context is lost. This leads to the following
guidance:

* Avoid rebasing your PRs for as long as you can, as a true rebase requires a
force push.
* When addressing feedback, add fixup commits instead of amending.

These constraints make maintaining a large patch series very difficult in an
active project like e.g. LLVM:

* The longer one avoids rebasing the more painful it eventually is, and the
higher the risk that the patch fundamentally changes in a way that will require
duplicated effort in reviewing later.
* Littering a series with many fixups makes it difficult to manage, and forces
the author to maintain the logical patchset in their mind rather than record it
to the branch.

Some of this can be mitigated with tools like `rerere` and frequent uncommited
rebasing/merging. However, it is fundamentally a chore, and one that is clearly
not fundamental to the code review process, as evidenced by tools like Gerrit
Just Working while requiring no such guidance.

# Ideas

Below I will refer to a "change" in the same way that it is understood in this
codebase: a unique change, identified by a string which is currently encoded
as a "Commit-Id:" footer in commit messages and PR bodies. A change "has"
local commit(s) and PR(s).

# Fast-forward-only change branches

Each change has one remote branch. The bottom PR targets the configured base
branch, and each later PR targets its parent's change branch.

On first publication, the branch tip has the local commit's tree and the
published parent branch tip as its parent. On update, a synthetic commit has:

* The previous tip of its own branch as its first parent.
* The newly published parent branch tip as a second parent when it is not
  already reachable from the previous tip.
* The local commit's cumulative tree.

This makes every existing branch update a fast-forward while preserving the
same PR base/head relationship. A parent update propagates through every
descendant, but an unchanged branch is not advanced when its tree and ancestry
already match.

The synthetic commits are written without changing local refs, the index, the
worktree, or existing commits. Only one branch per Change-Id is published; no
metadata, `orig`, or partial-stack staging refs are required. PR identity and
stack state are reconstructed from GitHub using the Change-Id in each PR body.

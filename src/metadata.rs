use crate::env;
use crate::util::RepoExt;
use anyhow::{Context, Result, bail};
use std::collections::HashSet;

#[derive(Default, Debug)]
pub struct StackMetadata {
    pub short_name: String,
    pub merged_change_ids: Vec<String>,
}

const SHORT_NAME_KEY: &str = "shortName";
const MERGED_CHANGE_IDS_KEY: &str = "mergedChangeIds";

impl StackMetadata {
    pub fn from_repo() -> Result<Self> {
        let repo = env::get().repo()?;
        let branch = repo.head_branch().context("HEAD must be a branch")?;
        let branch_config = repo.branch_config(&branch)?;
        let short_name = branch_config.get(SHORT_NAME_KEY)?;
        let merged_change_ids = branch_config
            .get(MERGED_CHANGE_IDS_KEY)?
            .split(':')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        let res = StackMetadata {
            short_name,
            merged_change_ids,
        };
        let mut changed_set = HashSet::new();
        for merged_change_id in res.merged_change_ids.iter() {
            if !changed_set.insert(merged_change_id.as_str()) {
                bail!(
                    "duplicate in merged_change_ids: {merged_change_id} (correct with `git branch --edit-description`)"
                );
            }
        }
        Ok(res)
    }
    pub fn to_repo(&self) -> Result<()> {
        if env::get().dry_run() {
            eprintln!("would-set: {:?}", self);
            return Ok(());
        }
        let repo = env::get().repo()?;
        let branch = repo.head_branch().context("HEAD must be a branch")?;
        let mut branch_config = repo.branch_config(&branch)?;
        branch_config.set(SHORT_NAME_KEY, &self.short_name)?;
        branch_config.set(MERGED_CHANGE_IDS_KEY, &self.merged_change_ids.join(":"))?;
        Ok(())
    }
}

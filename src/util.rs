// Copyright © 2026 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use anyhow::{Context, Result, bail};
use std::fmt::Debug;
use std::process::{Command, Output};

const MAX_VERBOSE_LINE_BYTES: usize = 100;

fn truncate_line(line: &str) -> String {
    if line.len() <= MAX_VERBOSE_LINE_BYTES {
        return line.to_owned();
    }
    let marker = "[...]";
    let prefix_bytes = (MAX_VERBOSE_LINE_BYTES - marker.len()) / 2;
    let suffix_bytes = MAX_VERBOSE_LINE_BYTES - marker.len() - prefix_bytes;
    let mut prefix_end = prefix_bytes;
    while !line.is_char_boundary(prefix_end) {
        prefix_end -= 1;
    }
    let mut suffix_start = line.len() - suffix_bytes;
    while !line.is_char_boundary(suffix_start) {
        suffix_start += 1;
    }
    format!("{}{}{}", &line[..prefix_end], marker, &line[suffix_start..])
}

fn print_output(prefix: &str, output: &[u8], truncate: bool) {
    for line in String::from_utf8_lossy(output).lines() {
        let line = if truncate {
            truncate_line(line)
        } else {
            line.to_owned()
        };
        eprintln!("{prefix}{line}");
    }
}

pub fn exec_impl(env: &crate::env::Env, cmd: &mut Command) -> Result<Output> {
    let id = env.next_exec_id();
    if env.always_echo() {
        eprintln!("exec-{}: {:?}", id, cmd);
    }
    let output = cmd
        .output()
        .with_context(|| format!("exec-failed: {:?}", cmd))?;
    if env.dry_run() || env.verbosity() > 0 || !output.status.success() {
        let truncate = env.verbosity() == 1;
        print_output(
            &format!("exec-{id}-stdout: "),
            output.stdout.as_ref(),
            truncate,
        );
        print_output(
            &format!("exec-{id}-stderr: "),
            output.stderr.as_ref(),
            truncate,
        );
    }
    if !output.status.success() {
        bail!("exec-{}-status-non-zero: {:?}", id, output.status);
    }
    Ok(output)
}

macro_rules! exec {
    ($env:expr, $cmd:ident) => {
        $crate::util::exec_impl($env, &mut $cmd)?
    };
    ($env:expr, dry_return=$dry_return:expr, $cmd:ident) => {{
        if $env.dry_run() {
            eprintln!("would-exec: {:?}", $cmd);
            return Ok($dry_return);
        } else {
            $crate::util::exec!($env, $cmd)
        }
    }};
}
pub(crate) use exec;

pub trait Extract {
    type T;

    fn extract(self) -> Self::T;
}

impl<T, E: Debug> Extract for std::result::Result<T, E> {
    type T = T;
    fn extract(self) -> T {
        match self {
            Ok(x) => x,
            Err(e) => {
                eprint!("Error: {e:?}");
                std::process::exit(-1);
            }
        }
    }
}

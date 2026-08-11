// Copyright © 2026 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use anyhow::{Context, Result, bail};
use std::fmt::Debug;
use std::process::{Command, Output};

pub fn exec_impl(env: &crate::env::Env, cmd: &mut Command) -> Result<Output> {
    let id = env.next_exec_id();
    if env.always_echo() {
        eprintln!("exec-{}: {:?}", id, cmd);
    }
    let output = cmd
        .output()
        .with_context(|| format!("exec-failed: {:?}", cmd))?;
    if env.always_echo() || !output.status.success() {
        for line in String::from_utf8_lossy(output.stdout.as_ref()).lines() {
            eprintln!("exec-{}-stdout: {}", id, line);
        }
        for line in String::from_utf8_lossy(output.stderr.as_ref()).lines() {
            eprintln!("exec-{}-stderr: {}", id, line);
        }
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

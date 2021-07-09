#![allow(unused)]
#![crate_name = "occlum_libos_core_rs"]
#![crate_type = "staticlib"]
#![cfg_attr(not(target_env = "sgx"), no_std)]
#![cfg_attr(target_env = "sgx", feature(rustc_private))]
#![feature(allocator_api)]
#![feature(core_intrinsics)]
#![feature(stmt_expr_attributes)]
#![feature(alloc_layout_extra)]
#![feature(concat_idents)]
#![feature(trace_macros)]
// for !Send in rw_lock
#![feature(negative_impls)]
// for may_dangle in rw_lock
#![feature(dropck_eyepatch)]
#![feature(option_expect_none)]
// for UntrustedSliceAlloc in slice_alloc
#![feature(slice_ptr_get)]
#![feature(maybe_uninit_extra)]
#![feature(get_mut_unchecked)]
#![feature(drain_filter)]
#![feature(test)]

#[macro_use]
extern crate async_rt;
#[macro_use]
extern crate alloc;
#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate bitvec;
extern crate sgx_types;
#[cfg(not(target_env = "sgx"))]
#[macro_use]
extern crate sgx_tstd as std;
extern crate sgx_tcrypto;
extern crate sgx_trts;
extern crate sgx_tse;
extern crate xmas_elf;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate rcore_fs;
extern crate rcore_fs_devfs;
extern crate rcore_fs_mountfs;
extern crate rcore_fs_ramfs;
extern crate rcore_fs_sefs;
extern crate rcore_fs_unionfs;
#[macro_use]
extern crate derive_builder;
extern crate ringbuf;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate memoffset;
extern crate vdso_time;

use sgx_trts::libc;
use sgx_types::*;
use std::backtrace::{self, PrintFormat};
use std::ffi::CStr; // a borrowed C string
use std::panic;

use crate::prelude::*;
use crate::process::pid_t;

#[macro_use]
mod prelude;

mod config;
mod entry;
mod fs;
mod io_uring;
mod misc;
mod net;
mod poll;
mod process;
mod sched;
mod signal;
mod time;
mod untrusted;
mod util;
mod vm;

// Export ECalls
pub use entry::enclave::*;

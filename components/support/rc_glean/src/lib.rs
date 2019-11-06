/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![allow(unknown_lints)]
#![warn(rust_2018_idioms)]

use std::convert::{TryFrom, TryInto};
use std::env;
use std::ffi::{CStr, CString};
use std::process::{Command, Stdio};
use std::sync::Once;
use std::time::Instant;

use lazy_static;

pub use glean_core::{CommonMetricData, Lifetime};
pub use glean_core::metrics::{TimerId, TimeUnit};
use glean_sys::*;

type Handle = u64;

// For ABI-compatibility reasons, it's important that we're loading exactly the
// right version of `libglean_ffi.so`; check it on first use.

static GLEAN_FFI_VERSION_CHECKED: Once = Once::new();

pub fn ensure_glean_ffi_version() {
    GLEAN_FFI_VERSION_CHECKED.call_once(|| {
        let version = unsafe { CStr::from_ptr(glean_get_version()) };
        // XXX TODO: need to actually check this...
        log::debug!("CHECKING GLEAN_FFI VERSION: {}", version.to_str().unwrap());
    })
}

// Glean seems to expect all metrics to be recorded using a single clock (which is reasonable!).
// This tries to construct a matching clock by measuring the offset between rust's `Instant` clock
// and whatever clock is being used by the containing application.
//
// The android lib uses SystemClock.elapsedRealTime, which is monotonic and keeps ticking during sleep.
// It's not clear to me whether we can achieve the same from rust...

static mut INITIAL_REALTIME_NANOS: u64 = 0;
lazy_static::lazy_static! {
    static ref INITIAL_REALTIME_INSTANT: Instant = {
        Instant::now()
    };
}

fn cur_timestamp_nanos() -> u64 {
    let elapsed = u64::try_from(INITIAL_REALTIME_INSTANT.elapsed().as_nanos()).unwrap();
    elapsed + unsafe { INITIAL_REALTIME_NANOS }
}

fn cur_timestamp_millis() -> u64 {
    let elapsed = u64::try_from(INITIAL_REALTIME_INSTANT.elapsed().as_millis()).unwrap();
    elapsed + (unsafe { INITIAL_REALTIME_NANOS } / 1000000 )
}

// We need a handle to the global "glean" instance.
// The glean FFI tries hard to be infallible, so if this doesn't get initialized to an actual handle
// before trying to use methods from this module, the worst that will happen is that it will log
// an "invalid handle" error. I think...

static mut GLEAN: Handle = 0;

// The calling application needs to initialize this module in order to
// plumb us in to their Glean singleton, and synchronize the clock.

pub fn initialize(handle: Handle, initial_realitime_nanos: u64) {
    unsafe {
        lazy_static::initialize(&INITIAL_REALTIME_INSTANT);
        INITIAL_REALTIME_NANOS = initial_realitime_nanos;
        assert!(GLEAN == 0);
        GLEAN = handle;
    }
}

// Now we need to implement all the various metric types.
// That's going to be a lot of manual labour.
//
// For now just doing a couple as a proof-of-concept.

fn lifetime_to_i32(lifetime: Lifetime) -> i32 {
    match lifetime {
        Lifetime::Ping => 0,
        Lifetime::Application => 1,
        Lifetime::User => 2,
    }
}

fn timeunit_to_i32(time_unit: TimeUnit) -> i32 {
    match time_unit {
        TimeUnit::Nanosecond => 0,
        TimeUnit::Microsecond => 1,
        TimeUnit::Millisecond => 2,
        TimeUnit::Second => 3,
        TimeUnit::Minute => 4,
        TimeUnit::Hour => 5,
        TimeUnit::Day => 6,
    }
}

pub struct EventMetric {
    handle: Handle,
}

impl EventMetric {
    pub fn new(meta: CommonMetricData /*extra_keys: [&str]*/) -> Self {
        ensure_glean_ffi_version();
        let send_in_pings_cstrs: Vec<CString> = meta
            .send_in_pings
            .iter()
            .map(|x: &String| CString::new(x.as_str()).unwrap())
            .collect();
        let send_in_pings_ptrs: Vec<*const i8> = send_in_pings_cstrs
            .iter()
            .map(|x: &CString| x.as_ptr())
            .collect();
        Self {
            handle: unsafe {
                glean_new_event_metric(
                    CString::new(meta.category).unwrap().as_ptr(),
                    CString::new(meta.name).unwrap().as_ptr(),
                    send_in_pings_ptrs.as_ptr(),
                    send_in_pings_ptrs.len().try_into().unwrap(),
                    lifetime_to_i32(meta.lifetime),
                    meta.disabled as u8,
                    // XXX TODO: no support for extra_keys currently.
                    std::ptr::null(),
                    0,
                )
            },
        }
    }

    pub fn record(&self, /* extra: M */) {
        log::debug!("RECORD {:?} {:?}", unsafe { GLEAN }, self.handle);
        let timestamp = cur_timestamp_millis();
        unsafe {
            glean_event_record(
                    GLEAN,
                    self.handle,
                    timestamp,
                    // XXX TODO: no support for extra_keys currently
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
            )
        }
    }

    // XXX TODO: extra methods for testing go here...
    // XXX TODO: impl Drop
}

pub struct TimingDistributionMetric {
    handle: Handle,
}

impl TimingDistributionMetric {
    pub fn new(meta: CommonMetricData, time_unit: TimeUnit) -> Self {
        ensure_glean_ffi_version();
        // XXX TODO : share this common setup code between impls.
        let send_in_pings_cstrs: Vec<CString> = meta
            .send_in_pings
            .iter()
            .map(|x: &String| CString::new(x.as_str()).unwrap())
            .collect();
        let send_in_pings_ptrs: Vec<*const i8> = send_in_pings_cstrs
            .iter()
            .map(|x: &CString| x.as_ptr())
            .collect();
        Self {
            handle: unsafe {
                glean_new_timing_distribution_metric(
                    CString::new(meta.category).unwrap().as_ptr(),
                    CString::new(meta.name).unwrap().as_ptr(),
                    send_in_pings_ptrs.as_ptr(),
                    send_in_pings_ptrs.len().try_into().unwrap(),
                    lifetime_to_i32(meta.lifetime),
                    meta.disabled as u8,
                    timeunit_to_i32(time_unit),
                )
            },
        }
    }

    pub fn start(&self) -> TimerId {
        log::debug!("START {:?}", self.handle);
        let timestamp = cur_timestamp_nanos();
        let timer_id = unsafe {
            glean_timing_distribution_set_start(
                self.handle,
                timestamp,
            )
        };
        log::debug!("STARTED {:?} {:?}", self.handle, timer_id);
        return timer_id;
    }

    pub fn stop_and_accumulate(&self, timer_id: TimerId) {
        log::debug!("STOP_AND_ACCUMULATE {:?} {:?} {:?}", unsafe { GLEAN }, self.handle, timer_id);
        let timestamp = cur_timestamp_nanos();
        unsafe {
            glean_timing_distribution_set_stop_and_accumulate(
                GLEAN,
                self.handle,
                timer_id,
                timestamp,
            )
        };
        log::debug!("STOPED_AND_ACCUMULATED {:?} {:?} {:?}", unsafe { GLEAN }, self.handle, timer_id);
    }

    pub fn cancel(&self, timer_id: TimerId) {
        unsafe {
            glean_timing_distribution_cancel(
                self.handle,
                timer_id,
            )
        }
    }

    // XXX TODO: glean_timing_distribution_accumulate_samples()
    // XXX TODO: extra methods for testing go here...
    // XXX TODO: impl Drop
}

// A build-time helper for generating bindings from a .yaml file.
// This should probably return an error instead of panicing, but hey...

pub fn generate_bindings(filename: &str) {
    println!("cargo:rerun-if-changed={}", filename);
    // XXX TODO: assumes you have glean_parser correctly installed.
    // How to bootstrap if you don't have it?
    let output = Command::new("glean_parser")
        .arg("translate")
        .arg("-o")
        .arg(env::var("OUT_DIR").unwrap())
        .arg("-f")
        .arg("rust")
        .arg(filename)
        .stderr(Stdio::inherit())
        .output()
        .unwrap();
    if !output.status.success() {
        panic!("failed to generate metrics stubs!")
    }
}

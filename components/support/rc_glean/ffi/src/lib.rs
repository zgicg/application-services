/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![allow(unknown_lints)]
#![warn(rust_2018_idioms)]

use rc_glean;

#[no_mangle]
pub extern "C" fn rc_glean_initialize(handle: u64, initial_time_millis: u64) {
    rc_glean::initialize(handle, initial_time_millis);
}

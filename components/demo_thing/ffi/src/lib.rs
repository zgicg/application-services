/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![allow(unknown_lints)]
#![warn(rust_2018_idioms)]

use demo_thing;

#[no_mangle]
pub extern "C" fn demo_thing_do_the_thing(intensity: u32) {
    demo_thing::do_the_thing(intensity);
}

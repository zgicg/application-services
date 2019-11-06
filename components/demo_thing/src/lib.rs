/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![allow(unknown_lints)]
#![warn(rust_2018_idioms)]

mod metrics;

pub fn do_the_thing(intensity: u32) {
    let timer = metrics::demo_thing_rs::THING_TIMER.start();

    log::debug!("DID THE THING WITH INTENSITY {}", intensity);

    metrics::demo_thing_rs::THING_TIMER.stop_and_accumulate(timer);
    metrics::demo_thing_rs::DID_IT.record();
}

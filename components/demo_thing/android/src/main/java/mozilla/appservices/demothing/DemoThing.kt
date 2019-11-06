/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

package mozilla.appservices.demothing

import org.mozilla.appservices.demothing.GleanMetrics.DemoThingKt;

import mozilla.appservices.demothing.rust.LibDemoThingFFI
import java.util.concurrent.atomic.AtomicLong

class DemoThing() {

    fun doTheThing(intensity: Int) {
      var timer = DemoThingKt.thingTimer.start()

      LibDemoThingFFI.INSTANCE.demo_thing_do_the_thing(intensity)

      DemoThingKt.thingTimer.stopAndAccumulate(timer)
      DemoThingKt.didIt.record()
    }

}

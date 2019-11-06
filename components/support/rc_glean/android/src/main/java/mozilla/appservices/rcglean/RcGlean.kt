/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

package mozilla.appservices.rcglean

import mozilla.appservices.rcglean.rust.LibRcGleanFFI

class RcGlean() {

    fun initialize(handle: Long, initialTimeNanos: Long) {
      LibRcGleanFFI.INSTANCE.rc_glean_initialize(handle, initialTimeNanos)
    }

}

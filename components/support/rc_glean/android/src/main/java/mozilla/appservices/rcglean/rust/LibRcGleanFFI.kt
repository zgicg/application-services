@file:Suppress("MaxLineLength")
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

package mozilla.appservices.rcglean.rust

import com.sun.jna.Library
import mozilla.appservices.support.native.loadIndirect
import org.mozilla.appservices.rcglean.BuildConfig

@Suppress("FunctionNaming", "FunctionParameterNaming", "LongParameterList", "TooGenericExceptionThrown")
internal interface LibRcGleanFFI : Library {
    companion object {
        internal var INSTANCE: LibRcGleanFFI =
            loadIndirect(componentName = "metrics_thing", componentVersion = BuildConfig.LIBRARY_VERSION)
    }

    fun rc_glean_initialize(handle: Long, initial_time_nanos: Long)
}

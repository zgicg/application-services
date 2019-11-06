@file:Suppress("MaxLineLength")
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

package mozilla.appservices.demothing.rust

import com.sun.jna.Library
import mozilla.appservices.support.native.loadIndirect
import org.mozilla.appservices.demothing.BuildConfig

@Suppress("FunctionNaming", "FunctionParameterNaming", "LongParameterList", "TooGenericExceptionThrown")
internal interface LibDemoThingFFI : Library {
    companion object {
        internal var INSTANCE: LibDemoThingFFI =
            loadIndirect(componentName = "demo_thing", componentVersion = BuildConfig.LIBRARY_VERSION)
    }

    fun demo_thing_do_the_thing(intensity: Int)
}

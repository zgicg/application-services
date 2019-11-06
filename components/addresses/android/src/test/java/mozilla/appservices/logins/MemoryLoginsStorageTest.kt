/* Any copyright is dedicated to the Public Domain.
   http://creativecommons.org/publicdomain/zero/1.0/ */

package mozilla.appservices.addresses

import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(manifest = Config.NONE)
class MemoryAddressesStorageTest : AddressesStorageTest() {

    override fun createTestStore(): AddressesStorage {
        return MemoryAddressesStorage(listOf())
    }
}

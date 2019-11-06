@file:Suppress("MaxLineLength")
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

package mozilla.appservices.addresses.rust

import com.sun.jna.Library
import com.sun.jna.Pointer
import com.sun.jna.PointerType
import mozilla.appservices.support.native.loadIndirect
import org.mozilla.appservices.addresses.BuildConfig

@Suppress("FunctionNaming", "FunctionParameterNaming", "LongParameterList", "TooGenericExceptionThrown")
internal interface PasswordSyncAdapter : Library {
    companion object {
        internal var INSTANCE: PasswordSyncAdapter =
            loadIndirect(componentName = "addresses", componentVersion = BuildConfig.LIBRARY_VERSION)
    }

    fun sync15_passwords_state_new(
        mentat_db_path: String,
        encryption_key: String,
        error: RustError.ByReference
    ): AddressesDbHandle

    fun sync15_passwords_state_new_with_hex_key(
        db_path: String,
        encryption_key_bytes: ByteArray,
        encryption_key_len: Int,
        error: RustError.ByReference
    ): AddressesDbHandle

    fun sync15_passwords_state_destroy(handle: AddressesDbHandle, error: RustError.ByReference)

    // Important: strings returned from rust as *char must be Pointers on this end, returning a
    // String will work but either force us to leak them, or cause us to corrupt the heap (when we
    // free them).

    // Returns null if the id does not exist, otherwise json
    fun sync15_passwords_get_by_id(handle: AddressesDbHandle, id: String, error: RustError.ByReference): Pointer?

    // return json array
    fun sync15_passwords_get_all(handle: AddressesDbHandle, error: RustError.ByReference): Pointer?

    // return json array
    fun sync15_passwords_get_by_hostname(handle: AddressesDbHandle, hostname: String, error: RustError.ByReference): Pointer?

    // Returns a JSON string containing a sync ping.
    fun sync15_passwords_sync(
        handle: AddressesDbHandle,
        key_id: String,
        access_token: String,
        sync_key: String,
        token_server_url: String,
        error: RustError.ByReference
    ): Pointer?

    fun sync15_passwords_wipe(handle: AddressesDbHandle, error: RustError.ByReference)
    fun sync15_passwords_wipe_local(handle: AddressesDbHandle, error: RustError.ByReference)
    fun sync15_passwords_reset(handle: AddressesDbHandle, error: RustError.ByReference)

    fun sync15_passwords_touch(handle: AddressesDbHandle, id: String, error: RustError.ByReference)
    // This is 1 for true and 0 for false, it would be a boolean but we need to return a value with
    // a known size.
    fun sync15_passwords_delete(handle: AddressesDbHandle, id: String, error: RustError.ByReference): Byte
    // Note: returns guid of new address entry (unless one was specifically requested)
    fun sync15_passwords_add(handle: AddressesDbHandle, new_address_json: String, error: RustError.ByReference): Pointer?
    fun sync15_passwords_update(handle: AddressesDbHandle, existing_address_json: String, error: RustError.ByReference)
    fun sync15_passwords_import(handle: AddressesDbHandle, addresses_json: String, error: RustError.ByReference): Long

    fun sync15_passwords_destroy_string(p: Pointer)

    fun sync15_passwords_new_interrupt_handle(handle: AddressesDbHandle, error: RustError.ByReference): RawAddressesInterruptHandle?
    fun sync15_passwords_interrupt(handle: RawAddressesInterruptHandle, error: RustError.ByReference)
    fun sync15_passwords_interrupt_handle_destroy(handle: RawAddressesInterruptHandle)
}

internal typealias AddressesDbHandle = Long

internal class RawAddressesInterruptHandle : PointerType()

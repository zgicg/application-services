/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

package mozilla.appservices.addresses.rust

import com.sun.jna.Pointer
import com.sun.jna.Structure
import mozilla.appservices.addresses.IdCollisionException
import mozilla.appservices.addresses.InvalidKeyException
import mozilla.appservices.addresses.InvalidRecordException
import mozilla.appservices.addresses.AddressesStorageException
import mozilla.appservices.addresses.NoSuchRecordException
import mozilla.appservices.addresses.RequestFailedException
import mozilla.appservices.addresses.InterruptedException
import mozilla.appservices.addresses.SyncAuthInvalidException
import mozilla.appservices.addresses.getAndConsumeRustString
import mozilla.appservices.addresses.getRustString

/**
 * This should be considered private, but it needs to be public for JNA.
 */
@Structure.FieldOrder("code", "message")
open class RustError : Structure() {

    class ByReference : RustError(), Structure.ByReference

    @JvmField var code: Int = 0
    @JvmField var message: Pointer? = null

    /**
     * Does this represent failure?
     */
    fun isFailure(): Boolean {
        return code != 0
    }

    @Suppress("ReturnCount", "TooGenericExceptionThrown", "ComplexMethod")
    fun intoException(): AddressesStorageException {
        if (!isFailure()) {
            // It's probably a bad idea to throw here! We're probably leaking something if this is
            // ever hit! (But we shouldn't ever hit it?)
            throw RuntimeException("[Bug] intoException called on non-failure!")
        }
        val message = this.consumeErrorMessage()
        when (code) {
            1 -> return SyncAuthInvalidException(message)
            2 -> return NoSuchRecordException(message)
            3 -> return IdCollisionException(message)
            4 -> return InvalidRecordException(message)
            5 -> return InvalidKeyException(message)
            6 -> return RequestFailedException(message)
            7 -> return InterruptedException(message)
            else -> return AddressesStorageException(message)
        }
    }

    /**
     * Get and consume the error message, or null if there is none.
     */
    @Synchronized
    fun consumeErrorMessage(): String {
        val result = this.message?.getAndConsumeRustString()
        this.message = null
        if (result == null) {
            throw NullPointerException("consumeErrorMessage called with null message!")
        }
        return result
    }

    @Synchronized
    fun ensureConsumed() {
        this.message?.getAndConsumeRustString()
        this.message = null
    }

    /**
     * Get the error message or null if there is none.
     */
    fun getMessage(): String? {
        return this.message?.getRustString()
    }
}

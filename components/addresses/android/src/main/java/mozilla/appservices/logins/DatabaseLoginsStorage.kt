/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

package mozilla.appservices.addresses

import com.sun.jna.Pointer
import mozilla.appservices.addresses.rust.PasswordSyncAdapter
import mozilla.appservices.addresses.rust.RustError
import mozilla.appservices.sync15.SyncTelemetryPing
import java.util.concurrent.atomic.AtomicLong
import org.json.JSONArray

/**
 * AddressesStorage implementation backed by a database.
 */
class DatabaseAddressesStorage(private val dbPath: String) : AutoCloseable, AddressesStorage {
    private var raw: AtomicLong = AtomicLong(0)

    override fun isLocked(): Boolean {
        return raw.get() == 0L
    }

    private fun checkUnlocked(): Long {
        val handle = raw.get()
        if (handle == 0L) {
            throw AddressesStorageException("Using DatabaseAddressesStorage without unlocking first")
        }
        return handle
    }

    /**
     * Return the raw handle used to reference this addresses database.
     *
     * Generally should only be used to pass the handle into `SyncManager.setAddresses`.
     *
     * Note: handles do not remain valid after locking / unlocking the addresses database.
     */
    override fun getHandle(): Long {
        return this.raw.get()
    }

    @Synchronized
    @Throws(AddressesStorageException::class)
    override fun lock() {
        val raw = this.raw.getAndSet(0)
        if (raw == 0L) {
            throw MismatchedLockException("Lock called when we are already locked")
        }
        rustCall { error ->
            PasswordSyncAdapter.INSTANCE.sync15_passwords_state_destroy(raw, error)
        }
    }

    @Synchronized
    @Throws(AddressesStorageException::class)
    override fun unlock(encryptionKey: String) {
        return rustCall {
            if (!isLocked()) {
                throw MismatchedLockException("Unlock called when we are already unlocked")
            }
            raw.set(PasswordSyncAdapter.INSTANCE.sync15_passwords_state_new(
                    dbPath,
                    encryptionKey,
                    it))
        }
    }

    @Synchronized
    @Throws(AddressesStorageException::class)
    override fun unlock(encryptionKey: ByteArray) {
        return rustCall {
            if (!isLocked()) {
                throw MismatchedLockException("Unlock called when we are already unlocked")
            }
            raw.set(PasswordSyncAdapter.INSTANCE.sync15_passwords_state_new_with_hex_key(
                    dbPath,
                    encryptionKey,
                    encryptionKey.size,
                    it))
        }
    }

    @Synchronized
    @Throws(AddressesStorageException::class)
    override fun ensureUnlocked(encryptionKey: String) {
        if (isLocked()) {
            this.unlock(encryptionKey)
        }
    }

    @Synchronized
    @Throws(AddressesStorageException::class)
    override fun ensureUnlocked(encryptionKey: ByteArray) {
        if (isLocked()) {
            this.unlock(encryptionKey)
        }
    }

    @Synchronized
    override fun ensureLocked() {
        if (!isLocked()) {
            this.lock()
        }
    }

    @Throws(AddressesStorageException::class)
    override fun sync(syncInfo: SyncUnlockInfo): SyncTelemetryPing {
        val json = rustCallWithLock { raw, error ->
            PasswordSyncAdapter.INSTANCE.sync15_passwords_sync(
                    raw,
                    syncInfo.kid,
                    syncInfo.fxaAccessToken,
                    syncInfo.syncKey,
                    syncInfo.tokenserverURL,
                    error
            )?.getAndConsumeRustString()
        }
        return SyncTelemetryPing.fromJSONString(json)
    }

    @Throws(AddressesStorageException::class)
    override fun reset() {
        rustCallWithLock { raw, error ->
            PasswordSyncAdapter.INSTANCE.sync15_passwords_reset(raw, error)
        }
    }

    @Throws(AddressesStorageException::class)
    override fun wipe() {
        rustCallWithLock { raw, error ->
            PasswordSyncAdapter.INSTANCE.sync15_passwords_wipe(raw, error)
        }
    }

    @Throws(AddressesStorageException::class)
    override fun wipeLocal() {
        rustCallWithLock { raw, error ->
            PasswordSyncAdapter.INSTANCE.sync15_passwords_wipe_local(raw, error)
        }
    }

    @Throws(AddressesStorageException::class)
    override fun delete(id: String): Boolean {
        return rustCallWithLock { raw, error ->
            val deleted = PasswordSyncAdapter.INSTANCE.sync15_passwords_delete(raw, id, error)
            deleted.toInt() != 0
        }
    }

    @Throws(AddressesStorageException::class)
    override fun get(id: String): ServerPassword? {
        val json = nullableRustCallWithLock { raw, error ->
            PasswordSyncAdapter.INSTANCE.sync15_passwords_get_by_id(raw, id, error)
        }?.getAndConsumeRustString()
        return json?.let { ServerPassword.fromJSON(it) }
    }

    @Throws(AddressesStorageException::class)
    override fun touch(id: String) {
        rustCallWithLock { raw, error ->
            PasswordSyncAdapter.INSTANCE.sync15_passwords_touch(raw, id, error)
        }
    }

    @Throws(AddressesStorageException::class)
    override fun list(): List<ServerPassword> {
        val json = rustCallWithLock { raw, error ->
            PasswordSyncAdapter.INSTANCE.sync15_passwords_get_all(raw, error)
        }.getAndConsumeRustString()
        return ServerPassword.fromJSONArray(json)
    }

    @Throws(AddressesStorageException::class)
    override fun getByHostname(hostname: String): List<ServerPassword> {
        val json = rustCallWithLock { raw, error ->
            PasswordSyncAdapter.INSTANCE.sync15_passwords_get_by_hostname(raw, hostname, error)
        }.getAndConsumeRustString()
        return ServerPassword.fromJSONArray(json)
    }

    @Throws(AddressesStorageException::class)
    override fun add(address: ServerPassword): String {
        val s = address.toJSON().toString()
        return rustCallWithLock { raw, error ->
            PasswordSyncAdapter.INSTANCE.sync15_passwords_add(raw, s, error)
        }.getAndConsumeRustString()
    }

    @Throws(AddressesStorageException::class)
    override fun importAddresses(addresses: Array<ServerPassword>): Long {
        val s = JSONArray().apply {
            addresses.forEach {
                put(it.toJSON())
            }
        }.toString()
        return rustCallWithLock { raw, error ->
            PasswordSyncAdapter.INSTANCE.sync15_passwords_import(raw, s, error)
        }
    }

    @Throws(AddressesStorageException::class)
    override fun update(address: ServerPassword) {
        val s = address.toJSON().toString()
        return rustCallWithLock { raw, error ->
            PasswordSyncAdapter.INSTANCE.sync15_passwords_update(raw, s, error)
        }
    }

    @Synchronized
    @Throws(AddressesStorageException::class)
    override fun close() {
        val handle = this.raw.getAndSet(0)
        if (handle != 0L) {
            rustCall { err ->
                PasswordSyncAdapter.INSTANCE.sync15_passwords_state_destroy(handle, err)
            }
        }
    }

    // In practice we usually need to be synchronized to call this safely, so it doesn't
    // synchronize itself
    private inline fun <U> nullableRustCall(callback: (RustError.ByReference) -> U?): U? {
        val e = RustError.ByReference()
        try {
            val ret = callback(e)
            if (e.isFailure()) {
                throw e.intoException()
            }
            return ret
        } finally {
            // This only matters if `callback` throws (or does a non-local return, which
            // we currently don't do)
            e.ensureConsumed()
        }
    }

    private inline fun <U> rustCall(callback: (RustError.ByReference) -> U?): U {
        return nullableRustCall(callback)!!
    }

    private inline fun <U> nullableRustCallWithLock(callback: (Long, RustError.ByReference) -> U?): U? {
        return synchronized(this) {
            val handle = checkUnlocked()
            nullableRustCall { callback(handle, it) }
        }
    }

    private inline fun <U> rustCallWithLock(callback: (Long, RustError.ByReference) -> U?): U {
        return nullableRustCallWithLock(callback)!!
    }
}

/**
 * Helper to read a null terminated String out of the Pointer and free it.
 *
 * Important: Do not use this pointer after this! For anything!
 */
internal fun Pointer.getAndConsumeRustString(): String {
    try {
        return this.getRustString()
    } finally {
        PasswordSyncAdapter.INSTANCE.sync15_passwords_destroy_string(this)
    }
}

/**
 * Helper to read a null terminated string out of the pointer.
 *
 * Important: doesn't free the pointer, use [getAndConsumeRustString] for that!
 */
internal fun Pointer.getRustString(): String {
    return this.getString(0, "utf8")
}

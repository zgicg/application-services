/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

package mozilla.appservices.addresses
import mozilla.appservices.sync15.SyncTelemetryPing

class SyncUnlockInfo(
    val kid: String,
    val fxaAccessToken: String,
    val syncKey: String,
    val tokenserverURL: String
)

interface AddressesStorage : AutoCloseable {
    /**
     * Lock (close) the database.
     *
     * @throws [MismatchedLockException] if the database is already locked
     */
    @Throws(AddressesStorageException::class)
    fun lock()

    /**
     * Unlock (open) the database.
     *
     * @throws [MismatchedLockException] if the database is already unlocked
     * @throws [InvalidKeyException] if the encryption key is wrong, or the db is corrupt
     * @throws [AddressesStorageException] if there was some other error opening the database
     */
    @Throws(AddressesStorageException::class)
    fun unlock(encryptionKey: String)

    /**
     * Unlock (open) the database, using a byte string as the key.
     * This is equivalent to calling unlock() after hex-encoding the bytes (lower
     * case hexadecimal characters are used).
     *
     * @throws [MismatchedLockException] if the database is already unlocked
     * @throws [InvalidKeyException] if the encryption key is wrong, or the db is corrupt
     * @throws [AddressesStorageException] if there was some other error opening the database
     */
    @Throws(AddressesStorageException::class)
    fun unlock(encryptionKey: ByteArray)

    /**
     * Returns true if the database is locked, false otherwise.
     */
    fun isLocked(): Boolean

    /**
     * Equivalent to `unlock(encryptionKey)`, but does not throw in the case
     * that the database is already unlocked.
     *
     * @throws [InvalidKeyException] if the encryption key is wrong, or the db is corrupt
     * @throws [AddressesStorageException] if there was some other error opening the database
     */
    @Throws(AddressesStorageException::class)
    fun ensureUnlocked(encryptionKey: String)

    /**
     * Equivalent to `unlock(encryptionKey)`, but does not throw in the case
     * that the database is already unlocked.
     *
     * @throws [InvalidKeyException] if the encryption key is wrong, or the db is corrupt
     * @throws [AddressesStorageException] if there was some other error opening the database
     */
    @Throws(AddressesStorageException::class)
    fun ensureUnlocked(encryptionKey: ByteArray)

    /**
     * Equivalent to `lock()`, but does not throw in the case that
     * the database is already unlocked. Never throws.
     */
    fun ensureLocked()

    /**
     * Synchronize the addresses storage layer with a remote layer.
     *
     * @throws [SyncAuthInvalidException] if authentication needs to be refreshed
     * @throws [RequestFailedException] if there was a network error during connection.
     * @throws [AddressesStorageException] On unexpected errors (IO failure, rust panics, etc)
     */
    @Throws(AddressesStorageException::class)
    fun sync(syncInfo: SyncUnlockInfo): SyncTelemetryPing

    /**
     * Delete all locally stored address sync metadata (last sync timestamps, etc).
     *
     * @throws [AddressesStorageException] On unexpected errors (IO failure, rust panics, etc)
     */
    @Throws(AddressesStorageException::class)
    @Deprecated("Most uses should be replaced with wipe or wipeLocal instead")
    fun reset()

    /**
     * Delete all address records. These deletions will be synced to the server on the next call to sync.
     *
     * @throws [AddressesStorageException] On unexpected errors (IO failure, rust panics, etc)
     */
    @Throws(AddressesStorageException::class)
    fun wipe()

    /**
     * Clear out all local state, bringing us back to the state before the first sync.
     *
     * @throws [AddressesStorageException] On unexpected errors (IO failure, rust panics, etc)
     */
    @Throws(AddressesStorageException::class)
    fun wipeLocal()

    /**
     * Deletes the password with the given ID.
     *
     * Returns true if the deletion did anything, false if no such record exists.
     *
     * @throws [AddressesStorageException] On unexpected errors (IO failure, rust panics, etc)
     */
    @Throws(AddressesStorageException::class)
    fun delete(id: String): Boolean

    /**
     * Fetch a password from the underlying storage layer by ID.
     *
     * Returns `null` if the record does not exist.
     *
     * @throws [AddressesStorageException] On unexpected errors (IO failure, rust panics, etc)
     */
    @Throws(AddressesStorageException::class)
    fun get(id: String): ServerPassword?

    /**
     * Mark the address with the given ID as `in-use`.
     *
     * @throws [NoSuchRecordException] If the record with that ID does not exist.
     * @throws [AddressesStorageException] On unexpected errors (IO failure, rust panics, etc)
     */
    @Throws(AddressesStorageException::class)
    fun touch(id: String)

    /**
     * Fetch the full list of passwords from the underlying storage layer.
     *
     * @throws [AddressesStorageException] On unexpected errors (IO failure, rust panics, etc)
     */
    @Throws(AddressesStorageException::class)
    fun list(): List<ServerPassword>

    /**
     * Fetch the list of passwords for some hostname from the underlying storage layer.
     *
     * @throws [AddressesStorageException] On unexpected errors (IO failure, rust panics, etc)
     */
    @Throws(AddressesStorageException::class)
    fun getByHostname(hostname: String): List<ServerPassword>

    /**
     * Inserts the provided address into the database, returning its id.
     *
     * This function ignores values in metadata fields (`timesUsed`,
     * `timeCreated`, `timeLastUsed`, and `timePasswordChanged`).
     *
     * If address has an empty id field, then a GUID will be
     * generated automatically. The format of generated guids
     * are left up to the implementation of AddressesStorage (in
     * practice the [DatabaseAddressesStorage] generates 12-character
     * base64url (RFC 4648) encoded strings, and [MemoryAddressesStorage]
     * generates strings using [java.util.UUID.toString])
     *
     * This will return an error result if a GUID is provided but
     * collides with an existing record, or if the provided record
     * is invalid (missing password, hostname, or doesn't have exactly
     * one of formSubmitURL and httpRealm).
     *
     * @throws [IdCollisionException] if a nonempty id is provided, and
     * @throws [InvalidRecordException] if the record is invalid.
     * @throws [AddressesStorageException] On unexpected errors (IO failure, rust panics, etc)
     */
    @Throws(AddressesStorageException::class)
    fun add(address: ServerPassword): String

    /**
     * Imports provided addresses into the database.
     * GUIDs are thrown away and replaced by auto-generated ones from the crate.
     *
     * @throws [AddressesStorageException] On unexpected errors (IO failure, rust panics, etc)
     */
    @Throws(AddressesStorageException::class)
    fun importAddresses(addresses: Array<ServerPassword>): Long

    /**
     * Updates the fields in the provided record.
     *
     * This will return an error if `address.id` does not refer to
     * a record that exists in the database, or if the provided record
     * is invalid (missing password, hostname, or doesn't have exactly
     * one of formSubmitURL and httpRealm).
     *
     * Like `add`, this function will ignore values in metadata
     * fields (`timesUsed`, `timeCreated`, `timeLastUsed`, and
     * `timePasswordChanged`).
     *
     * @throws [NoSuchRecordException] if the address does not exist.
     * @throws [InvalidRecordException] if the update would create an invalid record.
     * @throws [AddressesStorageException] On unexpected errors (IO failure, rust panics, etc)
     */
    @Throws(AddressesStorageException::class)
    fun update(address: ServerPassword)

    /**
     * Return the raw handle used to reference this addresses database.
     *
     * This is only valid for the DatabaseAddressesStorage, and was added to this
     * interface regardless by popular demand. Other types will throw an
     * UnsupportedOperationException.
     *
     * Generally should only be used to pass the handle into `SyncManager.setAddresses`.
     *
     * Note: handles do not remain valid after locking / unlocking the addresses database.
     */
    fun getHandle(): Long
}

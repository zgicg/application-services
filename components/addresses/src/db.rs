/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::error::*;
use crate::address::{LocalAddress, Address, MirrorAddress, SyncAddressData, SyncStatus};
use crate::schema;
use crate::update_plan::UpdatePlan;
use crate::util;
use lazy_static::lazy_static;
use rusqlite::{
    named_params,
    types::{FromSql, ToSql},
    Connection, NO_PARAMS,
};
use sql_support::{self, ConnExt};
use sql_support::{SqlInterruptHandle, SqlInterruptScope};
use std::collections::HashSet;
use std::ops::Deref;
use std::path::Path;
use std::result;
use std::sync::{atomic::AtomicUsize, Arc};
use std::time::SystemTime;
use sync15::{
    extract_v1_state, telemetry, CollSyncIds, CollectionRequest, IncomingChangeset,
    OutgoingChangeset, Payload, ServerTimestamp, Store, StoreSyncAssociation,
};
use sync_guid::Guid;

pub struct AddressesDb {
    pub db: Connection,
    interrupt_counter: Arc<AtomicUsize>,
}

impl AddressesDb {
    pub fn with_connection(db: Connection, encryption_key: Option<&str>) -> Result<Self> {
        #[cfg(test)]
        {
            util::init_test_logging();
        }

        if let Some(key) = encryption_key {
            db.set_pragma("key", key)?
                .set_pragma("secure_delete", true)?;

            // SQLcipher pre-4.0.0 compatibility. Using SHA1 still
            // is less than ideal, but should be fine. Real uses of
            // this (lockwise, etc) use a real random string for the
            // encryption key, so the reduced KDF iteration count
            // is fine.
            db.set_pragma("cipher_page_size", 1024)?
                .set_pragma("kdf_iter", 64000)?
                .set_pragma("cipher_hmac_algorithm", "HMAC_SHA1")?
                .set_pragma("cipher_kdf_algorithm", "PBKDF2_HMAC_SHA1")?;
        }

        // `temp_store = 2` is required on Android to force the DB to keep temp
        // files in memory, since on Android there's no tmp partition. See
        // https://github.com/mozilla/mentat/issues/505. Ideally we'd only
        // do this on Android, or allow caller to configure it.
        db.set_pragma("temp_store", 2)?;

        let mut addresses = Self {
            db,
            interrupt_counter: Arc::new(AtomicUsize::new(0)),
        };
        let tx = addresses.db.transaction()?;
        schema::init(&tx)?;
        tx.commit()?;
        Ok(addresses)
    }

    pub fn open(path: impl AsRef<Path>, encryption_key: Option<&str>) -> Result<Self> {
        Ok(Self::with_connection(
            Connection::open(path)?,
            encryption_key,
        )?)
    }

    pub fn open_in_memory(encryption_key: Option<&str>) -> Result<Self> {
        Ok(Self::with_connection(
            Connection::open_in_memory()?,
            encryption_key,
        )?)
    }

    pub fn disable_mem_security(&self) -> Result<()> {
        self.conn().set_pragma("cipher_memory_security", false)?;
        Ok(())
    }

    pub fn new_interrupt_handle(&self) -> SqlInterruptHandle {
        SqlInterruptHandle::new(
            self.db.get_interrupt_handle(),
            self.interrupt_counter.clone(),
        )
    }

    #[inline]
    pub fn begin_interrupt_scope(&self) -> SqlInterruptScope {
        SqlInterruptScope::new(self.interrupt_counter.clone())
    }
}

impl ConnExt for AddressesDb {
    #[inline]
    fn conn(&self) -> &Connection {
        &self.db
    }
}

impl Deref for AddressesDb {
    type Target = Connection;
    #[inline]
    fn deref(&self) -> &Connection {
        &self.db
    }
}

// address specific stuff.

impl AddressesDb {
    fn mark_as_synchronized(
        &self,
        guids: &[&str],
        ts: ServerTimestamp,
        scope: &SqlInterruptScope,
    ) -> Result<()> {
        let tx = self.unchecked_transaction()?;
        sql_support::each_chunk(guids, |chunk, _| -> Result<()> {
            self.db.execute(
                &format!(
                    "DELETE FROM addressesM WHERE guid IN ({vars})",
                    vars = sql_support::repeat_sql_vars(chunk.len())
                ),
                chunk,
            )?;
            scope.err_if_interrupted()?;

            self.db.execute(
                &format!(
                    "INSERT OR IGNORE INTO addressesM (
                         {common_cols}, is_overridden, server_modified
                     )
                     SELECT {common_cols}, 0, {modified_ms_i64}
                     FROM addressesL
                     WHERE is_deleted = 0 AND guid IN ({vars})",
                    common_cols = schema::COMMON_COLS,
                    modified_ms_i64 = ts.as_millis() as i64,
                    vars = sql_support::repeat_sql_vars(chunk.len())
                ),
                chunk,
            )?;
            scope.err_if_interrupted()?;

            self.db.execute(
                &format!(
                    "DELETE FROM addressesL WHERE guid IN ({vars})",
                    vars = sql_support::repeat_sql_vars(chunk.len())
                ),
                chunk,
            )?;
            scope.err_if_interrupted()?;
            Ok(())
        })?;
        self.set_last_sync(ts)?;
        tx.commit()?;
        Ok(())
    }

    // Fetch all the data for the provided IDs.
    // TODO: Might be better taking a fn instead of returning all of it... But that func will likely
    // want to insert stuff while we're doing this so ugh.
    fn fetch_address_data(
        &self,
        records: &[(sync15::Payload, ServerTimestamp)],
        telem: &mut telemetry::EngineIncoming,
        scope: &SqlInterruptScope,
    ) -> Result<Vec<SyncAddressData>> {
        let mut sync_data = Vec::with_capacity(records.len());
        {
            let mut seen_ids: HashSet<Guid> = HashSet::with_capacity(records.len());
            for incoming in records.iter() {
                if seen_ids.contains(&incoming.0.id) {
                    throw!(ErrorKind::DuplicateGuid(incoming.0.id.to_string()))
                }
                seen_ids.insert(incoming.0.id.clone());
                match SyncAddressData::from_payload(incoming.0.clone(), incoming.1) {
                    Ok(v) => sync_data.push(v),
                    Err(e) => {
                        log::error!("Failed to deserialize record {:?}: {}", incoming.0.id, e);
                        // Ideally we'd track new_failed, but it's unclear how
                        // much value it has.
                        telem.failed(1);
                    }
                }
            }
        }
        scope.err_if_interrupted()?;

        sql_support::each_chunk_mapped(
            &records,
            |r| r.0.id.as_str(),
            |chunk, offset| -> Result<()> {
                // pairs the bound parameter for the guid with an integer index.
                let values_with_idx = sql_support::repeat_display(chunk.len(), ",", |i, f| {
                    write!(f, "({},?)", i + offset)
                });
                let query = format!(
                    "WITH to_fetch(guid_idx, fetch_guid) AS (VALUES {vals})
                     SELECT
                         {common_cols},
                         is_overridden,
                         server_modified,
                         NULL as local_modified,
                         NULL as is_deleted,
                         NULL as sync_status,
                         1 as is_mirror,
                         to_fetch.guid_idx as guid_idx
                     FROM addressesM
                     JOIN to_fetch
                         ON addressesM.guid = to_fetch.fetch_guid

                     UNION ALL

                     SELECT
                         {common_cols},
                         NULL as is_overridden,
                         NULL as server_modified,
                         local_modified,
                         is_deleted,
                         sync_status,
                         0 as is_mirror,
                         to_fetch.guid_idx as guid_idx
                     FROM addressesL
                     JOIN to_fetch
                         ON addressesL.guid = to_fetch.fetch_guid",
                    // give each VALUES item 2 entries, an index and the parameter.
                    vals = values_with_idx,
                    common_cols = schema::COMMON_COLS,
                );

                let mut stmt = self.db.prepare(&query)?;

                let rows = stmt.query_and_then(chunk, |row| {
                    let guid_idx_i = row.get::<_, i64>("guid_idx")?;
                    // Hitting this means our math is wrong...
                    assert!(guid_idx_i >= 0);

                    let guid_idx = guid_idx_i as usize;
                    let is_mirror: bool = row.get("is_mirror")?;
                    if is_mirror {
                        sync_data[guid_idx].set_mirror(MirrorAddress::from_row(row)?)?;
                    } else {
                        sync_data[guid_idx].set_local(LocalAddress::from_row(row)?)?;
                    }
                    scope.err_if_interrupted()?;
                    Ok(())
                })?;
                // `rows` is an Iterator<Item = Result<()>>, so we need to collect to handle the errors.
                rows.collect::<Result<_>>()?;
                Ok(())
            },
        )?;
        Ok(sync_data)
    }

    // It would be nice if this were a batch-ish api (e.g. takes a slice of records and finds dupes
    // for each one if they exist)... I can't think of how to write that query, though.
    fn find_dupe(&self, l: &Address) -> Result<Option<Address>> {
        let form_submit_host_port = l
            .form_submit_url
            .as_ref()
            .and_then(|s| util::url_host_port(&s));
        let args = named_params! {
            ":hostname": l.hostname,
            ":http_realm": l.http_realm,
            ":username": l.username,
            ":form_submit": form_submit_host_port,
        };
        let mut query = format!(
            "SELECT {common}
             FROM addressesL
             WHERE hostname IS :hostname
               AND httpRealm IS :http_realm
               AND username IS :username",
            common = schema::COMMON_COLS,
        );
        if form_submit_host_port.is_some() {
            // Stolen from iOS
            query += " AND (formSubmitURL = '' OR (instr(formSubmitURL, :form_submit) > 0))";
        } else {
            query += " AND formSubmitURL IS :form_submit"
        }
        Ok(self.try_query_row(&query, args, |row| Address::from_row(row), false)?)
    }

    pub fn get_all(&self) -> Result<Vec<Address>> {
        let mut stmt = self.db.prepare_cached(&GET_ALL_SQL)?;
        let rows = stmt.query_and_then(NO_PARAMS, Address::from_row)?;
        rows.collect::<Result<_>>()
    }

    pub fn get_by_hostname(&self, hostname: &str) -> Result<Vec<Address>> {
        let mut stmt = self.db.prepare_cached(&GET_ALL_BY_HOSTNAME_SQL)?;
        let rows = stmt.query_and_then(&[hostname], Address::from_row)?;
        rows.collect::<Result<_>>()
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<Address>> {
        self.try_query_row(
            &GET_BY_GUID_SQL,
            &[(":guid", &id as &dyn ToSql)],
            Address::from_row,
            true,
        )
    }

    pub fn touch(&self, id: &str) -> Result<()> {
        let tx = self.unchecked_transaction()?;
        self.ensure_local_overlay_exists(id)?;
        self.mark_mirror_overridden(id)?;
        let now_ms = util::system_time_ms_i64(SystemTime::now());
        // As on iOS, just using a record doesn't flip it's status to changed.
        // TODO: this might be wrong for lockbox!
        self.execute_named_cached(
            "UPDATE addressesL
             SET timeLastUsed = :now_millis,
                 timesUsed = timesUsed + 1,
                 local_modified = :now_millis
             WHERE guid = :guid
                 AND is_deleted = 0",
            named_params! {
                ":now_millis": now_ms,
                ":guid": id,
            },
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn add(&self, mut address: Address) -> Result<Address> {
        address.check_valid()?;

        let tx = self.unchecked_transaction()?;
        let now_ms = util::system_time_ms_i64(SystemTime::now());

        // Allow an empty GUID to be passed to indicate that we should generate
        // one. (Note that the FFI, does not require that the `id` field be
        // present in the JSON, and replaces it with an empty string if missing).
        if address.guid.is_empty() {
            address.guid = Guid::random()
        }

        // Fill in default metadata.
        // TODO: allow this to be provided for testing?
        address.time_created = now_ms;
        address.time_password_changed = now_ms;
        address.time_last_used = now_ms;
        address.times_used = 1;

        let sql = format!(
            "INSERT OR IGNORE INTO addressesL (
                hostname,
                httpRealm,
                formSubmitURL,
                usernameField,
                passwordField,
                timesUsed,
                username,
                password,
                guid,
                timeCreated,
                timeLastUsed,
                timePasswordChanged,
                local_modified,
                is_deleted,
                sync_status
            ) VALUES (
                :hostname,
                :http_realm,
                :form_submit_url,
                :username_field,
                :password_field,
                :times_used,
                :username,
                :password,
                :guid,
                :time_created,
                :time_last_used,
                :time_password_changed,
                :local_modified,
                0, -- is_deleted
                {new} -- sync_status
            )",
            new = SyncStatus::New as u8
        );

        let rows_changed = self.execute_named(
            &sql,
            named_params! {
                ":hostname": address.hostname,
                ":http_realm": address.http_realm,
                ":form_submit_url": address.form_submit_url,
                ":username_field": address.username_field,
                ":password_field": address.password_field,
                ":username": address.username,
                ":password": address.password,
                ":guid": address.guid,
                ":time_created": address.time_created,
                ":times_used": address.times_used,
                ":time_last_used": address.time_last_used,
                ":time_password_changed": address.time_password_changed,
                ":local_modified": now_ms,
            },
        )?;
        if rows_changed == 0 {
            log::error!(
                "Record {:?} already exists (use `update` to update records, not add)",
                address.guid
            );
            throw!(ErrorKind::DuplicateGuid(address.guid.into_string()));
        }
        tx.commit()?;
        Ok(address)
    }

    pub fn import_multiple(&self, addresses: &[Address]) -> Result<u64> {
        // Check if the addresses table is empty first.
        let mut num_existing_addresses =
            self.query_row::<i64, _, _>("SELECT COUNT(*) FROM addressesL", NO_PARAMS, |r| r.get(0))?;
        num_existing_addresses +=
            self.query_row::<i64, _, _>("SELECT COUNT(*) FROM addressesM", NO_PARAMS, |r| r.get(0))?;
        if num_existing_addresses > 0 {
            return Err(ErrorKind::NonEmptyTable.into());
        }
        let tx = self.unchecked_transaction()?;
        let now_ms = util::system_time_ms_i64(SystemTime::now());
        let sql = format!(
            "INSERT OR IGNORE INTO addressesL (
                hostname,
                httpRealm,
                formSubmitURL,
                usernameField,
                passwordField,
                timesUsed,
                username,
                password,
                guid,
                timeCreated,
                timeLastUsed,
                timePasswordChanged,
                local_modified,
                is_deleted,
                sync_status
            ) VALUES (
                :hostname,
                :http_realm,
                :form_submit_url,
                :username_field,
                :password_field,
                :times_used,
                :username,
                :password,
                :guid,
                :time_created,
                :time_last_used,
                :time_password_changed,
                :local_modified,
                0, -- is_deleted
                {new} -- sync_status
            )",
            new = SyncStatus::New as u8
        );
        let mut num_failed = 0;
        for address in addresses {
            if let Err(e) = address.check_valid() {
                log::warn!("Skipping address {} as it is invalid ({}).", address.guid, e);
                num_failed += 1;
                continue;
            }
            let old_guid = &address.guid; // Keep the old GUID around so we can debug errors easily.
            let guid = if old_guid.is_valid_for_sync_server() {
                old_guid.clone()
            } else {
                Guid::random()
            };
            match self.execute_named_cached(
                &sql,
                named_params! {
                    ":hostname": address.hostname,
                    ":http_realm": address.http_realm,
                    ":form_submit_url": address.form_submit_url,
                    ":username_field": address.username_field,
                    ":password_field": address.password_field,
                    ":username": address.username,
                    ":password": address.password,
                    ":guid": guid,
                    ":time_created": address.time_created,
                    ":times_used": address.times_used,
                    ":time_last_used": address.time_last_used,
                    ":time_password_changed": address.time_password_changed,
                    ":local_modified": now_ms,
                },
            ) {
                Ok(_) => log::info!("Imported {} (new GUID {}) successfully.", old_guid, guid),
                Err(e) => {
                    log::warn!("Could not import {} ({}).", old_guid, e);
                    num_failed += 1;
                }
            };
        }
        tx.commit()?;
        Ok(num_failed)
    }

    pub fn update(&self, address: Address) -> Result<()> {
        address.check_valid()?;
        let tx = self.unchecked_transaction()?;
        // Note: These fail with DuplicateGuid if the record doesn't exist.
        self.ensure_local_overlay_exists(address.guid_str())?;
        self.mark_mirror_overridden(address.guid_str())?;

        let now_ms = util::system_time_ms_i64(SystemTime::now());

        let sql = format!(
            "UPDATE addressesL
             SET local_modified      = :now_millis,
                 timeLastUsed        = :now_millis,
                 -- Only update timePasswordChanged if, well, the password changed.
                 timePasswordChanged = (CASE
                     WHEN password = :password
                     THEN timePasswordChanged
                     ELSE :now_millis
                 END),
                 httpRealm           = :http_realm,
                 formSubmitURL       = :form_submit_url,
                 usernameField       = :username_field,
                 passwordField       = :password_field,
                 timesUsed           = timesUsed + 1,
                 username            = :username,
                 password            = :password,
                 hostname            = :hostname,
                 -- leave New records as they are, otherwise update them to `changed`
                 sync_status         = max(sync_status, {changed})
             WHERE guid = :guid",
            changed = SyncStatus::Changed as u8
        );

        self.db.execute_named(
            &sql,
            named_params! {
                ":hostname": address.hostname,
                ":username": address.username,
                ":password": address.password,
                ":http_realm": address.http_realm,
                ":form_submit_url": address.form_submit_url,
                ":username_field": address.username_field,
                ":password_field": address.password_field,
                ":guid": address.guid,
                ":now_millis": now_ms,
            },
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn exists(&self, id: &str) -> Result<bool> {
        Ok(self.db.query_row_named(
            "SELECT EXISTS(
                 SELECT 1 FROM addressesL
                 WHERE guid = :guid AND is_deleted = 0
                 UNION ALL
                 SELECT 1 FROM addressesM
                 WHERE guid = :guid AND is_overridden IS NOT 1
             )",
            named_params! { ":guid": id },
            |row| row.get(0),
        )?)
    }

    /// Delete the record with the provided id. Returns true if the record
    /// existed already.
    pub fn delete(&self, id: &str) -> Result<bool> {
        let tx = self.unchecked_transaction_imm()?;
        let exists = self.exists(id)?;
        let now_ms = util::system_time_ms_i64(SystemTime::now());

        // Directly delete IDs that have not yet been synced to the server
        self.execute_named(
            &format!(
                "DELETE FROM addressesL
                 WHERE guid = :guid
                     AND sync_status = {status_new}",
                status_new = SyncStatus::New as u8
            ),
            named_params! { ":guid": id },
        )?;

        // For IDs that have, mark is_deleted and clear sensitive fields
        self.execute_named(
            &format!(
                "UPDATE addressesL
                 SET local_modified = :now_ms,
                     sync_status = {status_changed},
                     is_deleted = 1,
                     password = '',
                     hostname = '',
                     username = ''
                 WHERE guid = :guid",
                status_changed = SyncStatus::Changed as u8
            ),
            named_params! { ":now_ms": now_ms, ":guid": id },
        )?;

        // Mark the mirror as overridden
        self.execute_named(
            "UPDATE addressesM SET is_overridden = 1 WHERE guid = :guid",
            named_params! { ":guid": id },
        )?;

        // If we don't have a local record for this ID, but do have it in the mirror
        // insert a tombstone.
        self.execute_named(&format!("
            INSERT OR IGNORE INTO addressesL
                    (guid, local_modified, is_deleted, sync_status, hostname, timeCreated, timePasswordChanged, password, username)
            SELECT   guid, :now_ms,        1,          {changed},   '',       timeCreated, :now_ms,                   '',       ''
            FROM addressesM
            WHERE guid = :guid",
            changed = SyncStatus::Changed as u8),
            named_params! { ":now_ms": now_ms, ":guid": id })?;
        tx.commit()?;
        Ok(exists)
    }

    fn mark_mirror_overridden(&self, guid: &str) -> Result<()> {
        self.execute_named_cached(
            "UPDATE addressesM SET is_overridden = 1 WHERE guid = :guid",
            named_params! { ":guid": guid },
        )?;
        Ok(())
    }

    fn ensure_local_overlay_exists(&self, guid: &str) -> Result<()> {
        let already_have_local: bool = self.db.query_row_named(
            "SELECT EXISTS(SELECT 1 FROM addressesL WHERE guid = :guid)",
            named_params! { ":guid": guid },
            |row| row.get(0),
        )?;

        if already_have_local {
            return Ok(());
        }

        log::debug!("No overlay; cloning one for {:?}.", guid);
        let changed = self.clone_mirror_to_overlay(guid)?;
        if changed == 0 {
            log::error!("Failed to create local overlay for GUID {:?}.", guid);
            throw!(ErrorKind::NoSuchRecord(guid.to_owned()));
        }
        Ok(())
    }

    fn clone_mirror_to_overlay(&self, guid: &str) -> Result<usize> {
        Ok(self
            .execute_named_cached(&*CLONE_SINGLE_MIRROR_SQL, &[(":guid", &guid as &dyn ToSql)])?)
    }

    pub fn reset(&self, assoc: &StoreSyncAssociation) -> Result<()> {
        log::info!("Executing reset on password store!");
        let tx = self.db.unchecked_transaction()?;
        self.execute_all(&[
            &*CLONE_ENTIRE_MIRROR_SQL,
            "DELETE FROM addressesM",
            &format!("UPDATE addressesL SET sync_status = {}", SyncStatus::New as u8),
        ])?;
        self.set_last_sync(ServerTimestamp(0))?;
        match assoc {
            StoreSyncAssociation::Disconnected => {
                self.delete_meta(schema::GLOBAL_SYNCID_META_KEY)?;
                self.delete_meta(schema::COLLECTION_SYNCID_META_KEY)?;
            }
            StoreSyncAssociation::Connected(ids) => {
                self.put_meta(schema::GLOBAL_SYNCID_META_KEY, &ids.global)?;
                self.put_meta(schema::COLLECTION_SYNCID_META_KEY, &ids.coll)?;
            }
        };
        self.delete_meta(schema::GLOBAL_STATE_META_KEY)?;
        tx.commit()?;
        Ok(())
    }

    pub fn wipe(&self, scope: &SqlInterruptScope) -> Result<()> {
        let tx = self.unchecked_transaction()?;
        log::info!("Executing wipe on password store!");
        let now_ms = util::system_time_ms_i64(SystemTime::now());
        self.execute(
            &format!(
                "DELETE FROM addressesL WHERE sync_status = {new}",
                new = SyncStatus::New as u8
            ),
            NO_PARAMS,
        )?;
        scope.err_if_interrupted()?;
        self.execute_named(
            &format!(
                "
                UPDATE addressesL
                SET local_modified = :now_ms,
                    sync_status = {changed},
                    is_deleted = 1,
                    password = '',
                    hostname = '',
                    username = ''
                WHERE is_deleted = 0",
                changed = SyncStatus::Changed as u8
            ),
            named_params! { ":now_ms": now_ms },
        )?;
        scope.err_if_interrupted()?;

        self.execute("UPDATE addressesM SET is_overridden = 1", NO_PARAMS)?;
        scope.err_if_interrupted()?;

        self.execute_named(
            &format!("
                INSERT OR IGNORE INTO addressesL
                      (guid, local_modified, is_deleted, sync_status, hostname, timeCreated, timePasswordChanged, password, username)
                SELECT guid, :now_ms,        1,          {changed},   '',       timeCreated, :now_ms,             '',       ''
                FROM addressesM",
                changed = SyncStatus::Changed as u8),
            named_params! { ":now_ms": now_ms })?;
        scope.err_if_interrupted()?;
        tx.commit()?;
        Ok(())
    }

    pub fn wipe_local(&self) -> Result<()> {
        log::info!("Executing wipe_local on password store!");
        let tx = self.unchecked_transaction()?;
        self.execute_all(&[
            "DELETE FROM addressesL",
            "DELETE FROM addressesM",
            "DELETE FROM addressesSyncMeta",
        ])?;
        tx.commit()?;
        Ok(())
    }

    fn reconcile(
        &self,
        records: Vec<SyncAddressData>,
        server_now: ServerTimestamp,
        telem: &mut telemetry::EngineIncoming,
        scope: &SqlInterruptScope,
    ) -> Result<UpdatePlan> {
        let mut plan = UpdatePlan::default();

        for mut record in records {
            scope.err_if_interrupted()?;
            log::debug!("Processing remote change {}", record.guid());
            let upstream = if let Some(inbound) = record.inbound.0.take() {
                inbound
            } else {
                log::debug!("Processing inbound deletion (always prefer)");
                plan.plan_delete(record.guid.clone());
                continue;
            };
            let upstream_time = record.inbound.1;
            match (record.mirror.take(), record.local.take()) {
                (Some(mirror), Some(local)) => {
                    log::debug!("  Conflict between remote and local, Resolving with 3WM");
                    plan.plan_three_way_merge(local, mirror, upstream, upstream_time, server_now);
                    telem.reconciled(1);
                }
                (Some(_mirror), None) => {
                    log::debug!("  Forwarding mirror to remote");
                    plan.plan_mirror_update(upstream, upstream_time);
                    telem.applied(1);
                }
                (None, Some(local)) => {
                    log::debug!("  Conflicting record without shared parent, using newer");
                    plan.plan_two_way_merge(&local.address, (upstream, upstream_time));
                    telem.reconciled(1);
                }
                (None, None) => {
                    if let Some(dupe) = self.find_dupe(&upstream)? {
                        log::debug!(
                            "  Incoming record {} was is a dupe of local record {}",
                            upstream.guid,
                            dupe.guid
                        );
                        plan.plan_two_way_merge(&dupe, (upstream, upstream_time));
                    } else {
                        log::debug!("  No dupe found, inserting into mirror");
                        plan.plan_mirror_insert(upstream, upstream_time, false);
                    }
                    telem.applied(1);
                }
            }
        }
        Ok(plan)
    }

    fn execute_plan(&self, plan: UpdatePlan, scope: &SqlInterruptScope) -> Result<()> {
        // Because rusqlite want a mutable reference to create a transaction
        // (as a way to save us from ourselves), we side-step that by creating
        // it manually.
        let tx = self.db.unchecked_transaction()?;
        plan.execute(&tx, scope)?;
        tx.commit()?;
        Ok(())
    }

    pub fn fetch_outgoing(
        &self,
        st: ServerTimestamp,
        scope: &SqlInterruptScope,
    ) -> Result<OutgoingChangeset> {
        // Taken from iOS. Arbitrarily large, so that clients that want to
        // process deletions first can; for us it doesn't matter.
        const TOMBSTONE_SORTINDEX: i32 = 5_000_000;
        const DEFAULT_SORTINDEX: i32 = 1;
        let mut outgoing = OutgoingChangeset::new("passwords".into(), st);
        let mut stmt = self.db.prepare_cached(&format!(
            "SELECT * FROM addressesL WHERE sync_status IS NOT {synced}",
            synced = SyncStatus::Synced as u8
        ))?;
        let rows = stmt.query_and_then(NO_PARAMS, |row| {
            scope.err_if_interrupted()?;
            Ok(if row.get::<_, bool>("is_deleted")? {
                Payload::new_tombstone(row.get::<_, String>("guid")?)
                    .with_sortindex(TOMBSTONE_SORTINDEX)
            } else {
                let address = Address::from_row(row)?;
                Payload::from_record(address)?.with_sortindex(DEFAULT_SORTINDEX)
            })
        })?;
        outgoing.changes = rows.collect::<Result<_>>()?;

        Ok(outgoing)
    }

    fn do_apply_incoming(
        &self,
        inbound: IncomingChangeset,
        telem: &mut telemetry::Engine,
        scope: &SqlInterruptScope,
    ) -> Result<OutgoingChangeset> {
        let mut incoming_telemetry = telemetry::EngineIncoming::new();
        let data = self.fetch_address_data(&inbound.changes, &mut incoming_telemetry, scope)?;
        let plan = {
            let result = self.reconcile(data, inbound.timestamp, &mut incoming_telemetry, scope);
            telem.incoming(incoming_telemetry);
            result
        }?;
        self.execute_plan(plan, scope)?;
        Ok(self.fetch_outgoing(inbound.timestamp, scope)?)
    }

    fn put_meta(&self, key: &str, value: &dyn ToSql) -> Result<()> {
        self.execute_named_cached(
            "REPLACE INTO addressesSyncMeta (key, value) VALUES (:key, :value)",
            named_params! { ":key": key, ":value": value },
        )?;
        Ok(())
    }

    fn get_meta<T: FromSql>(&self, key: &str) -> Result<Option<T>> {
        Ok(self.try_query_row(
            "SELECT value FROM addressesSyncMeta WHERE key = :key",
            named_params! { ":key": key },
            |row| Ok::<_, Error>(row.get(0)?),
            true,
        )?)
    }

    fn delete_meta(&self, key: &str) -> Result<()> {
        self.execute_named_cached(
            "DELETE FROM addressesSyncMeta WHERE key = :key",
            named_params! { ":key": key },
        )?;
        Ok(())
    }

    fn set_last_sync(&self, last_sync: ServerTimestamp) -> Result<()> {
        log::debug!("Updating last sync to {}", last_sync);
        let last_sync_millis = last_sync.as_millis() as i64;
        self.put_meta(schema::LAST_SYNC_META_KEY, &last_sync_millis)
    }

    fn get_last_sync(&self) -> Result<Option<ServerTimestamp>> {
        let millis = self.get_meta::<i64>(schema::LAST_SYNC_META_KEY)?.unwrap();
        Ok(Some(ServerTimestamp(millis)))
    }

    pub fn set_global_state(&self, state: &Option<String>) -> Result<()> {
        let to_write = match state {
            Some(ref s) => s,
            None => "",
        };
        self.put_meta(schema::GLOBAL_STATE_META_KEY, &to_write)
    }

    pub fn get_global_state(&self) -> Result<Option<String>> {
        self.get_meta::<String>(schema::GLOBAL_STATE_META_KEY)
    }

    /// A utility we can kill by the end of 2019 ;)
    pub fn migrate_global_state(&self) -> Result<()> {
        let tx = self.unchecked_transaction_imm()?;
        if let Some(old_state) = self.get_meta("global_state")? {
            log::info!("there's old global state - migrating");
            let (new_sync_ids, new_global_state) = extract_v1_state(old_state, "passwords");
            if let Some(sync_ids) = new_sync_ids {
                self.put_meta(schema::GLOBAL_SYNCID_META_KEY, &sync_ids.global)?;
                self.put_meta(schema::COLLECTION_SYNCID_META_KEY, &sync_ids.coll)?;
                log::info!("migrated the sync IDs");
            }
            if let Some(new_global_state) = new_global_state {
                self.set_global_state(&Some(new_global_state))?;
                log::info!("migrated the global state");
            }
            self.delete_meta("global_state")?;
        }
        tx.commit()?;
        Ok(())
    }
}

pub struct AddressesStore<'a> {
    pub db: &'a AddressesDb,
    pub scope: sql_support::SqlInterruptScope,
}

impl<'a> AddressesStore<'a> {
    pub fn new(db: &'a AddressesDb) -> Self {
        Self {
            db,
            scope: db.begin_interrupt_scope(),
        }
    }
}

impl<'a> Store for AddressesStore<'a> {
    fn collection_name(&self) -> &'static str {
        "passwords"
    }

    fn apply_incoming(
        &self,
        inbound: IncomingChangeset,
        telem: &mut telemetry::Engine,
    ) -> result::Result<OutgoingChangeset, failure::Error> {
        Ok(self.db.do_apply_incoming(inbound, telem, &self.scope)?)
    }

    fn sync_finished(
        &self,
        new_timestamp: ServerTimestamp,
        records_synced: Vec<Guid>,
    ) -> result::Result<(), failure::Error> {
        self.db.mark_as_synchronized(
            &records_synced.iter().map(Guid::as_str).collect::<Vec<_>>(),
            new_timestamp,
            &self.scope,
        )?;
        Ok(())
    }

    fn get_collection_request(&self) -> result::Result<CollectionRequest, failure::Error> {
        let since = self.db.get_last_sync()?.unwrap_or_default();
        Ok(CollectionRequest::new("passwords").full().newer_than(since))
    }

    fn get_sync_assoc(&self) -> result::Result<StoreSyncAssociation, failure::Error> {
        let global = self.db.get_meta(schema::GLOBAL_SYNCID_META_KEY)?;
        let coll = self.db.get_meta(schema::COLLECTION_SYNCID_META_KEY)?;
        Ok(if let (Some(global), Some(coll)) = (global, coll) {
            StoreSyncAssociation::Connected(CollSyncIds { global, coll })
        } else {
            StoreSyncAssociation::Disconnected
        })
    }

    fn reset(&self, assoc: &StoreSyncAssociation) -> result::Result<(), failure::Error> {
        self.db.reset(assoc)?;
        Ok(())
    }

    fn wipe(&self) -> result::Result<(), failure::Error> {
        self.db.wipe(&self.scope)?;
        Ok(())
    }
}

lazy_static! {
    static ref GET_ALL_SQL: String = format!(
        "SELECT {common_cols} FROM addressesL WHERE is_deleted = 0
         UNION ALL
         SELECT {common_cols} FROM addressesM WHERE is_overridden = 0",
        common_cols = schema::COMMON_COLS,
    );
    static ref GET_BY_GUID_SQL: String = format!(
        "SELECT {common_cols}
         FROM addressesL
         WHERE is_deleted = 0
           AND guid = :guid

         UNION ALL

         SELECT {common_cols}
         FROM addressesM
         WHERE is_overridden IS NOT 1
           AND guid = :guid
         ORDER BY hostname ASC

         LIMIT 1",
        common_cols = schema::COMMON_COLS,
    );
    static ref GET_ALL_BY_HOSTNAME_SQL: String = format!(
        "SELECT {common_cols}
         FROM addressesL
         WHERE is_deleted = 0
           AND hostname = :hostname
         UNION ALL

         SELECT {common_cols}
         FROM addressesM
         WHERE is_overridden = 0
           AND hostname = :hostname",
        common_cols = schema::COMMON_COLS,
    );
    static ref CLONE_ENTIRE_MIRROR_SQL: String = format!(
        "INSERT OR IGNORE INTO addressesL ({common_cols}, local_modified, is_deleted, sync_status)
         SELECT {common_cols}, NULL AS local_modified, 0 AS is_deleted, 0 AS sync_status
         FROM addressesM",
        common_cols = schema::COMMON_COLS,
    );
    static ref CLONE_SINGLE_MIRROR_SQL: String =
        format!("{} WHERE guid = :guid", &*CLONE_ENTIRE_MIRROR_SQL,);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_bad_record() {
        let db = AddressesDb::open_in_memory(Some("testing")).unwrap();
        let scope = db.begin_interrupt_scope();
        let mut telem = sync15::telemetry::EngineIncoming::new();
        let res = db
            .fetch_address_data(
                &[
                    // tombstone
                    (
                        sync15::Payload::new_tombstone("dummy_000001".into()),
                        sync15::ServerTimestamp(10000),
                    ),
                    // invalid
                    (
                        sync15::Payload::from_json(serde_json::json!({
                            "id": "dummy_000002",
                            "garbage": "data",
                            "etc": "not a address"
                        }))
                        .unwrap(),
                        sync15::ServerTimestamp(10000),
                    ),
                    // valid
                    (
                        sync15::Payload::from_json(serde_json::json!({
                            "id": "dummy_000003",
                            "formSubmitURL": "https://www.example.com/submit",
                            "hostname": "https://www.example.com",
                            "username": "test",
                            "password": "test",
                        }))
                        .unwrap(),
                        sync15::ServerTimestamp(10000),
                    ),
                ],
                &mut telem,
                &scope,
            )
            .unwrap();
        assert_eq!(telem.get_failed(), 1);
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].guid, "dummy_000001");
        assert_eq!(res[1].guid, "dummy_000003");
    }
}

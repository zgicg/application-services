// To run from the root directory:
//
//     cargo run -p sync-test -- --oauth-retries 5
//
// (You can safely ignore the noisy 500 for
// `https://stable.dev.lcip.org/auth/v1/account/destroy` at the end).

use std::cell::{Cell, RefCell};

use failure::Error;
use interrupt::NeverInterrupts;
use log::*;
use serde_derive::*;
use sync15::{
    telemetry, CollectionRequest, IncomingChangeset, MemoryCachedState, OutgoingChangeset, Payload,
    ServerTimestamp, Store, StoreSyncAssociation,
};
use sync_guid::Guid;

use crate::auth::TestClient;
use crate::testing::TestGroup;

// A test record. It has to derive `Serialize` and `Deserialize` (which we import
// in scope when we do `use serde_derive::*`), so that the `sync15` crate can
// serialize them to JSON, and parse them from JSON. Deriving `Debug` lets us
// print it with `{:?}` below.
#[derive(Debug, Deserialize, Serialize)]
struct TestRecord {
    // This field is required for all Sync records, but can be set to whatever
    // value we want. In the test, we just generate a random GUID.
    id: Guid,
    // And a test field for our record.
    test1: String,
}

// A test store, that doesn't hold on to any state (yet!)
struct TestStore {
    name: &'static str,
    store_sync_assoc: RefCell<StoreSyncAssociation>,
    was_reset_called: Cell<bool>,
    test_record: RefCell<Vec<TestRecord>>,
}

// Lotsa boilerplate to implement `Store`... 😅
impl Store for TestStore {
    fn collection_name(&self) -> std::borrow::Cow<'static, str> {
        // HACK: We have to use a "well-known" collection name in `meta/global`
        // (you can see the list in `DEFAULT_ENGINES`, inside
        // `components/sync15/src/state.rs`). Otherwise, `LocalCollStateMachine`
        // won't know how to set up the sync IDs. Even though `TestRecord` isn't
        // actually an address record, that's OK—they're all encrypted, so the
        // server can't check their contents.
        "addresses".into()
    }

    fn apply_incoming(
        &self,
        inbound: Vec<IncomingChangeset>,
        _telem: &mut telemetry::Engine,
    ) -> Result<OutgoingChangeset, Error> {
        let inbound = inbound.into_iter().next().unwrap();
        for (payload, _timestamp) in inbound.changes {
            let incoming_record: TestRecord = payload.into_record()?;
            // TODO: Push `incoming_record` into `self.test_record`.
            // `info!` is a macro from the `log` crate. It's like `println!`,
            // except it'll give us a green "INFO" line, and also let us filter
            // them out with the `RUST_LOG` environment variable.
            info!("Got incoming record {:?}", incoming_record);
        }

        // Let's make an outgoing record to upload...
        /*
        let outgoing_record = TestRecord {
            // Random for now, but we can use any GUID we want...
            // `"recordAAAAAA".into()` also works!
            id: Guid::random(),
            test1: self.record.clone(),
        };
        */
        let outgoing_record = self.test_record.borrow();
        let mut outgoing = OutgoingChangeset::new(self.collection_name(), inbound.timestamp);
        // TODO: Turn `outgoing_record` (which is a `&Vec<TestRecord>`)
        // into a `Vec<Payload>`, so that we can assign it to
        // `outgoing.changes`.
        outgoing
            .changes
            .push(Payload::from_record(outgoing_record)?);
        Ok(outgoing)
    }

    fn sync_finished(
        &self,
        _new_timestamp: ServerTimestamp,
        records_synced: Vec<Guid>,
    ) -> Result<(), Error> {
        // This should print something like:
        // `[... INFO sync_test::sync15] Uploaded records: [Guid("ai5xy_LtNAuN")]`
        // If we were a real store, this is where we'd mark our outgoing records
        // as uploaded. In a test, we can just assert that the records we uploaded
        info!("Uploaded records: {:?}", records_synced);
        Ok(())
    }

    fn get_collection_requests(
        &self,
        _server_timestamp: ServerTimestamp,
    ) -> Result<Vec<CollectionRequest>, Error> {
        // This is where we can add a `since` bound, so we only fetch records
        // since the last sync time...but, we aren't storing that yet, so we
        // just fetch all records that we've ever written.
        Ok(vec![CollectionRequest::new(self.collection_name()).full()])
    }

    fn get_sync_assoc(&self) -> Result<StoreSyncAssociation, Error> {
        // If we held on to the collection's sync ID (and global sync ID),
        // this is where we'd return them...but, for now, we just pretend
        // like it's a first sync.
        let our_assoc = self.store_sync_assoc.borrow();
        println!(
            "TEST {}: get_sync_assoc called with {:?}",
            self.name, *our_assoc
        );
        Ok(our_assoc.clone())
    }

    /// Reset the store without wiping local data, ready for a "first sync".
    /// `assoc` defines how this store is to be associated with sync.
    fn reset(&self, assoc: &StoreSyncAssociation) -> Result<(), Error> {
        // If we held on to any state, this is where we'd drop it, and replace
        // it with what we were given in `assoc`. But we don't, so we do
        // nothing.
        println!("TEST {}: Reset called", self.name);
        self.was_reset_called.set(true);
        *self.store_sync_assoc.borrow_mut() = assoc.clone();
        Ok(())
    }

    fn wipe(&self) -> Result<(), Error> {
        // This is where we'd erase all data and Sync state. Since we're
        // just an in-memory store, and `sync_multiple` doesn't exercise
        // this, we do nothing.
        Ok(())
    }
}

fn sync_first_client(c0: &mut TestClient, store: &Store) {
    let (init, key, _device_id) = c0
        .data_for_sync()
        .expect("Should have data for syncing first client");

    let mut persisted_global_state = None;
    let mut mem_cached_state = MemoryCachedState::default();
    let result = sync15::sync_multiple(
        &[store],
        &mut persisted_global_state,
        &mut mem_cached_state,
        &init,
        &key,
        &NeverInterrupts,
        None,
    );
    println!("Finished syncing first client: {:?}", result);
}

fn sync_second_client(c1: &mut TestClient, store: &Store) {
    let (init, key, _device_id) = c1
        .data_for_sync()
        .expect("Should have data for syncing second client");

    let mut persisted_global_state = None;
    let mut mem_cached_state = MemoryCachedState::default();
    let result = sync15::sync_multiple(
        &[store],
        &mut persisted_global_state,
        &mut mem_cached_state,
        &init,
        &key,
        &NeverInterrupts,
        None,
    );
    println!("Finished syncing second client: {:?}", result);
}

fn test_sync_multiple(c0: &mut TestClient, c1: &mut TestClient) {
    let first_client_store = TestStore {
        name: "c0",
        store_sync_assoc: RefCell::new(StoreSyncAssociation::Disconnected),
        was_reset_called: Cell::new(false),
        test_record: RefCell::new(vec![TestRecord {
            id: Guid::random(),
            test1: "💖".into(),
        }]),
    };
    sync_first_client(c0, &first_client_store);
    assert_eq!(
        first_client_store.was_reset_called.get(),
        true,
        "Should have called first reset"
    );

    let second_client_store = TestStore {
        name: "c1",
        store_sync_assoc: first_client_store.store_sync_assoc,
        was_reset_called: Cell::new(false),
        test_record: RefCell::default(),
    };
    sync_second_client(c1, &second_client_store);
    assert_eq!(
        second_client_store.was_reset_called.get(),
        false,
        "Second client shouldn't call reset"
    );
    // TODO: Assert that we uploaded our test record.
}

// Boilerplate...
pub fn get_test_group() -> TestGroup {
    TestGroup::new("sync15", vec![("test_sync_multiple", test_sync_multiple)])
}

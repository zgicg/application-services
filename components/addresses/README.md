# Addresses Component

## Implementation Overview

Addresses component implements storage and syncing for addresses.

See the header comment in `src/schema.rs` for an overview of the schema.

## Directory structure
The relevant directories are as follows:

- `src`: The meat of the library. This contains cross-platform rust code that
  implements the actual storage and sync of address records.
- `example`: This contains example rust code for syncing, displaying, and
  editing addresses using the code in `src`.
- `ffi`: The Rust public FFI bindings. This is a (memory-unsafe, by necessity)
  API that is exposed to Kotlin and Swift. It leverages the `ffi_support` crate
  to avoid many issues and make it more safe than it otherwise would be. At the
  time of this writing, it uses JSON for marshalling data over the FFI, however
  in the future we will likely use protocol buffers.
- `android`: This contains android bindings to addresses, written in Kotlin. These
  use JNA to call into to the code in `ffi`.
- `ios`: This contains the iOS binding to addresses, written in Swift. These use
  Swift's native support for calling code written in C to call into the code in
  `ffi`.

## Features

## Business Logic

### Record storage

At any given time records can exist in 3 places, the local storage, the remote record, and the shared parent.  The shared parent refers to a record that has been synced previously and is referred to in the code as the mirror. Address records are encrypted and stored locally. For any record that does not have a shared parent the address component tracks that the record has never been synced.

Reference the [Addresses chapter of the synconomicon](https://mozilla.github.io/application-services/synconomicon/ch01.1-addresses.html) for detailed information on the record storage format.

### Sign-out behavior
When the user signs out of their Firefox Account, we reset the storage and clear the shared parent.

### Merging records
When records are added, the addresses component performs a three-way merge between the local record, the remote record and the shared parent (last update on the server).  Details on the merging algorithm are contained in the [generic sync rfc](https://github.com/mozilla/application-services/blob/1e2ba102ee1709f51d200a2dd5e96155581a81b2/docs/design/remerge/rfc.md#three-way-merge-algorithm).

### Record de-duplication

De-duplication compares the records for same the username and same url, but with different passwords. De-duplication compares the records for same the username and same url, but with different passwords.  Deduplication logic is based on age, the username and hostname.
- If the changes are more recent than the local record it performs an update.
- If the change is older than our local records, and you have changed the same field on both, the record is not updated.

## Getting started

**Prerequisites**: Firefox account authentication is necessary to obtain the keys to decrypt synced address data.  See the [android-components FxA Client readme](https://github.com/mozilla-mobile/android-components/blob/master/components/service/firefox-accounts/README.md) for details on how to implement on Android.  For iOS, Firefox for iOS still implement the legacy oauth.

**Platform-specific details**:
- Android: add support for the addresses component via android-components [Firefox Sync - Addresses](https://github.com/mozilla-mobile/android-components/blob/master/components/service/sync-addresses/README.md) service.
- iOS: start with the [guide to consuming rust components on iOS](https://github.com/mozilla/application-services/blob/master/docs/howtos/consuming-rust-components-on-ios.md)

## API Documentation
- TODO [Expand and update API docs](https://github.com/mozilla/application-services/issues/1747)

## Testing

![status-img](https://img.shields.io/static/v1?label=test%20status&message=acceptable&color=darkgreen)

Our goal is to seek an _acceptable_ level of test coverage. When making changes in an area, make an effort to improve (or minimally not reduce) coverage. Test coverage assessment includes:
* [rust tests](https://github.com/mozilla/application-services/blob/master/testing/sync-test/src/addresses.rs)
* [android tests](https://github.com/mozilla/application-services/tree/master/components/addresses/android/src/test/java/mozilla/appservices/addresses)
* [ios tests](https://github.com/mozilla/application-services/blob/master/megazords/ios/MozillaAppServicesTests/AddressesTests.swift)
* TODO [measure and report test coverage of addresses component](https://github.com/mozilla/application-services/issues/1745)

## Telemetry
- TODO [implement addresses sync ping telemety via glean](https://github.com/mozilla/application-services/issues/1867)
- TODO [Define instrument and measure success metrics](https://github.com/mozilla/application-services/issues/1749)
- TODO [Define instrument and measure quality metrics](https://github.com/mozilla/application-services/issues/1748)

## Examples
- [Android integration](https://github.com/mozilla-mobile/android-components/blob/master/components/service/sync-addresses/README.md)

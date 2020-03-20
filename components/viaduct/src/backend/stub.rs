/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

// This is a stub backend for `viaduct`, designed to help test code that hits the network.
// To use it, calling code should create a struct implementing the `StubBackend` trait:
//
//      ```
//      use viaduct::{Request, Response, Error};
//      use viaduct::stub::StubBackend;
//
//      impl StubBackend for MyStub {
//          pub fn send(req: Request) -> Result<Response, Error> {
//              Ok(Response::from_string("Everything is awesome!"))
//          }
//      }
//      ```
//
// Then install the stub for a specific host, like so:
//
//      ```
//      viaduct::stub::install_stub_for_host("example.com", MyStub::new())
//      ```
//
// The stub will be enabled until the returned value goes out of scope, RAII-style.
//
// There can only be one stub installed per host at any one time. If you try to install
// a stub for a host that is already stubbed, the call will block until the existing
// stub is dropped. This is designed to help with writing tests that may run in parallel
// (like rust tests do by default) but could cause deadlocks if used carelessly. It might
// turn out to be a bad idea. So, ugh, be careful I guess...

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Condvar, RwLock};
use lazy_static::lazy_static;

use crate::{Error, Request, Response};

// The Trait that consumers need to implement in order to provide a stub.
// It's pretty simple because we only have one method!
//
// We might consider some helpers here once we get a bit of experience using this approach.

pub trait StubBackend {
    fn send(&self, request: Request) -> Result<Response, Error>;
}

// Turns out mutable global state is a PITA for testing; who knew?!?
//
// Anyway, this is essentially a global mutable map from hostnames to `StubBackend` impls,
// with some conveniences for waiting until a hostname is free before installing the stub.
// There is a top-level RwLock protecting the hashmap, and a (Mutex, Condvar) pair for each
// stubbed hostname which allows us to synchronize and queue updates to that stub.
//
// The ability to concurrently use stubs for different hostnames *might* allow a bit more
// concurrency between independent tests, but TBH it's mostly about the intended mental
// model of stubbing out a specific host rather than arbitrary requests.

type BoxedStubBackend = Box<dyn StubBackend + Send>;
type StubbedHostMap = HashMap<String, Arc<StubbedHost>>;

lazy_static! {
    static ref STUBBED_HOSTS: RwLock<StubbedHostMap> = RwLock::new(HashMap::new());
}

// Install a stub for the given hostname, blocking if one is already installed.

pub fn install_stub_for_host(host: &str, stub: BoxedStubBackend) -> StubbedHostGuard {
    let host = host.to_string();
    let mut stubbed_hosts = STUBBED_HOSTS.write().unwrap();
    let stubbed_host = stubbed_hosts.entry(host.clone()).or_default().clone();
    // We might block waiting to install, so release the lock.
    stubbed_host.install(stub);
    StubbedHostGuard::new(host)
}

// A struct for serializing the ability to install a stub.
// The `pending` field on this struct lets us manage a little queue of other threads waiting
// for their turn to install a stub into this hostname slot.

struct StubbedHost {
    active: Mutex<Option<BoxedStubBackend>>,
    pending: Condvar,
}

impl Default for StubbedHost {
    fn default() -> Self {
        StubbedHost { active: Mutex::new(None), pending: Condvar::new() }
    }
}

impl StubbedHost {
    // Install a stub for this host.
    // If there is already a stub installed, this will wait until it is removed
    fn install(&self, stub: BoxedStubBackend) {
        let mut active = self.active.lock().unwrap();
        while active.is_some() {
            active = self.pending.wait(active).unwrap();
        }
        active.replace(stub);
    }

    // Remove the current stub for this host.
    // This gets called automatically when the corresponding `StubbedHostGuard` is dropped.
    fn remove(&self) -> BoxedStubBackend {
        let mut active = self.active.lock().unwrap();
        self.pending.notify_one();
        active.take().expect("If we're calling remove, there must be an active stub")
    }

    // Dispatch a request via the currently active stub.
    // Returns an error if no stub is active.
    fn send(&self, req: Request) -> Result<Response, Error> {
        let active = self.active.lock().unwrap();
        match &(*active) {
            None => Err(Error::BackendError("No stub installed".to_string())),
            Some(stub) => stub.send(req),
        }
    }
}

// A RAII-style guard struct for automatically removing an installed stub.

pub struct StubbedHostGuard {
    host: String,
}

impl StubbedHostGuard {
    fn new(host: String) -> Self {
        StubbedHostGuard { host: host }
    }

    pub fn remove(&self) -> BoxedStubBackend {
        let stubbed_hosts = STUBBED_HOSTS.read().unwrap();
        let stubbed_host = stubbed_hosts.get(&self.host).expect("If we have a guard, the entry must already exist");
        stubbed_host.remove()
    }
}

impl Drop for StubbedHostGuard {
    fn drop(&mut self) {
        self.remove();
    }
}

// Dispatch a request to the appropriate stub based on the requested host.
// Errors if no stub is installed for the requested host.

pub fn send(request: Request) -> Result<Response, Error> {
    super::note_backend("stub (for testing only)");
    let host = match request.url.host_str() {
        Some(h) => h,
        _ => return Err(Error::BackendError("Cannot stub that request".to_string())),
    };
    let stubbed_hosts = STUBBED_HOSTS.read().unwrap();
    match stubbed_hosts.get(host) {
        None => Err(Error::BackendError("No stub installed".to_string())),
        Some(stubbed_host) => stubbed_host.send(request),
    }
}

#[cfg(test)]
mod tests {

    use url::Url;
    use super::{Request, Response, Error, StubBackend, install_stub_for_host};
    use crate::{Method, Headers};

    #[test]
    fn test_basic_stubbing() {

        struct MyStub {};
        impl StubBackend for MyStub {
            fn send(&self, request: Request) -> Result<Response, Error> {
                assert_eq!(request.url.as_str(), "https://example.comm/foo");
                Ok(Response {
                    url: request.url.clone(),
                    request_method: request.method,
                    body: vec![],
                    status: 200,
                    headers: Headers::new(),
                })
            }
        }

        // XXX TODO: would be nice to have the `Box` be an implementation detail, but lifetimes confuse and frighten me.
        install_stub_for_host("example.com", Box::new(MyStub{}));
        let response = Request::get(Url::parse("https://example.com/foo").unwrap()).send().unwrap();
        assert_eq!(response.request_method, Method::Get);
        assert_eq!(response.text(), "hello world");
    }
}
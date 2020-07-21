/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use cli_support::prompt::prompt_string;
use fxa_client::{device, Config, FirefoxAccount, IncomingDeviceCommand};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    sync::{Arc, Mutex},
    thread, time,
};
use url::Url;

static CREDENTIALS_PATH: &str = "credentials.json";
static CONTENT_SERVER: &str = "https://accounts.firefox.com";
static CLIENT_ID: &str = "a2270f727f45f648";
static REDIRECT_URI: &str = "https://accounts.firefox.com/oauth/success/a2270f727f45f648";
static SCOPES: &[&str] = &["profile", "https://identity.mozilla.com/apps/oldsync"];
static DEFAULT_DEVICE_NAME: &str = "Bobo device";

use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};

fn load_fxa_creds() -> Result<FirefoxAccount> {
    let mut file = fs::File::open(CREDENTIALS_PATH)?;
    let mut s = String::new();
    file.read_to_string(&mut s)?;
    Ok(FirefoxAccount::from_json(&s)?)
}

fn load_or_create_fxa_creds(cfg: Config) -> Result<FirefoxAccount> {
    let acct = load_fxa_creds().or_else(|_e| create_fxa_creds(cfg))?;
    persist_fxa_state(&acct);
    Ok(acct)
}

fn persist_fxa_state(acct: &FirefoxAccount) {
    let json = acct.to_json().unwrap();
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(true)
        .create(true)
        .open(CREDENTIALS_PATH)
        .unwrap();
    write!(file, "{}", json).unwrap();
    file.flush().unwrap();
}

fn create_fxa_creds(cfg: Config) -> Result<FirefoxAccount> {
    let mut acct = FirefoxAccount::with_config(cfg);
    let oauth_uri = acct.begin_oauth_flow(&SCOPES, "device_api_example", None)?;

    if webbrowser::open(&oauth_uri.as_ref()).is_err() {
        println!("Please visit this URL, sign in, and then copy-paste the final URL below.");
        println!("\n    {}\n", oauth_uri);
    } else {
        println!("Please paste the final URL below:\n");
    }

    let redirect_uri: String = prompt_string("Final URL").unwrap();
    let redirect_uri = Url::parse(&redirect_uri).unwrap();
    let query_params: HashMap<_, _> = redirect_uri.query_pairs().into_owned().collect();
    let code = &query_params["code"];
    let state = &query_params["state"];
    acct.complete_oauth_flow(&code, &state).unwrap();
    persist_fxa_state(&acct);
    Ok(acct)
}

fn main() -> Result<()> {
    viaduct_reqwest::use_reqwest_backend();
    let cfg = Config::new(CONTENT_SERVER, CLIENT_ID, REDIRECT_URI);
    let mut acct = load_or_create_fxa_creds(cfg)?;

    // Make sure the device and the send-tab command are registered.
    acct.initialize_device(
        DEFAULT_DEVICE_NAME,
        device::Type::Desktop,
        &[device::Capability::SendTab],
    )
    .unwrap();
    persist_fxa_state(&acct);

    let acct: Arc<Mutex<FirefoxAccount>> = Arc::new(Mutex::new(acct));
    {
        let acct = acct.clone();
        thread::spawn(move || {
            loop {
                let evts = acct
                    .lock()
                    .unwrap()
                    .poll_device_commands()
                    .unwrap_or_else(|_| vec![]); // Ignore 404 errors for now.
                persist_fxa_state(&acct.lock().unwrap());
                for e in evts {
                    match e {
                        IncomingDeviceCommand::TabReceived { sender, payload } => {
                            let tab = &payload.entries[0];
                            match sender {
                                Some(ref d) => {
                                    println!("Tab received from {}: {}", d.display_name, tab.url)
                                }
                                None => println!("Tab received: {}", tab.url),
                            };
                            webbrowser::open(&tab.url).unwrap();
                        }
                    }
                }
                thread::sleep(time::Duration::from_secs(1));
            }
        });
    }

    // Menu:
    loop {
        let now = SystemTime::now();
        let timestamp = now
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        let devices = acct.lock().unwrap().get_devices(false).unwrap();
        let devices_names: Vec<String> = devices.iter().map(|i| i.display_name.clone()).collect();
        println!("{:?} - Got devices: {:?}", timestamp, devices_names);
        thread::sleep(time::Duration::from_millis(500));
    }
}

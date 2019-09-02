const rust = import('./pkg/fxaclient_ffi');
rust.then(async m => {
   const h = m.fxa_new("https://accounts.firefox.com", "0000000000000000", "https://example.com/redirect");
   console.log("HANDLE", h);
   const url = await m.fxa_begin_oauth_flow(h, "test scopes");
   document.body.textContent = "OAuth URL is:" + url;
})
.catch(console.error)

interface KeyInfo {
    readonly attribute String auth;
    readonly attribute String p256dh;
};

interface SubscriptionInfo {
    readonly attribute String endpoint;
    readonly attribute KeyInfo keys;
};

interface SubscriptionResponse {
    readonly attribute String channelID;
    readonly attribute SubscriptionInfo subscriptionInfo;
};

interface DispatchInfo {
    readonly attribute String uaid;
    readonly attribute String scope;
};

interface PushAPI {

    SubscriptionResponse subscribe(optional String channelID = "", optional String scope = "");

    boolean unsubscribe(String channelID);

    boolean unsubscribeAll();

    boolean update(String registrationToken);

    boolean verifyConnection();

    Bytes decrypt(
        String channelID,
        String body,  // XXX TODO is this really a string, not bytes?
        optional String encoding = "aes128gcm",
        optional String salt = "",
        optional String dh = ""
    );

    DispatchInfo dispatchInfoForChid(String channelID);
};
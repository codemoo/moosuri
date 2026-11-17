use super::*;
#[test]
fn tailnet_route_preserves_other_ports_and_refuses_public_or_shared_origin() {
    let origin = "https://assistant.synthetic.ts.net";
    let base = json!({"TCP":{"8443":{"HTTPS":true}},"Web":{"other.synthetic.ts.net:8443":{"Handlers":{"/":{"Text":"other"}}}},"AllowFunnel":{"other.synthetic.ts.net:8443":true},"Services":{"synthetic":"untouched"}});
    let added = config::changed(&base, origin, 3210, true).unwrap();
    assert_eq!(
        config::route(&added, origin, 3210).unwrap(),
        Route::Matching
    );
    assert_eq!(config::changed(&added, origin, 3210, false).unwrap(), base);
    let mut unsafe_config = added.clone();
    unsafe_config["AllowFunnel"]["assistant.synthetic.ts.net:443"] = json!(true);
    assert!(config::changed(&unsafe_config, origin, 3210, false).is_err());
    let mut shared = added.clone();
    shared["Web"]["assistant.synthetic.ts.net:443"]["Handlers"]["/other"] =
        json!({"Text":"preserve"});
    assert!(config::changed(&shared, origin, 3210, false).is_err());
    let mut foreground = json!({"Foreground":{"another-session":{"TCP":{"443":{"HTTPS":true}}}}});
    assert!(config::changed(&foreground, origin, 3210, true).is_err());
    foreground["Foreground"]["another-session"]["TCP"] = json!({"8443":{"HTTPS":true}});
    assert!(config::changed(&foreground, origin, 3210, true).is_ok());
}
#[test]
fn detected_origin_requires_the_current_nodes_certificate_domain() {
    let mut node = json!({"BackendState":"Running","Self":{"DNSName":"assistant.synthetic.ts.net."},"CertDomains":["assistant.synthetic.ts.net"]});
    assert_eq!(
        config::node_origin(&node).unwrap(),
        "https://assistant.synthetic.ts.net"
    );
    node["CertDomains"] = json!(["other.synthetic.ts.net"]);
    assert!(config::node_origin(&node).is_err());
    for value in [
        "https://example.com",
        "https://evil.ts.net/path",
        "https://a.ts.net:8443",
        "https://u:p@a.ts.net",
        "http://a.ts.net",
    ] {
        assert!(config::host(value).is_err());
    }
}

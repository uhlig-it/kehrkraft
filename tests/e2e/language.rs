//! Internationalization: browser-language defaulting, the cookie that wins
//! from then on, and the /language/{de|en} switch.

use crate::harness::{self, Harness, ADMIN_PASS, ADMIN_USER};
use reqwest::header::SET_COOKIE;
use reqwest::StatusCode;

fn basic_auth(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    req.basic_auth(ADMIN_USER, Some(ADMIN_PASS))
}

/// An authenticated client with a cookie jar and no redirect following.
fn language_client() -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build client")
}

#[tokio::test]
async fn browser_preference_selects_the_language() {
    let h = harness::start().await;
    let client = reqwest::Client::new();

    // No preference at all → German (the app's historical default).
    let de = basic_auth(client.get(format!("{}/admin", h.base_url)))
        .send()
        .await
        .expect("fetch /admin (no preference)");
    let de_body = de.text().await.expect("admin body");
    assert!(
        de_body.contains("Gebäude"),
        "German default expected, got {de_body:?}"
    );
    assert!(
        de_body.contains(r#"<html lang="de">"#),
        "lang attribute, got {de_body:?}"
    );

    // An English browser gets the English UI.
    let en = basic_auth(client.get(format!("{}/admin", h.base_url)))
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9,de;q=0.8")
        .send()
        .await
        .expect("fetch /admin (en)");
    let en_body = en.text().await.expect("admin body");
    assert!(
        en_body.contains("Buildings"),
        "English UI expected, got {en_body:?}"
    );
    assert!(
        en_body.contains(r#"<html lang="en">"#),
        "lang attribute, got {en_body:?}"
    );

    // A German browser stays German.
    let de2 = basic_auth(client.get(format!("{}/admin", h.base_url)))
        .header(reqwest::header::ACCEPT_LANGUAGE, "de-DE,de;q=0.9")
        .send()
        .await
        .expect("fetch /admin (de)");
    let de2_body = de2.text().await.expect("admin body");
    assert!(
        de2_body.contains("Gebäude"),
        "German UI expected, got {de2_body:?}"
    );
}

#[tokio::test]
async fn explicit_choice_sets_a_cookie_that_wins_from_then_on() {
    let h: Harness = harness::start().await;
    let client = language_client();
    let base = &h.base_url;

    // Start English (browser preference).
    let en = basic_auth(client.get(format!("{base}/admin")))
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .send()
        .await
        .expect("fetch /admin (en)");
    assert!(
        en.text().await.expect("body").contains("Buildings"),
        "English UI initially"
    );

    // Switch to German from the people page: sets the cookie and redirects
    // back to the referring page.
    let switch = basic_auth(client.get(format!("{base}/language/de")))
        .header(reqwest::header::REFERER, format!("{base}/admin/people"))
        .send()
        .await
        .expect("switch to German");
    assert_eq!(
        switch.status(),
        StatusCode::SEE_OTHER,
        "language switch redirects"
    );
    assert_eq!(
        switch
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok()),
        Some("/admin/people"),
        "redirects back to the referring page"
    );
    let cookie = switch
        .headers()
        .get(SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .expect("Set-Cookie header");
    assert!(cookie.contains("lang=de"), "cookie set, got {cookie:?}");

    // The cookie now wins even though the browser still prefers English.
    let de = basic_auth(client.get(format!("{base}/admin")))
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .send()
        .await
        .expect("fetch /admin (de cookie)");
    let de_body = de.text().await.expect("body");
    assert!(
        de_body.contains("Gebäude"),
        "cookie preference wins, got {de_body:?}"
    );

    // Switching back to English re-overrides the cookie.
    let back = basic_auth(client.get(format!("{base}/language/en")))
        .header(reqwest::header::REFERER, format!("{base}/admin"))
        .send()
        .await
        .expect("switch to English");
    assert_eq!(back.status(), StatusCode::SEE_OTHER);
    let cookie = back
        .headers()
        .get(SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .expect("Set-Cookie header");
    assert!(cookie.contains("lang=en"), "cookie updated, got {cookie:?}");

    let en2 = basic_auth(client.get(format!("{base}/admin/people")))
        .header(reqwest::header::ACCEPT_LANGUAGE, "de-DE,de;q=0.9")
        .send()
        .await
        .expect("fetch /admin/people (en cookie)");
    let en2_body = en2.text().await.expect("body");
    assert!(
        en2_body.contains("People"),
        "English wins over the browser, got {en2_body:?}"
    );
}

#[tokio::test]
async fn unknown_language_code_is_rejected() {
    let h = harness::start().await;
    let client = reqwest::Client::new();
    let resp = basic_auth(client.get(format!("{}/language/xx", h.base_url)))
        .send()
        .await
        .expect("fetch /language/xx");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

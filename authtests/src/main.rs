use reqwest::Client;
use serde_json::json;

const BASE_URL: &str = "http://localhost:8080/api/v1/auth";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!();
    println!("========================================");
    println!("AUTH INTEGRATION TEST STARTED");
    println!("========================================");
    println!();

    let client = Client::builder()
        .cookie_store(true)
        .build()?;

    let email = "testuser@example.com";
    let username = "testuser";
    let password = "password123";

    // =========================================================
    // SIGNUP
    // =========================================================

    println!("------------------------------------------------");
    println!("1. TESTING SIGNUP");
    println!("------------------------------------------------");

    let signup_payload = json!({
        "username": username,
        "email": email,
        "password": password
    });

    let signup_response = client
        .post(format!("{BASE_URL}/signup"))
        .json(&signup_payload)
        .send()
        .await?;

    let signup_status = signup_response.status();
    let signup_body = signup_response.text().await?;

    println!("STATUS  : {}", signup_status);
    println!("RESPONSE: {}", signup_body);
    println!();

    // =========================================================
    // LOGOUT
    // =========================================================

    println!("------------------------------------------------");
    println!("2. TESTING LOGOUT");
    println!("------------------------------------------------");

    let logout_response = client
        .post(format!("{BASE_URL}/logout"))
        .send()
        .await?;

    let logout_status = logout_response.status();
    let logout_body = logout_response.text().await?;

    println!("STATUS  : {}", logout_status);
    println!("RESPONSE: {}", logout_body);
    println!();

    // =========================================================
    // LOGIN
    // =========================================================

    println!("------------------------------------------------");
    println!("3. TESTING LOGIN");
    println!("------------------------------------------------");

    let login_payload = json!({
        "email": email,
        "password": password
    });

    let login_response = client
        .post(format!("{BASE_URL}/login"))
        .json(&login_payload)
        .send()
        .await?;

    let login_status = login_response.status();
    let login_body = login_response.text().await?;

    println!("STATUS  : {}", login_status);
    println!("RESPONSE: {}", login_body);
    println!();

    // =========================================================
    // CHECK AUTH (EXPECTED SUCCESS)
    // =========================================================

    println!("------------------------------------------------");
    println!("4. TESTING CHECK AUTH");
    println!("------------------------------------------------");

    let auth_response = client
        .get(format!("{BASE_URL}/check-auth"))
        .send()
        .await?;

    let auth_status = auth_response.status();
    let auth_body = auth_response.text().await?;

    println!("STATUS  : {}", auth_status);
    println!("RESPONSE: {}", auth_body);
    println!();

    // =========================================================
    // LOGOUT AGAIN
    // =========================================================

    println!("------------------------------------------------");
    println!("5. TESTING LOGOUT");
    println!("------------------------------------------------");

    let logout_response = client
        .post(format!("{BASE_URL}/logout"))
        .send()
        .await?;

    let logout_status = logout_response.status();
    let logout_body = logout_response.text().await?;

    println!("STATUS  : {}", logout_status);
    println!("RESPONSE: {}", logout_body);
    println!();

    // =========================================================
    // CHECK AUTH AGAIN (EXPECTED FAILURE)
    // =========================================================

    println!("------------------------------------------------");
    println!("6. TESTING CHECK AUTH AFTER LOGOUT");
    println!("------------------------------------------------");

    let auth_response = client
        .get(format!("{BASE_URL}/check-auth"))
        .send()
        .await?;

    let auth_status = auth_response.status();
    let auth_body = auth_response.text().await?;

    println!("STATUS  : {}", auth_status);
    println!("RESPONSE: {}", auth_body);
    println!();

    println!("========================================");
    println!("AUTH INTEGRATION TEST FINISHED");
    println!("========================================");

    Ok(())
}
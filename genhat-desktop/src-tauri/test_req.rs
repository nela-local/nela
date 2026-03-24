use reqwest::Client;

#[tokio::main]
async fn main() {
    let res = Client::builder().cookie_store(true).build();
    match res {
        Ok(_) => println!("OK"),
        Err(e) => println!("Error: {}", e),
    }
}

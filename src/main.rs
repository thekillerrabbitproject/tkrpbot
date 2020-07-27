use std::env;

use futures::StreamExt;
use telegram_bot::*;
use tokio_postgres::{Client};
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres_openssl::MakeTlsConnector;
use serde::Deserialize;
use reqwest;
use std::error::Error;

use gotham::state::State;

const HELLO_WORLD: &'static str = "Hello World!";

/// Create a `Handler` which is invoked when responding to a `Request`.
///
/// How does a function become a `Handler`?.
/// We've simply implemented the `Handler` trait, for functions that match the signature used here,
/// within Gotham itself.
pub fn say_hello(state: State) -> (State, &'static str) {
    (state, HELLO_WORLD)
}

#[derive(Deserialize, Debug)]
struct Posts {
    title: String,
    slug: String,
}

async fn get_latest(api: Api, message: Message) -> Result<(), Box<dyn Error>> {
    let request_url = env::var("TKRP_TOP_5").expect("TKRP_TOP_5 not set");
    let response = reqwest::get(&request_url).await?;
    let posts: Vec<Posts> = response.json().await?;
    
    api.send(message.chat.text("Here are 3 the latest:")).await?;

    for post in posts {
        let link = format!("{} https://killerrabb.it/{}", post.title, post.slug);
        api.send(message.chat.text(link)).await?;
    }

    Ok(())
}

async fn connect() -> Result<Client, Box<dyn Error>> {
    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL not set");

    let mut builder = SslConnector::builder(SslMethod::tls())?;
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    let server: String = format!("{}", db_url);
    let (client, connection) = tokio_postgres::connect(&server, connector).await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });
    client.batch_execute("CREATE TABLE IF NOT EXISTS chat (id SERIAL PRIMARY KEY, chat_id TEXT UNIQUE NOT NULL)").await?;
    Ok(client)
}


async fn register(api: Api, message: Message) -> Result<(), Box<dyn Error>> {
    let client = connect().await?;
    let chat_id = format!("{}", message.chat.id().clone());

    client.execute("INSERT INTO chat (chat_id) values ($1) ON CONFLICT (chat_id) DO NOTHING ", &[&chat_id]).await?;
    api.send(message.chat.text("Thank you for subscribing!")).await?;
    Ok(())
}

async fn send_to_all(api: Api, message: Message) -> Result<(), Box<dyn Error>> {
    let client = connect().await?;

    let all_chat_id = client.query("SELECT chat_id from chat", &[]).await?;

    let message_text = message.text();

    match message_text {
        None => println!("No msg"),
        Some(msg) => {
            for row in all_chat_id {
                let id: String = row.get(0);
                let id_int = id.parse::<i64>();
                let chat = ChatId::new(id_int.unwrap());
                api.spawn(chat.text(msg.clone()));
            }
        }
    }
    
    
    Ok(())
}

async fn send_message(api: Api, message: Message) -> Result<(), Box<dyn Error>> {
    let username = message.clone().from.username;
    let admin = env::var("TELEGRAM_BOT_ADMIN").expect("TELEGRAM_BOT_ADMIN not set");
    let message_text = message.clone().text();

    if message_text == Some(String::from("/start")) {
        register(api.clone(), message.clone()).await?;
    } else if message_text == Some(String::from("/latest")) {
        get_latest(api.clone(), message.clone()).await?;
    } else {
        match username {
            None => {},
            Some(u) => {
                if u == admin {
                    let chat = message.clone().chat;
                    api.send(chat.text(format!("Sending to all!")))
                    .await?;
                    send_to_all(api.clone(), message.clone()).await?;
                }
            }
        }
    }
    Ok(())
}

async fn bot_init() -> Result<(), Box<dyn Error>> {
    let token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");
    let api = Api::new(token);
    let mut stream = api.stream();

    while let Some(update) = stream.next().await {
        let update = update?;
        if let UpdateKind::Message(message) = update.kind {
            send_message(api.clone(), message).await?;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let port: i64 = env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .expect("PORT must be a number");

    loop {
        tokio::spawn(async move {
            let addr = format!("0.0.0.0:{}", port);
            gotham::start(addr.clone(), || Ok(say_hello));
            println!("Listening for requests at http://{}", addr);
        });

        bot_init().await?;
    }
}

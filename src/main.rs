use dotenv::{dotenv, Result as DotEnvRes};
use lettre::{
    transport::smtp::authentication::Credentials, AsyncSmtpTransport, AsyncTransport, Message,
    Tokio1Executor,
};
// use reqwest::Result as RwResult;
// use std::prelude::*;

use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::time;
use futures::future;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server_list_file = include_str!("data/server_list.json");
    let server_list = serde_json::from_str::<Vec<ServerInfo>>(server_list_file).unwrap();
    
    let futures: Vec<_> = server_list.into_iter().map(
        |server| {
            async move {
                let mail_info = MailInfo::new().unwrap();
                check_server(&mail_info, server).await
            }
        }
    ).collect();
    future::join_all(futures).await;
    Ok(())
}

enum ServerState {
    ServerOk,
    ServerUnresponsive,
    // Other
}

#[derive(Debug)]
struct MailInfo {
    smtp_credentials: Credentials,
    smtp_relay: String,
    // target_url: String,
    mail_list: String,
}

#[derive(Debug)]
struct Mail {
    from: String,
    to: String,
    subject: String,
    body: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ServerInfo {
    target_url: String,
    server_name: String
}

impl MailInfo {
    fn new() -> DotEnvRes<Self> {
        dotenv().ok();
        let smtp_user: String = std::env::var("SMTP_USER").expect("SMTP_USER not found!");
        let smtp_relay: String = std::env::var("SMTP_RELAY").expect("SMTP_relay not found!");
        let smtp_pass: String = std::env::var("SMTP_PASSWORD").expect("SMTP_pass not found!");
        // let target_url: String = std::env::var("TARGET_IP").expect("target ip not found!");
        let mail_list: String = std::env::var("EMAIL_LIST").expect("email list not found!");
        let smtp_credentials = Credentials::new(smtp_user, smtp_pass);

        Ok(Self {
            smtp_credentials,
            smtp_relay,
            // target_url,
            mail_list,
        })
    }
}

impl Mail {
    fn new(to: impl Into<String>) -> Self {
        dotenv().ok();
        Self {
            from: std::env::var("SMTP_USER").expect("SMTP_USER not found!"),
            subject: "Control Servidores RTO".into(),
            to: to.into(),
            body: "".into(),
        }
    }

    fn set_body(self, server_name: impl Into<String>) -> Self {
        let body = format!("Servidor {} potencialmente apagado revisar.", server_name.into());
        Self { body, ..self }
    }

    async fn send_email_smtp(
        self,
        mailer: &AsyncSmtpTransport<Tokio1Executor>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let email = Message::builder()
            .from(self.from.parse()?)
            .to(self.to.parse()?)
            .subject(self.subject)
            .body(self.body.to_string())?;

        // mailer.send(email).await?;

        match mailer.send(email).await {
            Ok(_) => println!("Email sent successfully!"),
            Err(e) => println!("Could not send email: {e:?}"),
        }

        Ok(())
    }
}

async fn ping_server(server_info: &ServerInfo) -> Result<ServerState, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client.get(&server_info.target_url)
        .timeout(time::Duration::from_secs(10))
        .send()
        .await;
    match response {
        Ok(val) => match val.status() {
            reqwest::StatusCode::OK => {
                let text = val.text().await?;
                println!("{:?}", text);
                Ok(ServerState::ServerOk)
            }
            _a @ reqwest::StatusCode::REQUEST_TIMEOUT => Ok(ServerState::ServerUnresponsive),
            resp => panic!("Some other Response {}", resp),
        },
        Err(v) => {
            println!("{:?}", v);
            Ok(ServerState::ServerUnresponsive)
        }
    }
}

async fn check_server(mail_info: &MailInfo, server_info: ServerInfo) -> Result<(), Box<dyn std::error::Error>> {
    
    println!("Checking server: {}", &server_info.server_name);
    let email_list = serde_json::from_str::<HashMap<String, Vec<String>>>(&mail_info.mail_list)
        .expect("JSON INCORRECT!");
    let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&mail_info.smtp_relay)?
        .credentials(mail_info.smtp_credentials.clone())
        .build();

    // match mailer.test_connection() {
    //     Ok(val) => println!("SMTP SERVER OK"),
    //     Err(e) => panic!("SMTP SERVER UNREACHABLE!")
    // };
    let mut interval = async_timer::Interval::platform_new(time::Duration::from_secs(2));

    let mut times = 0;
    let mut down_times = 0;
    while times < 10 {
        match ping_server(&server_info).await? {
            ServerState::ServerOk => {
                println!("Iter:{}, Server:{} - ServerOk!", times, &server_info.server_name);
            }
            ServerState::ServerUnresponsive => {
                println!("Iter:{}, Server:{} - Server Unresponsive!!", times, &server_info.server_name);
                down_times += 1
            }
        }
        interval.as_mut().await;
        times += 1;
        if down_times > 5 {
            println!("Exceeded timeouts sending emails...");
            for email_dir in email_list["emails"].iter(){
                // dbg!(&mail_info);
                Mail::new(email_dir)
                    .set_body(&server_info.server_name)
                    .send_email_smtp(&mailer)
                    .await?;
            }
            break
        }
    }
    Ok(())
}

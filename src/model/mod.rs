pub mod date_type;
pub mod form;

use anyhow::Result;
use chrono::{Local, NaiveDateTime};
use rocket_db_pools::Connection;
use serde::{Deserialize, Serialize};

use crate::{Config, Database};

pub use date_type::*;
pub use form::*;

#[derive(Serialize, Deserialize)]
pub struct Message {
    pub text_key: String,
    pub message_type: MessageType,
}

#[derive(Serialize, Deserialize)]
pub enum MessageType {
    Success,
    Error,
    Info,
}

pub async fn validate_room<'a>(
    room: &'a str,
    messages: &mut Vec<Message>,
    db: &mut Connection<Database>,
) -> anyhow::Result<(&'a str, i32)> {
    let room_id = sqlx::query!("select id from rooms where room_number = $1", &room)
        .fetch_optional(&mut **db)
        .await?
        .map(|x| x.id);

    Ok(match room_id {
        Some(room_id) => (room, room_id),
        None => {
            messages.push(Message {
                text_key: String::from("validation-room"),
                message_type: MessageType::Error,
            });
            ("", -1)
        }
    })
}

pub async fn get_announcement(
    position: &str,
    lang: &str,
    db: &mut Connection<Database>,
) -> anyhow::Result<String> {
    let content = sqlx::query_scalar!(
        r#"select content from announcements
        where position = ($1::text)::announcement_position and lang = ($2::text)::language"#,
        &position,
        &lang,
    )
    .fetch_one(&mut **db)
    .await?;
    Ok(content)
}

#[derive(Serialize, Deserialize, sqlx::FromRow)]
pub struct Room {
    pub id: i32,
    pub room_number: String,
}

pub async fn check_date_type_access(
    date_type: &str,
    token: Option<&str>,
    config: &Config,
    db: &mut Connection<Database>,
) -> Result<bool> {
    let deadline = match config.application_deadlines.get(date_type) {
        Some(deadline) => NaiveDateTime::parse_from_str(deadline, crate::BROWSER_DATETIME_FORMAT)
            .unwrap()
            .and_local_timezone(Local)
            .earliest()
            .unwrap(),
        None => return Ok(true),
    };

    Ok(match token {
        Some(token) => {
            let entered = sqlx::query_scalar!(
                r#"select entered from waiting_list where token = $1"#,
                &token
            )
            .fetch_optional(&mut **db)
            .await?;

            match entered {
                Some(entered) => entered < deadline,
                None => false,
            }
        }

        None => false,
    })
}

pub async fn get_waiting_list_email(
    token: Option<&str>,
    db: &mut Connection<Database>,
) -> Result<Option<String>> {
    Ok(match token {
        Some(token) => {
            sqlx::query_scalar!(r#"select email from waiting_list where token = $1"#, &token)
                .fetch_optional(&mut **db)
                .await?
        }
        None => None,
    })
}

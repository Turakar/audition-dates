use std::error::Error;

use anyhow::anyhow;
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use lazy_regex::regex_is_match;
use rocket::form::{self, error::ErrorKind, FromFormField, ValueField};
use rocket::request::FromParam;
use rocket_db_pools::Connection;
use serde::{Deserialize, Serialize};
use sqlx::{
    database::{HasArguments, HasValueRef},
    encode::IsNull,
    Decode, Encode,
};

use crate::Database;

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

pub trait IntoInner<I> {
    fn into_inner(self) -> I;
}

pub fn handle_form_error<I: Default, T: IntoInner<I>>(
    field: form::Result<T>,
    messages: &mut Vec<Message>,
) -> I {
    match field {
        Ok(value) => value.into_inner(),
        Err(errors) => {
            messages.extend(errors.into_iter().map(|error| match error.kind {
                ErrorKind::Validation(msg) => Message {
                    text_key: String::from(msg),
                    message_type: MessageType::Error,
                },
                _ => Message {
                    text_key: String::from("validation-unknown"),
                    message_type: MessageType::Error,
                },
            }));
            I::default()
        }
    }
}

pub struct DisplayName<'r>(&'r str);

impl<'r> IntoInner<&'r str> for DisplayName<'r> {
    fn into_inner(self) -> &'r str {
        self.0
    }
}

impl<'r> FromFormField<'r> for DisplayName<'r> {
    fn from_value(field: ValueField<'r>) -> form::Result<'r, Self> {
        let value = field.value;
        if regex_is_match!("[a-zA-Z0-9äöüß ]{2,}", value) {
            Ok(DisplayName(value))
        } else {
            Err(form::Error::validation("validation-display-name").into())
        }
    }
}

pub struct Email<'r>(pub &'r str);

impl<'r> IntoInner<&'r str> for Email<'r> {
    fn into_inner(self) -> &'r str {
        self.0
    }
}

impl<'r> FromFormField<'r> for Email<'r> {
    fn from_value(field: ValueField<'r>) -> form::Result<'r, Self> {
        let value = field.value;
        if regex_is_match!(
            r#"(?:[a-z0-9!#$%&'*+/=?^_`{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_`{|}~-]+)*|"(?:[\x01-\x08\x0b\x0c\x0e-\x1f\x21\x23-\x5b\x5d-\x7f]|\\[\x01-\x09\x0b\x0c\x0e-\x7f])*")@(?:(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?|\[(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?|[a-z0-9-]*[a-z0-9]:(?:[\x01-\x08\x0b\x0c\x0e-\x1f\x21-\x5a\x53-\x7f]|\\[\x01-\x09\x0b\x0c\x0e-\x7f])+)\])"#,
            value
        ) {
            Ok(Email(value))
        } else {
            Err(form::Error::validation("validation-email").into())
        }
    }
}

pub struct Password<'r>(&'r str);

impl<'r> IntoInner<&'r str> for Password<'r> {
    fn into_inner(self) -> &'r str {
        self.0
    }
}

impl<'r> FromFormField<'r> for Password<'r> {
    fn from_value(field: ValueField<'r>) -> form::Result<'r, Self> {
        let value = field.value;
        if regex_is_match!(r#"[A-Za-z0-9!@#$%^&*()\-_ +.äöüß]{8,}"#, value)
            && regex_is_match!(r#".*[A-Z].*"#, value)
            && regex_is_match!(r#".*[a-z].*"#, value)
            && regex_is_match!(r#".*[0-9].*"#, value)
            && regex_is_match!(r#".*[!@#$%^&*()\-_ +.].*"#, value)
        {
            Ok(Password(value))
        } else {
            Err(form::Error::validation("validation-password").into())
        }
    }
}

pub struct FormDateTime(DateTime<Local>);

impl IntoInner<DateTime<Local>> for FormDateTime {
    fn into_inner(self) -> DateTime<Local> {
        self.0
    }
}

impl<'r> FromFormField<'r> for FormDateTime {
    fn from_value(field: ValueField<'r>) -> form::Result<'r, Self> {
        let value = field.value;
        let naive = NaiveDateTime::parse_from_str(value, crate::BROWSER_DATETIME_FORMAT)
            .map_err(|_| form::Error::validation("validation-date"))?;
        let local = Local
            .from_local_datetime(&naive)
            .single()
            .ok_or_else(|| form::Error::validation("validation-date-ambiguous"))?;
        Ok(FormDateTime(local))
    }
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

#[derive(Serialize)]
pub struct InteropEnumTera {
    display_name: &'static str,
    value: &'static str,
}

macro_rules! interop_enum {
    ($name:ident : $($item_name:ident ($value:literal, $display_name:literal)),+) => {
        #[derive(Serialize, Deserialize, Clone, Copy)]
        pub enum $name {
            $($item_name),+
        }

        impl $name {
            pub fn tera(&self) -> InteropEnumTera {
                use $name::*;
                match self {
                    $($item_name => InteropEnumTera { display_name: $display_name, value: $value }),+
                }
            }

            pub fn get_value(&self) -> &'static str {
                use $name::*;
                match self {
                    $($item_name => $value),+
                }
            }
        }

        impl<'r> FromFormField<'r> for $name {
            fn from_value(field: ValueField<'r>) -> form::Result<'r, Self> {
                use $name::*;
                let value = field.value;
                match value {
                    $($value => Ok($item_name)),+,
                    _ => Err(form::Error::validation("validation-select").into())
                }
            }
        }

        impl<'r> FromParam<'r> for $name {
            type Error = &'r str;
            fn from_param(param: &'r str) -> Result<Self, Self::Error> {
                use $name::*;
                match param {
                    $($value => Ok($item_name)),+,
                    _ => Err("Unknown date type!")
                }
            }
        }

        impl <'r, DB: sqlx::Database> Decode<'r, DB> for $name where &'r str: Decode<'r, DB> {
            fn decode(value: <DB as HasValueRef<'r>>::ValueRef) -> Result<Self, Box<dyn Error + 'static + Send + Sync>> {
                use $name::*;
                let value = <&str as Decode<DB>>::decode(value)?;
                match value {
                    $($value => Ok($item_name)),+,
                    _ => Err(Box::from(anyhow!("Invalid date type from database!")))
                }
            }
        }

        impl <'q, DB: sqlx::Database> Encode<'q, DB> for $name where &'q str: Encode<'q, DB> {
            fn encode_by_ref(&self, buf: &mut <DB as HasArguments<'q>>::ArgumentBuffer) -> IsNull {
                use $name::*;
                let s = match self {
                    $($item_name => $value),+
                };
                <&str as Encode<DB>>::encode_by_ref(&s, buf)
            }
        }
    }
}

interop_enum!(DateType:
    Choir("choir", "date-type-choir"),
    Orchestra("orchestra", "date-type-orchestra"),
    ChamberChoir("chamber-choir", "date-type-chamber-choir")
);
interop_enum!(Voice:
    // choir voices
    Female("female", "voice-female"),
    Male("male", "voice-male"),
    // orchestra voices
    Violin("violin", "voice-violin"),
    Trumpet("trumpet", "voice-trumpet"),
    Horn("horn", "voice-horn"),
    // chamber choir voices
    Soprano("soprano", "voice-soprano"),
    Alto("alto", "voice-alto"),
    Tenor("tenor", "voice-tenor"),
    Bass("bass", "voice-bass")
);

impl DateType {
    pub fn get_variants() -> &'static [DateType] {
        &[
            DateType::Choir,
            // DateType::Orchestra,
            DateType::ChamberChoir,
        ]
    }

    pub fn get_variants_tera() -> Vec<InteropEnumTera> {
        Self::get_variants().iter().map(|v| v.tera()).collect()
    }

    pub fn get_voices(&self) -> &'static [Voice] {
        match self {
            DateType::Choir => &[Voice::Female, Voice::Male],
            DateType::Orchestra => &[Voice::Violin, Voice::Trumpet, Voice::Horn],
            DateType::ChamberChoir => &[Voice::Soprano, Voice::Alto, Voice::Tenor, Voice::Bass],
        }
    }

    pub fn get_voices_tera(&self) -> Vec<InteropEnumTera> {
        Self::get_voices(self).iter().map(|v| v.tera()).collect()
    }
}

#[derive(Serialize, Deserialize, sqlx::FromRow)]
pub struct Room {
    pub id: i32,
    pub room_number: String,
}

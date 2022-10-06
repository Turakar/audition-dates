use serde::Serialize;
use serde::Deserialize;
use rocket::form;
use rocket::form::FromFormField;
use rocket::form::ValueField;
use rocket::request::FromParam;
use sqlx::Decode;
use sqlx::Encode;
use sqlx::database::HasValueRef;
use sqlx::database::HasArguments;
use sqlx::encode::IsNull;
use std::error::Error;
use anyhow::anyhow;


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
            // DateType::ChamberChoir,
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

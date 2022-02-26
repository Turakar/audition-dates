use lazy_regex::regex_is_match;
use rocket::form::{self, error::ErrorKind, FromFormField, ValueField};
use serde::Serialize;

#[derive(Serialize)]
pub struct Message {
    pub text_key: String,
    pub message_type: MessageType,
}

#[derive(Serialize)]
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
        if regex_is_match!("[a-zA-Z0-9äöüß ]{8,}", value) {
            Ok(DisplayName(value))
        } else {
            Err(form::Error::validation("validation-username").into())
        }
    }
}

pub struct Email<'r>(&'r str);

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

use anyhow::anyhow;
use argon2::password_hash::SaltString;
use argon2::Argon2;
use argon2::PasswordHash;
use argon2::PasswordHasher;
use argon2::PasswordVerifier;
use chrono::DateTime;
use chrono::{Duration, Utc};
use lettre::AsyncTransport;
use lettre::Message as EmailMessage;
use rand_core::OsRng;
use rocket::form::Form;
use rocket::form::Result as FormResult;
use rocket::http::Cookie;
use rocket::http::CookieJar;
use rocket::http::Status;
use rocket::request;
use rocket::request::FromRequest;
use rocket::response::Redirect;
use rocket::Request;
use rocket::State;
use rocket_db_pools::sqlx;
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};
use serde::Deserialize;
use serde::Serialize;

use crate::Config;
use crate::Database;
use crate::Mailer;
use crate::RocketResult;
use crate::MAIL_TEMPLATES;

use crate::util::handle_form_error;
use crate::util::DisplayName;
use crate::util::Email;
use crate::util::Password;
use crate::{
    language::{Language, LOCALES},
    util::{Message, MessageType},
};

#[derive(FromForm)]
pub struct InviteForm<'r> {
    email: FormResult<'r, Email<'r>>,
}

#[get("/admin/invite")]
pub async fn invite_get(
    lang: Language,
    db: Connection<Database>,
    admin: Option<Admin>,
) -> RocketResult<Result<Template, Status>> {
    Ok(match admin {
        Some(_) => Ok(Template::render(
            "invite",
            context! { lang: lang.into_string() },
        )),
        None if no_one_registered(db).await? => Ok(Template::render(
            "invite",
            context! {
                lang: lang.into_string(),
                messages: [Message { text_key: String::from("initial-registration-info"), message_type: MessageType::Info }],
            },
        )),
        _ => Err(Status::Unauthorized),
    })
}

async fn no_one_registered(mut db: Connection<Database>) -> anyhow::Result<bool> {
    Ok(sqlx::query!("select id from admins")
        .fetch_optional(&mut *db)
        .await?
        .is_none())
}

#[post("/admin/invite", data = "<invite>")]
pub async fn invite_post<'r>(
    lang: Language,
    mut db: Connection<Database>,
    mailer: &State<Mailer>,
    config: &State<Config>,
    invite: Form<InviteForm<'r>>,
) -> RocketResult<Template> {
    let lang = lang.into_string();
    let InviteForm { email } = invite.into_inner();
    let mut messages = Vec::new();
    let email = handle_form_error(email, &mut messages);
    if !messages.is_empty() {
        return Ok(Template::render(
            "invite",
            context! {
                lang,
                messages,
            },
        ));
    }

    if sqlx::query!("select email from admins where email = $1", &email)
        .fetch_optional(&mut *db)
        .await?
        .is_some()
    {
        return Ok(Template::render(
            "invite",
            context! {
                lang,
                messages: [Message {
                    text_key: String::from("email-already-registered"),
                    message_type: MessageType::Error
                }],
            },
        ));
    }

    let token = match sqlx::query!("select token from invites where email = $1", &email)
        .fetch_optional(&mut *db)
        .await?
    {
        Some(record) => {
            sqlx::query!(
                "update invites set created = now() where email = $1",
                &email
            )
            .execute(&mut *db)
            .await?;
            record.token
        }
        None => {
            sqlx::query!(
                "insert into invites (token, email, created) values (DEFAULT, $1, now()) returning token",
                &email,
            )
            .fetch_one(&mut *db)
            .await?.token
        }
    };

    let link = format!("{}/admin/register?token={}", &config.web_address, &token);
    let mut mail_context = tera::Context::new();
    mail_context.insert("lang", &lang);
    mail_context.insert("link", &link);
    let mail = EmailMessage::builder()
        .to(email.parse()?)
        .from(config.email_from_address.parse()?)
        .subject(
            LOCALES
                .lookup_single_language::<&str>(&lang.parse()?, "mail-invite-subject", None)
                .ok_or(anyhow!("Missing translation for mail-invite-subject!"))?,
        )
        .body(MAIL_TEMPLATES.render("invite.tera", &mail_context)?)?;
    mailer.send(mail).await?;

    Ok(Template::render(
        "invite",
        context! {
            lang,
            messages: [Message {
                text_key: String::from("invite-successful"),
                message_type: MessageType::Success
            }],
        },
    ))
}

#[derive(FromForm)]
pub struct RegisterForm<'r> {
    email: FormResult<'r, Email<'r>>,
    display_name: FormResult<'r, DisplayName<'r>>,
    password: FormResult<'r, Password<'r>>,
    token: &'r str,
}

#[get("/admin/register?<token>")]
pub async fn register_get(
    lang: Language,
    mut db: Connection<Database>,
    token: &str,
) -> RocketResult<Result<Template, Status>> {
    match sqlx::query!("select email from invites where token = $1", &token)
        .fetch_optional(&mut *db)
        .await?
    {
        None => Ok(Err(Status::Unauthorized)),
        Some(record) => Ok(Ok(Template::render(
            "register",
            context! { lang: lang.into_string(), token, email: record.email, display_name: "" },
        ))),
    }
}

#[post("/admin/register", data = "<form>")]
pub async fn register_post(
    lang: Language,
    mut db: Connection<Database>,
    form: Form<RegisterForm<'_>>,
) -> RocketResult<Result<Redirect, Template>> {
    let RegisterForm {
        email,
        display_name,
        password,
        token,
    } = form.into_inner();
    let mut messages = Vec::new();
    let email = handle_form_error(email, &mut messages);
    let display_name = handle_form_error(display_name, &mut messages);
    let password = handle_form_error(password, &mut messages);
    if !messages.is_empty() {
        return Ok(Err(Template::render(
            "register",
            context! {
                lang: lang.into_string(),
                token,
                email,
                display_name,
                messages,
            },
        )));
    }

    let rows_affected = sqlx::query!("delete from invites where token = $1", &token)
        .execute(&mut *db)
        .await?
        .rows_affected();
    if rows_affected == 0 {
        return Ok(Err(Template::render(
            "register",
            context! {
                lang: lang.into_string(),
                token,
                email,
                display_name,
                messages: [Message { text_key: String::from("invite-invalid-token"), message_type: MessageType::Error }],
            },
        )));
    }

    sqlx::query!(
        "insert into admins (display_name, email, password) values ($1, $2, $3)",
        &display_name,
        &email,
        hash_password(&password)?
    )
    .execute(&mut *db)
    .await?;

    Ok(Ok(Redirect::to(uri!(login_get))))
}

fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| anyhow!("Could not hash password!"))?;
    Ok(password_hash.to_string())
}

fn verify_password(password: &str, hash: &str) -> anyhow::Result<bool> {
    let parsed_hash =
        PasswordHash::new(hash).map_err(|_| anyhow!("Could not parse password hash!"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

#[derive(FromForm)]
pub struct LoginForm<'r> {
    email: FormResult<'r, Email<'r>>,
    password: FormResult<'r, Password<'r>>,
    remember: bool,
}

#[derive(Serialize, Deserialize)]
struct LoginCookie {
    id: String,
    valid_until: DateTime<Utc>,
}

#[get("/admin/login")]
pub async fn login_get(lang: Language) -> Template {
    Template::render("login", context! { lang: lang.into_string(), email: "" })
}

#[post("/admin/login", data = "<form>")]
pub async fn login_post<'r>(
    lang: Language,
    mut db: Connection<Database>,
    cookies: &CookieJar<'_>,
    form: Form<LoginForm<'r>>,
) -> RocketResult<Result<Redirect, Template>> {
    let LoginForm {
        email,
        password,
        remember,
    } = form.into_inner();
    let mut messages = Vec::new();
    let email = handle_form_error(email, &mut messages);
    let password = handle_form_error(password, &mut messages);
    if !messages.is_empty() {
        return Ok(Err(Template::render(
            "login",
            context! { lang: lang.into_string(), email },
        )));
    }

    match sqlx::query!("select id, password from admins where email = $1", &email)
        .fetch_optional(&mut *db)
        .await?
    {
        Some(record) if verify_password(password, &record.password)? => {
            let valid_until = Utc::now() + Duration::days(7);
            let cookie_value = serde_json::to_string(&LoginCookie {
                id: record.id,
                valid_until,
            })?;
            let cookie = if remember {
                Cookie::build("login", cookie_value)
                    .http_only(true)
                    .finish()
            } else {
                Cookie::build("login", cookie_value)
                    .http_only(true)
                    .expires(None)
                    .finish()
            };
            cookies.add_private(cookie);
            Ok(Ok(Redirect::to(uri!(invite_get))))
        }
        _ => Ok(Err(Template::render(
            "login",
            context! {
                lang: lang.into_string(),
                email,
                messages: [Message {
                    text_key: String::from("invalid-login"),
                    message_type: MessageType::Error,
                }],
            },
        ))),
    }
}

#[derive(FromForm)]
pub struct RequestPasswordResetForm<'r> {
    email: FormResult<'r, Email<'r>>,
}

#[get("/admin/password-reset-request")]
pub async fn password_reset_request_get(lang: Language) -> Template {
    Template::render(
        "password-reset-request",
        context! { lang: lang.into_string(), email: "" },
    )
}

#[post("/admin/password-reset-request", data = "<form>")]
pub async fn password_reset_request_post<'r>(
    lang: Language,
    mut db: Connection<Database>,
    mailer: &State<Mailer>,
    config: &State<Config>,
    form: Form<RequestPasswordResetForm<'r>>
) -> RocketResult<Template> {
    let lang = lang.into_string();
    let RequestPasswordResetForm { email } = form.into_inner();
    let mut messages = Vec::new();
    let email = handle_form_error(email, &mut messages);
    if !messages.is_empty() {
        return Ok(Template::render(
            "password-reset-request",
            context! { lang, email, messages },
        ));
    }

    let admin_id = sqlx::query!(
        "select id from admins where email = $1",
        &email,
    ).fetch_optional(&mut *db).await?.map(|record| record.id);
    println!("admin ID: {:?}", &admin_id);

    if let Some(admin_id) = admin_id {
        let token = sqlx::query!(
            "insert into password_resets (admin_id) values ($1) returning token",
            &admin_id,
        ).fetch_one(&mut *db).await?.token;

        let link = format!("{}/admin/password-reset?token={}", &config.web_address, &token);
        let mut mail_context = tera::Context::new();
        mail_context.insert("lang", &lang);
        mail_context.insert("link", &link);
        let mail = EmailMessage::builder()
            .to(email.parse()?)
            .from(config.email_from_address.parse()?)
            .subject(
                LOCALES
                    .lookup_single_language::<&str>(&lang.parse()?, "mail-password-reset-subject", None)
                    .ok_or(anyhow!("Missing translation for mail-password-reset-subject!"))?,
            )
            .body(MAIL_TEMPLATES.render("password-reset.tera", &mail_context)?)?;
        mailer.send(mail).await?;
    }

    Ok(Template::render(
        "password-reset-request",
        context! {
            lang,
            email: "",
            messages: [Message { text_key: String::from("password-reset-sent"), message_type: MessageType::Success }]
        },
    ))
}

#[derive(FromForm)]
pub struct PasswordResetForm<'r> {
    token: &'r str,
    password: FormResult<'r, Password<'r>>,
}

#[get("/admin/password-reset?<token>")]
pub async fn password_reset_get(lang: Language, token: &str) -> Template {
    Template::render(
        "password-reset",
        context! { lang: lang.into_string(), token },
    )
}

#[post("/admin/password-reset", data = "<form>")]
pub async fn password_reset_post<'r>(
    lang: Language,
    mut db: Connection<Database>,
    mailer: &State<Mailer>,
    config: &State<Config>,
    form: Form<PasswordResetForm<'r>>
) -> RocketResult<Result<Redirect, Template>> {
    let lang = lang.into_string();
    let PasswordResetForm { token, password } = form.into_inner();
    let mut messages = Vec::new();
    let password = handle_form_error(password, &mut messages);
    if !messages.is_empty() {
        return Ok(Err(Template::render(
            "password-reset",
            context! { lang, token, messages },
        )))
    }

    let id = sqlx::query!(
        "delete from password_resets where token = $1 returning admin_id",
        &token,
    ).fetch_optional(&mut *db).await?;

    match id {
        Some(record) => {
            let id = record.admin_id;
            let email = sqlx::query!(
                "update admins set password = $1 where id = $2 returning email",
                &hash_password(password)?,
                &id,
            ).fetch_one(&mut *db).await?.email;

            let mut mail_context = tera::Context::new();
            mail_context.insert("lang", &lang);
            let mail = EmailMessage::builder()
                .to(email.parse()?)
                .from(config.email_from_address.parse()?)
                .subject(
                    LOCALES
                        .lookup_single_language::<&str>(&lang.parse()?, "mail-password-was-reset-subject", None)
                        .ok_or(anyhow!("Missing translation for mail-password-was-reset-subject!"))?,
                )
                .body(MAIL_TEMPLATES.render("password-was-reset.tera", &mail_context)?)?;
            mailer.send(mail).await?;

            Ok(Ok(
                Redirect::to(uri!(login_get))
            ))
        },
        None => {
            Ok(Err(Template::render(
                "password-reset",
                context! { lang, token, messages: [Message { text_key: String::from("password-reset-invalid"), message_type: MessageType::Error }]}
            )))
        }
    }
}

pub struct Admin {
    pub id: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Admin {
    type Error = anyhow::Error;

    async fn from_request(req: &'r Request<'_>) -> request::Outcome<Self, Self::Error> {
        use rocket::outcome::Outcome::{Failure, Success};

        let cookies = req.guard::<&CookieJar<'r>>().await.unwrap();

        let login: LoginCookie = match cookies.get_private("login") {
            Some(cookie) => match serde_json::from_str(cookie.value()) {
                Ok(login) => login,
                Err(_) => return Failure((Status::Unauthorized, anyhow!("Invalid login cookie!"))),
            },
            None => return Failure((Status::Unauthorized, anyhow!("No login cookie present!"))),
        };

        if login.valid_until > Utc::now() {
            Success(Admin { id: login.id })
        } else {
            Failure((Status::Unauthorized, anyhow!("Invalid login cookie!")))
        }
    }
}

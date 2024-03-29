use anyhow::anyhow;
use anyhow::Context as AnyhowContext;
use argon2::password_hash::SaltString;
use argon2::Argon2;
use argon2::PasswordHash;
use argon2::PasswordHasher;
use argon2::PasswordVerifier;
use chrono::DateTime;
use chrono::{Duration, Utc};
use lettre::message::header;
use lettre::message::header::ContentTransferEncoding;
use lettre::message::IntoBody;
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
use tera::Context;

use crate::Config;
use crate::Database;
use crate::Mailer;
use crate::RocketResult;
use crate::MAIL_TEMPLATES;

use crate::mail::send_mail;
use crate::mail::MailBody;
use crate::model::handle_form_error;
use crate::model::DisplayName;
use crate::model::Email;
use crate::model::Password;
use crate::{
    language::{Language, LOCALES},
    model::{Message, MessageType},
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
                .ok_or_else(|| anyhow!("Missing translation for mail-invite-subject!"))?,
        )
        .header(header::ContentType::TEXT_PLAIN)
        .body(
            MAIL_TEMPLATES
                .render("invite.tera", &mail_context)?
                .into_body(Some(ContentTransferEncoding::Base64)),
        )?;
    mailer
        .send(mail)
        .await
        .context("Could not send invitation mail!")?;

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

    Ok(Ok(Redirect::to(uri!(login_get(
        redirect = Option::<&str>::None
    )))))
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

#[get("/admin/login?<redirect>")]
#[allow(unused_variables)]
pub async fn login_get(lang: Language, redirect: Option<&str>) -> Template {
    Template::render("login", context! { lang: lang.into_string(), email: "" })
}

#[post("/admin/login?<redirect>", data = "<form>")]
pub async fn login_post<'r>(
    lang: Language,
    mut db: Connection<Database>,
    cookies: &CookieJar<'_>,
    redirect: Option<&'r str>,
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
            sqlx::query!(
                "update admins set last_login = now() where id = $1",
                &record.id
            )
            .execute(&mut *db)
            .await?;
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
            let redirect = match redirect {
                None => Redirect::to(uri!(crate::admin::dashboard(day = Option::<&str>::None))),
                Some(redirect) => Redirect::to(String::from(redirect)),
            };
            Ok(Ok(redirect))
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

#[get("/admin/logout")]
pub async fn logout(cookies: &CookieJar<'_>) -> Redirect {
    cookies.remove_private(Cookie::new("login", ""));
    Redirect::to(uri!("/"))
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
    form: Form<RequestPasswordResetForm<'r>>,
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

    let admin_id = sqlx::query!("select id from admins where email = $1", &email,)
        .fetch_optional(&mut *db)
        .await?
        .map(|record| record.id);

    if let Some(admin_id) = admin_id {
        let token = sqlx::query!(
            "insert into password_resets (admin_id) values ($1) returning token",
            &admin_id,
        )
        .fetch_one(&mut *db)
        .await?
        .token;

        send_mail(
            config,
            mailer,
            email,
            &lang,
            "mail-password-reset-subject",
            None,
            MailBody::Template(
                "password-reset.tera",
                &Context::from_serialize(context! {
                    lang: &lang,
                    link: format!(
                        "{}/admin/password-reset?token={}",
                        &config.web_address, &token
                    )
                })?,
            ),
        )
        .await?;
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
    form: Form<PasswordResetForm<'r>>,
) -> RocketResult<Result<Redirect, Template>> {
    let lang = lang.into_string();
    let PasswordResetForm { token, password } = form.into_inner();
    let mut messages = Vec::new();
    let password = handle_form_error(password, &mut messages);
    if !messages.is_empty() {
        return Ok(Err(Template::render(
            "password-reset",
            context! { lang, token, messages },
        )));
    }

    let id = sqlx::query!(
        "delete from password_resets where token = $1 returning admin_id",
        &token,
    )
    .fetch_optional(&mut *db)
    .await?;

    match id {
        Some(record) => {
            let id = record.admin_id;
            let email = sqlx::query!(
                "update admins set password = $1 where id = $2 returning email",
                &hash_password(password)?,
                &id,
            )
            .fetch_one(&mut *db)
            .await?
            .email;

            send_mail(
                config,
                mailer,
                &email,
                &lang,
                "mail-password-was-reset-subject",
                None,
                MailBody::Template(
                    "password-was-reset.tera",
                    &Context::from_serialize(context! {
                        lang: &lang
                    })?,
                ),
            )
            .await?;

            Ok(Ok(Redirect::to(uri!(login_get(
                redirect = Option::<&str>::None
            )))))
        }
        None => Ok(Err(Template::render(
            "password-reset",
            context! { lang, token, messages: [Message { text_key: String::from("password-reset-invalid"), message_type: MessageType::Error }]},
        ))),
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

#[catch(401)]
pub async fn unauthorized_handler(req: &Request<'_>) -> Redirect {
    let to = req.uri().to_string();
    Redirect::to(uri!(login_get(redirect = Some(to))))
}

mod admin;
mod auth;
mod language;
mod model;

#[macro_use]
extern crate rocket;

#[macro_use]
extern crate lazy_static;

use std::collections::{HashMap};

use chrono::{DateTime, Local};
use lettre::{AsyncSmtpTransport, Tokio1Executor};
use model::Date;
use rocket::{
    fairing::AdHoc,
    fs::FileServer,
    request::Request,
    response::{self, Responder},
};
use rocket_db_pools::{sqlx, Database as DatabaseTrait};
use rocket_dyn_templates::Template;
use serde::Deserialize;
use tera::Tera;
use itertools::Itertools;

pub type Mailer = AsyncSmtpTransport<Tokio1Executor>;

pub type RocketResult<T = ()> = std::result::Result<T, RocketError>;
pub struct RocketError(pub anyhow::Error);

impl<E> From<E> for RocketError
where
    E: Into<anyhow::Error>,
{
    fn from(error: E) -> Self {
        RocketError(error.into())
    }
}

impl<'r> Responder<'r, 'r> for RocketError {
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'r> {
        response::Debug(self.0).respond_to(request)
    }
}

#[derive(DatabaseTrait)]
#[database("database")]
pub struct Database(sqlx::PgPool);

#[derive(Deserialize)]
pub struct Config {
    email_from_address: String,
    web_address: String,
}

lazy_static! {
    pub static ref MAIL_TEMPLATES: Tera = {
        let mut tera = match Tera::new("templates-mail/**/*.tera") {
            Ok(t) => t,
            Err(e) => {
                println!("Parsing error(s) in mail templates: {}", e);
                ::std::process::exit(1);
            }
        };
        tera.register_function("fluent", language::make_fluent_loader());
        tera
    };
}

pub const BROWSER_DATETIME_FORMAT: &str = "%Y-%m-%dT%H:%M";

pub fn tera_now(_args: &HashMap<String, tera::Value>) -> tera::Result<tera::Value> {
    let now = chrono::Local::now().naive_local();
    Ok(tera::to_value(format!(
        "{}",
        now.format(BROWSER_DATETIME_FORMAT)
    ))?)
}

pub fn tera_format_date(
    value: &tera::Value,
    _args: &HashMap<String, tera::Value>,
) -> tera::Result<tera::Value> {
    let date: DateTime<Local> = tera::from_value(value.clone())?;
    let naive = date.naive_local();
    Ok(tera::to_value(format!(
        "{}",
        naive.format(BROWSER_DATETIME_FORMAT)
    ))?)
}

pub fn tera_days(
    value: &tera::Value,
    _args: &HashMap<String, tera::Value>,
) -> tera::Result<tera::Value> {
    let dates: Vec<Date> = tera::from_value(value.clone())?;
    let days: Vec<DateTime<Local>> = dates.into_iter()
        .map(|date| date.from_date.date().and_hms(0, 0, 0))
        .unique()
        .collect();
    Ok(tera::to_value(days)?)
}

pub fn tera_on_day(
    value: &tera::Value,
    args: &HashMap<String, tera::Value>
) -> tera::Result<tera::Value> {
    let dates: Vec<Date> = tera::from_value(value.clone())?;
    let day = args.get("day").ok_or_else(|| tera::Error::msg("Missing required argument 'day'!"))?;
    let day = tera::from_value::<DateTime<Local>>(day.clone())?.date();
    let filtered: Vec<Date> = dates.into_iter()
        .filter(|date| date.from_date.date() == day)
        .collect();
    Ok(tera::to_value(filtered)?)
}

#[launch]
fn rocket() -> _ {
    let mailer: Mailer = Mailer::unencrypted_localhost();

    rocket::build()
        .attach(Template::custom(|engines| {
            engines
                .tera
                .register_function("fluent", language::make_fluent_loader());
            engines
                .tera
                .register_function("supported_languages", language::supported_languages);
            engines.tera.register_function("now", tera_now);
            engines
                .tera
                .register_filter("format_date", tera_format_date);
            engines
                .tera
                .register_filter("days", tera_days);
                engines
                    .tera
                    .register_filter("on_day", tera_on_day);
        }))
        .attach(Database::init())
        .attach(AdHoc::config::<Config>())
        .manage(mailer)
        .register("/", catchers![auth::unauthorized_handler])
        .mount("/static", FileServer::from("static/"))
        .mount(
            "/",
            routes![
                auth::login_get,
                auth::login_post,
                auth::invite_get,
                auth::invite_post,
                auth::register_get,
                auth::register_post,
                auth::password_reset_request_get,
                auth::password_reset_request_post,
                auth::password_reset_get,
                auth::password_reset_post,
            ],
        )
        .mount("/", routes![
            admin::dashboard,
            admin::date_new_1_get,
            admin::date_new_1_post,
            admin::date_new_2_post,
            admin::room_manage_get,
            admin::room_manage_post,
        ])
}

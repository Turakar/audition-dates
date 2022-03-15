mod admin;
mod auth;
mod language;
mod model;
mod user;

#[macro_use]
extern crate rocket;

#[macro_use]
extern crate lazy_static;

use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};

use chrono::{DateTime, Local};
use itertools::Itertools;
use lettre::{AsyncSmtpTransport, Tokio1Executor};
use rocket::{
    fairing::{self, AdHoc, Fairing},
    fs::FileServer,
    request::{FromRequest, Request},
    response::{self, Redirect, Responder},
};
use rocket_db_pools::{sqlx, Database as DatabaseTrait};
use rocket_dyn_templates::Template;
use serde::Deserialize;
use sqlx::migrate::Migrator;
use tera::Tera;

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

static MIGRATOR: Migrator = sqlx::migrate!();

pub struct MigrationFairing(AtomicBool);

impl MigrationFairing {
    pub fn new() -> Self {
        MigrationFairing(AtomicBool::new(false))
    }
}

impl Default for MigrationFairing {
    fn default() -> Self {
        Self::new()
    }
}

#[rocket::async_trait]
impl Fairing for MigrationFairing {
    fn info(&self) -> fairing::Info {
        fairing::Info {
            name: "Database Migration",
            kind: fairing::Kind::Request,
        }
    }

    async fn on_request(&self, request: &mut Request<'_>, _: &mut rocket::Data<'_>) {
        if !self.0.swap(true, Ordering::SeqCst) {
            let db = <&Database>::from_request(request).await.unwrap();
            MIGRATOR.run(&db.0).await.unwrap();
        }
    }
}

#[derive(Deserialize)]
pub struct Config {
    email_host: String,
    email_port: u16,
    email_from_address: String,
    web_address: String,
    dates_per_day: usize,
    days_deadline: u32,
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
    let array = value
        .as_array()
        .ok_or_else(|| tera::Error::msg("Invalid argument!"))?;
    let days = array
        .iter()
        .map(|value| {
            let value = value
                .get("from_date")
                .ok_or_else(|| tera::Error::msg("Invalid argument!"))?;
            let datetime: DateTime<Local> = tera::from_value(value.clone())?;
            Ok(datetime.date().and_hms(0, 0, 0))
        })
        .collect::<tera::Result<Vec<DateTime<Local>>>>()?;
    let days: Vec<DateTime<Local>> = days.into_iter().unique().collect();
    Ok(tera::to_value(days)?)
}

pub fn tera_on_day(
    value: &tera::Value,
    args: &HashMap<String, tera::Value>,
) -> tera::Result<tera::Value> {
    let day = args
        .get("day")
        .ok_or_else(|| tera::Error::msg("Missing required argument 'day'!"))?;
    let day = tera::from_value::<DateTime<Local>>(day.clone())?.date();

    let array = value
        .as_array()
        .ok_or_else(|| tera::Error::msg("Invalid argument!"))?;
    let filtered = array
        .iter()
        .map(|value| {
            let date_value = value
                .get("from_date")
                .ok_or_else(|| tera::Error::msg("Invalid argument!"))?;
            let datetime: DateTime<Local> = tera::from_value(date_value.clone())?;
            Ok((value, datetime.date()))
        })
        .filter_ok(|(_, date)| *date == day)
        .map_ok(|(value, _)| value.clone())
        .collect::<tera::Result<Vec<tera::Value>>>()?;

    Ok(tera::to_value(filtered)?)
}

#[get("/favicon.ico")]
pub async fn favicon_get() -> Redirect {
    Redirect::to("/static/favicon/favicon.ico")
}

#[launch]
fn rocket() -> _ {
    let rocket = rocket::build()
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
            engines.tera.register_filter("days", tera_days);
            engines.tera.register_filter("on_day", tera_on_day);
        }))
        .attach(Database::init())
        .attach(AdHoc::config::<Config>())
        .attach(MigrationFairing::new())
        .register("/", catchers![auth::unauthorized_handler])
        .register("/booking", catchers![user::date_gone_handler])
        .mount("/static", FileServer::from("static/"))
        .mount("/", routes![favicon_get])
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
                auth::logout,
            ],
        )
        .mount(
            "/",
            routes![
                admin::dashboard,
                admin::date_new_1_get,
                admin::date_new_1_post,
                admin::date_new_2_post,
                admin::room_manage_get,
                admin::room_manage_post,
                admin::announcements_get,
                admin::announcements_post,
            ],
        )
        .mount(
            "/",
            routes![
                user::index_get,
                user::date_overview_get,
                user::booking_new_get,
                user::booking_new_post,
                user::booking_delete_get,
                user::booking_delete_post,
            ],
        );

    let config: Config = rocket.figment().extract().expect("config");

    let mailer: Mailer = Mailer::builder_dangerous(config.email_host)
        .port(config.email_port)
        .build();
    
    rocket.manage(mailer)
}

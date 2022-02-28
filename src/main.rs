mod auth;
mod dashboard;
mod language;
mod util;

#[macro_use]
extern crate rocket;

#[macro_use]
extern crate lazy_static;

use lettre::{AsyncSmtpTransport, Tokio1Executor};
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
        .mount("/", routes![dashboard::dashboard])
}

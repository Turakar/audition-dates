use anyhow::anyhow;
use anyhow::Result;
use chrono::Duration;
use chrono::{DateTime, Local};
use lettre::AsyncTransport;
use lettre::Message as EmailMessage;
use rocket::form::error::ErrorKind;
use rocket::form::Contextual;
use rocket::form::Form;
use rocket::form::FromForm;
use rocket::http::Status;
use rocket::request::FromParam;
use rocket::request::FromRequest;
use rocket::Request;
use rocket::State;
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};
use serde::{Deserialize, Serialize};

use crate::language::LOCALES;
use crate::model::Message;
use crate::model::MessageType;
use crate::model::Voice;
use crate::model::get_announcement;
use crate::model::{DateType, Email};
use crate::Mailer;
use crate::MAIL_TEMPLATES;
use crate::{language::Language, Config, Database, RocketResult};

#[derive(Serialize, Deserialize)]
pub struct Date {
    id: i32,
    from_date: DateTime<Local>,
    to_date: DateTime<Local>,
    room_number: String,
    date_type: DateType,
}

#[get("/")]
pub async fn index_get(lang: Language, mut db: Connection<Database>) -> RocketResult<Template> {
    let lang = lang.into_string();
    let announcement = get_announcement("general", &lang, &mut db).await?;
    Ok(Template::render(
        "index",
        context! {
            lang,
            date_types: DateType::get_variants_tera(),
            announcement,
        },
    ))
}

#[get("/<date_type>")]
pub async fn date_overview_get(
    lang: Language,
    mut db: Connection<Database>,
    config: &State<Config>,
    date_type: DateType,
) -> RocketResult<Template> {
    let dates =
        get_active_dates(&mut db, Some(date_type.get_value()), config.dates_per_day, config.days_deadline).await?;
    let lang = lang.into_string();
    let announcement = get_announcement(date_type.get_value(), &lang, &mut db).await?;
    Ok(Template::render(
        "date-overview",
        context! {
            lang,
            date_type: date_type.tera(),
            dates,
            announcement,
        },
    ))
}

async fn get_active_dates(
    db: &mut Connection<Database>,
    date_type: Option<&str>,
    dates_per_day: usize,
    days_deadline: u32,
) -> Result<Vec<Date>> {
    let mut dates: Vec<Date> = match date_type {
        Some(date_type) => sqlx::query!(
            "select dates.id as id, from_date, to_date, room_number, date_type \
                from dates \
                join rooms on rooms.id = dates.room_id \
                left join bookings on dates.id = bookings.date_id \
                where token is null \
                and date_type = $1 \
                order by from_date asc",
            &date_type
        )
        .fetch_all(&mut **db)
        .await?
        .into_iter()
        .map(|record| Date {
            id: record.id,
            from_date: record.from_date.with_timezone(&Local),
            to_date: record.to_date.with_timezone(&Local),
            room_number: record.room_number,
            date_type: DateType::from_param(&record.date_type).unwrap(),
        })
        .collect(),
        None => sqlx::query!(
            "select dates.id as id, from_date, to_date, room_number, date_type \
                from dates \
                join rooms on rooms.id = dates.room_id \
                left join bookings on dates.id = bookings.date_id \
                where token is null \
                order by from_date asc",
        )
        .fetch_all(&mut **db)
        .await?
        .into_iter()
        .map(|record| Date {
            id: record.id,
            from_date: record.from_date.with_timezone(&Local),
            to_date: record.to_date.with_timezone(&Local),
            room_number: record.room_number,
            date_type: DateType::from_param(&record.date_type).unwrap(),
        })
        .collect(),
    };

    let today = Local::today();
    dates = dates.into_iter()
        .filter(|date| date.from_date.date() >= today + Duration::days(days_deadline as i64))
        .collect();

    if dates.is_empty() || dates_per_day == 0 {
        return Ok(dates);
    }

    let mut i = 1;
    let mut current_day = dates[0].from_date.date();
    let mut current_count = 1;
    while i < dates.len() {
        let next_day = dates[i].from_date.date();
        if current_day == next_day {
            if current_count < dates_per_day {
                current_count += 1;
                i += 1;
            } else {
                dates.remove(i);
            }
        } else {
            current_count = 1;
            i += 1;
            current_day = next_day;
        }
    }

    Ok(dates)
}

#[derive(FromForm)]
pub struct BookingForm<'r> {
    email: Email<'r>,
    person_name: &'r str,
    notes: &'r str,
    voice: Voice,
}

#[get("/booking/new/<id>")]
pub async fn booking_new_get(
    lang: Language,
    mut db: Connection<Database>,
    config: &State<Config>,
    id: i32,
) -> RocketResult<Result<Template, Status>> {
    let available = get_active_dates(&mut db, None, config.dates_per_day, config.days_deadline).await?;
    let date = available.into_iter().find(|date| date.id == id);
    let lang = lang.into_string();

    match date {
        None => Ok(Err(Status::Gone)),
        Some(date) => {
            let announcement = get_announcement(date.date_type.get_value(), &lang, &mut db).await?;
            Ok(Ok(Template::render(
                "booking-new",
                context! {
                    lang,
                    voices: date.date_type.get_voices_tera(),
                    date,
                    email: "",
                    person_name: "",
                    notes: "",
                    announcement,
                },
            )))
        },
    }
}

#[post("/booking/new/<id>", data = "<form>")]
pub async fn booking_new_post(
    lang: Language,
    mut db: Connection<Database>,
    config: &State<Config>,
    mailer: &State<Mailer>,
    form: Form<Contextual<'_, BookingForm<'_>>>,
    id: i32,
) -> RocketResult<Result<Template, Status>> {
    let lang = lang.into_string();

    let available = get_active_dates(&mut db, None, config.dates_per_day, config.days_deadline).await?;
    let date = match available.into_iter().find(|date| date.id == id) {
        None => return Ok(Err(Status::Gone)),
        Some(date) => date,
    };
    let announcement = get_announcement(date.date_type.get_value(), &lang, &mut db).await?;

    match &form.value {
        None => {
            let context = &form.context;
            let messages: Vec<Message> = context
                .errors()
                .map(|error| match &error.kind {
                    ErrorKind::Validation(msg) => Message {
                        text_key: msg.to_string(),
                        message_type: MessageType::Error,
                    },
                    _ => Message {
                        text_key: String::from("validation-unknown"),
                        message_type: MessageType::Error,
                    },
                })
                .collect();

            return Ok(Ok(Template::render(
                "booking-new",
                context! {
                    lang,
                    date,
                    email: context.field_value("email").unwrap_or_default(),
                    person_name: context.field_value("person").unwrap_or_default(),
                    notes: context.field_value("notes").unwrap_or_default(),
                    messages,
                    announcement,
                },
            )));
        }
        Some(BookingForm {
            email: Email(email),
            person_name,
            notes,
            voice,
        }) => {
            let token = sqlx::query_scalar!(
                "insert into bookings (date_id, email, person_name, notes, voice) \
            values ($1, $2, $3, $4, $5) \
            returning token",
                &date.id,
                &email,
                &person_name,
                &notes,
                &voice.get_value()
            )
            .fetch_one(&mut *db)
            .await?;

            let link = format!("{}/booking/delete/{}", &config.web_address, &token);
            let mut mail_context = tera::Context::new();
            mail_context.insert("lang", &lang);
            mail_context.insert("link", &link);
            mail_context.insert(
                "day",
                format!("{}", date.from_date.naive_local().format("%d.%m.%Y")).as_str(),
            );
            mail_context.insert(
                "from",
                format!("{}", date.from_date.naive_local().format("%H:%M")).as_str(),
            );
            mail_context.insert(
                "to",
                format!("{}", date.to_date.naive_local().format("%H:%M")).as_str(),
            );
            mail_context.insert("room_number", &date.room_number);
            mail_context.insert("announcement", &announcement);
            let mail = EmailMessage::builder()
                .to(email.parse()?)
                .from(config.email_from_address.parse()?)
                .subject(
                    LOCALES
                        .lookup_single_language::<&str>(
                            &lang.parse()?,
                            "mail-booking-subject",
                            None,
                        )
                        .ok_or(anyhow!("Missing translation for mail-booking-subject!"))?,
                )
                .body(MAIL_TEMPLATES.render("booking.tera", &mail_context)?)?;
            mailer.send(mail).await?;

            Ok(Ok(Template::render(
                "booking-success",
                context! {
                    lang,
                },
            )))
        }
    }
}

#[catch(410)]
pub async fn date_gone_handler(req: &Request<'_>) -> Template {
    let lang = Language::from_request(req).await.unwrap().into_string();
    Template::render(
        "date-gone",
        context! {
            lang
        },
    )
}

#[get("/booking/delete/<_token>")]
pub async fn booking_delete_get(lang: Language, _token: &str) -> Template {
    Template::render(
        "booking-delete",
        context! { lang: lang.into_string() }
    )
}

#[post("/booking/delete/<token>")]
pub async fn booking_delete_post(lang: Language, mut db: Connection<Database>, token: &str) -> RocketResult<Template> {
    sqlx::query!(
        "delete from bookings where token = $1",
        &token
    ).execute(&mut *db).await?;

    Ok(Template::render(
        "booking-delete-confirm",
        context! { lang: lang.into_string() }
    ))
}

use anyhow::Result;
use map_macro::map;
use rocket::form::error::ErrorKind;
use rocket::form::Contextual;
use rocket::form::Form;
use rocket::form::FromForm;
use rocket::http::Status;
use rocket::request::FromRequest;
use rocket::response::Redirect;
use rocket::Request;
use rocket::State;
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};
use tera::Context;

use crate::mail::send_mail;
use crate::mail::waiting_list_notify;
use crate::mail::MailBody;
use crate::model::check_date_type_access;
use crate::model::get_announcement;
use crate::model::get_waiting_list_email;
use crate::model::Date;
use crate::model::Message;
use crate::model::MessageType;
use crate::model::SelectString;
use crate::model::{DateType, Email};
use crate::Mailer;
use crate::{language::Language, Config, Database, RocketResult};

#[get("/")]
pub async fn index_get(
    lang: Language,
    mut db: Connection<Database>,
    config: &State<Config>,
) -> RocketResult<Template> {
    let lang = lang.into_string();
    let announcement = get_announcement("general", &lang, &mut db).await?;
    let date_types: Vec<DateType> = DateType::get_variants(&mut db, &lang)
        .await?
        .into_iter()
        .filter(|date_type| config.enabled_date_types.contains(&date_type.value))
        .collect();
    Ok(Template::render(
        "index",
        context! {
            lang,
            date_types,
            announcement,
        },
    ))
}

#[get("/dates/<date_type>?<token>")]
pub async fn date_overview_get(
    lang: Language,
    mut db: Connection<Database>,
    config: &State<Config>,
    date_type: &str,
    token: Option<&str>,
) -> RocketResult<Template> {
    let lang = lang.into_string();
    let ignore_waiting_list = check_date_type_access(date_type, token, config, &mut db).await?;
    let dates = Date::get_available_dates(
        &mut db,
        date_type,
        config,
        Some(lang.as_str()),
        ignore_waiting_list,
    )
    .await?;
    let announcement = get_announcement(date_type, &lang, &mut db).await?;
    let date_type = DateType::get_by_value(&mut db, date_type, &lang).await?;
    Ok(Template::render(
        "date-overview",
        context! {
            lang,
            date_type,
            dates,
            announcement,
            token,
        },
    ))
}

#[derive(FromForm)]
#[allow(dead_code)]
pub struct BookingForm<'r> {
    email: Email<'r>,
    person_name: &'r str,
    notes: &'r str,
    voice: SelectString<'r>,
    token: Option<&'r str>,
}

#[get("/booking/new/<id>?<token>")]
pub async fn booking_new_get(
    lang: Language,
    mut db: Connection<Database>,
    config: &State<Config>,
    id: i32,
    token: Option<&str>,
) -> RocketResult<Result<Template, Status>> {
    let lang = lang.into_string();
    let date = Date::get_available_date(&mut db, id, lang.as_str(), config, token).await?;

    match date {
        None => Ok(Err(Status::Gone)),
        Some(date) => {
            let announcement = get_announcement(&date.date_type.value, &lang, &mut db).await?;
            let voices = date.date_type.get_voices(&mut db, &lang, "booking").await?;
            let email = get_waiting_list_email(token, &mut db).await?;
            let email_fixed = email.is_some();
            Ok(Ok(Template::render(
                "booking-new",
                context! {
                    lang,
                    voices,
                    date,
                    email: email.unwrap_or_default(),
                    email_fixed,
                    person_name: "",
                    notes: "",
                    voice_selected: "",
                    announcement,
                    token,
                },
            )))
        }
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
    let token = form.context.field_value("token");
    let date = match Date::get_available_date(&mut db, id, lang.as_str(), config, token).await? {
        Some(date) => date,
        None => {
            return Ok(Err(Status::Gone));
        }
    };

    let announcement = get_announcement(&date.date_type.value, &lang, &mut db).await?;

    match &form.value {
        None => {
            let voices = date.date_type.get_voices(&mut db, &lang, "booking").await?;
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
            Ok(Ok(Template::render(
                "booking-new",
                context! {
                    lang,
                    voices,
                    date,
                    email: context.field_value("email").unwrap_or_default(),
                    email_fixed: token.is_some(),
                    person_name: context.field_value("person_name").unwrap_or_default(),
                    notes: context.field_value("notes").unwrap_or_default(),
                    voice_selected: context.field_value("voice").unwrap_or_default(),
                    messages,
                    announcement,
                    token,
                },
            )))
        }
        Some(BookingForm {
            email: Email(email),
            person_name,
            notes,
            voice: SelectString(voice),
            token: _,
        }) => {
            if let Some(waiting_list_email) = get_waiting_list_email(token, &mut db).await? {
                println!("{} {}", waiting_list_email, email);
                if waiting_list_email != *email {
                    return Ok(Err(Status::Gone));
                }
            }

            let token = sqlx::query_scalar!(
                "insert into bookings (date_id, email, person_name, notes, voice, lang) \
            values ($1, $2, $3, $4, (select id from voices where value = $5 and date_type = $6 and position = 'booking'), $7) \
            returning token",
                &date.id,
                &email,
                &person_name,
                &notes,
                &voice,
                &date.date_type.value,
                &lang,
            )
            .fetch_one(&mut *db)
            .await?;

            sqlx::query!(
                r#"delete from waiting_list
                where email = $1
                and date_type = $2"#,
                &email,
                &date.date_type.value
            )
            .execute(&mut *db)
            .await?;

            let link = format!("{}/booking/delete/{}", &config.web_address, &token);
            send_mail(
                config,
                mailer,
                email,
                &lang,
                "mail-booking-subject",
                None,
                MailBody::Template(
                    "booking.tera",
                    &Context::from_serialize(context! {
                        lang: &lang,
                        link,
                        day: format!("{}", date.from_date.naive_local().format("%d.%m.%Y")),
                        from: format!("{}", date.from_date.naive_local().format("%H:%M")),
                        to: format!("{}", date.to_date.naive_local().format("%H:%M")),
                        room_number: &date.room_number,
                        announcement: &announcement,
                    })?,
                ),
            )
            .await?;

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
    Template::render("booking-delete", context! { lang: lang.into_string() })
}

#[post("/booking/delete/<token>")]
pub async fn booking_delete_post(
    lang: Language,
    mut db: Connection<Database>,
    config: &State<Config>,
    mailer: &State<Mailer>,
    token: &str,
) -> RocketResult<Template> {
    let record = sqlx::query!(
        r#"select from_date < now() as "too_late!", date_type
        from dates
        join bookings on dates.id = bookings.date_id
        where bookings.token = $1"#,
        &token
    )
    .map(|record| (record.too_late, record.date_type))
    .fetch_optional(&mut *db)
    .await?;

    match record {
        Some((true, _)) => Ok(Template::render(
            "booking-delete",
            context! { lang: lang.into_string(), messages: [Message { text_key: String::from("booking-delete-too-late"), message_type: MessageType::Error }] },
        )),
        Some((false, date_type)) => {
            sqlx::query!("delete from bookings where token = $1", &token)
                .execute(&mut *db)
                .await?;
            waiting_list_notify(&mut db, &date_type, config, mailer).await?;
            Ok(Template::render(
                "booking-delete-confirm",
                context! { lang: lang.into_string() },
            ))
        }
        None => Ok(Template::render(
            "booking-delete-confirm",
            context! { lang: lang.into_string() },
        )),
    }
}

#[derive(FromForm)]
pub struct WaitingListForm<'r> {
    email: Email<'r>,
}

#[post("/waiting-list/subscribe/<date_type>", data = "<form>")]
pub async fn waiting_list_subscribe_post(
    lang: Language,
    mut db: Connection<Database>,
    config: &State<Config>,
    mailer: &State<Mailer>,
    date_type: &str,
    form: Form<WaitingListForm<'_>>,
) -> RocketResult<Template> {
    let email = form.into_inner().email.0;
    let lang = lang.into_string();
    let token = sqlx::query_scalar!(
        r#"insert into waiting_list (date_type, email, lang)
        values  ($1, $2, $3)
        on conflict do nothing
        returning token"#,
        &date_type,
        &email,
        &lang
    )
    .fetch_optional(&mut *db)
    .await?;
    let token = match token {
        Some(token) => token,
        None => {
            sqlx::query_scalar!(
                "select token from waiting_list where date_type = $1 and email = $2",
                &date_type,
                &email
            )
            .fetch_one(&mut *db)
            .await?
        }
    };
    let date_type = DateType::get_by_value(&mut db, date_type, &lang).await?;
    let subject_args = map! {
        "datetype" => date_type.display_name.as_deref().unwrap()
    };
    send_mail(
        config,
        mailer,
        email,
        &lang,
        "waiting-list",
        Some(&subject_args),
        MailBody::Template(
            "waiting-list-confirmation.tera",
            &Context::from_serialize(context! {
                lang: &lang,
                unsubscribe: format!(
                    "{}/waiting-list/unsubscribe/{}",
                    &config.web_address, &token
                )
            })?,
        ),
    )
    .await?;
    Ok(Template::render(
        "waiting-list-confirmation",
        context! { lang, date_type },
    ))
}

#[get("/waiting-list/unsubscribe/<token>")]
pub async fn waiting_list_unsubscribe_get(
    lang: Language,
    mut db: Connection<Database>,
    token: &str,
) -> RocketResult<Template> {
    let lang = lang.into_string();
    let date_type = sqlx::query_scalar!(
        r#"select display_name
        from date_types_translations
        join waiting_list on waiting_list.date_type = date_types_translations.date_type
        where waiting_list.token = $1
        and date_types_translations.lang = $2"#,
        &token,
        &lang,
    )
    .fetch_one(&mut *db)
    .await?;
    Ok(Template::render(
        "waiting-list-unsubscribe",
        context! {
            lang, date_type
        },
    ))
}

#[post("/waiting-list/unsubscribe/<token>")]
pub async fn waiting_list_unsubscribe_post(
    mut db: Connection<Database>,
    token: &str,
) -> RocketResult<Redirect> {
    sqlx::query!(
        r#"delete from waiting_list
        where token = $1"#,
        &token
    )
    .execute(&mut *db)
    .await?;
    Ok(Redirect::to(uri!(index_get)))
}

#[get("/impressum")]
pub async fn impressum_get(lang: Language, config: &State<Config>) -> Template {
    Template::render(
        "impressum",
        context! {
            lang: lang.into_string(),
            impressum: config.impressum.as_str()
        },
    )
}

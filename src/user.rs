use std::borrow::Cow;

use anyhow::anyhow;
use anyhow::Result;
use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::Loader;
use futures::TryStreamExt;
use lettre::message::header;
use lettre::message::header::ContentTransferEncoding;
use lettre::message::IntoBody;
use lettre::AsyncTransport;
use lettre::Message as EmailMessage;
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

use crate::language::LOCALES;
use crate::model::get_announcement;
use crate::model::Date;
use crate::model::Message;
use crate::model::MessageType;
use crate::model::{DateType, Email};
use crate::Mailer;
use crate::MAIL_TEMPLATES;
use crate::{language::Language, Config, Database, RocketResult};

#[get("/")]
pub async fn index_get(lang: Language, mut db: Connection<Database>) -> RocketResult<Template> {
    let lang = lang.into_string();
    let announcement = get_announcement("general", &lang, &mut db).await?;
    let date_types = DateType::get_variants(&mut db, &lang).await?;
    Ok(Template::render(
        "index",
        context! {
            lang,
            date_types,
            announcement,
        },
    ))
}

#[get("/dates/<date_type>")]
pub async fn date_overview_get(
    lang: Language,
    mut db: Connection<Database>,
    config: &State<Config>,
    date_type: &str,
) -> RocketResult<Template> {
    let lang = lang.into_string();
    let dates = Date::get_available_dates(
        &mut db,
        Some(date_type),
        config.dates_per_day,
        config.days_deadline,
        Some(lang.as_str()),
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
        },
    ))
}

#[derive(FromForm)]
pub struct BookingForm<'r> {
    email: Email<'r>,
    person_name: &'r str,
    notes: &'r str,
    voice: &'r str,
}

#[get("/booking/new/<id>")]
pub async fn booking_new_get(
    lang: Language,
    mut db: Connection<Database>,
    config: &State<Config>,
    id: i32,
) -> RocketResult<Result<Template, Status>> {
    let lang = lang.into_string();
    let date = Date::get_available_date(
        &mut db,
        id,
        lang.as_str(),
        config.dates_per_day,
        config.days_deadline,
    )
    .await?;

    match date {
        None => Ok(Err(Status::Gone)),
        Some(date) => {
            let announcement = get_announcement(&date.date_type.value, &lang, &mut db).await?;
            let voices = date.date_type.get_voices(&mut db, &lang, "booking").await?;
            Ok(Ok(Template::render(
                "booking-new",
                context! {
                    lang,
                    voices,
                    date,
                    email: "",
                    person_name: "",
                    notes: "",
                    voice_selected: "",
                    announcement,
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
    let date = match Date::get_available_date(
        &mut db,
        id,
        lang.as_str(),
        config.dates_per_day,
        config.days_deadline,
    )
    .await?
    {
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
                    person_name: context.field_value("person_name").unwrap_or_default(),
                    notes: context.field_value("notes").unwrap_or_default(),
                    voice_selected: context.field_value("voice").unwrap_or_default(),
                    messages,
                    announcement,
                },
            )))
        }
        Some(BookingForm {
            email: Email(email),
            person_name,
            notes,
            voice,
        }) => {
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
                        .ok_or_else(|| anyhow!("Missing translation for mail-booking-subject!"))?,
                )
                .header(header::ContentType::TEXT_PLAIN)
                .body(
                    MAIL_TEMPLATES
                        .render("booking.tera", &mail_context)?
                        .into_body(Some(ContentTransferEncoding::Base64)),
                )?;
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
            waiting_list_notify(&mut db, &date_type, &config, &mailer).await?;
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
    let mut mail_context = tera::Context::new();
    mail_context.insert(
        "unsubscribe",
        format!(
            "{}/waiting-list/unsubscribe/{}",
            &config.web_address, &token
        )
        .as_str(),
    );
    mail_context.insert("lang", &lang);
    let date_type = DateType::get_by_value(&mut db, &date_type, &lang).await?;
    let mail_header_args = map! {
        "datetype" => date_type.display_name.clone().unwrap().into()
    };
    let mail = EmailMessage::builder()
        .to(email.parse()?)
        .from(config.email_from_address.parse()?)
        .subject(
            LOCALES
                .lookup_single_language(&lang.parse()?, "waiting-list", Some(&mail_header_args))
                .ok_or_else(|| anyhow!("Missing translation for waiting-list!"))?,
        )
        .header(header::ContentType::TEXT_PLAIN)
        .body(
            MAIL_TEMPLATES
                .render("waiting-list-confirmation.tera", &mail_context)?
                .into_body(Some(ContentTransferEncoding::Base64)),
        )?;
    mailer.send(mail).await?;
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

pub async fn waiting_list_notify(
    db: &mut Connection<Database>,
    date_type: &str,
    config: &Config,
    mailer: &Mailer,
) -> Result<()> {
    let recipients: Vec<(String, String, String)> = sqlx::query!(
        r#"select email, lang, token
        from waiting_list
        where date_type = $1"#,
        &date_type
    )
    .fetch(&mut **db)
    .map_ok(|record| (record.email, record.lang, record.token))
    .try_collect::<Vec<(String, String, String)>>()
    .await?;

    for (email, lang, token) in recipients {
        let date_type = DateType::get_by_value(db, &date_type, &lang).await?;
        let mut mail_context = tera::Context::new();
        mail_context.insert(
            "unsubscribe",
            format!(
                "{}/waiting-list/unsubscribe/{}",
                &config.web_address, &token
            )
            .as_str(),
        );
        mail_context.insert(
            "link",
            format!("{}/dates/{}", &config.web_address, &date_type.value).as_str(),
        );
        mail_context.insert("lang", &lang);
        let mail_header_args = map! {
            "datetype" => date_type.display_name.clone().unwrap().into()
        };
        let message = EmailMessage::builder()
            .to(email.parse()?)
            .from(config.email_from_address.parse()?)
            .subject(
                LOCALES
                    .lookup_single_language(
                        &lang.parse()?,
                        "waiting-list",
                        Some(&mail_header_args),
                    )
                    .ok_or_else(|| anyhow!("Missing translation for waiting-list!"))?,
            )
            .header(header::ContentType::TEXT_PLAIN)
            .body(
                MAIL_TEMPLATES
                    .render("waiting-list-invite.tera", &mail_context)?
                    .into_body(Some(ContentTransferEncoding::Base64)),
            )?;
        mailer.send(message).await?;
    }
    Ok(())
}

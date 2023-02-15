use std::collections::BTreeMap;
use std::collections::HashSet;

use anyhow::anyhow;
use chrono::DateTime;
use chrono::Duration;
use chrono::Local;
use chrono::NaiveDateTime;
use rocket::form::Form;
use rocket::form::FromForm;
use rocket::response::Redirect;
use rocket::serde::json::Json;
use rocket::State;
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};
use serde::Deserialize;
use serde::Serialize;

use crate::mail::send_mail;
use crate::mail::waiting_list_notify;
use crate::mail::MailBody;
use crate::model::validate_room;
use crate::model::DateType;
use crate::model::FormDateTime;
use crate::model::IntoInner;
use crate::model::Message;
use crate::model::MessageType;
use crate::model::Room;
use crate::model::Voice;
use crate::util::datetime_to_day;
use crate::Config;
use crate::Mailer;
use crate::{auth::Admin, language::Language, Database, RocketResult};

#[derive(Serialize, Deserialize)]
pub struct Date {
    pub from_date: DateTime<Local>,
    pub to_date: DateTime<Local>,
    pub room_id: i32,
    pub date_type: DateType,
}

#[derive(Serialize)]
pub struct BookableDate {
    pub id: i32,
    pub from_date: DateTime<Local>,
    pub to_date: DateTime<Local>,
    pub room_number: String,
    pub booking: Option<Booking>,
    pub date_type: DateType,
}

#[derive(Serialize)]
pub struct Booking {
    email: String,
    person_name: String,
    notes: String,
    voice: Voice,
}

#[get("/admin/dashboard?<day>")]
pub async fn dashboard(
    lang: Language,
    admin: Admin,
    mut db: Connection<Database>,
    day: Option<&str>,
) -> RocketResult<Template> {
    let lang = lang.into_string();
    let display_name = sqlx::query!("select display_name from admins where id = $1", &admin.id)
        .fetch_one(&mut *db)
        .await?
        .display_name;
    let day = datetime_to_day(match day {
        Some(day) => NaiveDateTime::parse_from_str(day, crate::BROWSER_DATETIME_FORMAT)?
            .date()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap(),
        None => {
            let now = match sqlx::query_scalar!(
                "select min(from_date) from dates where from_date >= now()"
            )
            .fetch_one(&mut *db)
            .await?
            {
                Some(date) => date.with_timezone(&Local),
                None => Local::now(),
            };
            datetime_to_day(now)
        }
    });
    let available_days: Vec<DateTime<Local>> = sqlx::query!(
        r#"select distinct date_trunc('day', from_date) as "day!" from dates order by "day!" asc"#
    )
    .fetch_all(&mut *db)
    .await?
    .into_iter()
    .map(|record| datetime_to_day(record.day.with_timezone(&Local)))
    .collect();
    let dates: Vec<BookableDate> = sqlx::query!(
        r#"select
        dates.id as dates_id,
        from_date,
        to_date,
        room_number,
        dates.date_type,
        date_types_translations.display_name as date_type_display_name,
        email as "email?",
        person_name as "person_name?",
        notes as "notes?",
        voices.value as "voice?",
        voices_translations.display_name as "voice_display_name?"
        from dates
        join date_types_translations on date_types_translations.date_type = dates.date_type
        join rooms on dates.room_id = rooms.id
        left join bookings on bookings.date_id = dates.id
        left join voices on bookings.voice = voices.id
        left join voices_translations on voices.id = voices_translations.voice
        where $1 <= from_date and from_date <= $1 + interval '1 day'
        and date_types_translations.lang = $2
        and (voices_translations.lang is null or voices_translations.lang = $2)
        order by from_date asc, date_type asc, room_number asc"#,
        &day,
        &lang
    )
    .fetch_all(&mut *db)
    .await?
    .into_iter()
    .map(|record| {
        let booking = match record.email.is_some() {
            false => None,
            true => Some(Booking {
                email: record.email.unwrap(),
                person_name: record.person_name.unwrap(),
                notes: record.notes.unwrap(),
                voice: match (record.voice, record.voice_display_name) {
                    (Some(voice), Some(display_name)) => Voice {
                        value: voice,
                        display_name: Some(display_name),
                    },
                    _ => panic!("Booking without a voice or a voice without a translation!"),
                },
            }),
        };
        BookableDate {
            id: record.dates_id,
            from_date: record.from_date.with_timezone(&Local),
            to_date: record.to_date.with_timezone(&Local),
            room_number: record.room_number,
            booking,
            date_type: DateType {
                value: record.date_type,
                display_name: Some(record.date_type_display_name),
            },
        }
    })
    .collect();
    Ok(Template::render(
        "dashboard",
        context! { lang, display_name, dates, available_days, day },
    ))
}

#[get("/admin/date-cancel?<dates>")]
pub async fn date_cancel_get(lang: Language, _admin: Admin, dates: Vec<i32>) -> Template {
    Template::render("date-cancel", context! { lang: lang.into_string(), dates })
}

#[derive(FromForm)]
pub struct DateCancelForm<'r> {
    dates: Vec<i32>,
    explanations: BTreeMap<String, &'r str>,
}

#[post("/admin/date-cancel", data = "<form>")]
pub async fn date_cancel_post(
    _admin: Admin,
    mut db: Connection<Database>,
    config: &State<Config>,
    mailer: &State<Mailer>,
    form: Form<DateCancelForm<'_>>,
) -> RocketResult<Redirect> {
    let DateCancelForm {
        dates,
        explanations,
    } = form.into_inner();
    let mut emails = Vec::new();
    for date in &dates {
        emails.extend(
            sqlx::query!("select email, lang from bookings where date_id = $1", &date)
                .fetch_all(&mut *db)
                .await?
                .into_iter()
                .map(|record| (record.email, record.lang)),
        );
    }
    for (email, lang) in emails {
        let explanation = explanations[&lang];
        if !explanation.is_empty() {
            send_mail(
                config,
                mailer,
                &email,
                &lang,
                "mail-date-cancel-subject",
                None,
                MailBody::Raw(String::from(explanation)),
            )
            .await?;
        }
    }
    for date in &dates {
        sqlx::query!("delete from dates where id = $1", &date)
            .execute(&mut *db)
            .await?;
    }
    Ok(Redirect::to(uri!(dashboard(day = Option::<&str>::None))))
}

#[get("/admin/date-new-1")]
pub async fn date_new_1_get(
    lang: Language,
    _admin: Admin,
    mut db: Connection<Database>,
) -> RocketResult<Template> {
    let lang = lang.into_string();
    let rooms: Vec<String> = sqlx::query!("select room_number from rooms order by room_number asc")
        .fetch_all(&mut *db)
        .await?
        .into_iter()
        .map(|record| record.room_number)
        .collect();
    let date_types = DateType::get_variants(&mut db, &lang).await?;
    Ok(Template::render(
        "date-new-1",
        context! {
            lang,
            rooms,
            room_selected: "",
            date_types,
            date_type_selected: "",
            from_date: Local::now(),
            to_date: Local::now() + Duration::hours(1),
            interval: 10i32,
        },
    ))
}

#[derive(FromForm)]
pub struct DateNew1Form<'r> {
    date_type: &'r str,
    room: &'r str,
    from_date: FormDateTime,
    to_date: FormDateTime,
    interval: u32,
}

#[post("/admin/date-new-1", data = "<form>")]
pub async fn date_new_1_post(
    lang: Language,
    _admin: Admin,
    mut db: Connection<Database>,
    form: Form<DateNew1Form<'_>>,
) -> RocketResult<Template> {
    let lang = lang.into_string();
    let DateNew1Form {
        date_type,
        room,
        from_date,
        to_date,
        interval,
    } = form.into_inner();

    let mut messages = Vec::new();
    let (room, room_id) = validate_room(room, &mut messages, &mut db).await?;
    let from_date = from_date.into_inner();
    let to_date = to_date.into_inner();
    let interval = interval as i64;

    let num_minutes = (to_date - from_date).num_minutes();
    if from_date >= to_date {
        messages.push(Message {
            text_key: String::from("wrong-date-order"),
            message_type: MessageType::Error,
        });
    } else if num_minutes % interval != 0 {
        messages.push(Message {
            text_key: String::from("interval-not-even"),
            message_type: MessageType::Error,
        });
    } else if num_minutes / interval > 1000 {
        messages.push(Message {
            text_key: String::from("too-many-dates"),
            message_type: MessageType::Error,
        });
    }

    if !messages.is_empty() {
        let rooms: Vec<String> =
            sqlx::query!("select room_number from rooms order by room_number asc")
                .fetch_all(&mut *db)
                .await?
                .into_iter()
                .map(|record| record.room_number)
                .collect();
        let date_types = DateType::get_variants(&mut db, &lang).await?;
        return Ok(Template::render(
            "date-new-1",
            context! {
                lang,
                rooms,
                room_selected: room,
                date_types,
                date_type_selected: date_type,
                messages,
                from_date,
                to_date,
                interval,
            },
        ));
    }

    let date_type = DateType::get_by_value(&mut db, date_type, &lang).await?;

    let num_dates = (num_minutes / interval) as i32;
    let dates: Vec<Date> = (0..num_dates)
        .into_iter()
        .map(|i| Date {
            from_date: from_date + Duration::minutes(interval) * i,
            to_date: from_date + Duration::minutes(interval) * (i + 1),
            room_id,
            date_type: date_type.clone(),
        })
        .collect();

    Ok(Template::render(
        "date-new-2",
        context! {
            lang,
            dates,
            interval,
        },
    ))
}

#[derive(FromForm)]
pub struct DateNew2Form {
    date_selected: Vec<bool>,
    dates: Json<Vec<Date>>,
}

#[post("/admin/date-new-2", data = "<form>")]
pub async fn date_new_2_post(
    _admin: Admin,
    mut db: Connection<Database>,
    config: &State<Config>,
    mailer: &State<Mailer>,
    form: Form<DateNew2Form>,
) -> RocketResult<Redirect> {
    let DateNew2Form {
        date_selected,
        dates,
    } = form.into_inner();
    let dates: Vec<Date> = dates
        .0
        .into_iter()
        .zip(date_selected.into_iter())
        .filter(|(_date, selected)| *selected)
        .map(|(date, _selected)| date)
        .collect();

    let invalid = dates.iter().any(|date| date.from_date > date.to_date);
    if invalid {
        return Err(anyhow!("Invalid buffered dates!").into());
    }

    let mut date_types = HashSet::new();
    for date in dates {
        let Date {
            from_date,
            to_date,
            room_id,
            date_type,
        } = date;
        date_types.insert(date_type.value.clone());
        sqlx::query!(
            "insert into dates (from_date, to_date, room_id, date_type) values ($1, $2, $3, $4)",
            &from_date,
            &to_date,
            &room_id,
            &date_type.value,
        )
        .execute(&mut *db)
        .await?;
    }

    for date_type in date_types.into_iter() {
        waiting_list_notify(&mut db, &date_type, config, mailer).await?;
    }

    Ok(Redirect::to(uri!(dashboard(day = Option::<&str>::None))))
}

#[get("/admin/room-manage")]
pub async fn room_manage_get(
    lang: Language,
    mut db: Connection<Database>,
    _admin: Admin,
) -> RocketResult<Template> {
    let rooms: Vec<Room> = sqlx::query_as!(Room, "select id, room_number from rooms")
        .fetch_all(&mut *db)
        .await?;
    Ok(Template::render(
        "room-manage",
        context! {
            lang: lang.into_string(),
            rooms
        },
    ))
}

#[derive(FromForm)]
pub struct RoomManageForm<'r> {
    room_number: &'r str,
    button: &'r str,
}

#[post("/admin/room-manage", data = "<form>")]
pub async fn room_manage_post(
    lang: Language,
    mut db: Connection<Database>,
    _admin: Admin,
    form: Form<RoomManageForm<'_>>,
) -> RocketResult<Template> {
    let RoomManageForm {
        room_number,
        button,
    } = form.into_inner();
    let mut messages = Vec::new();
    if button == "create" {
        sqlx::query!("insert into rooms (room_number) values ($1)", &room_number)
            .execute(&mut *db)
            .await?;
        messages.push(Message {
            text_key: String::from("room-created"),
            message_type: MessageType::Success,
        });
    } else if button.starts_with("delete-") {
        let dash_position = button.chars().position(|c| c == '-').unwrap();
        let id_str: String = button.chars().skip(dash_position + 1).collect();
        let id = id_str.parse::<i32>()?;
        sqlx::query!("delete from rooms where id = $1", &id)
            .execute(&mut *db)
            .await?;
        messages.push(Message {
            text_key: String::from("room-deleted"),
            message_type: MessageType::Success,
        });
    } else {
        messages.push(Message {
            text_key: String::from("validation-unknown"),
            message_type: MessageType::Error,
        });
    }

    let rooms: Vec<Room> = sqlx::query_as!(Room, "select id, room_number from rooms")
        .fetch_all(&mut *db)
        .await?;
    Ok(Template::render(
        "room-manage",
        context! {
            lang: lang.into_string(),
            rooms,
            messages,
        },
    ))
}

#[derive(FromForm)]
pub struct AnnouncementsForm<'r> {
    pub announcements: BTreeMap<&'r str, BTreeMap<&'r str, &'r str>>,
}

#[derive(sqlx::FromRow, Serialize, Debug)]
pub struct Announcement {
    pub lang: String,
    pub position: String,
    pub description: String,
    pub content: String,
}

#[get("/admin/announcements")]
pub async fn announcements_get(
    lang: Language,
    mut db: Connection<Database>,
) -> RocketResult<Template> {
    let announcements = sqlx::query_as!(
        Announcement,
        r#"select lang::text as "lang!", position::text as "position!", description as "description!", content as "content!"
        from announcements
        order by position, lang"#
    ).fetch_all(&mut *db).await?;
    Ok(Template::render(
        "announcements",
        context! {
            lang: lang.into_string(),
            announcements
        },
    ))
}

#[post("/admin/announcements", data = "<form>")]
pub async fn announcements_post(
    mut db: Connection<Database>,
    form: Form<AnnouncementsForm<'_>>,
) -> RocketResult<Redirect> {
    let AnnouncementsForm { announcements } = form.into_inner();
    for (p, map) in announcements {
        for (l, c) in map {
            sqlx::query!(
                "update announcements set content = $1 \
                where position = ($2::text)::announcement_position and lang = ($3::text)::language",
                &c,
                &p,
                &l,
            )
            .execute(&mut *db)
            .await?;
        }
    }
    Ok(Redirect::to(uri!(announcements_get)))
}

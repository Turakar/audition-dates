use anyhow::anyhow;
use chrono::Duration;
use chrono::Local;
use rocket::form;
use rocket::form::Form;
use rocket::form::FromForm;
use rocket::response::Redirect;
use rocket::serde::json::Json;
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};

use crate::model::Room;
use crate::model::validate_room;
use crate::model::Date;
use crate::model::FormDateTime;
use crate::model::IntoInner;
use crate::model::Message;
use crate::model::MessageType;
use crate::{auth::Admin, language::Language, model::DateType, Database, RocketResult};

#[get("/admin/dashboard")]
pub async fn dashboard(
    lang: Language,
    admin: Admin,
    mut db: Connection<Database>,
) -> RocketResult<Template> {
    let display_name = sqlx::query!("select display_name from admins where id = $1", &admin.id)
        .fetch_one(&mut *db)
        .await?
        .display_name;
    Ok(Template::render(
        "dashboard",
        context! { lang: lang.into_string(), display_name },
    ))
}

#[get("/admin/date-new-1")]
pub async fn date_new_1_get(
    lang: Language,
    _admin: Admin,
    mut db: Connection<Database>,
) -> RocketResult<Template> {
    let rooms: Vec<String> = sqlx::query!("select room_number from rooms order by room_number asc")
        .fetch_all(&mut *db)
        .await?
        .into_iter()
        .map(|record| record.room_number)
        .collect();
    Ok(Template::render(
        "date-new-1",
        context! {
            lang: lang.into_string(),
            rooms,
            room_selected: "",
            date_types: DateType::get_variants_tera(),
            date_type_selected: "",
            from_date: Local::now(),
            to_date: Local::now() + Duration::hours(1),
            interval: 10i32,
        },
    ))
}

#[derive(FromForm)]
pub struct DateNew1Form<'r> {
    date_type: form::Result<'r, DateType>,
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
    let DateNew1Form {
        date_type,
        room,
        from_date,
        to_date,
        interval,
    } = form.into_inner();

    let mut messages = Vec::new();
    let date_type_selected = date_type
        .as_ref()
        .map(|x| x.get_value())
        .unwrap_or_default();
    let (room, room_id) = validate_room(room, &mut messages, &mut db).await?;
    let from_date = from_date.into_inner();
    let to_date = to_date.into_inner();
    let interval = interval as i64;

    let num_minutes = (to_date - from_date).num_minutes() as i64;
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
        return Ok(Template::render(
            "date-new-1",
            context! {
                lang: lang.into_string(),
                rooms,
                room_selected: room,
                date_types: DateType::get_variants_tera(),
                date_type_selected: date_type_selected,
                messages,
                from_date,
                to_date,
                interval,
            },
        ));
    }

    let date_type = date_type.unwrap();

    let num_dates = (num_minutes / interval) as i32;
    let dates: Vec<Date> = (0..num_dates)
        .into_iter()
        .map(|i| Date {
            id: None,
            from_date: from_date + Duration::minutes(interval) * i,
            to_date: from_date + Duration::minutes(interval) * (i + 1),
            room_id,
            date_type,
        })
        .collect();

    Ok(Template::render(
        "date-new-2",
        context! {
            lang: lang.into_string(),
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
pub async fn date_new_2_post(_admin: Admin, mut db: Connection<Database>, form: Form<DateNew2Form>) -> RocketResult<Redirect> {
    let DateNew2Form { date_selected, dates } = form.into_inner();
    let dates: Vec<Date> = dates.0.into_iter()
        .zip(date_selected.into_iter())
        .filter(|(_date, selected)| *selected)
        .map(|(date, _selected)| date)
        .collect();
    
    let invalid = dates.iter()
        .any(|date| date.from_date > date.to_date || date.id.is_some());
    if invalid {
        return Err(anyhow!("Invalid buffered dates!").into());
    }

    for date in dates {
        let Date { from_date, to_date, room_id, date_type, .. } = date;
        sqlx::query!(
            "insert into dates (from_date, to_date, room_id, date_type) values ($1, $2, $3, $4)",
            &from_date,
            &to_date,
            &room_id,
            &date_type.get_value(),
        ).execute(&mut *db).await?;
    }

    Ok(Redirect::to(uri!(dashboard)))
}

#[get("/admin/room-manage")]
pub async fn room_manage_get(lang: Language, mut db: Connection<Database>, _admin: Admin) -> RocketResult<Template> {
    let rooms: Vec<Room> = sqlx::query_as!(Room, "select id, room_number from rooms").fetch_all(&mut *db).await?;
    Ok(Template::render(
        "room-manage",
        context! {
            lang: lang.into_string(),
            rooms
        }
    ))
}

#[derive(FromForm)]
pub struct RoomManageForm<'r> {
    room_number: &'r str,
    button: &'r str,
}

#[post("/admin/room-manage", data = "<form>")]
pub async fn room_manage_post(lang: Language, mut db: Connection<Database>, _admin: Admin, form: Form<RoomManageForm<'_>>) -> RocketResult<Template> {
    let RoomManageForm { room_number, button } = form.into_inner();
    let mut messages = Vec::new();
    if button == "create" {
        sqlx::query!("insert into rooms (room_number) values ($1)", &room_number).execute(&mut *db).await?;
        messages.push(Message { text_key: String::from("room-created"), message_type: MessageType::Success });
    } else if button.starts_with("delete-") {
        let dash_position = button.chars().position(|c| c == '-').unwrap();
        let id_str: String = button.chars()
            .skip(dash_position + 1)
            .collect();
        let id = id_str.parse::<i32>()?;
        sqlx::query!("delete from rooms where id = $1", &id).execute(&mut *db).await?;
        messages.push(Message { text_key: String::from("room-deleted"), message_type: MessageType::Success });
    } else {
        messages.push(Message { text_key: String::from("validation-unknown"), message_type: MessageType::Error });
    }

    let rooms: Vec<Room> = sqlx::query_as!(Room, "select id, room_number from rooms").fetch_all(&mut *db).await?;
    Ok(Template::render(
        "room-manage",
        context! {
            lang: lang.into_string(),
            rooms,
            messages,
        }
    ))
}

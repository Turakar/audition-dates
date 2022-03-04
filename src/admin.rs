use std::alloc::handle_alloc_error;

use chrono::Duration;
use chrono::{Local, DateTime};
use rocket::form;
use rocket::form::Form;
use rocket::form::FromForm;
use rocket_db_pools::Connection;
use rocket_dyn_templates::{context, Template};

use crate::model::Date;
use crate::model::FormDateTime;
use crate::model::IntoInner;
use crate::model::Message;
use crate::model::MessageType;
use crate::model::handle_form_error;
use crate::model::validate_room;
use crate::{auth::Admin, language::Language, Database, RocketResult, model::DateType};

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
pub async fn date_new_1_get(lang: Language, _admin: Admin, mut db: Connection<Database>) -> RocketResult<Template> {
    let rooms: Vec<String> = sqlx::query!("select room_number from rooms order by room_number asc")
        .fetch_all(&mut *db)
        .await?.into_iter().map(|record| record.room_number).collect();
    Ok(Template::render(
        "date-new-1",
        context! { lang: lang.into_string(), rooms, room_selected: "", date_types: DateType::get_variants_tera(), date_type_selected: "" }
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
pub async fn date_new_1_post<'r> (lang: Language, _admin: Admin, mut db: Connection<Database>, form: Form<DateNew1Form<'r>>) -> RocketResult<Template> {
    let DateNew1Form { date_type, room, from_date, to_date, interval } = form.into_inner();
    
    let mut messages = Vec::new();
    let date_type_selected = date_type.map(|x| x.get_value()).unwrap_or_default();
    let (room, room_id) = validate_room(room, &mut messages, &mut db).await?;
    let from_date = from_date.into_inner();
    let to_date = to_date.into_inner();
    let interval = interval as i64;

    let num_minutes = (to_date - from_date).num_minutes() as i64;
    if from_date >= to_date {
        messages.push(Message { text_key: String::from("wrong-date-order"), message_type: MessageType::Error });
    } else if num_minutes % interval != 0 {
        messages.push(Message { text_key: String::from("interval-not-even"), message_type: MessageType::Error });
    } else if num_minutes / interval > 1000 {
        messages.push(Message { text_key: String::from("too-many-dates"), message_type: MessageType::Error });
    }

    if !messages.is_empty() {
        let rooms: Vec<String> = sqlx::query!("select room_number from rooms order by room_number asc")
            .fetch_all(&mut *db)
            .await?.into_iter().map(|record| record.room_number).collect();
        return Ok(Template::render(
            "date-new-1",
            context! {
                lang: lang.into_string(),
                rooms,
                room_selected: room,
                date_types: DateType::get_variants_tera(),
                date_type_selected: date_type_selected,
                messages,
            }
        ))
    }

    let date_type = date_type.unwrap();

    let num_dates = (num_minutes / interval) as i32;
    let dates = (0..num_dates).into_iter()
        .map(|i| Date {
            id: None,
            from_date: from_date + Duration::minutes(interval) * i,
            to_date: from_date + Duration::minutes(interval) * (i + 1),
            room_id,
            date_type,
            active: false,
        })
        .collect();
    
    Ok(Templates::render(
        "date-new-2",
        context! {
            
        }
    ))
}
